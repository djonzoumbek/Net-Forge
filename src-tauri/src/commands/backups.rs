//! Configuration backup commands: capture (type a command in the session and
//! save its output), list and read the saved files of a device.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;

use crate::backups::{self, BackupEntry};
use crate::errors::AppError;
use crate::persistence::{Database, profiles};
use crate::sessions::{SessionCommand, SessionId, SessionManager};

/// `~/Documents/NetForge/backups` (next to the session logs, visible to the user).
fn base_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .document_dir()
        .or_else(|_| app.path().home_dir())
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("NetForge")
        .join("backups")
}

fn check_device(device: &str) -> Result<(), AppError> {
    if device.trim().is_empty() || device.len() > 200 {
        return Err(AppError::InvalidInput("nom d'équipement invalide.".into()));
    }
    Ok(())
}

/// Types `command` in the session, waits for the prompt and saves the output.
/// `device` names the folder (the profile name, or the target of a quick
/// connection); with `profile_id`, the command is remembered for next time.
#[tauri::command]
pub async fn backup_capture(
    app: AppHandle,
    manager: State<'_, SessionManager>,
    db: State<'_, Database>,
    session_id: SessionId,
    device: String,
    command: String,
    profile_id: Option<i64>,
) -> Result<BackupEntry, AppError> {
    check_device(&device)?;
    let command = command.trim().to_string();
    if command.is_empty() || command.len() > 200 || command.contains(['\r', '\n']) {
        return Err(AppError::InvalidInput(
            "commande de sauvegarde : une seule ligne, 200 caractères au plus.".into(),
        ));
    }
    let (reply, answer) = oneshot::channel();
    manager
        .send(
            session_id,
            SessionCommand::Capture {
                command: command.clone(),
                reply,
            },
        )
        .await?;
    // A dropped sender: the session ended during the capture.
    let content = answer.await.map_err(|_| {
        AppError::InvalidInput("la session s'est fermée pendant la sauvegarde.".into())
    })??;

    let base = base_dir(&app);
    let entry = tokio::task::spawn_blocking(move || backups::save(&base, &device, &content))
        .await
        .map_err(|e| AppError::Storage(e.to_string()))??;
    if let Some(id) = profile_id {
        db.run(move |conn| profiles::set_backup_command(conn, id, &command))
            .await?;
    }
    tracing::info!(
        session = session_id,
        bytes = entry.bytes,
        "configuration backup saved"
    );
    Ok(entry)
}

#[tauri::command]
pub async fn backup_list(app: AppHandle, device: String) -> Result<Vec<BackupEntry>, AppError> {
    check_device(&device)?;
    let base = base_dir(&app);
    tokio::task::spawn_blocking(move || backups::list(&base, &device))
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
}

#[tauri::command]
pub async fn backup_read(app: AppHandle, device: String, name: String) -> Result<String, AppError> {
    check_device(&device)?;
    let base = base_dir(&app);
    tokio::task::spawn_blocking(move || backups::read(&base, &device, &name))
        .await
        .map_err(|e| AppError::Storage(e.to_string()))?
}
