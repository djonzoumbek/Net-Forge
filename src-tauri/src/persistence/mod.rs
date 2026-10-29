//! SQLite storage (profiles, folders). Never contains secrets.

pub mod profiles;

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
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
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
}
