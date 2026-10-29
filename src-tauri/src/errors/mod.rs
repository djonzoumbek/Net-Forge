//! Central error type, converted into `{ code, message }` for the frontend.

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

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

    #[error("Clé d'hôte refusée : connexion annulée.")]
    HostKeyRejected,

    #[error(
        "Aucun algorithme {kind} commun avec le serveur. Le serveur propose : {server}. \
         Il s'agit probablement d'un équipement ancien : le mode compatibilité (phase 3) sera nécessaire."
    )]
    NoCommonAlgorithm { kind: String, server: String },

    #[error("Session {0} introuvable (déjà fermée ?).")]
    SessionNotFound(u64),

    #[error("Erreur SSH : {0}")]
    Ssh(String),
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
            AppError::HostKeyRejected => "host_key_rejected",
            AppError::NoCommonAlgorithm { .. } => "no_common_algorithm",
            AppError::SessionNotFound(_) => "session_not_found",
            AppError::Ssh(_) => "ssh",
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
