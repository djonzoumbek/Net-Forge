//! TCP connection shared by the network transports (SSH, Telnet).

use std::time::Duration;

use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::errors::AppError;

const DNS_TIMEOUT: Duration = Duration::from_secs(10);
const TCP_TIMEOUT: Duration = Duration::from_secs(10);

/// Resolves and connects, with distinct errors for unknown host, refused
/// connection, unreachable network and timeout.
///
/// `keepalive`: also enables the kernel's TCP keepalive, so a dead peer (network
/// cut, device rebooted) is detected after about 4 × the interval even when no
/// application data flows.
pub async fn open_tcp(
    host: &str,
    port: u16,
    keepalive: Option<Duration>,
) -> Result<TcpStream, AppError> {
    // Resolving first lets us tell "unknown host name" apart from "host unreachable".
    let addrs: Vec<_> = timeout(DNS_TIMEOUT, tokio::net::lookup_host((host, port)))
        .await
        .map_err(|_| AppError::Timeout(format!("la résolution DNS de « {host} » ne répond pas.")))?
        .map_err(|_| AppError::HostNotFound(host.to_string()))?
        .collect();
    if addrs.is_empty() {
        return Err(AppError::HostNotFound(host.to_string()));
    }

    let tcp = timeout(TCP_TIMEOUT, TcpStream::connect(&addrs[..]))
        .await
        .map_err(|_| {
            AppError::Timeout(format!(
                "aucune réponse de {host}:{port} après {} s (hôte éteint ou pare-feu ?).",
                TCP_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| {
            use std::io::ErrorKind as K;
            AppError::Unreachable(match e.kind() {
                K::ConnectionRefused => {
                    format!(
                        "connexion refusée par {host}:{port} (aucun service n'écoute sur ce port ?)."
                    )
                }
                K::NetworkUnreachable | K::HostUnreachable => {
                    format!("{host} n'est pas joignable (réseau ou route absente).")
                }
                _ => format!("{host}:{port} : {e}"),
            })
        })?;
    // Interactive traffic: send keystrokes immediately instead of batching them.
    if let Err(e) = tcp.set_nodelay(true) {
        tracing::debug!("set_nodelay failed: {e}");
    }
    if let Some(interval) = keepalive {
        let params = TcpKeepalive::new()
            .with_time(interval)
            .with_interval(interval)
            .with_retries(3);
        if let Err(e) = SockRef::from(&tcp).set_tcp_keepalive(&params) {
            tracing::warn!("TCP keepalive not enabled: {e}");
        }
    }
    Ok(tcp)
}
