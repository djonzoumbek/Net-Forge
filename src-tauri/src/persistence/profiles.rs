//! Connection profiles and folders.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::db_error;
use crate::errors::AppError;
use crate::security::secrets::{Secrets, profile_account};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    Password,
    KeyFile,
    Agent,
}

impl AuthMethod {
    fn as_str(self) -> &'static str {
        match self {
            AuthMethod::Password => "password",
            AuthMethod::KeyFile => "keyFile",
            AuthMethod::Agent => "agent",
        }
    }

    fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "password" => Ok(AuthMethod::Password),
            "keyFile" => Ok(AuthMethod::KeyFile),
            "agent" => Ok(AuthMethod::Agent),
            other => Err(AppError::Storage(format!(
                "méthode d'authentification inconnue : {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
}

/// What the UI sees. The keyring entry name stays internal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: i64,
    pub folder_id: Option<i64>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub key_path: Option<String>,
    /// A password (or key passphrase) is saved in the system keyring.
    pub has_saved_secret: bool,
    pub legacy_mode: bool,
}

#[derive(Debug, Serialize)]
pub struct Library {
    pub folders: Vec<Folder>,
    pub profiles: Vec<Profile>,
}

/// What to do with the saved password/passphrase when saving a profile.
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SecretChange {
    Keep,
    Set { value: String },
    Clear,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    pub folder_id: Option<i64>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub key_path: Option<String>,
    pub legacy_mode: bool,
    pub secret: SecretChange,
}

impl ProfileInput {
    fn normalized(mut self) -> Result<Self, AppError> {
        self.name = self.name.trim().to_string();
        self.host = self.host.trim().to_string();
        self.username = self.username.trim().to_string();
        self.key_path = self
            .key_path
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty());
        let invalid = |m: &str| Err(AppError::InvalidInput(m.into()));
        if self.name.is_empty() || self.name.len() > 100 {
            return invalid("le nom du profil est obligatoire (100 caractères max).");
        }
        if self.host.is_empty() || self.host.len() > 255 || self.host.contains(char::is_whitespace)
        {
            return invalid("nom d'hôte invalide.");
        }
        if self.port == 0 {
            return invalid("le port doit être entre 1 et 65535.");
        }
        if self.username.is_empty() || self.username.len() > 255 {
            return invalid("nom d'utilisateur invalide.");
        }
        match self.auth_method {
            AuthMethod::KeyFile if self.key_path.is_none() => {
                return invalid("chemin de la clé privée manquant.");
            }
            AuthMethod::KeyFile => {}
            // Only key files have a path; the agent never has a saved secret.
            AuthMethod::Password => self.key_path = None,
            AuthMethod::Agent => {
                self.key_path = None;
                self.secret = SecretChange::Clear;
            }
        }
        if let SecretChange::Set { value } = &self.secret
            && value.is_empty()
        {
            self.secret = SecretChange::Clear;
        }
        Ok(self)
    }
}

const PROFILE_COLUMNS: &str =
    "id, folder_id, name, host, port, username, auth_method, key_path, secret_ref, legacy_mode";

/// Profile plus the internal keyring reference.
struct StoredProfile {
    profile: Profile,
    secret_ref: Option<String>,
}

fn read_profile(row: &Row) -> rusqlite::Result<(StoredProfile, String)> {
    let secret_ref: Option<String> = row.get(8)?;
    let auth_method: String = row.get(6)?;
    let profile = Profile {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        name: row.get(2)?,
        host: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        // Placeholder, parsed by the caller (rusqlite closures cannot return AppError).
        auth_method: AuthMethod::Password,
        key_path: row.get(7)?,
        has_saved_secret: secret_ref.is_some(),
        legacy_mode: row.get(9)?,
    };
    Ok((
        StoredProfile {
            profile,
            secret_ref,
        },
        auth_method,
    ))
}

fn finish(row: (StoredProfile, String)) -> Result<StoredProfile, AppError> {
    let (mut stored, auth_method) = row;
    stored.profile.auth_method = AuthMethod::parse(&auth_method)?;
    Ok(stored)
}

fn get_stored(conn: &Connection, id: i64) -> Result<StoredProfile, AppError> {
    let row = conn
        .query_row(
            &format!("SELECT {PROFILE_COLUMNS} FROM profiles WHERE id = ?1"),
            params![id],
            read_profile,
        )
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| AppError::NotFound(format!("profil {id}")))?;
    finish(row)
}

pub fn list(conn: &Connection) -> Result<Library, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, name, parent_id FROM folders ORDER BY name COLLATE NOCASE")
        .map_err(db_error)?;
    let folders = stmt
        .query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_id: r.get(2)?,
            })
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;

    let mut stmt = conn
        .prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM profiles ORDER BY name COLLATE NOCASE"
        ))
        .map_err(db_error)?;
    let profiles = stmt
        .query_map([], read_profile)
        .map_err(db_error)?
        .map(|row| finish(row.map_err(db_error)?).map(|s| s.profile))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Library { folders, profiles })
}

pub fn get(conn: &Connection, id: i64) -> Result<Profile, AppError> {
    get_stored(conn, id).map(|s| s.profile)
}

/// Profile + its saved secret (if any), to open a session.
pub fn get_with_secret(
    conn: &Connection,
    secrets: &Secrets,
    id: i64,
) -> Result<(Profile, Option<zeroize::Zeroizing<String>>), AppError> {
    let stored = get_stored(conn, id)?;
    let secret = match &stored.secret_ref {
        Some(account) => secrets.get(account)?,
        None => None,
    };
    Ok((stored.profile, secret))
}

fn check_folder(conn: &Connection, folder_id: Option<i64>) -> Result<(), AppError> {
    if let Some(id) = folder_id {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM folders WHERE id = ?1)",
                params![id],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if !exists {
            return Err(AppError::NotFound(format!("dossier {id}")));
        }
    }
    Ok(())
}

pub fn create(
    conn: &mut Connection,
    secrets: &Secrets,
    input: ProfileInput,
) -> Result<Profile, AppError> {
    let input = input.normalized()?;
    check_folder(conn, input.folder_id)?;
    // The keyring is not transactional: write to it inside the SQL transaction, so
    // that a keyring failure rolls the new profile back.
    let tx = conn.transaction().map_err(db_error)?;
    tx.execute(
        "INSERT INTO profiles (folder_id, name, host, port, username, auth_method, key_path, legacy_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.folder_id,
            input.name,
            input.host,
            input.port,
            input.username,
            input.auth_method.as_str(),
            input.key_path,
            input.legacy_mode
        ],
    )
    .map_err(db_error)?;
    let id = tx.last_insert_rowid();
    if let SecretChange::Set { value } = &input.secret {
        let account = profile_account(id);
        secrets.set(&account, value)?;
        tx.execute(
            "UPDATE profiles SET secret_ref = ?1 WHERE id = ?2",
            params![account, id],
        )
        .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;
    get(conn, id)
}

pub fn update(
    conn: &mut Connection,
    secrets: &Secrets,
    id: i64,
    input: ProfileInput,
) -> Result<Profile, AppError> {
    let input = input.normalized()?;
    check_folder(conn, input.folder_id)?;
    let current = get_stored(conn, id)?;
    let tx = conn.transaction().map_err(db_error)?;
    tx.execute(
        "UPDATE profiles SET folder_id = ?1, name = ?2, host = ?3, port = ?4, username = ?5,
             auth_method = ?6, key_path = ?7, legacy_mode = ?8, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?9",
        params![
            input.folder_id,
            input.name,
            input.host,
            input.port,
            input.username,
            input.auth_method.as_str(),
            input.key_path,
            input.legacy_mode,
            id
        ],
    )
    .map_err(db_error)?;
    match &input.secret {
        SecretChange::Keep => {}
        SecretChange::Set { value } => {
            let account = profile_account(id);
            secrets.set(&account, value)?;
            tx.execute(
                "UPDATE profiles SET secret_ref = ?1 WHERE id = ?2",
                params![account, id],
            )
            .map_err(db_error)?;
        }
        SecretChange::Clear => {
            if let Some(account) = &current.secret_ref {
                secrets.delete(account)?;
            }
            tx.execute(
                "UPDATE profiles SET secret_ref = NULL WHERE id = ?1",
                params![id],
            )
            .map_err(db_error)?;
        }
    }
    tx.commit().map_err(db_error)?;
    get(conn, id)
}

/// Saves a secret typed at connection time (after a successful login).
pub fn save_secret(
    conn: &Connection,
    secrets: &Secrets,
    id: i64,
    value: &str,
) -> Result<(), AppError> {
    let current = get_stored(conn, id)?;
    if current.profile.auth_method == AuthMethod::Agent || value.is_empty() {
        return Err(AppError::InvalidInput(
            "rien à enregistrer pour ce profil.".into(),
        ));
    }
    let account = profile_account(id);
    secrets.set(&account, value)?;
    conn.execute(
        "UPDATE profiles SET secret_ref = ?1 WHERE id = ?2",
        params![account, id],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn delete(conn: &Connection, secrets: &Secrets, id: i64) -> Result<(), AppError> {
    let current = get_stored(conn, id)?;
    conn.execute("DELETE FROM profiles WHERE id = ?1", params![id])
        .map_err(db_error)?;
    // The profile is gone either way; a leftover keyring entry is only logged.
    if let Some(account) = current.secret_ref
        && let Err(e) = secrets.delete(&account)
    {
        tracing::warn!(profile = id, "keyring entry not deleted: {e}");
    }
    Ok(())
}

fn folder_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(AppError::InvalidInput("nom de dossier invalide.".into()));
    }
    Ok(name.to_string())
}

pub fn create_folder(
    conn: &Connection,
    name: &str,
    parent_id: Option<i64>,
) -> Result<Folder, AppError> {
    let name = folder_name(name)?;
    check_folder(conn, parent_id)?;
    conn.execute(
        "INSERT INTO folders (name, parent_id) VALUES (?1, ?2)",
        params![name, parent_id],
    )
    .map_err(db_error)?;
    Ok(Folder {
        id: conn.last_insert_rowid(),
        name,
        parent_id,
    })
}

pub fn rename_folder(conn: &Connection, id: i64, name: &str) -> Result<(), AppError> {
    let name = folder_name(name)?;
    let changed = conn
        .execute(
            "UPDATE folders SET name = ?1 WHERE id = ?2",
            params![name, id],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("dossier {id}")));
    }
    Ok(())
}

/// Sub-folders are deleted; profiles inside are moved to the root (see migration).
pub fn delete_folder(conn: &Connection, id: i64) -> Result<(), AppError> {
    conn.execute("DELETE FROM folders WHERE id = ?1", params![id])
        .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Database;

    fn input(name: &str, secret: SecretChange) -> ProfileInput {
        ProfileInput {
            folder_id: None,
            name: name.into(),
            host: "10.0.0.1".into(),
            port: 22,
            username: "admin".into(),
            auth_method: AuthMethod::Password,
            key_path: None,
            legacy_mode: false,
            secret,
        }
    }

    async fn with_db<T: Send + 'static>(
        f: impl FnOnce(&mut Connection, &Secrets) -> Result<T, AppError> + Send + 'static,
    ) -> T {
        let db = Database::open_in_memory().unwrap();
        db.run(move |conn| f(conn, &Secrets::in_memory()))
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn crud_with_secret_in_keyring_only() {
        with_db(|conn, secrets| {
            let p = create(
                conn,
                secrets,
                input("R1", SecretChange::Set { value: "pw".into() }),
            )?;
            assert!(p.has_saved_secret);
            let ref_in_db: String = conn
                .query_row("SELECT secret_ref FROM profiles", [], |r| r.get(0))
                .unwrap();
            assert_eq!(ref_in_db, profile_account(p.id));
            assert_eq!(
                get_with_secret(conn, secrets, p.id)?.1.unwrap().as_str(),
                "pw"
            );

            let p = update(conn, secrets, p.id, input("R1 bis", SecretChange::Keep))?;
            assert_eq!(p.name, "R1 bis");
            assert!(p.has_saved_secret, "Keep keeps the secret");

            let p = update(conn, secrets, p.id, input("R1 bis", SecretChange::Clear))?;
            assert!(!p.has_saved_secret);
            assert!(secrets.get(&profile_account(p.id))?.is_none());

            delete(conn, secrets, p.id)?;
            assert!(list(conn)?.profiles.is_empty());
            Ok(())
        })
        .await;
    }

    #[tokio::test]
    async fn switching_to_agent_clears_the_secret() {
        with_db(|conn, secrets| {
            let p = create(
                conn,
                secrets,
                input("R1", SecretChange::Set { value: "pw".into() }),
            )?;
            let mut agent = input("R1", SecretChange::Keep);
            agent.auth_method = AuthMethod::Agent;
            let p = update(conn, secrets, p.id, agent)?;
            assert!(!p.has_saved_secret);
            assert!(secrets.get(&profile_account(p.id))?.is_none());
            Ok(())
        })
        .await;
    }

    #[tokio::test]
    async fn deleting_a_folder_keeps_its_profiles() {
        with_db(|conn, secrets| {
            let lab = create_folder(conn, "Lab", None)?;
            let sub = create_folder(conn, "Cisco", Some(lab.id))?;
            let mut i = input("R1", SecretChange::Keep);
            i.folder_id = Some(sub.id);
            let p = create(conn, secrets, i)?;
            delete_folder(conn, lab.id)?;
            let library = list(conn)?;
            assert!(library.folders.is_empty(), "sub-folders are deleted too");
            assert_eq!(library.profiles[0].id, p.id);
            assert_eq!(library.profiles[0].folder_id, None, "profile moved to root");
            Ok(())
        })
        .await;
    }

    #[tokio::test]
    async fn rejects_invalid_profiles() {
        with_db(|conn, secrets| {
            assert!(create(conn, secrets, input(" ", SecretChange::Keep)).is_err());
            let mut no_key = input("R1", SecretChange::Keep);
            no_key.auth_method = AuthMethod::KeyFile;
            assert!(create(conn, secrets, no_key).is_err());
            let mut bad_folder = input("R1", SecretChange::Keep);
            bad_folder.folder_id = Some(42);
            assert!(create(conn, secrets, bad_folder).is_err());
            Ok(())
        })
        .await;
    }

    /// Criterion "no secret in the SQLite database", checked on the actual file
    /// bytes (database + WAL journal), not through SQL.
    #[tokio::test]
    async fn database_file_never_contains_the_secret() {
        let dir = std::env::temp_dir().join(format!("netforge-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("netforge.db");
        let _ = std::fs::remove_file(&path);
        let secret = "Sup3r-S3cret-Passw0rd!";
        {
            let db = Database::open(&path).unwrap();
            db.run(move |conn| {
                let secrets = Secrets::in_memory();
                let p = create(
                    conn,
                    &secrets,
                    input(
                        "R1",
                        SecretChange::Set {
                            value: secret.into(),
                        },
                    ),
                )?;
                update(
                    conn,
                    &secrets,
                    p.id,
                    input(
                        "R2",
                        SecretChange::Set {
                            value: secret.into(),
                        },
                    ),
                )
            })
            .await
            .unwrap();
        }
        for entry in std::fs::read_dir(&dir).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(
                !bytes.windows(secret.len()).any(|w| w == secret.as_bytes()),
                "secret found in a database file"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
