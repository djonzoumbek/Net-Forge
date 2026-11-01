//! Connection profiles and folders.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::db_error;
use crate::errors::AppError;
use crate::security::secrets::{Secrets, profile_account};
use crate::transport::serial::{FlowControl, Parity, SerialSettings};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Protocol {
    Ssh,
    Telnet,
    /// `host` is the device path (/dev/ttyUSB0), `port` is 0.
    Serial,
}

impl Protocol {
    fn as_str(self) -> &'static str {
        match self {
            Protocol::Ssh => "ssh",
            Protocol::Telnet => "telnet",
            Protocol::Serial => "serial",
        }
    }

    fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "ssh" => Ok(Protocol::Ssh),
            "telnet" => Ok(Protocol::Telnet),
            "serial" => Ok(Protocol::Serial),
            other => Err(AppError::Storage(format!("protocole inconnu : {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMethod {
    /// Telnet: the device asks for credentials itself, in the terminal.
    None,
    Password,
    KeyFile,
    Agent,
}

impl AuthMethod {
    fn as_str(self) -> &'static str {
        match self {
            AuthMethod::None => "none",
            AuthMethod::Password => "password",
            AuthMethod::KeyFile => "keyFile",
            AuthMethod::Agent => "agent",
        }
    }

    fn parse(s: &str) -> Result<Self, AppError> {
        match s {
            "none" => Ok(AuthMethod::None),
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
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub key_path: Option<String>,
    /// A password (or key passphrase) is saved in the system keyring.
    pub has_saved_secret: bool,
    pub legacy_mode: bool,
    /// Line settings (serial profiles; defaults elsewhere).
    pub serial: SerialSettings,
    #[serde(flatten)]
    pub options: SessionOptions,
}

/// Session recording to a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogMode {
    Off,
    /// Plain text: escape sequences removed, readable in any editor.
    Text,
    /// Exact bytes received, replayable with `cat`.
    Raw,
}

impl LogMode {
    fn as_str(self) -> &'static str {
        match self {
            LogMode::Off => "off",
            LogMode::Text => "text",
            LogMode::Raw => "raw",
        }
    }

    fn parse(s: &str) -> rusqlite::Result<Self> {
        match s {
            "off" => Ok(LogMode::Off),
            "text" => Ok(LogMode::Text),
            "raw" => Ok(LogMode::Raw),
            other => Err(rusqlite::Error::InvalidColumnType(
                15,
                format!("session_log={other}"),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

/// How a session of this profile behaves once connected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOptions {
    /// Keepalive interval in seconds, 0 = disabled.
    pub keepalive_secs: u32,
    /// Reconnect when the connection is lost (never after a normal exit).
    pub auto_reconnect: bool,
    /// Answer `--More--`-like pager prompts with a space.
    pub pager_auto_reply: bool,
    /// One command per line, sent at the first prompt. Stored in clear.
    pub login_commands: String,
    /// Record the session in a file (may contain secrets typed in the terminal).
    pub session_log: LogMode,
}

impl SessionOptions {
    pub fn login_command_lines(&self) -> Vec<String> {
        self.login_commands
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.keepalive_secs > 3600 {
            return Err(AppError::InvalidInput(
                "keepalive : 3600 secondes maximum.".into(),
            ));
        }
        if self.login_commands.len() > 4000 || self.login_commands.lines().count() > 50 {
            return Err(AppError::InvalidInput(
                "commandes de connexion : 50 lignes et 4000 caractères maximum.".into(),
            ));
        }
        Ok(())
    }
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
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub key_path: Option<String>,
    pub legacy_mode: bool,
    pub secret: SecretChange,
    #[serde(default)]
    pub serial: SerialSettings,
    #[serde(flatten)]
    pub options: SessionOptions,
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
        if self.username.len() > 255 {
            return invalid("nom d'utilisateur trop long.");
        }
        self.options.validate()?;
        if self.protocol == Protocol::Serial {
            if !crate::platform::is_serial_device(&self.host) {
                return Err(AppError::InvalidInput(format!(
                    "port série invalide (ex. {}).",
                    crate::platform::SERIAL_EXAMPLE
                )));
            }
            self.serial = self.serial.validate()?;
            // A local cable: no network port, no authentication, no keepalive or
            // reconnection (the line has no notion of a lost connection to retry).
            self.port = 0;
            self.username.clear();
            self.auth_method = AuthMethod::None;
            self.key_path = None;
            self.legacy_mode = false;
            self.secret = SecretChange::Clear;
            self.options.keepalive_secs = 0;
            self.options.auto_reconnect = false;
            return Ok(self);
        }
        if self.host.is_empty() || self.host.len() > 255 || self.host.contains(char::is_whitespace)
        {
            return invalid("nom d'hôte invalide.");
        }
        if self.port == 0 {
            return invalid("le port doit être entre 1 et 65535.");
        }
        // Line settings only matter for serial profiles.
        self.serial = SerialSettings::default();
        if self.protocol == Protocol::Telnet {
            // No SSH authentication, secret or algorithms: the device asks for its
            // credentials in the terminal.
            self.auth_method = AuthMethod::None;
            self.key_path = None;
            self.legacy_mode = false;
            self.secret = SecretChange::Clear;
            return Ok(self);
        }
        if self.username.is_empty() {
            return invalid("le nom d'utilisateur est obligatoire en SSH.");
        }
        match self.auth_method {
            AuthMethod::None => return invalid("choisis une méthode d'authentification SSH."),
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

const PROFILE_COLUMNS: &str = "id, folder_id, name, host, port, username, auth_method, \
     key_path, secret_ref, legacy_mode, protocol, keepalive_secs, auto_reconnect, \
     pager_auto_reply, login_commands, session_log, serial_baud_rate, serial_data_bits, \
     serial_parity, serial_stop_bits, serial_flow_control";

/// Text columns parsed after the row is read (rusqlite closures cannot return
/// `AppError`).
struct RawRow {
    stored: StoredProfile,
    auth_method: String,
    protocol: String,
}

/// Profile plus the internal keyring reference.
struct StoredProfile {
    profile: Profile,
    secret_ref: Option<String>,
}

fn read_profile(row: &Row) -> rusqlite::Result<RawRow> {
    let secret_ref: Option<String> = row.get(8)?;
    let profile = Profile {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        name: row.get(2)?,
        // Placeholders, replaced in `finish`.
        protocol: Protocol::Ssh,
        host: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        auth_method: AuthMethod::None,
        key_path: row.get(7)?,
        has_saved_secret: secret_ref.is_some(),
        legacy_mode: row.get(9)?,
        serial: SerialSettings {
            baud_rate: row.get(16)?,
            data_bits: row.get(17)?,
            parity: parse_parity(&row.get::<_, String>(18)?)?,
            stop_bits: row.get(19)?,
            flow_control: parse_flow(&row.get::<_, String>(20)?)?,
        },
        options: SessionOptions {
            keepalive_secs: row.get(11)?,
            auto_reconnect: row.get(12)?,
            pager_auto_reply: row.get(13)?,
            login_commands: row.get(14)?,
            session_log: LogMode::parse(&row.get::<_, String>(15)?)?,
        },
    };
    Ok(RawRow {
        stored: StoredProfile {
            profile,
            secret_ref,
        },
        auth_method: row.get(6)?,
        protocol: row.get(10)?,
    })
}

fn column_error(index: usize, value: &str) -> rusqlite::Error {
    rusqlite::Error::InvalidColumnType(index, value.into(), rusqlite::types::Type::Text)
}

fn parity_str(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "none",
        Parity::Odd => "odd",
        Parity::Even => "even",
    }
}

fn parse_parity(s: &str) -> rusqlite::Result<Parity> {
    match s {
        "none" => Ok(Parity::None),
        "odd" => Ok(Parity::Odd),
        "even" => Ok(Parity::Even),
        other => Err(column_error(18, other)),
    }
}

fn flow_str(flow: FlowControl) -> &'static str {
    match flow {
        FlowControl::None => "none",
        FlowControl::Software => "software",
        FlowControl::Hardware => "hardware",
    }
}

fn parse_flow(s: &str) -> rusqlite::Result<FlowControl> {
    match s {
        "none" => Ok(FlowControl::None),
        "software" => Ok(FlowControl::Software),
        "hardware" => Ok(FlowControl::Hardware),
        other => Err(column_error(20, other)),
    }
}

fn finish(row: RawRow) -> Result<StoredProfile, AppError> {
    let mut stored = row.stored;
    stored.profile.auth_method = AuthMethod::parse(&row.auth_method)?;
    stored.profile.protocol = Protocol::parse(&row.protocol)?;
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
        "INSERT INTO profiles (folder_id, name, host, port, username, auth_method, key_path, legacy_mode, protocol)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            input.folder_id,
            input.name,
            input.host,
            input.port,
            input.username,
            input.auth_method.as_str(),
            input.key_path,
            input.legacy_mode,
            input.protocol.as_str()
        ],
    )
    .map_err(db_error)?;
    let id = tx.last_insert_rowid();
    write_options(&tx, id, &input.options)?;
    write_serial(&tx, id, &input.serial)?;
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
             auth_method = ?6, key_path = ?7, legacy_mode = ?8, protocol = ?9,
             updated_at = CURRENT_TIMESTAMP
         WHERE id = ?10",
        params![
            input.folder_id,
            input.name,
            input.host,
            input.port,
            input.username,
            input.auth_method.as_str(),
            input.key_path,
            input.legacy_mode,
            input.protocol.as_str(),
            id
        ],
    )
    .map_err(db_error)?;
    write_options(&tx, id, &input.options)?;
    write_serial(&tx, id, &input.serial)?;
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

fn write_options(conn: &Connection, id: i64, options: &SessionOptions) -> Result<(), AppError> {
    conn.execute(
        "UPDATE profiles SET keepalive_secs = ?1, auto_reconnect = ?2, pager_auto_reply = ?3,
             login_commands = ?4, session_log = ?5
         WHERE id = ?6",
        params![
            options.keepalive_secs,
            options.auto_reconnect,
            options.pager_auto_reply,
            options.login_commands,
            options.session_log.as_str(),
            id
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

fn write_serial(conn: &Connection, id: i64, serial: &SerialSettings) -> Result<(), AppError> {
    conn.execute(
        "UPDATE profiles SET serial_baud_rate = ?1, serial_data_bits = ?2, serial_parity = ?3,
             serial_stop_bits = ?4, serial_flow_control = ?5
         WHERE id = ?6",
        params![
            serial.baud_rate,
            serial.data_bits,
            parity_str(serial.parity),
            serial.stop_bits,
            flow_str(serial.flow_control),
            id
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

/// Saves a secret typed at connection time (after a successful login).
pub fn save_secret(
    conn: &Connection,
    secrets: &Secrets,
    id: i64,
    value: &str,
) -> Result<(), AppError> {
    let current = get_stored(conn, id)?;
    if matches!(
        current.profile.auth_method,
        AuthMethod::Agent | AuthMethod::None
    ) || value.is_empty()
    {
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

/// Moves a profile to a folder (`None`: root). Only `folder_id` changes: the
/// rest of the profile, and its keyring entry, are left untouched.
pub fn move_profile(conn: &Connection, id: i64, folder_id: Option<i64>) -> Result<(), AppError> {
    check_folder(conn, folder_id)?;
    let changed = conn
        .execute(
            "UPDATE profiles SET folder_id = ?1 WHERE id = ?2",
            params![folder_id, id],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("profil {id}")));
    }
    Ok(())
}

/// Moves a folder (with its content) under `parent_id` (`None`: root).
pub fn move_folder(conn: &Connection, id: i64, parent_id: Option<i64>) -> Result<(), AppError> {
    check_folder(conn, parent_id)?;
    if let Some(parent) = parent_id {
        // The new parent must not be the folder itself or one of its sub-folders:
        // that would detach a loop of folders from the tree.
        let inside: bool = conn
            .query_row(
                "WITH RECURSIVE subtree(id) AS (
                     SELECT ?1
                     UNION ALL
                     SELECT f.id FROM folders f JOIN subtree s ON f.parent_id = s.id
                 )
                 SELECT EXISTS(SELECT 1 FROM subtree WHERE id = ?2)",
                params![id, parent],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if inside {
            return Err(AppError::InvalidInput(
                "un dossier ne peut pas être déplacé dans lui-même ni dans un de ses sous-dossiers."
                    .into(),
            ));
        }
    }
    let changed = conn
        .execute(
            "UPDATE folders SET parent_id = ?1 WHERE id = ?2",
            params![parent_id, id],
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
            protocol: Protocol::Ssh,
            host: "10.0.0.1".into(),
            port: 22,
            username: "admin".into(),
            auth_method: AuthMethod::Password,
            key_path: None,
            legacy_mode: false,
            secret,
            serial: SerialSettings::default(),
            options: SessionOptions {
                keepalive_secs: 30,
                auto_reconnect: false,
                pager_auto_reply: false,
                login_commands: String::new(),
                session_log: LogMode::Off,
            },
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
    async fn serial_profile_keeps_line_settings_and_drops_network_fields() {
        with_db(|conn, secrets| {
            let mut console = input("R1 console", SecretChange::Set { value: "pw".into() });
            console.protocol = Protocol::Serial;
            // A valid port name for the OS running the tests (/dev/ttyUSB0 or COM3).
            console.host = crate::platform::SERIAL_EXAMPLE.into();
            console.options.auto_reconnect = true;
            console.serial = SerialSettings {
                baud_rate: 115_200,
                data_bits: 7,
                parity: Parity::Even,
                stop_bits: 2,
                flow_control: FlowControl::Hardware,
            };
            let p = create(conn, secrets, console)?;
            assert_eq!(p.protocol, Protocol::Serial);
            assert_eq!(
                (p.host.as_str(), p.port),
                (crate::platform::SERIAL_EXAMPLE, 0)
            );
            assert_eq!(p.auth_method, AuthMethod::None);
            assert!(!p.has_saved_secret, "no secret for a serial console");
            assert!(!p.options.auto_reconnect);
            assert_eq!(p.serial.baud_rate, 115_200);
            assert_eq!(p.serial.parity, Parity::Even);
            assert_eq!(p.serial.flow_control, FlowControl::Hardware);

            let mut bad = input("x", SecretChange::Keep);
            bad.protocol = Protocol::Serial;
            bad.host = "10.0.0.1".into();
            assert!(create(conn, secrets, bad).is_err(), "not a device path");
            let mut bad = input("x", SecretChange::Keep);
            bad.protocol = Protocol::Serial;
            bad.host = crate::platform::SERIAL_EXAMPLE.into();
            bad.serial.data_bits = 9;
            assert!(create(conn, secrets, bad).is_err());

            // Back to SSH: line settings reset, a network port is required again.
            let mut ssh = input("R1 console", SecretChange::Keep);
            ssh.serial.baud_rate = 115_200;
            let p = update(conn, secrets, p.id, ssh)?;
            assert_eq!(p.serial, SerialSettings::default());
            assert_eq!(p.port, 22);
            Ok(())
        })
        .await;
    }

    #[tokio::test]
    async fn profiles_and_folders_can_be_moved_without_loops() {
        with_db(|conn, secrets| {
            let lab = create_folder(conn, "Labo", None)?;
            let core = create_folder(conn, "Cœur", Some(lab.id))?;
            let other = create_folder(conn, "Prod", None)?;
            let p = create(
                conn,
                secrets,
                input("R1", SecretChange::Set { value: "pw".into() }),
            )?;

            move_profile(conn, p.id, Some(core.id))?;
            let moved = get(conn, p.id)?;
            assert_eq!(moved.folder_id, Some(core.id));
            assert!(moved.has_saved_secret, "moving keeps the saved secret");
            move_profile(conn, p.id, None)?;
            assert_eq!(get(conn, p.id)?.folder_id, None);
            assert!(move_profile(conn, p.id, Some(9999)).is_err());
            assert!(move_profile(conn, 9999, None).is_err());

            move_folder(conn, lab.id, Some(other.id))?;
            move_folder(conn, core.id, None)?;
            // Into itself, or into its own sub-folder: refused.
            assert!(move_folder(conn, other.id, Some(other.id)).is_err());
            assert!(move_folder(conn, other.id, Some(lab.id)).is_err());
            let parents: Vec<_> = list(conn)?
                .folders
                .into_iter()
                .map(|f| (f.name, f.parent_id))
                .collect();
            assert!(parents.contains(&("Labo".into(), Some(other.id))));
            assert!(parents.contains(&("Cœur".into(), None)));
            assert!(parents.contains(&("Prod".into(), None)));
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
    async fn telnet_profile_has_no_authentication_nor_secret() {
        with_db(|conn, secrets| {
            let mut console = input("R1 console", SecretChange::Set { value: "pw".into() });
            console.protocol = Protocol::Telnet;
            console.port = 5000;
            console.username = String::new();
            console.legacy_mode = true;
            let p = create(conn, secrets, console)?;
            assert_eq!(p.protocol, Protocol::Telnet);
            assert_eq!(p.auth_method, AuthMethod::None);
            assert!(!p.has_saved_secret, "no secret is ever stored for Telnet");
            assert!(!p.legacy_mode, "SSH algorithms do not apply to Telnet");
            assert!(secrets.get(&profile_account(p.id))?.is_none());

            // An SSH profile still needs a user name and a method.
            let mut ssh = input("R1 ssh", SecretChange::Keep);
            ssh.username = String::new();
            assert!(create(conn, secrets, ssh).is_err());
            let mut ssh = input("R1 ssh", SecretChange::Keep);
            ssh.auth_method = AuthMethod::None;
            assert!(create(conn, secrets, ssh).is_err());
            Ok(())
        })
        .await;
    }

    #[tokio::test]
    async fn session_options_are_saved_and_parsed() {
        with_db(|conn, secrets| {
            let mut i = input("R1", SecretChange::Keep);
            i.options = SessionOptions {
                keepalive_secs: 60,
                auto_reconnect: true,
                pager_auto_reply: true,
                login_commands: "terminal length 0\n\n  \nshow clock  \n".into(),
                session_log: LogMode::Text,
            };
            let p = create(conn, secrets, i)?;
            assert_eq!(p.options.keepalive_secs, 60);
            assert!(p.options.auto_reconnect && p.options.pager_auto_reply);
            assert_eq!(p.options.session_log, LogMode::Text);
            assert_eq!(
                p.options.login_command_lines(),
                vec!["terminal length 0", "show clock"]
            );

            let mut too_long = input("R2", SecretChange::Keep);
            too_long.options.keepalive_secs = 7200;
            assert!(create(conn, secrets, too_long).is_err());
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
