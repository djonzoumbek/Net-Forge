//! SFTP commands: browse the remote side of an SSH session, upload, download, cancel.
//!
//! Local paths come from the native file dialogs of the frontend; they are only
//! checked to be absolute (the user picks any file they can read or write).

use std::path::PathBuf;

use tauri::State;
use tauri::ipc::Channel;

use crate::errors::AppError;
use crate::sessions::{SessionId, SessionManager};
use crate::transfers::{self, RemoteListing, TransferEvent, TransferId, TransferManager};

fn absolute(path: String) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(AppError::InvalidInput(format!(
            "chemin local non absolu : {}",
            path.display()
        )))
    }
}

/// Events are pushed on a per-transfer channel; a closed window is not an error.
fn sink(on_event: Channel<TransferEvent>) -> Box<dyn Fn(TransferEvent) + Send + Sync> {
    Box::new(move |event| {
        if let Err(e) = on_event.send(event) {
            tracing::debug!("transfer event not delivered: {e}");
        }
    })
}

/// `path`: empty for the home directory.
#[tauri::command]
pub async fn sftp_list(
    manager: State<'_, SessionManager>,
    session_id: SessionId,
    path: String,
) -> Result<RemoteListing, AppError> {
    let sftp = manager.sftp(session_id).await?;
    transfers::list_dir(&sftp, &path).await
}

/// Returns the path of the new directory.
#[tauri::command]
pub async fn sftp_mkdir(
    manager: State<'_, SessionManager>,
    session_id: SessionId,
    parent: String,
    name: String,
) -> Result<String, AppError> {
    let sftp = manager.sftp(session_id).await?;
    transfers::make_dir(&sftp, &parent, &name).await
}

#[tauri::command]
pub async fn sftp_download(
    manager: State<'_, SessionManager>,
    transfers: State<'_, TransferManager>,
    session_id: SessionId,
    remote_path: String,
    local_path: String,
    on_event: Channel<TransferEvent>,
) -> Result<TransferId, AppError> {
    let local = absolute(local_path)?;
    let sftp = manager.sftp(session_id).await?;
    Ok(
        transfers.start(sink(on_event), move |cancel, sink| async move {
            transfers::download(&sftp, &remote_path, &local, &cancel, &*sink).await
        }),
    )
}

#[tauri::command]
pub async fn sftp_upload(
    manager: State<'_, SessionManager>,
    transfers: State<'_, TransferManager>,
    session_id: SessionId,
    local_path: String,
    remote_dir: String,
    overwrite: bool,
    on_event: Channel<TransferEvent>,
) -> Result<TransferId, AppError> {
    let local = absolute(local_path)?;
    let sftp = manager.sftp(session_id).await?;
    Ok(
        transfers.start(sink(on_event), move |cancel, sink| async move {
            transfers::upload(&sftp, &local, &remote_dir, overwrite, &cancel, &*sink).await
        }),
    )
}

#[tauri::command]
pub fn transfer_cancel(
    transfers: State<'_, TransferManager>,
    id: TransferId,
) -> Result<(), AppError> {
    transfers.cancel(id)
}
