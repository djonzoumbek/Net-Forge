//! Integration tests against busybox telnetd (`./scripts/ssh-test-server.sh start`).

use std::time::Duration;

use tokio::time::{Instant, timeout};

use super::*;

const SIZE: TermSize = TermSize { cols: 80, rows: 24 };

fn port() -> u16 {
    std::env::var("NETFORGE_TELNET_TEST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2323)
}

async fn read_until(t: &mut TelnetTransport, needle: &str, within: Duration) -> String {
    let mut out = Vec::new();
    let deadline = Instant::now() + within;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, t.recv()).await {
            Ok(TransportEvent::Data(d)) => out.extend_from_slice(&d),
            Ok(TransportEvent::Closed(r) | TransportEvent::Lost(r)) => {
                panic!("closed while waiting for {needle:?}: {r}")
            }
            Err(_) => panic!(
                "timeout waiting for {needle:?}, got: {:?}",
                String::from_utf8_lossy(&out)
            ),
        }
        let text = String::from_utf8_lossy(&out);
        if text.contains(needle) {
            return text.into_owned();
        }
    }
}

/// Connects and logs in like a user would on a device console.
async fn login() -> TelnetTransport {
    let params = TelnetParams {
        host: "127.0.0.1".into(),
        port: port(),
        keepalive: Some(Duration::from_secs(15)),
    };
    let mut t = connect(params, SIZE).await.expect("telnet connection");
    read_until(&mut t, "login:", Duration::from_secs(5)).await;
    t.send(b"netforge\r").await.unwrap();
    read_until(&mut t, "Password:", Duration::from_secs(5)).await;
    t.send(b"netforge-test\r").await.unwrap();
    run(&mut t, "true", 0).await;
    t
}

async fn run(t: &mut TelnetTransport, command: &str, n: u32) -> String {
    t.send(format!("{command}; echo END-$(({n}+1))\r").as_bytes())
        .await
        .unwrap();
    read_until(t, &format!("END-{}", n + 1), Duration::from_secs(10)).await
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn login_and_run_a_command() {
    let mut t = login().await;
    assert!(
        !t.info().encrypted,
        "telnet must be flagged as not encrypted"
    );
    assert!(run(&mut t, "whoami", 1).await.contains("netforge"));
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn window_size_is_sent_with_naws() {
    let mut t = login().await;
    assert!(run(&mut t, "stty size", 2).await.contains("24 80"));
    t.resize(TermSize {
        cols: 132,
        rows: 43,
    })
    .await
    .unwrap();
    assert!(run(&mut t, "stty size", 3).await.contains("43 132"));
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn ctrl_c_and_utf8() {
    let mut t = login().await;
    t.send(b"ping -i 0.2 127.0.0.1\r").await.unwrap();
    read_until(&mut t, "bytes from 127.0.0.1", Duration::from_secs(5)).await;
    t.send(b"\x03").await.unwrap();
    read_until(&mut t, "packets transmitted", Duration::from_secs(5)).await;
    let out = run(&mut t, "printf 'é à € ✓\\n'", 4).await;
    assert!(out.contains("é à € ✓"), "{out}");
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn large_output_and_remote_exit() {
    let mut t = login().await;
    t.send(b"yes | head -n 100000; echo END-$((90+1))\r")
        .await
        .unwrap();
    let mut total = 0;
    let mut tail = Vec::new();
    loop {
        match timeout(Duration::from_secs(30), t.recv())
            .await
            .expect("stalled")
        {
            TransportEvent::Data(d) => {
                total += d.len();
                tail.extend_from_slice(&d);
                if tail.len() > 64 {
                    tail.drain(..tail.len() - 64);
                }
                if String::from_utf8_lossy(&tail).contains("END-91") {
                    break;
                }
            }
            TransportEvent::Closed(r) | TransportEvent::Lost(r) => panic!("closed: {r}"),
        }
    }
    assert!(total >= 300_000, "received {total} bytes");

    t.send(b"exit\r").await.unwrap();
    let reason = loop {
        match timeout(Duration::from_secs(5), t.recv())
            .await
            .expect("no close")
        {
            TransportEvent::Data(_) => continue,
            TransportEvent::Closed(reason) => break reason,
            TransportEvent::Lost(reason) => panic!("normal exit reported as lost: {reason}"),
        }
    };
    assert!(reason.contains("fermée par l'équipement"), "{reason}");
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn refused_port_is_reported() {
    let params = TelnetParams {
        host: "127.0.0.1".into(),
        port: 1,
        keepalive: Some(Duration::from_secs(15)),
    };
    let err = connect(params, SIZE).await.err().expect("must fail");
    assert_eq!(err.code(), "unreachable");
}
