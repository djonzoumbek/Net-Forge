//! Explicit replacement of a changed host key.

use std::sync::Arc;

use tauri::State;

use crate::errors::AppError;
use crate::security::known_hosts::KnownHosts;

/// Replaces the recorded key of `host:port` by the key that was just refused.
/// `fingerprint` is the new fingerprint the user was shown and confirmed.
#[tauri::command]
pub async fn known_hosts_replace(
    known_hosts: State<'_, Arc<KnownHosts>>,
    host: String,
    port: u16,
    fingerprint: String,
) -> Result<(), AppError> {
    let known_hosts = Arc::clone(&known_hosts);
    tokio::task::spawn_blocking(move || known_hosts.replace(&host, port, &fingerprint))
        .await
        .map_err(|e| AppError::Storage(format!("tâche interrompue : {e}")))?
}
