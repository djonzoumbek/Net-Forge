//! The listener and relay logic with a plain TCP "tunnel" (no SSH server).
//! The SSH channel itself is covered by the ignored integration test in
//! `transport/ssh/tests.rs`.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use super::*;

/// Echo server on a free port: what a forwarded service would be.
async fn echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut r, mut w) = socket.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    address
}

/// Connects directly, like the SSH server would; records the requested targets.
fn tcp_dialer(seen: Arc<Mutex<Vec<(String, u16)>>>) -> Dialer {
    Arc::new(move |host, port, _origin| {
        seen.lock().unwrap().push((host.clone(), port));
        Box::pin(async move {
            let stream = TcpStream::connect((host.as_str(), port))
                .await
                .map_err(|e| AppError::Forward(e.to_string()))?;
            Ok(Box::new(stream) as Box<dyn Tunnel>)
        })
    })
}

fn local(target: SocketAddr) -> ForwardSpec {
    ForwardSpec {
        kind: ForwardKind::Local,
        bind_address: String::new(),
        bind_port: 0,
        target_host: target.ip().to_string(),
        target_port: target.port(),
    }
}

async fn round_trip(stream: &mut TcpStream, message: &[u8]) -> Vec<u8> {
    stream.write_all(message).await.unwrap();
    let mut back = vec![0u8; message.len()];
    timeout(Duration::from_secs(5), stream.read_exact(&mut back))
        .await
        .expect("echo in time")
        .unwrap();
    back
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    for _ in 0..100 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not reached");
}

#[test]
fn specs_are_validated_and_normalized() {
    let spec = local("10.0.0.1:80".parse().unwrap()).validate().unwrap();
    assert_eq!(spec.bind_address, "127.0.0.1", "loopback by default");
    assert!(!spec.exposed());

    let everywhere = ForwardSpec {
        bind_address: "0.0.0.0".into(),
        ..spec.clone()
    };
    assert!(everywhere.validate().unwrap().exposed());

    for bad in [
        ForwardSpec {
            bind_address: "my-laptop".into(),
            ..spec.clone()
        },
        ForwardSpec {
            target_host: "".into(),
            ..spec.clone()
        },
        ForwardSpec {
            target_port: 0,
            ..spec.clone()
        },
    ] {
        assert!(bad.validate().is_err());
    }

    let socks = ForwardSpec {
        kind: ForwardKind::Dynamic,
        target_host: "ignored".into(),
        target_port: 99,
        ..spec
    }
    .validate()
    .unwrap();
    assert_eq!((socks.target_host.as_str(), socks.target_port), ("", 0));
}

#[tokio::test]
async fn local_forward_relays_both_ways() {
    let target = echo_server().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let forward = start(local(target), tcp_dialer(Arc::clone(&seen)))
        .await
        .unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", forward.local_port))
        .await
        .unwrap();
    assert_eq!(
        round_trip(&mut client, b"show version").await,
        b"show version"
    );
    assert_eq!(forward.info(1).active_connections, 1);
    assert_eq!(
        *seen.lock().unwrap(),
        [(target.ip().to_string(), target.port())]
    );

    drop(client);
    wait_until(|| forward.info(1).active_connections == 0).await;
    assert_eq!(forward.info(1).total_connections, 1);
}

#[tokio::test]
async fn dynamic_forward_speaks_socks5() {
    let target = echo_server().await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let spec = ForwardSpec {
        kind: ForwardKind::Dynamic,
        ..local(target)
    };
    let forward = start(spec, tcp_dialer(Arc::clone(&seen))).await.unwrap();

    let mut client = TcpStream::connect(("127.0.0.1", forward.local_port))
        .await
        .unwrap();
    client.write_all(&[5, 1, 0]).await.unwrap();
    let mut method = [0u8; 2];
    client.read_exact(&mut method).await.unwrap();
    assert_eq!(method, [5, 0]);
    let mut request = vec![5, 1, 0, 3, 9];
    request.extend_from_slice(b"localhost");
    request.extend_from_slice(&target.port().to_be_bytes());
    client.write_all(&request).await.unwrap();
    let mut reply = [0u8; 10];
    client.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0, "CONNECT succeeded");
    assert_eq!(round_trip(&mut client, b"ping").await, b"ping");
    assert_eq!(
        *seen.lock().unwrap(),
        [("localhost".to_string(), target.port())]
    );
}

#[tokio::test]
async fn unreachable_target_is_reported() {
    // A port nobody listens on: bind then release it.
    let closed = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let forward = start(local(closed), tcp_dialer(Default::default()))
        .await
        .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", forward.local_port))
        .await
        .unwrap();
    let mut buf = [0u8; 1];
    // The forward closes the client connection.
    let read = timeout(Duration::from_secs(5), client.read(&mut buf))
        .await
        .unwrap();
    assert!(matches!(read, Ok(0) | Err(_)));
    wait_until(|| forward.info(1).last_error.is_some()).await;
}

#[tokio::test]
async fn busy_port_fails_at_start_and_stop_closes_connections() {
    let target = echo_server().await;
    let forward = start(local(target), tcp_dialer(Default::default()))
        .await
        .unwrap();
    let taken = ForwardSpec {
        bind_port: forward.local_port,
        ..local(target)
    };
    let err = start(taken, tcp_dialer(Default::default()))
        .await
        .err()
        .unwrap();
    assert_eq!(err.code(), "forward");
    assert!(err.to_string().contains("déjà utilisé"), "{err}");

    let mut client = TcpStream::connect(("127.0.0.1", forward.local_port))
        .await
        .unwrap();
    assert_eq!(round_trip(&mut client, b"x").await, b"x");
    let port = forward.local_port;
    drop(forward);
    // The open connection is cut and the port is free again.
    let mut buf = [0u8; 1];
    let read = timeout(Duration::from_secs(5), client.read(&mut buf))
        .await
        .unwrap();
    assert!(matches!(read, Ok(0) | Err(_)));
    wait_until(|| std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()).await;
}
