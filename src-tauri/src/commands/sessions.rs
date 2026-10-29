//! Session commands: open (quick connect or profile), input, resize, close.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use zeroize::Zeroizing;

use crate::errors::AppError;
use crate::persistence::Database;
use crate::persistence::profiles::{self, AuthMethod, Protocol};
use crate::security::known_hosts::KnownHosts;
use crate::security::secrets::Secrets;
use crate::sessions::{SessionCommand, SessionEvent, SessionId, SessionManager, SessionSinks};
use crate::transport::TermSize;
use crate::transport::ssh::{SshAuth, SshParams};
use crate::transport::telnet::TelnetParams;

/// Upper bound for one `session_write` call (a large paste).
const MAX_WRITE: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(
    tag = "method",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AuthRequest {
    Password {
        password: String,
    },
    KeyFile {
        path: String,
        passphrase: Option<String>,
    },
    Agent,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectRequest {
    host: String,
    port: u16,
    username: String,
    auth: AuthRequest,
    #[serde(default)]
    legacy: bool,
}

impl SshConnectRequest {
    fn into_params(self) -> Result<SshParams, AppError> {
        let host = self.host.trim().to_string();
        let username = self.username.trim().to_string();
        if host.is_empty() || host.len() > 255 || host.chars().any(char::is_whitespace) {
            return Err(AppError::InvalidInput("nom d'hôte invalide.".into()));
        }
        if self.port == 0 {
            return Err(AppError::InvalidInput(
                "le port doit être entre 1 et 65535.".into(),
            ));
        }
        if username.is_empty() || username.len() > 255 {
            return Err(AppError::InvalidInput("nom d'utilisateur invalide.".into()));
        }
        // Moving the String into `Zeroizing` keeps the same heap buffer: it is the
        // buffer that will be wiped on drop.
        let auth = match self.auth {
            AuthRequest::Password { password } => SshAuth::Password(Zeroizing::new(password)),
            AuthRequest::KeyFile { path, passphrase } => {
                let path = expand_home(path.trim());
                if path.as_os_str().is_empty() {
                    return Err(AppError::InvalidInput("chemin de clé vide.".into()));
                }
                SshAuth::KeyFile {
                    path,
                    passphrase: passphrase.filter(|p| !p.is_empty()).map(Zeroizing::new),
                }
            }
            AuthRequest::Agent => SshAuth::Agent { socket: None },
        };
        Ok(SshParams {
            host,
            port: self.port,
            username,
            auth,
            legacy: self.legacy,
        })
    }
}

/// `~/.ssh/id_ed25519` is what users type; the OS does not expand `~` for us.
pub(crate) fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

fn sinks(on_data: Channel<InvokeResponseBody>, on_event: Channel<SessionEvent>) -> SessionSinks {
    SessionSinks {
        // Raw bytes: delivered to JS as an ArrayBuffer, no JSON encoding.
        data: Box::new(move |bytes| {
            if let Err(e) = on_data.send(InvokeResponseBody::Raw(bytes)) {
                tracing::debug!("output channel closed: {e}");
            }
        }),
        event: Box::new(move |event| {
            if let Err(e) = on_event.send(event) {
                tracing::debug!("event channel closed: {e}");
            }
        }),
    }
}

/// Quick connect. Starts a session and returns its id immediately. Progress (host
/// key prompt, connected, failure) arrives on `on_event`; output on `on_data`.
#[tauri::command]
pub fn session_open_ssh(
    manager: State<'_, SessionManager>,
    known_hosts: State<'_, Arc<KnownHosts>>,
    request: SshConnectRequest,
    size: TermSize,
    on_data: Channel<InvokeResponseBody>,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId, AppError> {
    let params = request.into_params()?;
    let size = size.validate()?;
    tracing::info!(host = %params.host, port = params.port, "opening SSH session");
    let known_hosts = Arc::clone(&known_hosts);
    Ok(manager.open_ssh(params, size, known_hosts, sinks(on_data, on_event)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelnetConnectRequest {
    host: String,
    port: u16,
}

impl TelnetConnectRequest {
    fn into_params(self) -> Result<TelnetParams, AppError> {
        let host = validate_host(&self.host)?;
        if self.port == 0 {
            return Err(AppError::InvalidInput(
                "le port doit être entre 1 et 65535.".into(),
            ));
        }
        Ok(TelnetParams {
            host,
            port: self.port,
        })
    }
}

fn validate_host(host: &str) -> Result<String, AppError> {
    let host = host.trim();
    if host.is_empty() || host.len() > 255 || host.chars().any(char::is_whitespace) {
        return Err(AppError::InvalidInput("nom d'hôte invalide.".into()));
    }
    Ok(host.to_string())
}

/// Quick Telnet connection (lab consoles). Unencrypted: the UI warns about it.
#[tauri::command]
pub fn session_open_telnet(
    manager: State<'_, SessionManager>,
    request: TelnetConnectRequest,
    size: TermSize,
    on_data: Channel<InvokeResponseBody>,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId, AppError> {
    let params = request.into_params()?;
    let size = size.validate()?;
    tracing::info!(host = %params.host, port = params.port, "opening Telnet session");
    Ok(manager.open_telnet(params, size, sinks(on_data, on_event)))
}

/// Connects with a saved profile. `secret` is the password / passphrase typed by
/// the user when none is saved (or to override the saved one).
#[allow(clippy::too_many_arguments)] // Tauri injects states and channels as arguments.
#[tauri::command]
pub async fn session_open_profile(
    manager: State<'_, SessionManager>,
    known_hosts: State<'_, Arc<KnownHosts>>,
    db: State<'_, Database>,
    secrets: State<'_, Arc<Secrets>>,
    profile_id: i64,
    secret: Option<String>,
    size: TermSize,
    on_data: Channel<InvokeResponseBody>,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId, AppError> {
    let size = size.validate()?;
    let typed = secret.filter(|s| !s.is_empty()).map(Zeroizing::new);
    let secrets = Arc::clone(&secrets);
    // Reading the keyring may block (D-Bus): done on the blocking pool with the DB.
    let (profile, saved) = db
        .run(move |conn| profiles::get_with_secret(conn, &secrets, profile_id))
        .await?;
    if profile.protocol == Protocol::Telnet {
        let params = TelnetParams {
            host: profile.host,
            port: profile.port,
        };
        tracing::info!(profile = profile_id, host = %params.host, port = params.port, "opening Telnet session");
        return Ok(manager.open_telnet(params, size, sinks(on_data, on_event)));
    }
    let secret = typed.or(saved);
    let auth = match profile.auth_method {
        AuthMethod::None => {
            return Err(AppError::InvalidInput(
                "profil SSH sans méthode d'authentification.".into(),
            ));
        }
        AuthMethod::Password => SshAuth::Password(secret.ok_or(AppError::PasswordRequired)?),
        AuthMethod::KeyFile => SshAuth::KeyFile {
            path: expand_home(profile.key_path.as_deref().unwrap_or_default()),
            passphrase: secret,
        },
        AuthMethod::Agent => SshAuth::Agent { socket: None },
    };
    let params = SshParams {
        host: profile.host,
        port: profile.port,
        username: profile.username,
        auth,
        legacy: profile.legacy_mode,
    };
    tracing::info!(profile = profile_id, host = %params.host, port = params.port, "opening SSH session");
    let known_hosts = Arc::clone(&known_hosts);
    Ok(manager.open_ssh(params, size, known_hosts, sinks(on_data, on_event)))
}

#[tauri::command]
pub async fn session_write(
    manager: State<'_, SessionManager>,
    id: SessionId,
    data: Vec<u8>,
) -> Result<(), AppError> {
    if data.len() > MAX_WRITE {
        return Err(AppError::InvalidInput(
            "texte trop volumineux (1 Mio maximum).".into(),
        ));
    }
    manager.send(id, SessionCommand::Write(data)).await
}

#[tauri::command]
pub async fn session_resize(
    manager: State<'_, SessionManager>,
    id: SessionId,
    size: TermSize,
) -> Result<(), AppError> {
    manager
        .send(id, SessionCommand::Resize(size.validate()?))
        .await
}

#[tauri::command]
pub async fn session_ack(
    manager: State<'_, SessionManager>,
    id: SessionId,
    bytes: usize,
) -> Result<(), AppError> {
    manager.send(id, SessionCommand::Ack(bytes)).await
}

#[tauri::command]
pub async fn session_close(
    manager: State<'_, SessionManager>,
    id: SessionId,
) -> Result<(), AppError> {
    manager.send(id, SessionCommand::Close).await
}

#[tauri::command]
pub fn session_answer_host_key(
    manager: State<'_, SessionManager>,
    id: SessionId,
    accept: bool,
) -> Result<(), AppError> {
    manager.answer_host_key(id, accept)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(host: &str, port: u16, user: &str) -> SshConnectRequest {
        SshConnectRequest {
            host: host.into(),
            port,
            username: user.into(),
            auth: AuthRequest::Password {
                password: "x".into(),
            },
            legacy: false,
        }
    }

    #[test]
    fn rejects_invalid_connection_input() {
        assert!(request("", 22, "root").into_params().is_err());
        assert!(request("bad host", 22, "root").into_params().is_err());
        assert!(request("10.0.0.1", 0, "root").into_params().is_err());
        assert!(request("10.0.0.1", 22, "  ").into_params().is_err());
        assert!(request(" 10.0.0.1 ", 22, "root").into_params().is_ok());
    }

    #[test]
    fn deserializes_frontend_payload() {
        let json = r#"{"host":"r1","port":22,"username":"admin",
            "auth":{"method":"keyFile","path":"~/.ssh/id_ed25519","passphrase":""}}"#;
        let params = serde_json::from_str::<SshConnectRequest>(json)
            .unwrap()
            .into_params()
            .unwrap();
        match params.auth {
            SshAuth::KeyFile { path, passphrase } => {
                assert!(!path.starts_with("~"));
                assert!(passphrase.is_none(), "empty passphrase means none");
            }
            other => panic!("wrong auth method: {other:?}"),
        }
    }

    #[test]
    fn debug_output_never_contains_the_password() {
        let params = SshConnectRequest {
            auth: AuthRequest::Password {
                password: "s3cr3t".into(),
            },
            ..request("r1", 22, "admin")
        }
        .into_params()
        .unwrap();
        assert!(!format!("{params:?}").contains("s3cr3t"));
    }
}
