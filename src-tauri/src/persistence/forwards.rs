//! Port forwards saved in a profile (table `port_forwards`, migration 0007).

use std::collections::HashMap;

use rusqlite::{Connection, Row, params};

use super::db_error;
use crate::errors::AppError;
use crate::forwarding::{ForwardKind, ForwardSpec};

/// More would rather be a mistake (a pasted list, a loop in a script).
pub const MAX_PER_PROFILE: usize = 20;

fn read(row: &Row) -> rusqlite::Result<(i64, ForwardSpec)> {
    let kind: String = row.get(1)?;
    let kind = ForwardKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            format!("unknown forward kind {kind}").into(),
        )
    })?;
    Ok((
        row.get(0)?,
        ForwardSpec {
            kind,
            bind_address: row.get(2)?,
            bind_port: row.get(3)?,
            target_host: row.get(4)?,
            target_port: row.get(5)?,
        },
    ))
}

const COLUMNS: &str = "profile_id, kind, bind_address, bind_port, target_host, target_port";

pub fn load(conn: &Connection, profile_id: i64) -> Result<Vec<ForwardSpec>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM port_forwards WHERE profile_id = ?1 ORDER BY position"
        ))
        .map_err(db_error)?;
    stmt.query_map(params![profile_id], read)
        .map_err(db_error)?
        .map(|r| r.map(|(_, spec)| spec).map_err(db_error))
        .collect()
}

/// Every profile's forwards in one query (the profile list).
pub fn load_all(conn: &Connection) -> Result<HashMap<i64, Vec<ForwardSpec>>, AppError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM port_forwards ORDER BY profile_id, position"
        ))
        .map_err(db_error)?;
    let mut all: HashMap<i64, Vec<ForwardSpec>> = HashMap::new();
    for row in stmt.query_map([], read).map_err(db_error)? {
        let (profile_id, spec) = row.map_err(db_error)?;
        all.entry(profile_id).or_default().push(spec);
    }
    Ok(all)
}

/// Replaces the whole list (the editor always sends the complete list).
/// Called inside the profile's transaction.
pub fn replace(conn: &Connection, profile_id: i64, specs: &[ForwardSpec]) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM port_forwards WHERE profile_id = ?1",
        params![profile_id],
    )
    .map_err(db_error)?;
    for (position, spec) in specs.iter().enumerate() {
        conn.execute(
            "INSERT INTO port_forwards (profile_id, position, kind, bind_address, bind_port,
                                        target_host, target_port)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                profile_id,
                position as i64,
                spec.kind.as_str(),
                spec.bind_address,
                spec.bind_port,
                spec.target_host,
                spec.target_port
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

/// Validates each forward and the list as a whole.
pub fn normalize(specs: Vec<ForwardSpec>) -> Result<Vec<ForwardSpec>, AppError> {
    if specs.len() > MAX_PER_PROFILE {
        return Err(AppError::InvalidInput(format!(
            "{MAX_PER_PROFILE} redirections de port au plus par profil."
        )));
    }
    let specs = specs
        .into_iter()
        .map(ForwardSpec::validate)
        .collect::<Result<Vec<_>, _>>()?;
    // Two forwards on the same fixed local port: the second would always fail.
    for (i, a) in specs.iter().enumerate() {
        if a.bind_port != 0 && specs[..i].iter().any(|b| b.bind_port == a.bind_port) {
            return Err(AppError::InvalidInput(format!(
                "le port local {} est utilisé par deux redirections.",
                a.bind_port
            )));
        }
    }
    Ok(specs)
}
