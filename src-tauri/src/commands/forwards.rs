//! Port forwarding commands (SSH sessions only).

use tauri::State;

use crate::errors::AppError;
use crate::forwarding::{ForwardInfo, ForwardSpec};
use crate::sessions::{SessionId, SessionManager};

#[tauri::command]
pub async fn forward_start(
    manager: State<'_, SessionManager>,
    session_id: SessionId,
    spec: ForwardSpec,
) -> Result<ForwardInfo, AppError> {
    manager.start_forward(session_id, spec).await
}

#[tauri::command]
pub fn forward_stop(
    manager: State<'_, SessionManager>,
    session_id: SessionId,
    forward_id: u64,
) -> Result<(), AppError> {
    manager.stop_forward(session_id, forward_id)
}

/// Polled by the panel to refresh the connection counters.
#[tauri::command]
pub fn forward_list(
    manager: State<'_, SessionManager>,
    session_id: SessionId,
) -> Result<Vec<ForwardInfo>, AppError> {
    manager.list_forwards(session_id)
}
