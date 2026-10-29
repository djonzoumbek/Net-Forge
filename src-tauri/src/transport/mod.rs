//! Common interface implemented by every transport (SSH, Telnet, serial).
//!
//! The session runner only talks to this trait, so the terminal never depends on
//! the protocol. Connecting is transport-specific (each one has its own parameters)
//! and therefore stays outside the trait.

#[cfg(test)]
pub mod idle_proxy;
pub mod net;
pub mod serial;
pub mod ssh;
pub mod telnet;

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::errors::AppError;

/// Terminal size in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl TermSize {
    pub fn validate(self) -> Result<Self, AppError> {
        if (1..=1000).contains(&self.cols) && (1..=1000).contains(&self.rows) {
            Ok(self)
        } else {
            Err(AppError::InvalidInput(format!(
                "taille de terminal invalide ({}x{})",
                self.cols, self.rows
            )))
        }
    }
}

/// Security properties of an open connection, shown in the session's tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInfo {
    pub protocol: &'static str,
    /// `false` for Telnet: everything, passwords included, travels in clear text.
    pub encrypted: bool,
    /// SSH only: negotiated algorithms and which of them are legacy.
    pub algorithms: Option<ssh::algorithms::Negotiated>,
}

/// Something that happened on the remote side.
#[derive(Debug)]
pub enum TransportEvent {
    /// Raw bytes from the remote terminal. Not necessarily valid UTF-8:
    /// a multi-byte character may be split across two events.
    Data(Vec<u8>),
    /// The session ended normally (shell exited, device closed the connection).
    Closed(String),
    /// The connection was lost (network cut, keepalive without answer, reset).
    /// Auto-reconnect only reacts to this one, never to a normal exit.
    Lost(String),
}

/// `-> impl Future + Send` instead of `async fn`: the session runner is spawned on
/// tokio's multi-threaded runtime, so the futures must be `Send` (movable between
/// threads). Implementations can still be written with plain `async fn`.
pub trait Transport: Send + 'static {
    fn info(&self) -> ConnectionInfo;

    /// SSH only: access to the connection for SFTP. `None` for the others.
    fn sftp_link(&self) -> Option<ssh::SshLink> {
        None
    }

    /// Waits for the next remote event.
    ///
    /// Must be cancel-safe: the runner calls it inside `tokio::select!`, which drops
    /// the future when another branch wins. No data may be lost when that happens.
    fn recv(&mut self) -> impl Future<Output = TransportEvent> + Send;

    /// Sends user input (keystrokes, pasted text, control sequences) as raw bytes.
    fn send(&mut self, data: &[u8]) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Informs the remote side that the terminal size changed.
    fn resize(&mut self, size: TermSize) -> impl Future<Output = Result<(), AppError>> + Send;

    /// Closes the connection. Takes `self` by value: once closed, the transport
    /// cannot be used again, and dropping it releases the socket.
    fn close(self) -> impl Future<Output = ()> + Send;
}
