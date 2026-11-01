//! SQLite storage (profiles, folders). Never contains secrets.

pub mod buttons;
pub mod profiles;
pub mod settings;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::errors::AppError;

/// Versioned migrations, applied in order. The index + 1 is stored in SQLite's
/// `user_version`, so each one runs exactly once. Never edit a published
/// migration: add a new file instead.
const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/0001_initial.sql"),
    include_str!("../../migrations/0002_telnet_profiles.sql"),
    include_str!("../../migrations/0003_session_options_and_buttons.sql"),
    include_str!("../../migrations/0004_session_logs_and_settings.sql"),
    include_str!("../../migrations/0005_serial_profiles.sql"),
    include_str!("../../migrations/0006_clipboard_settings.sql"),
];

/// `rusqlite::Connection` can be sent to another thread but not shared between
/// threads (`Send` but not `Sync`), hence the `Mutex`. `Arc` lets every command
/// and blocking task hold a handle to the same connection.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let conn = Connection::open(path).map_err(db_error)?;
        restrict_permissions(path);
        backup_before_migration(&conn, path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, AppError> {
        Self::init(Connection::open_in_memory().map_err(db_error)?)
    }

    fn init(mut conn: Connection) -> Result<Self, AppError> {
        // Foreign keys are off by default in SQLite, per connection.
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(db_error)?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` on tokio's blocking thread pool: SQLite does disk I/O and must not
    /// block the async workers (nor the UI thread).
    pub async fn run<T, F>(&self, f: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, AppError> + Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut guard)
        })
        .await
        .map_err(|e| AppError::Storage(format!("tâche interrompue : {e}")))?
    }
}

/// Copies an existing database before upgrading it: `netforge.db.backup-v2` is the
/// state before migrating from version 2. `VACUUM INTO` writes a consistent
/// snapshot, WAL journal included.
fn backup_before_migration(conn: &Connection, path: &Path) -> Result<(), AppError> {
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;
    if version == 0 || version as usize >= MIGRATIONS.len() {
        return Ok(()); // new database, or nothing to migrate
    }
    let mut backup = path.as_os_str().to_owned();
    backup.push(format!(".backup-v{version}"));
    let backup = std::path::PathBuf::from(backup);
    // VACUUM INTO refuses to overwrite: an older backup of the same version goes.
    let _ = std::fs::remove_file(&backup);
    conn.execute("VACUUM INTO ?1", [backup.to_string_lossy()])
        .map_err(|e| AppError::Storage(format!("sauvegarde avant migration impossible : {e}")))?;
    restrict_permissions(&backup);
    tracing::info!(backup = %backup.display(), "database backed up before migration");
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<(), AppError> {
    let current: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;
    let current = current as usize;
    if current > MIGRATIONS.len() {
        return Err(AppError::Storage(
            "la base a été créée par une version plus récente de NetForge.".into(),
        ));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        // One transaction per migration: a failure leaves the base unchanged.
        let tx = conn.transaction().map_err(db_error)?;
        tx.execute_batch(sql).map_err(db_error)?;
        tx.pragma_update(None, "user_version", (index + 1) as u32)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        tracing::info!(version = index + 1, "database migration applied");
    }
    Ok(())
}

/// The database lists the hosts you administer: readable by you only.
fn restrict_permissions(path: &Path) {
    if let Err(e) = crate::platform::restrict(path, false) {
        tracing::warn!("cannot restrict database permissions: {e}");
    }
}

pub(crate) fn db_error(e: rusqlite::Error) -> AppError {
    AppError::Storage(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_applied_once() {
        let db = Database::open_in_memory().unwrap();
        let mut conn = db.conn.lock().unwrap();
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        // Running again is a no-op.
        migrate(&mut conn).unwrap();
    }

    #[test]
    fn existing_database_is_backed_up_before_migration() {
        let dir = std::env::temp_dir().join(format!("netforge-backup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("netforge.db");
        let _ = std::fs::remove_file(&path);
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO profiles (name, host, port, username, auth_method) VALUES ('R1', 'h', 22, 'u', 'password')",
                [],
            )
            .unwrap();
            let _ = &mut conn;
        }
        Database::open(&path).unwrap();
        let backup = Connection::open(dir.join("netforge.db.backup-v1")).unwrap();
        let version: u32 = backup
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let name: String = backup
            .query_row("SELECT name FROM profiles", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            (version, name.as_str()),
            (1, "R1"),
            "backup holds the pre-migration state"
        );
        // Opening an up-to-date database creates no new backup.
        std::fs::remove_file(dir.join("netforge.db.backup-v1")).unwrap();
        Database::open(&path).unwrap();
        assert!(
            !dir.join(format!("netforge.db.backup-v{}", MIGRATIONS.len()))
                .exists()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A database created by the previous version keeps its data when upgraded.
    #[test]
    fn upgrade_from_version_1_keeps_profiles() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch(
            "INSERT INTO folders (id, name) VALUES (1, 'Lab');
             INSERT INTO profiles (folder_id, name, host, port, username, auth_method, secret_ref)
             VALUES (1, 'R1', '10.0.0.1', 22, 'admin', 'password', 'profile-1');",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let row: (String, String, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT name, protocol, secret_ref, folder_id FROM profiles",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            ("R1".into(), "ssh".into(), Some("profile-1".into()), Some(1))
        );
        // The new constraint accepts Telnet profiles without authentication.
        conn.execute(
            "INSERT INTO profiles (name, protocol, host, port, username, auth_method)
             VALUES ('console', 'telnet', '127.0.0.1', 5000, '', 'none')",
            [],
        )
        .unwrap();
        // ...and the foreign key to folders still works after the rebuild.
        conn.execute("DELETE FROM folders WHERE id = 1", [])
            .unwrap();
        let folder: Option<i64> = conn
            .query_row(
                "SELECT folder_id FROM profiles WHERE name = 'R1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(folder, None);
    }

    /// 0005 rebuilds `profiles`: every phase 5 / 6 option must survive.
    #[test]
    fn upgrade_from_version_4_keeps_session_options() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        for (version, sql) in MIGRATIONS[..4].iter().enumerate() {
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version as i64 + 1)
                .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO profiles (name, protocol, host, port, username, auth_method, keepalive_secs,
                                   auto_reconnect, pager_auto_reply, login_commands, session_log)
             VALUES ('R1', 'telnet', '10.0.0.1', 5001, '', 'none', 60, 1, 1, 'terminal length 0', 'raw');",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let row: (i64, bool, bool, String, String, i64, String) = conn
            .query_row(
                "SELECT keepalive_secs, auto_reconnect, pager_auto_reply, login_commands, session_log,
                        serial_baud_rate, serial_parity
                 FROM profiles WHERE name = 'R1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .unwrap();
        assert_eq!(
            row,
            (
                60,
                true,
                true,
                "terminal length 0".into(),
                "raw".into(),
                9600,
                "none".into()
            )
        );
        // Serial profiles have port 0; network profiles still cannot.
        conn.execute(
            "INSERT INTO profiles (name, protocol, host, port, username, auth_method)
             VALUES ('console', 'serial', '/dev/ttyUSB0', 0, '', 'none')",
            [],
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO profiles (name, protocol, host, port, username, auth_method)
                 VALUES ('bad', 'ssh', 'h', 0, 'u', 'password')",
                [],
            )
            .is_err()
        );
    }
}
