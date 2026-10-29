//! Test helper: a TCP proxy that drops connections idle for too long, like a
//! firewall or NAT with an idle timeout (the usual killer of quiet sessions).

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, sleep_until};

/// Listens on a random local port and forwards to `target`. Returns the port.
pub async fn start(target: SocketAddr, idle_timeout: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind proxy");
    let port = listener.local_addr().expect("proxy address").port();
    tokio::spawn(async move {
        while let Ok((client, _)) = listener.accept().await {
            tokio::spawn(relay(client, target, idle_timeout));
        }
    });
    port
}

async fn relay(client: TcpStream, target: SocketAddr, idle_timeout: Duration) {
    let Ok(server) = TcpStream::connect(target).await else {
        return;
    };
    let (mut client_read, mut client_write) = client.into_split();
    let (mut server_read, mut server_write) = server.into_split();
    let (mut up, mut down) = (vec![0u8; 8192], vec![0u8; 8192]);
    let mut deadline = Instant::now() + idle_timeout;
    loop {
        tokio::select! {
            read = client_read.read(&mut up) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if server_write.write_all(&up[..n]).await.is_err() { break; }
                    deadline = Instant::now() + idle_timeout;
                }
            },
            read = server_read.read(&mut down) => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if client_write.write_all(&down[..n]).await.is_err() { break; }
                    deadline = Instant::now() + idle_timeout;
                }
            },
            // Idle for too long: both sides are dropped, like a firewall would.
            _ = sleep_until(deadline) => break,
        }
    }
}
