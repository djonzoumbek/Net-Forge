//! Integration tests against a real OpenSSH server.
//!
//! Start it with `./scripts/ssh-test-server.sh start`, then run
//! `cargo test -- --ignored`. Ignored by default so `cargo test` needs no Docker.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

use super::*;

const USER: &str = "netforge";
const PASSWORD: &str = "netforge-test";
const SIZE: TermSize = TermSize { cols: 80, rows: 24 };

fn port() -> u16 {
    std::env::var("NETFORGE_SSH_TEST_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2222)
}

fn key_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/ssh-test")
        .join(name)
}

fn params(auth: SshAuth) -> SshParams {
    SshParams {
        host: "127.0.0.1".into(),
        port: port(),
        username: USER.into(),
        auth,
    }
}

fn password(p: &str) -> SshAuth {
    SshAuth::Password(Zeroizing::new(p.into()))
}

/// Accepts the host key *explicitly* and records what was shown: verification is
/// exercised, never bypassed.
fn accepting_prompt(seen: Arc<Mutex<Vec<HostKeyInfo>>>) -> HostKeyPrompt {
    Box::new(move |info| {
        seen.lock().unwrap().push(info);
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(true);
        rx
    })
}

fn rejecting_prompt() -> HostKeyPrompt {
    Box::new(|_| {
        let (tx, rx) = oneshot::channel();
        let _ = tx.send(false);
        rx
    })
}

async fn open(auth: SshAuth) -> SshTransport {
    connect(params(auth), SIZE, accepting_prompt(Arc::default()))
        .await
        .expect("connection to the test server")
}

/// Reads output until `needle` appears; returns everything read.
async fn read_until(t: &mut SshTransport, needle: &str, within: Duration) -> String {
    let mut out = Vec::new();
    let deadline = Instant::now() + within;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, t.recv()).await {
            Ok(TransportEvent::Data(d)) => out.extend_from_slice(&d),
            Ok(TransportEvent::Closed(r)) => panic!("closed while waiting for {needle:?}: {r}"),
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

/// Runs a command and waits for a unique end marker. The marker is computed by the
/// shell (`$((...))`) so that the echo of the typed command never matches it.
async fn run(t: &mut SshTransport, command: &str, n: u32) -> String {
    t.send(format!("{command}; echo END-$(({n}+1))\r").as_bytes())
        .await
        .unwrap();
    read_until(t, &format!("END-{}", n + 1), Duration::from_secs(10)).await
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn password_login_shows_sha256_fingerprint() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut t = connect(
        params(password(PASSWORD)),
        SIZE,
        accepting_prompt(seen.clone()),
    )
    .await
    .unwrap();
    let info = seen.lock().unwrap()[0].clone();
    assert!(info.fingerprint.starts_with("SHA256:"), "{info:?}");
    assert_eq!(info.port, port());
    let out = run(&mut t, "whoami", 1).await;
    assert!(out.contains(USER));
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn rejected_host_key_blocks_the_connection() {
    let err = connect(params(password(PASSWORD)), SIZE, rejecting_prompt())
        .await
        .err()
        .expect("must fail");
    assert_eq!(err.code(), "host_key_rejected");
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn key_login_with_and_without_passphrase() {
    let mut t = open(SshAuth::KeyFile {
        path: key_path("id_plain"),
        passphrase: None,
    })
    .await;
    assert!(run(&mut t, "whoami", 2).await.contains(USER));
    t.close().await;

    let mut t = open(SshAuth::KeyFile {
        path: key_path("id_pass"),
        passphrase: Some(Zeroizing::new("test-passphrase".into())),
    })
    .await;
    assert!(run(&mut t, "whoami", 3).await.contains(USER));
    t.close().await;

    let missing = connect(
        params(SshAuth::KeyFile {
            path: key_path("id_pass"),
            passphrase: None,
        }),
        SIZE,
        accepting_prompt(Arc::default()),
    )
    .await
    .err()
    .expect("encrypted key without passphrase must fail");
    assert_eq!(missing.code(), "private_key");
    assert!(missing.to_string().contains("passphrase"), "{missing}");
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn resize_is_seen_by_stty() {
    let mut t = open(password(PASSWORD)).await;
    assert!(run(&mut t, "stty size", 10).await.contains("24 80"));
    t.resize(TermSize {
        cols: 132,
        rows: 43,
    })
    .await
    .unwrap();
    assert!(run(&mut t, "stty size", 11).await.contains("43 132"));
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn ctrl_c_interrupts_ping() {
    let mut t = open(password(PASSWORD)).await;
    t.send(b"ping -i 0.2 127.0.0.1\r").await.unwrap();
    read_until(&mut t, "bytes from 127.0.0.1", Duration::from_secs(5)).await;
    t.send(b"\x03").await.unwrap();
    // Statistics are only printed when ping is interrupted.
    read_until(&mut t, "packets transmitted", Duration::from_secs(5)).await;
    assert!(run(&mut t, "true", 20).await.contains("END-21"));
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn utf8_round_trip() {
    let mut t = open(password(PASSWORD)).await;
    let out = run(&mut t, "printf 'é à ç € ✓ 日本\\n'", 30).await;
    assert!(out.contains("é à ç € ✓ 日本"), "{out}");
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn large_output_is_fully_received() {
    let mut t = open(password(PASSWORD)).await;
    let start = Instant::now();
    t.send(b"yes | head -n 200000; echo END-$((40+1))\r")
        .await
        .unwrap();
    let mut total = 0usize;
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
                if String::from_utf8_lossy(&tail).contains("END-41") {
                    break;
                }
            }
            TransportEvent::Closed(r) => panic!("closed: {r}"),
        }
    }
    // 200 000 lines of "y\r\n" = 600 000 bytes, plus prompt and echo.
    assert!(total >= 600_000, "received {total} bytes");
    eprintln!("200k lines received in {:?}", start.elapsed());
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn remote_exit_is_detected() {
    let mut t = open(password(PASSWORD)).await;
    t.send(b"exit 3\r").await.unwrap();
    let closed = loop {
        match timeout(Duration::from_secs(5), t.recv())
            .await
            .expect("no close")
        {
            TransportEvent::Data(_) => continue,
            TransportEvent::Closed(reason) => break reason,
        }
    };
    assert!(closed.contains("code de sortie 3"), "{closed}");
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn distinct_errors_for_auth_refused_and_unknown_host() {
    let wrong = connect(
        params(password("wrong")),
        SIZE,
        accepting_prompt(Arc::default()),
    )
    .await
    .err()
    .expect("must fail");
    assert_eq!(wrong.code(), "auth_failed");

    let mut p = params(password(PASSWORD));
    p.port = 1; // nothing listens there
    let refused = connect(p, SIZE, accepting_prompt(Arc::default()))
        .await
        .err()
        .expect("must fail");
    assert_eq!(refused.code(), "unreachable");

    let mut p = params(password(PASSWORD));
    p.host = "host-that-does-not-exist.invalid".into();
    let unknown = connect(p, SIZE, accepting_prompt(Arc::default()))
        .await
        .err()
        .expect("must fail");
    assert_eq!(unknown.code(), "host_not_found");
}

#[tokio::test]
#[ignore = "needs network: waits for the TCP timeout (10 s)"]
async fn unanswered_tcp_connection_times_out() {
    let mut p = params(password(PASSWORD));
    // TEST-NET-1 (RFC 5737): reserved for documentation, never routed, so SYNs
    // are silently dropped (or rejected immediately if there is no default route).
    p.host = "192.0.2.1".into();
    p.port = 22;
    let err = connect(p, SIZE, accepting_prompt(Arc::default()))
        .await
        .err()
        .expect("must fail");
    assert!(
        matches!(err.code(), "timeout" | "unreachable"),
        "{}: {err}",
        err.code()
    );
}

/// Freezes the container (`docker pause`): the server stops answering without
/// closing the TCP connection, exactly like a network cut. Takes about a minute.
#[tokio::test]
#[ignore = "needs the test server; pauses the Docker container for ~1 min"]
async fn network_cut_is_detected_by_keepalive() {
    let mut t = open(password(PASSWORD)).await;
    run(&mut t, "true", 50).await;

    let docker = |action: &str| {
        let status = std::process::Command::new("docker")
            .args([action, "netforge-sshd"])
            .status()
            .expect("docker command");
        assert!(status.success(), "docker {action} failed");
    };
    docker("pause");
    let start = Instant::now();
    let limit = KEEPALIVE_INTERVAL * (KEEPALIVE_MAX as u32 + 2);
    let result = timeout(limit, async {
        loop {
            if let TransportEvent::Closed(reason) = t.recv().await {
                return reason;
            }
        }
    })
    .await;
    docker("unpause");

    let reason = result.expect("network cut not detected in time");
    eprintln!("detected after {:?}: {reason}", start.elapsed());
    assert!(reason.contains("Connexion perdue"), "{reason}");
}
