//! Secrets (passwords, key passphrases) stored in the system keyring: GNOME
//! Keyring / KWallet through the freedesktop Secret Service API on Linux, the
//! Credential Manager on Windows.
//!
//! The database only stores the *name* of the keyring entry, never the secret.
//! Every call here is blocking (D-Bus round trip, possibly an unlock prompt):
//! callers must run them on a blocking thread.

use std::sync::Arc;

use keyring_core::api::CredentialStore;
use zeroize::Zeroizing;

use crate::errors::AppError;

/// Groups NetForge entries in the keyring (visible in "Passwords and Keys").
const SERVICE: &str = "NetForge";

enum Backend {
    Keyring(Arc<CredentialStore>),
    /// No Secret Service on this system: NetForge works, but cannot save secrets.
    Unavailable(String),
    #[cfg(test)]
    Memory(std::sync::Mutex<std::collections::HashMap<String, String>>),
}

pub struct Secrets {
    backend: Backend,
}

#[cfg(target_os = "linux")]
fn system_store() -> keyring_core::Result<Arc<CredentialStore>> {
    let store = dbus_secret_service_keyring_store::Store::new()?;
    Ok(store)
}

#[cfg(windows)]
fn system_store() -> keyring_core::Result<Arc<CredentialStore>> {
    let store = windows_native_keyring_store::Store::new()?;
    Ok(store)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn system_store() -> keyring_core::Result<Arc<CredentialStore>> {
    Err(keyring_core::Error::NotSupportedByStore(
        "aucun trousseau pris en charge sur ce système".into(),
    ))
}

/// Name of the keyring entry holding a profile's password or passphrase.
pub fn profile_account(profile_id: i64) -> String {
    format!("profile-{profile_id}")
}

impl Secrets {
    pub fn system() -> Self {
        let backend = match system_store() {
            Ok(store) => Backend::Keyring(store),
            Err(e) => {
                tracing::warn!("system keyring unavailable: {e}");
                Backend::Unavailable(e.to_string())
            }
        };
        Self { backend }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            backend: Backend::Memory(Default::default()),
        }
    }

    pub fn set(&self, account: &str, secret: &str) -> Result<(), AppError> {
        match &self.backend {
            Backend::Keyring(store) => store
                .build(SERVICE, account, None)
                .and_then(|entry| entry.set_password(secret))
                .map_err(keyring_error),
            Backend::Unavailable(reason) => Err(unavailable(reason)),
            #[cfg(test)]
            Backend::Memory(map) => {
                map.lock().unwrap().insert(account.into(), secret.into());
                Ok(())
            }
        }
    }

    pub fn get(&self, account: &str) -> Result<Option<Zeroizing<String>>, AppError> {
        match &self.backend {
            Backend::Keyring(store) => {
                match store
                    .build(SERVICE, account, None)
                    .and_then(|e| e.get_password())
                {
                    Ok(secret) => Ok(Some(Zeroizing::new(secret))),
                    Err(keyring_core::Error::NoEntry) => Ok(None),
                    Err(e) => Err(keyring_error(e)),
                }
            }
            Backend::Unavailable(reason) => Err(unavailable(reason)),
            #[cfg(test)]
            Backend::Memory(map) => Ok(map
                .lock()
                .unwrap()
                .get(account)
                .cloned()
                .map(Zeroizing::new)),
        }
    }

    /// Deleting a missing entry is not an error.
    pub fn delete(&self, account: &str) -> Result<(), AppError> {
        match &self.backend {
            Backend::Keyring(store) => {
                match store
                    .build(SERVICE, account, None)
                    .and_then(|e| e.delete_credential())
                {
                    Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
                    Err(e) => Err(keyring_error(e)),
                }
            }
            // Nothing can have been saved.
            Backend::Unavailable(_) => Ok(()),
            #[cfg(test)]
            Backend::Memory(map) => {
                map.lock().unwrap().remove(account);
                Ok(())
            }
        }
    }
}

fn keyring_error(e: keyring_core::Error) -> AppError {
    AppError::Keyring(match e {
        keyring_core::Error::NoStorageAccess(_) => {
            "le trousseau est verrouillé ou l'accès a été refusé.".into()
        }
        other => other.to_string(),
    })
}

fn unavailable(reason: &str) -> AppError {
    AppError::Keyring(format!(
        "aucun trousseau système disponible ({reason}). Le mot de passe sera demandé à chaque connexion."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_delete_round_trip() {
        let secrets = Secrets::in_memory();
        let account = profile_account(3);
        assert!(secrets.get(&account).unwrap().is_none());
        secrets.set(&account, "s3cr3t").unwrap();
        assert_eq!(secrets.get(&account).unwrap().unwrap().as_str(), "s3cr3t");
        secrets.delete(&account).unwrap();
        secrets.delete(&account).unwrap();
        assert!(secrets.get(&account).unwrap().is_none());
    }

    /// Real keyring round trip. Run manually: `cargo test keyring -- --ignored`.
    #[test]
    #[ignore = "uses the real system keyring (GNOME Keyring must be unlocked)"]
    fn system_keyring_round_trip() {
        let secrets = Secrets::system();
        let account = "netforge-test-entry";
        secrets.set(account, "test-value").unwrap();
        assert_eq!(
            secrets.get(account).unwrap().unwrap().as_str(),
            "test-value"
        );
        secrets.delete(account).unwrap();
        assert!(secrets.get(account).unwrap().is_none());
    }
}
