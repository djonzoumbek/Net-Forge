//! Session manager: creates sessions, routes commands to them, tracks their lifetime.

mod automation;
pub mod logger;
mod runner;

pub use runner::Automation;

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use russh_sftp::client::SftpSession;
use serde::Serialize;
use tokio::sync::{OnceCell, mpsc, oneshot};

use crate::errors::AppError;
use crate::security::known_hosts::{HostKeyMismatch, KnownHosts};
use crate::transport::serial::{self, SerialParams};
use crate::transport::ssh::{self, HostKeyInfo, HostKeyPrompt, SshLink, SshParams};
use crate::transport::telnet::{self, TelnetParams};
use crate::transport::{ConnectionInfo, TermSize, Transport};

pub type SessionId = u64;

/// Control messages sent from the Tauri commands to a session task.
#[derive(Debug)]
pub enum SessionCommand {
    Write(Vec<u8>),
    /// Lines typed one after the other (each followed by Enter), `delay` apart.
    Sequence {
        lines: Vec<String>,
        delay: std::time::Duration,
    },
    Resize(TermSize),
    /// The UI rendered this many bytes of output (flow control, see `runner`).
    Ack(usize),
    Close,
}

/// Status changes pushed to the UI on the session's event channel.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SessionEvent {
    HostKeyPrompt {
        info: HostKeyInfo,
    },
    Connected {
        info: ConnectionInfo,
        /// Session log file, when recording is enabled.
        log_file: Option<String>,
        /// Recording was requested but could not start (the session works anyway).
        log_error: Option<String>,
    },
    /// The server key differs from the recorded one: connection blocked.
    HostKeyChanged {
        mismatch: HostKeyMismatch,
    },
    /// The connection could not be established.
    Failed {
        code: String,
        message: String,
    },
    /// An established session ended (by the user or by the remote side).
    Closed {
        reason: String,
        by_remote: bool,
        /// Connection lost (as opposed to a normal end): may trigger auto-reconnect.
        lost: bool,
    },
}

/// Where a session sends its output and events. In the app these wrap Tauri
/// channels; in tests they record into vectors.
pub struct SessionSinks {
    pub data: Box<dyn Fn(Vec<u8>) + Send + Sync>,
    pub event: Box<dyn Fn(SessionEvent) + Send + Sync>,
}

struct SessionEntry {
    commands: mpsc::Sender<SessionCommand>,
    pending_host_key: Option<oneshot::Sender<bool>>,
    /// SSH sessions, once connected: the connection used by SFTP.
    ssh_link: Option<SshLink>,
    /// SFTP channel, opened on first use and shared by every transfer of the
    /// session. Dropped with the entry when the session ends.
    sftp: Arc<OnceCell<Arc<SftpSession>>>,
}

/// Shared by every Tauri command through `tauri::State`.
///
/// `Arc` lets the session tasks keep a handle to the map to remove themselves when
/// they end. The map is protected by a *std* `Mutex` (not tokio's): it is only held
/// for a few instructions and never across an `.await`, which is the case where the
/// standard one is both simpler and faster.
#[derive(Clone, Default)]
pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<SessionId, SessionEntry>>>,
    next_id: Arc<AtomicU64>,
}

impl SessionManager {
    fn sessions(&self) -> MutexGuard<'_, HashMap<SessionId, SessionEntry>> {
        // A poisoned mutex means a thread panicked while holding it. The map itself
        // is still consistent (single insert/remove operations), so keep using it
        // instead of crashing every other session.
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn open_ssh(
        &self,
        params: SshParams,
        size: TermSize,
        known_hosts: Arc<KnownHosts>,
        automation: Automation,
        sinks: SessionSinks,
    ) -> SessionId {
        self.spawn_session(sinks, size, automation, move |ask| {
            ssh::connect(params, size, known_hosts, ask)
        })
    }

    pub fn open_telnet(
        &self,
        params: TelnetParams,
        size: TermSize,
        automation: Automation,
        sinks: SessionSinks,
    ) -> SessionId {
        // Telnet has no host key: the prompt callback is simply never used.
        self.spawn_session(sinks, size, automation, move |_ask| {
            telnet::connect(params, size)
        })
    }

    pub fn open_serial(
        &self,
        params: SerialParams,
        size: TermSize,
        automation: Automation,
        sinks: SessionSinks,
    ) -> SessionId {
        // No host key either: a local cable.
        self.spawn_session(sinks, size, automation, move |_ask| serial::connect(params))
    }

    /// Generic over the connection step so that tests can use a fake transport.
    fn spawn_session<T, F>(
        &self,
        sinks: SessionSinks,
        initial_size: TermSize,
        automation: Automation,
        connect: impl FnOnce(HostKeyPrompt) -> F + Send + 'static,
    ) -> SessionId
    where
        T: Transport,
        F: Future<Output = Result<T, AppError>> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        // Bounded: if the remote side blocks, a huge paste waits here instead of
        // piling up in memory.
        let (commands_tx, commands_rx) = mpsc::channel(256);
        self.sessions().insert(
            id,
            SessionEntry {
                commands: commands_tx,
                pending_host_key: None,
                ssh_link: None,
                sftp: Arc::new(OnceCell::new()),
            },
        );

        let manager = self.clone();
        let sinks = Arc::new(sinks);
        tauri::async_runtime::spawn(async move {
            tracing::info!(session = id, "session task started");
            // The session runs in a task of its own: if it panics (a bug), tokio
            // catches the panic in that task only, and the cleanup below still
            // runs. The other sessions and the application are not affected.
            let (inner_manager, inner_sinks) = (manager.clone(), Arc::clone(&sinks));
            let session = tauri::async_runtime::spawn(async move {
                inner_manager
                    .drive(
                        id,
                        inner_sinks,
                        initial_size,
                        automation,
                        connect,
                        commands_rx,
                    )
                    .await;
            });
            if let Err(e) = session.await {
                tracing::error!(session = id, "session task panicked: {e}");
                (sinks.event)(SessionEvent::Closed {
                    reason: "Erreur interne : la session s'est arrêtée (détails dans les logs)."
                        .into(),
                    by_remote: false,
                    lost: false,
                });
            }
            manager.sessions().remove(&id);
            // Everything the session owned (transport, socket, channels, buffers)
            // has been dropped at this point.
            tracing::info!(session = id, "session task ended, resources released");
        });
        id
    }

    async fn drive<T, F>(
        &self,
        id: SessionId,
        sinks: Arc<SessionSinks>,
        initial_size: TermSize,
        automation: Automation,
        connect: impl FnOnce(HostKeyPrompt) -> F,
        mut commands: mpsc::Receiver<SessionCommand>,
    ) where
        T: Transport,
        F: Future<Output = Result<T, AppError>>,
    {
        let ask_host_key: HostKeyPrompt = {
            let manager = self.clone();
            let sinks = sinks.clone();
            Box::new(move |info| {
                let (tx, rx) = oneshot::channel();
                if let Some(entry) = manager.sessions().get_mut(&id) {
                    entry.pending_host_key = Some(tx);
                }
                (sinks.event)(SessionEvent::HostKeyPrompt { info });
                rx
            })
        };

        // While connecting, the user can still cancel or resize the window.
        let connecting = connect(ask_host_key);
        tokio::pin!(connecting);
        let mut size_changed: Option<TermSize> = None;
        let connected = loop {
            tokio::select! {
                result = &mut connecting => break result,
                cmd = commands.recv() => match cmd {
                    Some(SessionCommand::Resize(size)) => size_changed = Some(size),
                    Some(SessionCommand::Close) | None => {
                        tracing::info!(session = id, "connection cancelled by the user");
                        (sinks.event)(SessionEvent::Closed {
                            reason: "Connexion annulée.".into(),
                            by_remote: false,
                            lost: false,
                        });
                        return;
                    }
                    // Nothing to send input to yet.
                    Some(
                        SessionCommand::Write(_)
                        | SessionCommand::Sequence { .. }
                        | SessionCommand::Ack(_),
                    ) => {}
                },
            }
        };

        let mut transport = match connected {
            Ok(transport) => transport,
            Err(AppError::HostKeyChanged(mismatch)) => {
                tracing::warn!(session = id, host = %mismatch.host, "host key changed, connection blocked");
                (sinks.event)(SessionEvent::HostKeyChanged {
                    mismatch: *mismatch,
                });
                return;
            }
            Err(e) => {
                tracing::info!(session = id, code = e.code(), "connection failed");
                (sinks.event)(SessionEvent::Failed {
                    code: e.code().into(),
                    message: e.to_string(),
                });
                return;
            }
        };
        if let Some(size) = size_changed
            && size != initial_size
            && let Err(e) = transport.resize(size).await
        {
            tracing::warn!(session = id, "resize failed: {e}");
        }
        let info = transport.info();
        if let Some(link) = transport.sftp_link()
            && let Some(entry) = self.sessions().get_mut(&id)
        {
            entry.ssh_link = Some(link);
        }
        let legacy = info
            .algorithms
            .as_ref()
            .map(|a| a.legacy.join(","))
            .unwrap_or_default();
        tracing::info!(session = id, legacy = %legacy, "connected");

        // Recording starts once connected: failed attempts leave no empty files.
        let (logger, log_file, log_error) = match automation.log.clone() {
            None => (None, None, None),
            // Creating the file is disk I/O: done on the blocking pool.
            Some(config) => {
                let started =
                    tokio::task::spawn_blocking(move || logger::SessionLogger::start(&config))
                        .await
                        .unwrap_or_else(|e| Err(AppError::Storage(e.to_string())));
                match started {
                    Ok(logger) => {
                        let path = logger.path().display().to_string();
                        (Some(logger), Some(path), None)
                    }
                    Err(e) => {
                        tracing::warn!(session = id, "session log not started: {e}");
                        (None, None, Some(e.to_string()))
                    }
                }
            }
        };
        (sinks.event)(SessionEvent::Connected {
            info,
            log_file,
            log_error,
        });
        runner::run_session(id, transport, commands, &sinks, automation, logger).await;
    }

    pub async fn send(&self, id: SessionId, command: SessionCommand) -> Result<(), AppError> {
        // Clone the sender and release the lock before awaiting.
        let sender = self
            .sessions()
            .get(&id)
            .map(|entry| entry.commands.clone())
            .ok_or(AppError::SessionNotFound(id))?;
        sender
            .send(command)
            .await
            .map_err(|_| AppError::SessionNotFound(id))
    }

    /// The session's SFTP channel, opened on the first call. Only for connected
    /// SSH sessions.
    pub async fn sftp(&self, id: SessionId) -> Result<Arc<SftpSession>, AppError> {
        // Take what we need and release the map lock before any `.await`.
        let (link, cell) = {
            let sessions = self.sessions();
            let entry = sessions.get(&id).ok_or(AppError::SessionNotFound(id))?;
            let link = entry.ssh_link.clone().ok_or_else(|| {
                AppError::Sftp("disponible uniquement sur une session SSH connectée.".into())
            })?;
            (link, Arc::clone(&entry.sftp))
        };
        // `OnceCell`: two simultaneous first calls open a single channel.
        let sftp = cell
            .get_or_try_init(|| async {
                let sftp = link.open_sftp().await?;
                tracing::info!(session = id, "sftp channel opened");
                Ok::<_, AppError>(Arc::new(sftp))
            })
            .await?;
        Ok(Arc::clone(sftp))
    }

    pub fn answer_host_key(&self, id: SessionId, accept: bool) -> Result<(), AppError> {
        let reply = self
            .sessions()
            .get_mut(&id)
            .ok_or(AppError::SessionNotFound(id))?
            .pending_host_key
            .take()
            .ok_or_else(|| AppError::InvalidInput("aucune clé d'hôte en attente.".into()))?;
        // The receiver may be gone if the connection already failed: nothing to do.
        let _ = reply.send(accept);
        Ok(())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.sessions().len()
    }
}

#[cfg(test)]
mod integration_tests;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::transport::TransportEvent;

    struct IdleTransport {
        closed: Arc<Mutex<bool>>,
    }

    impl Transport for IdleTransport {
        fn info(&self) -> ConnectionInfo {
            ConnectionInfo {
                protocol: "test",
                encrypted: true,
                algorithms: None,
            }
        }
        async fn recv(&mut self) -> TransportEvent {
            std::future::pending().await
        }
        async fn send(&mut self, _: &[u8]) -> Result<(), AppError> {
            Ok(())
        }
        async fn resize(&mut self, _: TermSize) -> Result<(), AppError> {
            Ok(())
        }
        async fn close(self) {
            *self.closed.lock().unwrap() = true;
        }
    }

    fn recording_sinks() -> (SessionSinks, Arc<Mutex<Vec<SessionEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let sinks = SessionSinks {
            data: Box::new(|_| {}),
            event: Box::new(move |e| ev.lock().unwrap().push(e)),
        };
        (sinks, events)
    }

    async fn wait_until(mut condition: impl FnMut() -> bool) {
        for _ in 0..200 {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("condition not reached in time");
    }

    const SIZE: TermSize = TermSize { cols: 80, rows: 24 };

    #[tokio::test]
    async fn closing_a_session_releases_it() {
        let manager = SessionManager::default();
        let closed = Arc::new(Mutex::new(false));
        let (sinks, events) = recording_sinks();
        let flag = closed.clone();
        let id = manager.spawn_session(sinks, SIZE, Automation::default(), move |_| async move {
            Ok(IdleTransport { closed: flag })
        });
        wait_until(|| {
            matches!(
                events.lock().unwrap().last(),
                Some(SessionEvent::Connected { .. })
            )
        })
        .await;
        assert_eq!(manager.len(), 1);

        manager.send(id, SessionCommand::Close).await.unwrap();
        wait_until(|| manager.len() == 0).await;
        assert!(*closed.lock().unwrap(), "transport.close() must be called");
        assert!(matches!(
            manager.send(id, SessionCommand::Close).await,
            Err(AppError::SessionNotFound(_))
        ));
    }

    /// A bug (panic) in one session ends that session only, and the UI is told.
    #[tokio::test]
    async fn a_panicking_session_is_reported_and_released() {
        let manager = SessionManager::default();
        let (sinks, events) = recording_sinks();
        manager.spawn_session(sinks, SIZE, Automation::default(), |_| async {
            if true {
                panic!("simulated bug in a transport");
            }
            Ok(IdleTransport {
                closed: Arc::default(),
            })
        });
        wait_until(|| manager.len() == 0).await;
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(SessionEvent::Closed { reason, .. }) if reason.contains("Erreur interne")
        ));
    }

    #[tokio::test]
    async fn failed_connection_is_reported_and_released() {
        let manager = SessionManager::default();
        let (sinks, events) = recording_sinks();
        manager.spawn_session(sinks, SIZE, Automation::default(), |_| async {
            Err::<IdleTransport, _>(AppError::AuthFailed("password".into()))
        });
        wait_until(|| manager.len() == 0).await;
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(SessionEvent::Failed { code, .. }) if code == "auth_failed"
        ));
    }

    #[tokio::test]
    async fn host_key_answer_reaches_the_connecting_task() {
        let manager = SessionManager::default();
        let (sinks, events) = recording_sinks();
        let id = manager.spawn_session(sinks, SIZE, Automation::default(), |ask| async move {
            let info = HostKeyInfo {
                host: "lab".into(),
                port: 22,
                algorithm: "ssh-ed25519".into(),
                fingerprint: "SHA256:test".into(),
            };
            if ask(info).await.unwrap_or(false) {
                Ok(IdleTransport {
                    closed: Arc::default(),
                })
            } else {
                Err(AppError::HostKeyRejected)
            }
        });
        wait_until(|| {
            matches!(
                events.lock().unwrap().last(),
                Some(SessionEvent::HostKeyPrompt { .. })
            )
        })
        .await;
        manager.answer_host_key(id, false).unwrap();
        wait_until(|| manager.len() == 0).await;
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(SessionEvent::Failed { code, .. }) if code == "host_key_rejected"
        ));
    }

    #[tokio::test]
    async fn sessions_get_distinct_ids() {
        let manager = SessionManager::default();
        let a = manager.spawn_session(
            recording_sinks().0,
            SIZE,
            Automation::default(),
            |_| async {
                Ok(IdleTransport {
                    closed: Arc::default(),
                })
            },
        );
        let b = manager.spawn_session(
            recording_sinks().0,
            SIZE,
            Automation::default(),
            |_| async {
                Ok(IdleTransport {
                    closed: Arc::default(),
                })
            },
        );
        assert_ne!(a, b);
    }
}
