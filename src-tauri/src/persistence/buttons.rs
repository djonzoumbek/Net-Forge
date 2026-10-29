//! Command bar buttons: a label and commands typed into the session.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::db_error;
use crate::errors::AppError;

pub const MAX_DELAY_MS: u32 = 60_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandButton {
    pub id: i64,
    pub label: String,
    /// One command per line.
    pub commands: String,
    /// Pause between two lines.
    pub delay_ms: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonInput {
    pub label: String,
    pub commands: String,
    pub delay_ms: u32,
}

impl ButtonInput {
    fn normalized(mut self) -> Result<Self, AppError> {
        self.label = self.label.trim().to_string();
        self.commands = self.commands.trim_end().to_string();
        let invalid = |m: &str| Err(AppError::InvalidInput(m.into()));
        if self.label.is_empty() || self.label.chars().count() > 40 {
            return invalid("le libellé du bouton est obligatoire (40 caractères max).");
        }
        if self.commands.trim().is_empty() {
            return invalid("le bouton doit contenir au moins une commande.");
        }
        if self.commands.len() > 8000 || self.commands.lines().count() > 100 {
            return invalid("100 lignes et 8000 caractères maximum par bouton.");
        }
        if self.delay_ms > MAX_DELAY_MS {
            return invalid("délai entre les lignes : 60 secondes maximum.");
        }
        Ok(self)
    }
}

pub fn list(conn: &Connection) -> Result<Vec<CommandButton>, AppError> {
    let mut stmt = conn
        .prepare("SELECT id, label, commands, delay_ms FROM command_buttons ORDER BY position, id")
        .map_err(db_error)?;
    stmt.query_map([], |r| {
        Ok(CommandButton {
            id: r.get(0)?,
            label: r.get(1)?,
            commands: r.get(2)?,
            delay_ms: r.get(3)?,
        })
    })
    .map_err(db_error)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(db_error)
}

fn get(conn: &Connection, id: i64) -> Result<CommandButton, AppError> {
    conn.query_row(
        "SELECT id, label, commands, delay_ms FROM command_buttons WHERE id = ?1",
        params![id],
        |r| {
            Ok(CommandButton {
                id: r.get(0)?,
                label: r.get(1)?,
                commands: r.get(2)?,
                delay_ms: r.get(3)?,
            })
        },
    )
    .optional()
    .map_err(db_error)?
    .ok_or_else(|| AppError::NotFound(format!("bouton {id}")))
}

pub fn create(conn: &Connection, input: ButtonInput) -> Result<CommandButton, AppError> {
    let input = input.normalized()?;
    // New buttons go at the end of the bar.
    conn.execute(
        "INSERT INTO command_buttons (label, commands, delay_ms, position)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM command_buttons))",
        params![input.label, input.commands, input.delay_ms],
    )
    .map_err(db_error)?;
    get(conn, conn.last_insert_rowid())
}

pub fn update(conn: &Connection, id: i64, input: ButtonInput) -> Result<CommandButton, AppError> {
    let input = input.normalized()?;
    let changed = conn
        .execute(
            "UPDATE command_buttons SET label = ?1, commands = ?2, delay_ms = ?3 WHERE id = ?4",
            params![input.label, input.commands, input.delay_ms, id],
        )
        .map_err(db_error)?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("bouton {id}")));
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: i64) -> Result<(), AppError> {
    conn.execute("DELETE FROM command_buttons WHERE id = ?1", params![id])
        .map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Database;

    fn input(label: &str, commands: &str) -> ButtonInput {
        ButtonInput {
            label: label.into(),
            commands: commands.into(),
            delay_ms: 200,
        }
    }

    #[tokio::test]
    async fn crud_keeps_insertion_order() {
        let db = Database::open_in_memory().unwrap();
        db.run(|conn| {
            let a = create(conn, input("Interfaces", "show ip interface brief"))?;
            let b = create(conn, input("Config", "terminal length 0\nshow run\n"))?;
            assert_eq!(
                b.commands, "terminal length 0\nshow run",
                "trailing newline trimmed"
            );
            assert_eq!(
                list(conn)?.iter().map(|b| b.id).collect::<Vec<_>>(),
                vec![a.id, b.id]
            );
            let a = update(conn, a.id, input("IP", "show ip int br"))?;
            assert_eq!(a.label, "IP");
            delete(conn, a.id)?;
            assert_eq!(list(conn)?.len(), 1);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn rejects_invalid_buttons() {
        let db = Database::open_in_memory().unwrap();
        db.run(|conn| {
            assert!(create(conn, input(" ", "show run")).is_err());
            assert!(create(conn, input("Vide", "  \n ")).is_err());
            let mut slow = input("Lent", "show run");
            slow.delay_ms = MAX_DELAY_MS + 1;
            assert!(create(conn, slow).is_err());
            Ok(())
        })
        .await
        .unwrap();
    }
}
