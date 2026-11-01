//! Telnet transport (GNS3 / EVE-NG / PNETLab consoles, devices without SSH).
//!
//! Everything travels in clear text: the UI shows a "not encrypted" warning.

mod protocol;

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use self::protocol::TelnetCodec;
use super::{ConnectionInfo, TermSize, Transport, TransportEvent, net};
use crate::errors::AppError;

#[derive(Debug)]
pub struct TelnetParams {
    pub host: String,
    pub port: u16,
    /// Interval of the Telnet NOP + TCP keepalive; `None` disables both.
    pub keepalive: Option<Duration>,
}

enum Outgoing {
    Data(Vec<u8>),
    Resize(TermSize),
}

/// The socket is owned by an internal I/O task; this handle talks to it through
/// channels. That way `recv` is a simple (cancel-safe) channel read, and writes
/// never wait for reads.
pub struct TelnetTransport {
    incoming: mpsc::Receiver<TransportEvent>,
    outgoing: mpsc::Sender<Outgoing>,
    io_task: JoinHandle<()>,
}

pub async fn connect(params: TelnetParams, size: TermSize) -> Result<TelnetTransport, AppError> {
    let tcp = net::open_tcp(&params.host, params.port, params.keepalive).await?;
    // Small bounded channels: if the UI is behind, the I/O task stops reading the
    // socket and TCP flow control slows the device down.
    let (incoming_tx, incoming) = mpsc::channel(8);
    let (outgoing, outgoing_rx) = mpsc::channel(64);
    let io_task = tokio::spawn(run_io(
        tcp,
        size,
        params.keepalive,
        incoming_tx,
        outgoing_rx,
    ));
    Ok(TelnetTransport {
        incoming,
        outgoing,
        io_task,
    })
}

async fn run_io(
    tcp: TcpStream,
    size: TermSize,
    keepalive: Option<Duration>,
    incoming: mpsc::Sender<TransportEvent>,
    mut outgoing: mpsc::Receiver<Outgoing>,
) {
    // Two halves so that a pending read does not prevent writing.
    let (mut reader, mut writer) = tcp.into_split();
    let mut codec = TelnetCodec::new(size);
    let mut buffer = vec![0u8; 16 * 1024];
    // Data decoded but not yet accepted by the session (channel full).
    let mut pending: Option<Vec<u8>> = None;
    // IAC NOP: invisible for the device, but keeps firewalls/NAT from dropping an
    // idle session, and makes a dead connection fail on write.
    let mut nop_timer = keepalive.map(|interval| {
        let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
        timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        timer
    });

    // `Lost` for errors, `Closed` when the device ends the session itself.
    let end = 'io: {
        if let Err(e) = writer.write_all(&codec.initial_negotiation()).await {
            break 'io lost(e);
        }
        loop {
            tokio::select! {
                command = outgoing.recv() => {
                    let bytes = match command {
                        Some(Outgoing::Data(data)) => codec.encode_input(&data),
                        Some(Outgoing::Resize(size)) => codec.resize(size).unwrap_or_default(),
                        // The transport was closed or dropped: end quietly.
                        None => {
                            let _ = writer.shutdown().await;
                            return;
                        }
                    };
                    if let Err(e) = writer.write_all(&bytes).await {
                        break 'io lost(e);
                    }
                }
                read = reader.read(&mut buffer), if pending.is_none() => match read {
                    Ok(0) => break 'io TransportEvent::Closed("Connexion fermée par l'équipement.".into()),
                    Ok(n) => {
                        let decoded = codec.decode(&buffer[..n]);
                        if !decoded.reply.is_empty()
                            && let Err(e) = writer.write_all(&decoded.reply).await
                        {
                            break 'io lost(e);
                        }
                        if !decoded.data.is_empty() {
                            pending = Some(decoded.data);
                        }
                    }
                    Err(e) => break 'io lost(e),
                },
                _ = next_tick(&mut nop_timer) => {
                    if let Err(e) = writer.write_all(&protocol::NOP).await {
                        break 'io lost(e);
                    }
                }
                permit = incoming.reserve(), if pending.is_some() => match permit {
                    Ok(permit) => permit.send(TransportEvent::Data(pending.take().unwrap_or_default())),
                    Err(_) => return,
                },
            }
        }
    };

    if let Some(data) = pending.take() {
        let _ = incoming.send(TransportEvent::Data(data)).await;
    }
    let _ = incoming.send(end).await;
}

/// Next keepalive tick, or never when keepalive is disabled.
async fn next_tick(timer: &mut Option<tokio::time::Interval>) {
    match timer {
        Some(timer) => {
            timer.tick().await;
        }
        None => std::future::pending().await,
    }
}

fn lost(error: std::io::Error) -> TransportEvent {
    TransportEvent::Lost(format!("Connexion perdue : {error}."))
}

impl Transport for TelnetTransport {
    fn info(&self) -> ConnectionInfo {
        ConnectionInfo {
            protocol: "telnet",
            encrypted: false,
            algorithms: None,
            via: Vec::new(),
        }
    }

    async fn recv(&mut self) -> TransportEvent {
        self.incoming
            .recv()
            .await
            .unwrap_or_else(|| TransportEvent::Closed("Connexion terminée.".into()))
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), AppError> {
        self.outgoing
            .send(Outgoing::Data(data.to_vec()))
            .await
            .map_err(|_| AppError::Unreachable("la connexion Telnet est fermée.".into()))
    }

    async fn resize(&mut self, size: TermSize) -> Result<(), AppError> {
        self.outgoing
            .send(Outgoing::Resize(size))
            .await
            .map_err(|_| AppError::Unreachable("la connexion Telnet est fermée.".into()))
    }

    async fn close(self) {
        let TelnetTransport {
            incoming,
            outgoing,
            mut io_task,
        } = self;
        // Dropping both channel ends tells the I/O task to stop, whatever it is
        // waiting for. It then shuts the socket down.
        drop(outgoing);
        drop(incoming);
        if tokio::time::timeout(Duration::from_secs(2), &mut io_task)
            .await
            .is_err()
        {
            io_task.abort();
        }
    }
}

#[cfg(test)]
mod tests;
