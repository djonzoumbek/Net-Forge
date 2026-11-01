//! SSH port forwarding: `ssh -L` (local port -> host:port reached from the SSH
//! server) and `ssh -D` (local SOCKS5 proxy).
//!
//! Each forward is one listener task. Every accepted connection opens a
//! `direct-tcpip` channel on the session's SSH connection and copies bytes both
//! ways. The connection tasks live in a `JoinSet` owned by the listener task:
//! stopping the forward (or dropping it with its session) aborts the listener,
//! which drops the `JoinSet`, which aborts every connection. Nothing outlives
//! the session.

pub mod socks;

use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardKind {
    /// `-L`: a fixed target, reached from the SSH server.
    Local,
    /// `-D`: SOCKS5 proxy, the client chooses the target of each connection.
    Dynamic,
}

impl ForwardKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ForwardKind::Local => "local",
            ForwardKind::Dynamic => "dynamic",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local" => Some(ForwardKind::Local),
            "dynamic" => Some(ForwardKind::Dynamic),
            _ => None,
        }
    }
}

/// What the user configures (stored in a profile, or added during a session).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardSpec {
    pub kind: ForwardKind,
    /// Local address to listen on: `127.0.0.1` by default. `0.0.0.0` exposes the
    /// tunnel to the whole local network.
    pub bind_address: String,
    /// 0: a free port chosen by the system.
    pub bind_port: u16,
    /// `Local` only (empty and 0 for `Dynamic`).
    pub target_host: String,
    pub target_port: u16,
}

impl ForwardSpec {
    pub fn validate(mut self) -> Result<Self, AppError> {
        let invalid = |m: &str| Err(AppError::InvalidInput(m.into()));
        self.bind_address = self.bind_address.trim().to_string();
        if self.bind_address.is_empty() {
            self.bind_address = "127.0.0.1".into();
        }
        if self.bind_address != "localhost" && self.bind_address.parse::<IpAddr>().is_err() {
            return invalid(
                "adresse d'écoute invalide : une adresse IP locale (127.0.0.1, 0.0.0.0…) ou localhost.",
            );
        }
        match self.kind {
            ForwardKind::Local => {
                self.target_host = self.target_host.trim().to_string();
                if self.target_host.is_empty()
                    || self.target_host.len() > 255
                    || self.target_host.contains(char::is_whitespace)
                {
                    return invalid("hôte de destination invalide.");
                }
                if self.target_port == 0 {
                    return invalid("le port de destination doit être entre 1 et 65535.");
                }
            }
            ForwardKind::Dynamic => {
                self.target_host.clear();
                self.target_port = 0;
            }
        }
        Ok(self)
    }

    /// Listening on something else than the loopback interface.
    pub fn exposed(&self) -> bool {
        self.bind_address != "localhost"
            && self
                .bind_address
                .parse::<IpAddr>()
                .is_ok_and(|ip| !ip.is_loopback())
    }
}

/// A byte stream to the target (an SSH channel; a plain TCP socket in tests).
pub trait Tunnel: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Tunnel for T {}

pub type DialFuture = Pin<Box<dyn Future<Output = Result<Box<dyn Tunnel>, AppError>> + Send>>;

/// Opens a tunnel to `(host, port)` for a client connected from `origin`.
/// A trait object rather than `SshLink` directly, so that the listener logic is
/// tested without an SSH server.
pub type Dialer = Arc<dyn Fn(String, u16, SocketAddr) -> DialFuture + Send + Sync>;

#[derive(Default)]
struct Stats {
    active: AtomicU32,
    total: AtomicU64,
    last_error: Mutex<Option<String>>,
}

impl Stats {
    fn error(&self, message: String) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(message);
    }
}

/// What the UI shows for a running forward.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub id: u64,
    pub spec: ForwardSpec,
    /// The port really listened on (differs from `spec.bind_port` when it is 0).
    pub local_port: u16,
    pub active_connections: u32,
    pub total_connections: u64,
    /// Last connection that failed (target refused, SOCKS error...).
    pub last_error: Option<String>,
}

/// A running forward. Dropping it stops it (listener and connections).
pub struct RunningForward {
    spec: ForwardSpec,
    local_port: u16,
    stats: Arc<Stats>,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Drop for RunningForward {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RunningForward {
    pub fn info(&self, id: u64) -> ForwardInfo {
        ForwardInfo {
            id,
            spec: self.spec.clone(),
            local_port: self.local_port,
            active_connections: self.stats.active.load(Ordering::Relaxed),
            total_connections: self.stats.total.load(Ordering::Relaxed),
            last_error: self
                .stats
                .last_error
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        }
    }
}

/// Binds the local port and starts accepting. Fails at once if the port is
/// taken, so that the user sees the error where they added the forward.
pub async fn start(spec: ForwardSpec, dial: Dialer) -> Result<RunningForward, AppError> {
    let spec = spec.validate()?;
    let address = format!("{}:{}", spec.bind_address, spec.bind_port);
    let listener = TcpListener::bind(&address).await.map_err(|e| {
        AppError::Forward(match e.kind() {
            std::io::ErrorKind::AddrInUse => {
                format!(
                    "le port {} est déjà utilisé sur cette machine.",
                    spec.bind_port
                )
            }
            std::io::ErrorKind::PermissionDenied => format!(
                "écoute sur le port {} refusée (les ports < 1024 sont réservés à root).",
                spec.bind_port
            ),
            _ => format!("écoute sur {address} impossible : {e}"),
        })
    })?;
    let local_port = listener
        .local_addr()
        .map_err(|e| AppError::Forward(e.to_string()))?
        .port();
    let stats = Arc::new(Stats::default());
    let task = tauri::async_runtime::spawn(accept_loop(
        listener,
        spec.clone(),
        dial,
        Arc::clone(&stats),
    ));
    tracing::info!(
        kind = spec.kind.as_str(),
        port = local_port,
        "port forward started"
    );
    if spec.exposed() {
        tracing::warn!(
            port = local_port,
            "port forward reachable from the local network"
        );
    }
    Ok(RunningForward {
        spec,
        local_port,
        stats,
        task,
    })
}

async fn accept_loop(listener: TcpListener, spec: ForwardSpec, dial: Dialer, stats: Arc<Stats>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((client, origin)) => {
                    stats.total.fetch_add(1, Ordering::Relaxed);
                    connections.spawn(relay(client, origin, spec.clone(), Arc::clone(&dial), Arc::clone(&stats)));
                }
                // Too many open files, for instance: keep listening.
                Err(e) => {
                    stats.error(format!("connexion locale refusée : {e}"));
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            },
            // Reap finished connections so the set does not grow forever.
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

/// Decrements the active counter whatever the way `relay` ends.
struct ActiveGuard(Arc<Stats>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::Relaxed);
    }
}

async fn relay(
    mut client: TcpStream,
    origin: SocketAddr,
    spec: ForwardSpec,
    dial: Dialer,
    stats: Arc<Stats>,
) {
    stats.active.fetch_add(1, Ordering::Relaxed);
    let _guard = ActiveGuard(Arc::clone(&stats));
    let _ = client.set_nodelay(true);

    let (host, port) = match spec.kind {
        ForwardKind::Local => (spec.target_host.clone(), spec.target_port),
        ForwardKind::Dynamic => match socks::accept(&mut client).await {
            Ok(target) => target,
            Err(e) => {
                stats.error(e.to_string());
                return;
            }
        },
    };
    let mut tunnel = match dial(host.clone(), port, origin).await {
        Ok(tunnel) => tunnel,
        Err(e) => {
            stats.error(format!("{host}:{port} : {e}"));
            if spec.kind == ForwardKind::Dynamic {
                let _ = socks::reply(&mut client, socks::Reply::ConnectionRefused).await;
            }
            return;
        }
    };
    if spec.kind == ForwardKind::Dynamic
        && socks::reply(&mut client, socks::Reply::Succeeded)
            .await
            .is_err()
    {
        return;
    }
    // Ends when both sides have closed (or one of them fails).
    if let Err(e) = tokio::io::copy_bidirectional(&mut client, &mut tunnel).await {
        tracing::debug!("forwarded connection ended: {e}");
    }
}

#[cfg(test)]
mod tests;
