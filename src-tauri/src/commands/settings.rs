//! Application settings.

use tauri::State;

use crate::errors::AppError;
use crate::persistence::Database;
use crate::persistence::settings::{self, AppSettings};

#[tauri::command]
pub async fn settings_get(db: State<'_, Database>) -> Result<AppSettings, AppError> {
    db.run(|conn| settings::get(conn)).await
}

#[tauri::command]
pub async fn settings_update(
    db: State<'_, Database>,
    settings: AppSettings,
) -> Result<AppSettings, AppError> {
    db.run(move |conn| settings::update(conn, settings)).await
}
