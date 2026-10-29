//! Central error type, converted into `{ code, message }` for the frontend.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

use crate::security::known_hosts::HostKeyMismatch;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Entrée invalide : {0}")]
    InvalidInput(String),

    #[error(
        "Nom d'hôte inconnu : impossible de résoudre « {0} » (vérifie l'orthographe ou le DNS)."
    )]
    HostNotFound(String),

    #[error("Hôte injoignable : {0}")]
    Unreachable(String),

    #[error("Délai dépassé : {0}")]
    Timeout(String),

    #[error("Authentification refusée par le serveur (méthodes encore acceptées : {0}).")]
    AuthFailed(String),

    #[error("Clé privée : {0}")]
    PrivateKey(String),

    #[error("La clé {0} est protégée par une passphrase : saisis-la.")]
    PassphraseRequired(String),

    #[error("Mot de passe requis pour ce profil.")]
    PasswordRequired,

    #[error("Agent SSH : {0}")]
    Agent(String),

    #[error(
        "ATTENTION : la clé d'hôte de {}:{} a changé ! Connexion bloquée. \
         Cela peut être une réinstallation légitime… ou une attaque de l'homme du milieu.",
        .0.host, .0.port
    )]
    HostKeyChanged(Box<HostKeyMismatch>),

    #[error("La clé d'hôte présentée est marquée comme révoquée (@revoked) : connexion refusée.")]
    HostKeyRevoked,

    #[error("Clé d'hôte refusée : connexion annulée.")]
    HostKeyRejected,

    #[error(
        "Aucun algorithme {kind} commun avec le serveur. Le serveur propose : {server}. {}",
        if *.legacy_enabled {
            "Le mode compatibilité est déjà actif : ces algorithmes ne sont pas pris en charge par NetForge."
        } else {
            "Équipement ancien ? Active le « mode compatibilité équipements anciens » dans le profil."
        }
    )]
    NoCommonAlgorithm {
        kind: String,
        server: String,
        legacy_enabled: bool,
    },

    #[error("Session {0} introuvable (déjà fermée ?).")]
    SessionNotFound(u64),

    #[error("Élément introuvable : {0}.")]
    NotFound(String),

    #[error("Stockage : {0}")]
    Storage(String),

    #[error("Trousseau système : {0}")]
    Keyring(String),

    #[error("Erreur SSH : {0}")]
    Ssh(String),

    #[error("Transfert de fichiers (SFTP) : {0}")]
    Sftp(String),

    #[error("Fichier local : {0}")]
    LocalFile(String),

    /// Upload refused to replace a file: the UI offers "Replace".
    #[error("{0} existe déjà sur le serveur.")]
    RemoteExists(String),

    #[error("Transfert annulé.")]
    Cancelled,

    #[error("Port série : {0}")]
    Serial(String),

    /// The message explains the `dialout` group fix for this machine.
    #[error("Accès au port série refusé. {0}")]
    SerialPermission(String),
}

impl AppError {
    /// Stable machine-readable code, so the UI can react without parsing messages.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::InvalidInput(_) => "invalid_input",
            AppError::HostNotFound(_) => "host_not_found",
            AppError::Unreachable(_) => "unreachable",
            AppError::Timeout(_) => "timeout",
            AppError::AuthFailed(_) => "auth_failed",
            AppError::PrivateKey(_) => "private_key",
            AppError::PassphraseRequired(_) => "passphrase_required",
            AppError::PasswordRequired => "password_required",
            AppError::Agent(_) => "agent",
            AppError::HostKeyRejected => "host_key_rejected",
            AppError::HostKeyChanged(_) => "host_key_changed",
            AppError::HostKeyRevoked => "host_key_revoked",
            AppError::NoCommonAlgorithm { .. } => "no_common_algorithm",
            AppError::SessionNotFound(_) => "session_not_found",
            AppError::NotFound(_) => "not_found",
            AppError::Storage(_) => "storage",
            AppError::Keyring(_) => "keyring",
            AppError::Ssh(_) => "ssh",
            AppError::Sftp(_) => "sftp",
            AppError::LocalFile(_) => "local_file",
            AppError::RemoteExists(_) => "remote_exists",
            AppError::Cancelled => "cancelled",
            AppError::Serial(_) => "serial",
            AppError::SerialPermission(_) => "serial_permission",
        }
    }
}

/// Tauri commands return `Result<T, AppError>`: the error must be serializable.
/// We only send the code and the human-readable message, never internal details.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

impl From<russh::Error> for AppError {
    fn from(err: russh::Error) -> Self {
        use russh::Error as E;
        match err {
            E::NoCommonAlgo { kind, theirs, .. } => AppError::NoCommonAlgorithm {
                kind: format!("{kind:?}").to_lowercase(),
                server: theirs.join(", "),
                legacy_enabled: false,
            },
            E::UnknownKey => AppError::HostKeyRejected,
            E::ConnectionTimeout | E::KeepaliveTimeout | E::InactivityTimeout => {
                AppError::Timeout("le serveur ne répond plus.".into())
            }
            E::HUP | E::Disconnect => {
                AppError::Unreachable("la connexion a été fermée par le serveur.".into())
            }
            E::IO(io) => AppError::Unreachable(io.to_string()),
            E::Keys(k) => AppError::PrivateKey(k.to_string()),
            other => AppError::Ssh(other.to_string()),
        }
    }
}

impl From<russh_sftp::client::error::Error> for AppError {
    fn from(err: russh_sftp::client::error::Error) -> Self {
        use russh_sftp::client::error::Error as E;
        use russh_sftp::protocol::StatusCode as S;
        AppError::Sftp(match err {
            E::Status(status) => match status.status_code {
                S::NoSuchFile => "fichier ou dossier introuvable sur le serveur.".into(),
                S::PermissionDenied => "permission refusée par le serveur.".into(),
                S::Failure if !status.error_message.is_empty() => {
                    format!(
                        "le serveur a refusé l'opération ({}).",
                        status.error_message
                    )
                }
                S::NoConnection | S::ConnectionLost => "la connexion SSH a été perdue.".into(),
                S::OpUnsupported => "opération non prise en charge par ce serveur.".into(),
                code => format!("{code} ({}).", status.error_message),
            },
            E::Timeout => "le serveur ne répond plus.".into(),
            other => other.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_code_and_message_only() {
        let json = serde_json::to_value(AppError::SessionNotFound(7)).unwrap();
        assert_eq!(json["code"], "session_not_found");
        assert!(json["message"].as_str().unwrap().contains('7'));
        assert_eq!(json.as_object().unwrap().len(), 2);
    }
}
