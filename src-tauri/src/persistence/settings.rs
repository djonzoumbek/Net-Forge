//! Application settings (single row of the `settings` table).

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::db_error;
use crate::errors::AppError;

/// Terminal themes known by the frontend.
const THEMES: &[&str] = &[
    "netforge-dark",
    "netforge-light",
    "solarized-dark",
    "high-contrast",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Lines kept in each terminal's history.
    pub scrollback: u32,
    pub theme: String,
    pub font_family: String,
    pub font_size: u16,
    /// Session logs directory; empty = default (~/Documents/NetForge/logs).
    pub log_dir: String,
}

impl AppSettings {
    fn validate(mut self) -> Result<Self, AppError> {
        let invalid = |m: &str| Err(AppError::InvalidInput(m.into()));
        if !(1000..=200_000).contains(&self.scrollback) {
            return invalid("historique : entre 1 000 et 200 000 lignes.");
        }
        if !THEMES.contains(&self.theme.as_str()) {
            return invalid("thème inconnu.");
        }
        self.font_family = self.font_family.trim().to_string();
        // The name ends up in a CSS font-family list: keep it to plain names.
        let plain = |c: char| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_');
        if self.font_family.is_empty()
            || self.font_family.len() > 100
            || !self.font_family.chars().all(plain)
        {
            return invalid("nom de police invalide (lettres, chiffres, espaces, - et _).");
        }
        if !(8..=36).contains(&self.font_size) {
            return invalid("taille de police : entre 8 et 36.");
        }
        self.log_dir = self.log_dir.trim().to_string();
        if !self.log_dir.is_empty() && !std::path::Path::new(&self.log_dir).is_absolute() {
            return invalid(
                "le dossier des journaux doit être un chemin absolu (ex. /home/…/logs).",
            );
        }
        Ok(self)
    }
}

pub fn get(conn: &Connection) -> Result<AppSettings, AppError> {
    conn.query_row(
        "SELECT scrollback, theme, font_family, font_size, log_dir FROM settings WHERE id = 1",
        [],
        |r| {
            Ok(AppSettings {
                scrollback: r.get(0)?,
                theme: r.get(1)?,
                font_family: r.get(2)?,
                font_size: r.get(3)?,
                log_dir: r.get(4)?,
            })
        },
    )
    .map_err(db_error)
}

pub fn update(conn: &Connection, settings: AppSettings) -> Result<AppSettings, AppError> {
    let s = settings.validate()?;
    conn.execute(
        "UPDATE settings SET scrollback = ?1, theme = ?2, font_family = ?3, font_size = ?4, log_dir = ?5
         WHERE id = 1",
        params![s.scrollback, s.theme, s.font_family, s.font_size, s.log_dir],
    )
    .map_err(db_error)?;
    get(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Database;

    #[tokio::test]
    async fn defaults_then_update_with_validation() {
        let db = Database::open_in_memory().unwrap();
        db.run(|conn| {
            let defaults = get(conn)?;
            assert_eq!(
                defaults.scrollback, 10_000,
                "enough for the 10 000-line search criterion"
            );
            assert_eq!(defaults.theme, "netforge-dark");

            let changed = AppSettings {
                scrollback: 50_000,
                theme: "solarized-dark".into(),
                font_family: " Ubuntu Mono ".into(),
                font_size: 16,
                log_dir: "/tmp/netforge-logs".into(),
            };
            let saved = update(conn, changed)?;
            assert_eq!(saved.font_family, "Ubuntu Mono");
            assert_eq!(get(conn)?, saved);

            for bad in [
                AppSettings {
                    scrollback: 10,
                    ..saved.clone()
                },
                AppSettings {
                    theme: "neon".into(),
                    ..saved.clone()
                },
                AppSettings {
                    font_family: "x'; } body { color: red".into(),
                    ..saved.clone()
                },
                AppSettings {
                    font_size: 99,
                    ..saved.clone()
                },
                AppSettings {
                    log_dir: "logs".into(),
                    ..saved.clone()
                },
            ] {
                assert!(update(conn, bad).is_err());
            }
            assert_eq!(get(conn)?, saved, "invalid updates change nothing");
            Ok(())
        })
        .await
        .unwrap();
    }
}
