//! Tauri commands: thin layer that validates input and delegates to `sessions`.

use std::path::PathBuf;

use serde::Deserialize;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};
use zeroize::Zeroizing;

use crate::errors::AppError;
use crate::sessions::{SessionCommand, SessionEvent, SessionId, SessionManager, SessionSinks};
use crate::transport::TermSize;
use crate::transport::ssh::{SshAuth, SshParams};

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
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectRequest {
    host: String,
    port: u16,
    username: String,
    auth: AuthRequest,
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
        };
        Ok(SshParams {
            host,
            port: self.port,
            username,
            auth,
        })
    }
}

/// `~/.ssh/id_ed25519` is what users type; the OS does not expand `~` for us.
fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(path),
    }
}

/// Starts a session and returns its id immediately. Progress (host key prompt,
/// connected, failure) arrives on `on_event`; terminal output on `on_data`.
#[tauri::command]
pub fn session_open_ssh(
    manager: State<'_, SessionManager>,
    request: SshConnectRequest,
    size: TermSize,
    on_data: Channel<InvokeResponseBody>,
    on_event: Channel<SessionEvent>,
) -> Result<SessionId, AppError> {
    let params = request.into_params()?;
    let size = size.validate()?;
    tracing::info!(host = %params.host, port = params.port, "opening SSH session");
    let sinks = SessionSinks {
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
    };
    Ok(manager.open_ssh(params, size, sinks))
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
            SshAuth::Password(_) => panic!("wrong auth method"),
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
