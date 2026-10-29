//! SSH transport built on `russh` (pure Rust, async, tokio-based).

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, DisconnectReason};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use tokio::net::TcpStream;
use tokio::sync::{oneshot, watch};
use tokio::time::{Instant, sleep, timeout};
use zeroize::Zeroizing;

use super::{TermSize, Transport, TransportEvent};
use crate::errors::AppError;

const DNS_TIMEOUT: Duration = Duration::from_secs(10);
const TCP_TIMEOUT: Duration = Duration::from_secs(10);
/// Key exchange must complete within this delay (time spent by the user on the
/// host key prompt is not counted).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Detects dead connections (network cut) after about 3 x 15 s without answer.
/// Will become configurable per profile in phase 5.
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEPALIVE_MAX: usize = 3;

pub enum SshAuth {
    Password(Zeroizing<String>),
    KeyFile {
        path: PathBuf,
        passphrase: Option<Zeroizing<String>>,
    },
}

// Manual Debug so that secrets can never end up in logs by accident.
impl fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SshAuth::Password(_) => f.write_str("Password(<redacted>)"),
            SshAuth::KeyFile { path, .. } => write!(f, "KeyFile({})", path.display()),
        }
    }
}

#[derive(Debug)]
pub struct SshParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
}

/// What the user needs to see to decide whether to trust a server.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    /// OpenSSH format, e.g. `SHA256:...`.
    pub fingerprint: String,
}

/// Asks the user to confirm a host key. The answer arrives later on the receiver;
/// a dropped sender counts as a refusal.
pub type HostKeyPrompt = Box<dyn Fn(HostKeyInfo) -> oneshot::Receiver<bool> + Send + Sync>;

/// russh calls into this handler during the connection (host key check, disconnection).
struct ClientHandler {
    host: String,
    port: u16,
    ask_host_key: HostKeyPrompt,
    /// `true` while waiting for the user, so the handshake timeout can be paused.
    prompting: watch::Sender<bool>,
    /// Why russh ended the connection, read later to explain the closure.
    disconnect_reason: Arc<Mutex<Option<String>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key,
            PublicKeyOrCertificate::Certificate(_) => {
                tracing::warn!("host certificates are not supported yet, rejecting");
                return Ok(false);
            }
        };
        let info = HostKeyInfo {
            host: self.host.clone(),
            port: self.port,
            algorithm: key.algorithm().as_str().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        };
        // Phase 2: the key is shown and confirmed at every connection.
        // Persistence in a known_hosts file comes in phase 3.
        self.prompting.send_replace(true);
        let accepted = (self.ask_host_key)(info).await.unwrap_or(false);
        self.prompting.send_replace(false);
        Ok(accepted)
    }

    async fn disconnected(
        &mut self,
        reason: DisconnectReason<Self::Error>,
    ) -> Result<(), Self::Error> {
        let text = match &reason {
            DisconnectReason::ReceivedDisconnect(info) => {
                format!("le serveur a fermé la connexion ({})", info.message)
            }
            DisconnectReason::Error(russh::Error::KeepaliveTimeout) => {
                "le serveur ne répond plus (keepalive sans réponse : coupure réseau ?)".into()
            }
            DisconnectReason::Error(e) => e.to_string(),
        };
        if let Ok(mut slot) = self.disconnect_reason.lock() {
            *slot = Some(text);
        }
        match reason {
            DisconnectReason::ReceivedDisconnect(_) => Ok(()),
            DisconnectReason::Error(e) => Err(e),
        }
    }
}

pub struct SshTransport {
    handle: client::Handle<ClientHandler>,
    channel: russh::Channel<client::Msg>,
    exit_status: Option<u32>,
    disconnect_reason: Arc<Mutex<Option<String>>>,
}

/// Opens the connection, checks the host key, authenticates and starts an
/// interactive shell with a PTY of the given size.
pub async fn connect(
    params: SshParams,
    size: TermSize,
    ask_host_key: HostKeyPrompt,
) -> Result<SshTransport, AppError> {
    let tcp = open_tcp(&params.host, params.port).await?;

    let config = Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE_INTERVAL),
        keepalive_max: KEEPALIVE_MAX,
        // `Preferred::DEFAULT` only contains modern algorithms. Legacy ones will be
        // enabled per profile by the compatibility mode (phase 3).
        ..Default::default()
    });
    let (prompting_tx, mut prompting_rx) = watch::channel(false);
    let disconnect_reason = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        host: params.host.clone(),
        port: params.port,
        ask_host_key,
        prompting: prompting_tx,
        disconnect_reason: disconnect_reason.clone(),
    };

    // Handshake with a timeout that is paused while the host key prompt is open:
    // the user may take as long as they want to read the fingerprint.
    let handshake = client::connect_stream(config, tcp, handler);
    tokio::pin!(handshake);
    let deadline = sleep(HANDSHAKE_TIMEOUT);
    tokio::pin!(deadline);
    let mut handle = loop {
        tokio::select! {
            result = &mut handshake => break result?,
            Ok(()) = prompting_rx.changed() => {
                if !*prompting_rx.borrow() {
                    deadline.as_mut().reset(Instant::now() + HANDSHAKE_TIMEOUT);
                }
            }
            _ = &mut deadline, if !*prompting_rx.borrow() => {
                return Err(AppError::Timeout(
                    "le serveur a accepté la connexion TCP mais la négociation SSH n'a pas abouti \
                     (est-ce bien un serveur SSH sur ce port ?)".into(),
                ));
            }
        }
    };

    timeout(AUTH_TIMEOUT, authenticate(&mut handle, params))
        .await
        .map_err(|_| AppError::Timeout("l'authentification n'a pas abouti.".into()))??;

    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(
            false,
            "xterm-256color",
            size.cols.into(),
            size.rows.into(),
            0,
            0,
            &[],
        )
        .await?;
    channel.request_shell(false).await?;

    Ok(SshTransport {
        handle,
        channel,
        exit_status: None,
        disconnect_reason,
    })
}

async fn open_tcp(host: &str, port: u16) -> Result<TcpStream, AppError> {
    // Resolving first lets us tell "unknown host name" apart from "host unreachable".
    let addrs: Vec<_> = timeout(DNS_TIMEOUT, tokio::net::lookup_host((host, port)))
        .await
        .map_err(|_| AppError::Timeout(format!("la résolution DNS de « {host} » ne répond pas.")))?
        .map_err(|_| AppError::HostNotFound(host.to_string()))?
        .collect();
    if addrs.is_empty() {
        return Err(AppError::HostNotFound(host.to_string()));
    }

    let tcp = timeout(TCP_TIMEOUT, TcpStream::connect(&addrs[..]))
        .await
        .map_err(|_| {
            AppError::Timeout(format!(
                "aucune réponse de {host}:{port} après {} s (hôte éteint ou pare-feu ?).",
                TCP_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| {
            use std::io::ErrorKind as K;
            AppError::Unreachable(match e.kind() {
                K::ConnectionRefused => {
                    format!(
                        "connexion refusée par {host}:{port} (aucun serveur SSH sur ce port ?)."
                    )
                }
                K::NetworkUnreachable | K::HostUnreachable => {
                    format!("{host} n'est pas joignable (réseau ou route absente).")
                }
                _ => format!("{host}:{port} : {e}"),
            })
        })?;
    // Interactive traffic: send keystrokes immediately instead of batching them.
    if let Err(e) = tcp.set_nodelay(true) {
        tracing::debug!("set_nodelay failed: {e}");
    }
    Ok(tcp)
}

async fn authenticate(
    handle: &mut client::Handle<ClientHandler>,
    params: SshParams,
) -> Result<(), AppError> {
    let result = match params.auth {
        SshAuth::Password(password) => {
            // russh copies the password into its own String: we cannot zeroize that
            // copy, but ours is wiped when `password` is dropped.
            handle
                .authenticate_password(params.username, password.as_str())
                .await?
        }
        SshAuth::KeyFile { path, passphrase } => {
            let key = load_private_key(path, passphrase).await?;
            // For RSA keys, pick the best signature hash the server supports
            // (rsa-sha2-512/256 instead of the legacy SHA-1 `ssh-rsa`).
            let hash = match handle.best_supported_rsa_hash().await? {
                Some(hash) => hash,
                None => Some(HashAlg::Sha256),
            };
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash);
            handle.authenticate_publickey(params.username, key).await?
        }
    };

    match result {
        client::AuthResult::Success => Ok(()),
        client::AuthResult::Failure {
            remaining_methods, ..
        } => {
            let methods: Vec<&str> = remaining_methods.iter().map(<&str>::from).collect();
            Err(AppError::AuthFailed(if methods.is_empty() {
                "aucune".into()
            } else {
                methods.join(", ")
            }))
        }
    }
}

async fn load_private_key(
    path: PathBuf,
    passphrase: Option<Zeroizing<String>>,
) -> Result<russh::keys::PrivateKey, AppError> {
    // Reading and decrypting the key is blocking (file I/O + KDF that can take
    // ~1 s): run it on tokio's blocking thread pool, not on the async workers.
    let display = path.display().to_string();
    tokio::task::spawn_blocking(move || {
        russh::keys::load_secret_key(&path, passphrase.as_ref().map(|p| p.as_str()))
    })
    .await
    .map_err(|_| AppError::PrivateKey("le chargement de la clé a échoué.".into()))?
    .map_err(|e| {
        use russh::keys::Error as K;
        AppError::PrivateKey(match e {
            K::KeyIsEncrypted => {
                format!("{display} est protégée par une passphrase : renseigne-la.")
            }
            K::IO(io) if io.kind() == std::io::ErrorKind::NotFound => {
                format!("fichier {display} introuvable.")
            }
            K::IO(io) if io.kind() == std::io::ErrorKind::PermissionDenied => {
                format!("lecture de {display} refusée (permissions).")
            }
            other => format!("impossible de lire {display} (passphrase incorrecte ?) : {other}"),
        })
    })
}

impl Transport for SshTransport {
    async fn recv(&mut self) -> TransportEvent {
        // `Channel::wait` reads from an mpsc channel, which is cancel-safe.
        loop {
            match self.channel.wait().await {
                Some(ChannelMsg::Data { data }) => return TransportEvent::Data(data.to_vec()),
                // stderr: with a PTY it is normally merged into stdout already.
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    return TransportEvent::Data(data.to_vec());
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => {
                    self.exit_status = Some(exit_status);
                }
                Some(ChannelMsg::Close) => {
                    let reason = match self.exit_status {
                        Some(code) => {
                            format!("Session terminée par le serveur (code de sortie {code}).")
                        }
                        None => "Session terminée par le serveur.".to_string(),
                    };
                    return TransportEvent::Closed(reason);
                }
                Some(_) => {}
                // The whole SSH connection is gone (network cut, keepalive timeout...).
                None => {
                    let detail = self
                        .disconnect_reason
                        .lock()
                        .ok()
                        .and_then(|r| r.clone())
                        .unwrap_or_else(|| "connexion interrompue".into());
                    return TransportEvent::Closed(format!("Connexion perdue : {detail}."));
                }
            }
        }
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), AppError> {
        self.channel.data(data).await.map_err(AppError::from)
    }

    async fn resize(&mut self, size: TermSize) -> Result<(), AppError> {
        self.channel
            .window_change(size.cols.into(), size.rows.into(), 0, 0)
            .await
            .map_err(AppError::from)
    }

    async fn close(self) {
        // Best effort: the connection may already be dead, errors are irrelevant here.
        let _ = self.channel.close().await;
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "session closed", "en")
            .await;
        // `self` is dropped here: the russh session task ends and the socket is closed.
    }
}

#[cfg(test)]
mod tests;
