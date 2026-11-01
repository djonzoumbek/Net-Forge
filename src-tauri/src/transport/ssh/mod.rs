//! SSH transport built on `russh` (pure Rust, async, tokio-based).

pub mod algorithms;

use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, DisconnectReason};
use russh::keys::agent::AgentIdentity;
use russh::keys::agent::client::{AgentClient, AgentStream};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use tokio::sync::{oneshot, watch};
use tokio::time::{Instant, sleep, timeout};
use zeroize::Zeroizing;

use self::algorithms::Negotiated;
use super::{ConnectionInfo, TermSize, Transport, TransportEvent, net};
use crate::errors::AppError;
use crate::security::known_hosts::{HostKeyMismatch, HostKeyStatus, KnownHosts};

/// Key exchange must complete within this delay (time spent by the user on the
/// host key prompt is not counted).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Keepalives left unanswered before the connection is declared dead.
const KEEPALIVE_MAX: usize = 3;

pub enum SshAuth {
    Password(Zeroizing<String>),
    KeyFile {
        path: PathBuf,
        passphrase: Option<Zeroizing<String>>,
    },
    /// Keys held by an SSH agent. `None`: the agent of the session, found through
    /// `SSH_AUTH_SOCK` (the normal case); `Some`: an explicit socket (tests).
    Agent {
        socket: Option<PathBuf>,
    },
}

// Manual Debug so that secrets can never end up in logs by accident.
impl fmt::Debug for SshAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SshAuth::Password(_) => f.write_str("Password(<redacted>)"),
            SshAuth::KeyFile { path, .. } => write!(f, "KeyFile({})", path.display()),
            SshAuth::Agent { .. } => f.write_str("Agent"),
        }
    }
}

#[derive(Debug)]
pub struct SshParams {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: SshAuth,
    /// "Compatibility mode for old devices": also offer legacy algorithms.
    pub legacy: bool,
    /// SSH keepalive interval; `None` disables it. Keeps idle sessions open
    /// through firewalls/NAT and detects dead connections.
    pub keepalive: Option<Duration>,
}

/// What the user needs to see to decide whether to trust an unknown server.
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
    known_hosts: Arc<KnownHosts>,
    ask_host_key: HostKeyPrompt,
    /// `true` while waiting for the user, so the handshake timeout can be paused.
    prompting: watch::Sender<bool>,
    /// Why russh ended the connection, read later to explain the closure.
    disconnect_reason: Arc<Mutex<Option<String>>>,
    /// Why the host key was refused, read by `connect` to build the error.
    refusal: Arc<Mutex<Option<HostKeyRefusal>>>,
    negotiated: Arc<Mutex<Negotiated>>,
}

enum HostKeyRefusal {
    Changed(HostKeyMismatch),
    Revoked,
    Certificate,
    StorageError(String),
}

impl ClientHandler {
    fn refuse(&self, reason: HostKeyRefusal) -> Result<bool, russh::Error> {
        if let Ok(mut slot) = self.refusal.lock() {
            *slot = Some(reason);
        }
        Ok(false)
    }

    async fn ask_user(&mut self, key: &PublicKey) -> bool {
        let info = HostKeyInfo {
            host: self.host.clone(),
            port: self.port,
            algorithm: key.algorithm().as_str().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        };
        self.prompting.send_replace(true);
        let accepted = (self.ask_host_key)(info).await.unwrap_or(false);
        self.prompting.send_replace(false);
        accepted
    }
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(_) => {
                return self.refuse(HostKeyRefusal::Certificate);
            }
        };
        // known_hosts files are read/written on the blocking pool (disk I/O).
        let (kh, host, port, k) = (
            self.known_hosts.clone(),
            self.host.clone(),
            self.port,
            key.clone(),
        );
        let status = tokio::task::spawn_blocking(move || kh.check(&host, port, &k)).await;
        match status {
            Ok(Ok(HostKeyStatus::Trusted)) => Ok(true),
            Ok(Ok(HostKeyStatus::Unknown)) => {
                if !self.ask_user(&key).await {
                    return Ok(false);
                }
                let (kh, host, port) = (self.known_hosts.clone(), self.host.clone(), self.port);
                let saved = tokio::task::spawn_blocking(move || kh.add(&host, port, &key)).await;
                if !matches!(saved, Ok(Ok(()))) {
                    // Accepted for this session even if it could not be remembered.
                    tracing::warn!("accepted host key could not be saved");
                }
                Ok(true)
            }
            Ok(Ok(HostKeyStatus::Changed(mismatch))) => {
                self.refuse(HostKeyRefusal::Changed(mismatch))
            }
            Ok(Ok(HostKeyStatus::Revoked)) => self.refuse(HostKeyRefusal::Revoked),
            Ok(Err(e)) => self.refuse(HostKeyRefusal::StorageError(e.to_string())),
            Err(e) => self.refuse(HostKeyRefusal::StorageError(e.to_string())),
        }
    }

    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if let Ok(mut slot) = self.negotiated.lock() {
            *slot = Negotiated::from_names(names);
        }
        Ok(())
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

/// Lets file transfers open their own channel on the session's SSH connection
/// (no second login, no second host key check). Cloning it only clones an `Arc`.
#[derive(Clone)]
pub struct SshLink(Arc<client::Handle<ClientHandler>>);

impl SshLink {
    /// Opens an SFTP subsystem channel. Fails if the server has no SFTP (many
    /// network devices only offer SCP, or nothing).
    pub async fn open_sftp(&self) -> Result<russh_sftp::client::SftpSession, AppError> {
        let channel = self.0.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|e| {
                AppError::Sftp(format!(
                    "le serveur ne propose pas SFTP sur cette connexion ({e})."
                ))
            })
    }

    /// Port forwarding: each call opens a `direct-tcpip` channel, i.e. asks the
    /// SSH server to connect to `host:port` itself (`ssh -L` / `ssh -D`).
    pub fn dialer(&self) -> crate::forwarding::Dialer {
        let handle = Arc::clone(&self.0);
        Arc::new(
            move |host: String, port: u16, origin: std::net::SocketAddr| {
                let handle = Arc::clone(&handle);
                Box::pin(async move {
                    let channel = handle
                        .channel_open_direct_tcpip(
                            host,
                            u32::from(port),
                            origin.ip().to_string(),
                            u32::from(origin.port()),
                        )
                        .await
                        .map_err(|e| match e {
                            // The server could not reach the target, or forbids
                            // forwarding (`AllowTcpForwarding no`, many routers).
                            russh::Error::ChannelOpenFailure(reason) => AppError::Forward(format!(
                                "refusé par le serveur SSH ({reason:?}) : destination injoignable \
                             depuis le serveur, ou redirections interdites."
                            )),
                            other => AppError::from(other),
                        })?;
                    Ok(Box::new(channel.into_stream()) as Box<dyn crate::forwarding::Tunnel>)
                })
            },
        )
    }
}

pub struct SshTransport {
    // `Arc`: shared with `SshLink` (file transfers). The connection itself is
    // closed explicitly by `close`, whoever still holds a clone.
    handle: Arc<client::Handle<ClientHandler>>,
    channel: russh::Channel<client::Msg>,
    exit_status: Option<u32>,
    disconnect_reason: Arc<Mutex<Option<String>>>,
    negotiated: Arc<Mutex<Negotiated>>,
}

/// Opens the connection, checks the host key, authenticates and starts an
/// interactive shell with a PTY of the given size.
pub async fn connect(
    params: SshParams,
    size: TermSize,
    known_hosts: Arc<KnownHosts>,
    ask_host_key: HostKeyPrompt,
) -> Result<SshTransport, AppError> {
    let tcp = net::open_tcp(&params.host, params.port, params.keepalive).await?;

    let config = Arc::new(client::Config {
        keepalive_interval: params.keepalive,
        keepalive_max: KEEPALIVE_MAX,
        preferred: algorithms::preferred(params.legacy),
        ..Default::default()
    });
    let (prompting_tx, mut prompting_rx) = watch::channel(false);
    let disconnect_reason = Arc::new(Mutex::new(None));
    let refusal = Arc::new(Mutex::new(None));
    let negotiated = Arc::new(Mutex::new(Negotiated::default()));
    let handler = ClientHandler {
        host: params.host.clone(),
        port: params.port,
        known_hosts,
        ask_host_key,
        prompting: prompting_tx,
        disconnect_reason: disconnect_reason.clone(),
        refusal: refusal.clone(),
        negotiated: negotiated.clone(),
    };

    // Handshake with a timeout that is paused while the host key prompt is open:
    // the user may take as long as they want to read the fingerprint.
    let handshake = client::connect_stream(config, tcp, handler);
    tokio::pin!(handshake);
    let deadline = sleep(HANDSHAKE_TIMEOUT);
    tokio::pin!(deadline);
    let mut handle = loop {
        tokio::select! {
            result = &mut handshake => {
                break result.map_err(|e| handshake_error(e, &refusal, params.legacy))?;
            }
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
        handle: Arc::new(handle),
        channel,
        exit_status: None,
        disconnect_reason,
        negotiated,
    })
}

/// Turns a failed handshake into the most useful error for the user.
fn handshake_error(
    error: russh::Error,
    refusal: &Mutex<Option<HostKeyRefusal>>,
    legacy_enabled: bool,
) -> AppError {
    if let russh::Error::UnknownKey = error {
        let reason = refusal.lock().ok().and_then(|mut r| r.take());
        return match reason {
            Some(HostKeyRefusal::Changed(mismatch)) => AppError::HostKeyChanged(Box::new(mismatch)),
            Some(HostKeyRefusal::Revoked) => AppError::HostKeyRevoked,
            Some(HostKeyRefusal::Certificate) => AppError::Ssh(
                "le serveur présente un certificat d'hôte, non pris en charge pour l'instant."
                    .into(),
            ),
            Some(HostKeyRefusal::StorageError(e)) => {
                AppError::Storage(format!("vérification de la clé d'hôte impossible : {e}"))
            }
            None => AppError::HostKeyRejected,
        };
    }
    if let russh::Error::HUP | russh::Error::Disconnect = error {
        // Closed before authentication: on network devices this is usually a
        // connection limit or an access list, not a network problem.
        return AppError::Unreachable(
            "le serveur a fermé la connexion pendant la négociation SSH (trop de connexions \
             simultanées, lignes VTY toutes occupées, ou accès refusé par une ACL ?)."
                .into(),
        );
    }
    match AppError::from(error) {
        AppError::NoCommonAlgorithm { kind, server, .. } => AppError::NoCommonAlgorithm {
            kind,
            server,
            legacy_enabled,
        },
        other => other,
    }
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
        SshAuth::Agent { socket } => {
            authenticate_with_agent(handle, params.username, socket).await?
        }
    };
    auth_result(result)
}

fn auth_result(result: client::AuthResult) -> Result<(), AppError> {
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

/// The agent's stream type depends on the OS: `dynamic()` boxes it so that the
/// rest of the authentication is written once.
type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin + 'static>>;

/// Linux: the Unix socket named by `SSH_AUTH_SOCK` (or `socket`, in tests).
#[cfg(unix)]
async fn connect_agent(socket: Option<PathBuf>) -> Result<Agent, russh::keys::Error> {
    let client = match socket {
        Some(path) => AgentClient::connect_uds(path).await?,
        None => AgentClient::connect_env().await?,
    };
    Ok(client.dynamic())
}

/// Windows: the OpenSSH agent's named pipe (service "OpenSSH Authentication
/// Agent"), then Pageant (PuTTY) if that service is not running.
#[cfg(windows)]
async fn connect_agent(socket: Option<PathBuf>) -> Result<Agent, russh::keys::Error> {
    let pipe = socket
        .map(PathBuf::into_os_string)
        .unwrap_or_else(|| r"\\.\pipe\openssh-ssh-agent".into());
    match AgentClient::connect_named_pipe(&pipe).await {
        Ok(client) => Ok(client.dynamic()),
        Err(_) => Ok(AgentClient::connect_pageant().await?.dynamic()),
    }
}

/// Tries each key of the SSH agent in turn. The private keys never leave the
/// agent: it only signs the authentication challenge for us.
async fn authenticate_with_agent(
    handle: &mut client::Handle<ClientHandler>,
    username: String,
    socket: Option<PathBuf>,
) -> Result<client::AuthResult, AppError> {
    let mut agent = connect_agent(socket).await.map_err(|e| {
        AppError::Agent(match e {
            russh::keys::Error::EnvVar(_) => {
                "aucun agent SSH détecté (variable SSH_AUTH_SOCK absente).".into()
            }
            other => format!("connexion à l'agent SSH impossible : {other}"),
        })
    })?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| AppError::Agent(format!("lecture des clés de l'agent : {e}")))?;
    let keys: Vec<PublicKey> = identities
        .into_iter()
        .filter_map(|identity| match identity {
            AgentIdentity::PublicKey { key, .. } => Some(key),
            AgentIdentity::Certificate { .. } => None,
        })
        .collect();
    if keys.is_empty() {
        return Err(AppError::Agent(
            "l'agent SSH ne contient aucune clé (ajoute-en une avec ssh-add).".into(),
        ));
    }
    let hash = match handle.best_supported_rsa_hash().await? {
        Some(hash) => hash,
        None => Some(HashAlg::Sha256),
    };
    let mut last = None;
    for key in keys {
        let result = handle
            .authenticate_publickey_with(username.clone(), key, hash, &mut agent)
            .await
            .map_err(|e| AppError::Agent(format!("signature par l'agent : {e}")))?;
        if let client::AuthResult::Success = result {
            return Ok(result);
        }
        last = Some(result);
    }
    // `keys` is not empty, so `last` is always set here.
    last.ok_or_else(|| AppError::Agent("aucune clé de l'agent n'a été essayée.".into()))
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
            K::KeyIsEncrypted => return AppError::PassphraseRequired(display),
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
    fn sftp_link(&self) -> Option<SshLink> {
        Some(SshLink(Arc::clone(&self.handle)))
    }

    fn info(&self) -> ConnectionInfo {
        let negotiated = self
            .negotiated
            .lock()
            .map(|n| n.clone())
            .unwrap_or_default();
        ConnectionInfo {
            protocol: "ssh",
            encrypted: true,
            algorithms: Some(negotiated),
        }
    }

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
                    return TransportEvent::Lost(format!("Connexion perdue : {detail}."));
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
