//! File transfers over SFTP: remote directory listing, upload and download with
//! progress and cancellation.
//!
//! Each transfer runs in its own tokio task. Cancellation is a shared flag
//! (`AtomicBool`) checked between two blocks: simple, and enough since a block
//! takes a few milliseconds at most.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;

use crate::errors::AppError;

pub type TransferId = u64;

/// Read/write block size: big enough for good throughput, small enough for a
/// quick reaction to "Cancel".
const BLOCK: usize = 64 * 1024;
/// Progress events are throttled: the UI does not need more than ~10 per second.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Suffix of a download in progress: the real file only appears once complete.
const PART_SUFFIX: &str = ".netforge-part";

/// Pushed to the UI on the transfer's channel.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TransferEvent {
    /// `total`: unknown when the server does not give the size.
    Progress {
        done: u64,
        total: Option<u64>,
    },
    Done {
        bytes: u64,
    },
    Cancelled,
    Failed {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Dir,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    /// Unix timestamp (seconds).
    pub modified: Option<u32>,
    /// `ls -l` style, e.g. `rwxr-xr-x`.
    pub permissions: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteListing {
    /// Absolute path, resolved by the server (`..` and `~` removed).
    pub path: String,
    pub entries: Vec<RemoteEntry>,
}

/// Lists a remote directory; `""` means the user's home directory.
pub async fn list_dir(sftp: &SftpSession, path: &str) -> Result<RemoteListing, AppError> {
    let path = sftp
        .canonicalize(if path.is_empty() { "." } else { path })
        .await?;
    let mut entries: Vec<RemoteEntry> = sftp
        .read_dir(path.as_str())
        .await?
        .filter(|e| e.file_name() != "." && e.file_name() != "..")
        .map(|e| {
            let meta = e.metadata();
            let file_type = e.file_type();
            RemoteEntry {
                name: e.file_name(),
                kind: if file_type.is_dir() {
                    EntryKind::Dir
                } else if file_type.is_symlink() {
                    EntryKind::Symlink
                } else if file_type.is_file() {
                    EntryKind::File
                } else {
                    EntryKind::Other
                },
                size: meta.size,
                modified: meta.mtime,
                permissions: meta.permissions.map(permission_string),
            }
        })
        .collect();
    // Directories first, then by name (case-insensitive), like a file manager.
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir)
            .cmp(&(b.kind != EntryKind::Dir))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(RemoteListing { path, entries })
}

pub async fn make_dir(sftp: &SftpSession, parent: &str, name: &str) -> Result<String, AppError> {
    let path = remote_join(parent, valid_name(name)?);
    sftp.create_dir(path.as_str()).await?;
    Ok(path)
}

/// `0o755` -> `rwxr-xr-x` (only the 9 permission bits).
fn permission_string(mode: u32) -> String {
    (0..9)
        .map(|i| {
            let bit = 1 << (8 - i);
            if mode & bit == 0 {
                '-'
            } else {
                ['r', 'w', 'x'][i % 3]
            }
        })
        .collect()
}

/// A single file or directory name: no path separator, no `.` / `..`.
fn valid_name(name: &str) -> Result<&str, AppError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(AppError::InvalidInput(format!("nom invalide : « {name} »")));
    }
    Ok(name)
}

/// Remote paths always use `/`, whatever the local OS.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// Throttled progress reporting.
struct Progress<'a> {
    sink: &'a (dyn Fn(TransferEvent) + Send + Sync),
    total: Option<u64>,
    done: u64,
    last: Instant,
}

impl<'a> Progress<'a> {
    fn new(sink: &'a (dyn Fn(TransferEvent) + Send + Sync), total: Option<u64>) -> Self {
        sink(TransferEvent::Progress { done: 0, total });
        Self {
            sink,
            total,
            done: 0,
            last: Instant::now(),
        }
    }

    fn add(&mut self, bytes: usize) {
        self.done += bytes as u64;
        if self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            (self.sink)(TransferEvent::Progress {
                done: self.done,
                total: self.total,
            });
        }
    }
}

fn local_error(path: &Path, e: std::io::Error) -> AppError {
    let path = path.display();
    AppError::LocalFile(match e.kind() {
        std::io::ErrorKind::NotFound => format!("{path} introuvable."),
        std::io::ErrorKind::PermissionDenied => format!("accès refusé à {path}."),
        _ => format!("{path} : {e}"),
    })
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), AppError> {
    if cancel.load(Ordering::Relaxed) {
        Err(AppError::Cancelled)
    } else {
        Ok(())
    }
}

/// Downloads `remote` to `local`. The data goes to `<local>.netforge-part`,
/// renamed only when complete: a failed or cancelled download never leaves a
/// truncated file under the real name. Created with mode 0600 (configurations
/// often contain secrets).
pub async fn download(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
    cancel: &AtomicBool,
    sink: &(dyn Fn(TransferEvent) + Send + Sync),
) -> Result<u64, AppError> {
    let total = sftp.metadata(remote).await?.size;
    let mut source = sftp.open(remote).await?;
    let mut part_name = local.as_os_str().to_owned();
    part_name.push(PART_SUFFIX);
    let part = PathBuf::from(part_name);
    let mut target = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&part)
        .await
        .map_err(|e| local_error(&part, e))?;

    let copied = async {
        let mut progress = Progress::new(sink, total);
        let mut buf = vec![0u8; BLOCK];
        loop {
            check_cancel(cancel)?;
            let n = source
                .read(&mut buf)
                .await
                .map_err(|e| AppError::Sftp(format!("lecture de {remote} : {e}")))?;
            if n == 0 {
                break;
            }
            target
                .write_all(&buf[..n])
                .await
                .map_err(|e| local_error(&part, e))?;
            progress.add(n);
        }
        target.flush().await.map_err(|e| local_error(&part, e))?;
        Ok::<_, AppError>(progress.done)
    }
    .await;
    drop(target);
    // Closing the remote handle is best effort: the data is already here.
    let _ = source.shutdown().await;

    match copied {
        Ok(bytes) => {
            tokio::fs::rename(&part, local)
                .await
                .map_err(|e| local_error(local, e))?;
            Ok(bytes)
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

/// Uploads `local` into the remote directory `remote_dir` (same file name).
/// Refuses to replace an existing file unless `overwrite`. A failed or
/// cancelled upload removes the incomplete remote file.
pub async fn upload(
    sftp: &SftpSession,
    local: &Path,
    remote_dir: &str,
    overwrite: bool,
    cancel: &AtomicBool,
    sink: &(dyn Fn(TransferEvent) + Send + Sync),
) -> Result<u64, AppError> {
    let name = local.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        AppError::LocalFile(format!("nom de fichier invalide : {}", local.display()))
    })?;
    let remote = remote_join(remote_dir, valid_name(name)?);
    let mut source = tokio::fs::File::open(local)
        .await
        .map_err(|e| local_error(local, e))?;
    let meta = source.metadata().await.map_err(|e| local_error(local, e))?;
    if !meta.is_file() {
        return Err(AppError::LocalFile(format!(
            "{} n'est pas un fichier (les dossiers ne sont pas pris en charge).",
            local.display()
        )));
    }
    if !overwrite && sftp.try_exists(remote.as_str()).await? {
        return Err(AppError::RemoteExists(remote));
    }
    let mut target = sftp.create(remote.as_str()).await?;

    let copied = async {
        let mut progress = Progress::new(sink, Some(meta.len()));
        let mut buf = vec![0u8; BLOCK];
        loop {
            check_cancel(cancel)?;
            let n = source
                .read(&mut buf)
                .await
                .map_err(|e| local_error(local, e))?;
            if n == 0 {
                break;
            }
            target
                .write_all(&buf[..n])
                .await
                .map_err(|e| AppError::Sftp(format!("écriture de {remote} : {e}")))?;
            progress.add(n);
        }
        // Waits for the server to acknowledge the last writes.
        target
            .shutdown()
            .await
            .map_err(|e| AppError::Sftp(format!("écriture de {remote} : {e}")))?;
        Ok::<_, AppError>(progress.done)
    }
    .await;

    if copied.is_err() {
        drop(target);
        let _ = sftp.remove_file(remote.as_str()).await;
    }
    copied
}

/// Running transfers, so that they can be cancelled. Shared through `tauri::State`.
#[derive(Clone, Default)]
pub struct TransferManager {
    running: Arc<Mutex<HashMap<TransferId, Arc<AtomicBool>>>>,
    next_id: Arc<AtomicU64>,
}

impl TransferManager {
    fn running(&self) -> MutexGuard<'_, HashMap<TransferId, Arc<AtomicBool>>> {
        // Same reasoning as `SessionManager::sessions`: a poisoned map is still usable.
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs `job` in its own task. The final outcome is always reported on
    /// `sink` (`Done`, `Cancelled` or `Failed`), never as a panic or a lost error.
    pub fn start<F>(
        &self,
        sink: Box<dyn Fn(TransferEvent) + Send + Sync>,
        job: impl FnOnce(Arc<AtomicBool>, Arc<dyn Fn(TransferEvent) + Send + Sync>) -> F,
    ) -> TransferId
    where
        F: Future<Output = Result<u64, AppError>> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.running().insert(id, Arc::clone(&cancel));
        let sink: Arc<dyn Fn(TransferEvent) + Send + Sync> = Arc::from(sink);
        let future = job(cancel, Arc::clone(&sink));
        let manager = self.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = future.await;
            manager.running().remove(&id);
            let event = match outcome {
                Ok(bytes) => TransferEvent::Done { bytes },
                Err(AppError::Cancelled) => TransferEvent::Cancelled,
                Err(e) => TransferEvent::Failed {
                    code: e.code().into(),
                    message: e.to_string(),
                },
            };
            tracing::info!(transfer = id, outcome = ?event, "transfer finished");
            sink(event);
        });
        id
    }

    pub fn cancel(&self, id: TransferId) -> Result<(), AppError> {
        let running = self.running();
        let flag = running
            .get(&id)
            .ok_or_else(|| AppError::NotFound(format!("transfert {id} (déjà terminé ?)")))?;
        flag.store(true, Ordering::Relaxed);
        Ok(())
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.running().len()
    }
}

#[cfg(test)]
mod tests;
