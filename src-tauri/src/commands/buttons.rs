//! Command bar buttons.

use tauri::State;

use crate::errors::AppError;
use crate::persistence::Database;
use crate::persistence::buttons::{self, ButtonInput, CommandButton};

#[tauri::command]
pub async fn buttons_list(db: State<'_, Database>) -> Result<Vec<CommandButton>, AppError> {
    db.run(|conn| buttons::list(conn)).await
}

#[tauri::command]
pub async fn button_create(
    db: State<'_, Database>,
    input: ButtonInput,
) -> Result<CommandButton, AppError> {
    db.run(move |conn| buttons::create(conn, input)).await
}

#[tauri::command]
pub async fn button_update(
    db: State<'_, Database>,
    id: i64,
    input: ButtonInput,
) -> Result<CommandButton, AppError> {
    db.run(move |conn| buttons::update(conn, id, input)).await
}

#[tauri::command]
pub async fn button_delete(db: State<'_, Database>, id: i64) -> Result<(), AppError> {
    db.run(move |conn| buttons::delete(conn, id)).await
}
