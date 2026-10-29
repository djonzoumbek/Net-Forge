//! Session manager: creates sessions, routes commands to them, tracks their lifetime.

mod runner;

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tokio::sync::{mpsc, oneshot};

use crate::errors::AppError;
use crate::security::known_hosts::{HostKeyMismatch, KnownHosts};
use crate::transport::ssh::{self, HostKeyInfo, HostKeyPrompt, SshParams};
use crate::transport::{ConnectionInfo, TermSize, Transport};

pub type SessionId = u64;

/// Control messages sent from the Tauri commands to a session task.
#[derive(Debug)]
pub enum SessionCommand {
    Write(Vec<u8>),
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
        sinks: SessionSinks,
    ) -> SessionId {
        self.spawn_session(sinks, size, move |ask| {
            ssh::connect(params, size, known_hosts, ask)
        })
    }

    /// Generic over the connection step so that tests can use a fake transport.
    fn spawn_session<T, F>(
        &self,
        sinks: SessionSinks,
        initial_size: TermSize,
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
            },
        );

        let manager = self.clone();
        let sinks = Arc::new(sinks);
        tauri::async_runtime::spawn(async move {
            tracing::info!(session = id, "session task started");
            manager
                .drive(id, sinks, initial_size, connect, commands_rx)
                .await;
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
                        });
                        return;
                    }
                    // Nothing to send input to yet.
                    Some(SessionCommand::Write(_) | SessionCommand::Ack(_)) => {}
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
        let legacy = info
            .algorithms
            .as_ref()
            .map(|a| a.legacy.join(","))
            .unwrap_or_default();
        tracing::info!(session = id, legacy = %legacy, "connected");
        (sinks.event)(SessionEvent::Connected { info });
        runner::run_session(id, transport, commands, &sinks).await;
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
        let id = manager.spawn_session(sinks, SIZE, move |_| async move {
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

    #[tokio::test]
    async fn failed_connection_is_reported_and_released() {
        let manager = SessionManager::default();
        let (sinks, events) = recording_sinks();
        manager.spawn_session(sinks, SIZE, |_| async {
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
        let id = manager.spawn_session(sinks, SIZE, |ask| async move {
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
        let a = manager.spawn_session(recording_sinks().0, SIZE, |_| async {
            Ok(IdleTransport {
                closed: Arc::default(),
            })
        });
        let b = manager.spawn_session(recording_sinks().0, SIZE, |_| async {
            Ok(IdleTransport {
                closed: Arc::default(),
            })
        });
        assert_ne!(a, b);
    }
}
