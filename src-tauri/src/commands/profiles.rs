//! Profile and folder management.

use std::sync::Arc;

use tauri::State;

use crate::errors::AppError;
use crate::persistence::Database;
use crate::persistence::profiles::{self, Folder, Library, Profile, ProfileInput};
use crate::security::secrets::Secrets;

#[tauri::command]
pub async fn library_list(db: State<'_, Database>) -> Result<Library, AppError> {
    db.run(|conn| profiles::list(conn)).await
}

#[tauri::command]
pub async fn profile_create(
    db: State<'_, Database>,
    secrets: State<'_, Arc<Secrets>>,
    input: ProfileInput,
) -> Result<Profile, AppError> {
    let secrets = Arc::clone(&secrets);
    db.run(move |conn| profiles::create(conn, &secrets, input))
        .await
}

#[tauri::command]
pub async fn profile_update(
    db: State<'_, Database>,
    secrets: State<'_, Arc<Secrets>>,
    id: i64,
    input: ProfileInput,
) -> Result<Profile, AppError> {
    let secrets = Arc::clone(&secrets);
    db.run(move |conn| profiles::update(conn, &secrets, id, input))
        .await
}

#[tauri::command]
pub async fn profile_delete(
    db: State<'_, Database>,
    secrets: State<'_, Arc<Secrets>>,
    id: i64,
) -> Result<(), AppError> {
    let secrets = Arc::clone(&secrets);
    db.run(move |conn| profiles::delete(conn, &secrets, id))
        .await
}

/// Saves the secret typed at connection time, once the login succeeded.
#[tauri::command]
pub async fn profile_save_secret(
    db: State<'_, Database>,
    secrets: State<'_, Arc<Secrets>>,
    id: i64,
    secret: String,
) -> Result<(), AppError> {
    let secrets = Arc::clone(&secrets);
    let secret = zeroize::Zeroizing::new(secret);
    db.run(move |conn| profiles::save_secret(conn, &secrets, id, &secret))
        .await
}

#[tauri::command]
pub async fn folder_create(
    db: State<'_, Database>,
    name: String,
    parent_id: Option<i64>,
) -> Result<Folder, AppError> {
    db.run(move |conn| profiles::create_folder(conn, &name, parent_id))
        .await
}

#[tauri::command]
pub async fn folder_rename(db: State<'_, Database>, id: i64, name: String) -> Result<(), AppError> {
    db.run(move |conn| profiles::rename_folder(conn, id, &name))
        .await
}

#[tauri::command]
pub async fn folder_delete(db: State<'_, Database>, id: i64) -> Result<(), AppError> {
    db.run(move |conn| profiles::delete_folder(conn, id)).await
}
