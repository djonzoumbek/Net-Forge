//! Integration tests against a real OpenSSH server.
//!
//! Start it with `./scripts/ssh-test-server.sh start`, then run
//! `cargo test -- --ignored`. Ignored by default so `cargo test` needs no Docker.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::oneshot;
use tokio::time::{Instant, timeout};

use super::*;
use crate::security::known_hosts::KnownHosts;

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

fn legacy_port() -> u16 {
    std::env::var("NETFORGE_SSH_LEGACY_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2223)
}

fn params(auth: SshAuth) -> SshParams {
    SshParams {
        host: "127.0.0.1".into(),
        port: port(),
        username: USER.into(),
        auth,
        legacy: false,
        keepalive: Some(Duration::from_secs(15)),
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "netforge-ssh-{tag}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Empty known_hosts (and no ~/.ssh/known_hosts): every test sees the prompt.
fn fresh_known_hosts() -> Arc<KnownHosts> {
    Arc::new(KnownHosts::new(temp_dir("kh").join("known_hosts"), None))
}

/// Shadows `super::connect` for the tests written before known_hosts existed.
async fn connect(
    params: SshParams,
    size: TermSize,
    ask: HostKeyPrompt,
) -> Result<SshTransport, AppError> {
    super::connect(params, size, fresh_known_hosts(), ask).await
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
    assert_eq!(missing.code(), "passphrase_required");
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
            TransportEvent::Closed(r) | TransportEvent::Lost(r) => panic!("closed: {r}"),
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
            TransportEvent::Lost(reason) => panic!("normal exit reported as lost: {reason}"),
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
    // Test parameters use a 15 s keepalive (see `params`).
    let limit = Duration::from_secs(15) * (KEEPALIVE_MAX as u32 + 2);
    let result = timeout(limit, async {
        loop {
            if let TransportEvent::Lost(reason) = t.recv().await {
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

/// Phase 3: the key is asked once, then remembered; a different key is blocked
/// until it is explicitly replaced.
#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn known_hosts_remember_block_and_replace() {
    let kh = fresh_known_hosts();
    let prompts = Arc::new(Mutex::new(Vec::new()));
    let t = super::connect(
        params(password(PASSWORD)),
        SIZE,
        kh.clone(),
        accepting_prompt(prompts.clone()),
    )
    .await
    .unwrap();
    t.close().await;
    assert_eq!(prompts.lock().unwrap().len(), 1, "first connection asks");
    let server_fp = prompts.lock().unwrap()[0].fingerprint.clone();

    // Known now: a prompt would refuse, so success proves it was not asked.
    let t = super::connect(
        params(password(PASSWORD)),
        SIZE,
        kh.clone(),
        rejecting_prompt(),
    )
    .await
    .expect("known host must not prompt");
    t.close().await;

    // Simulate a server whose key changed: record another key for this host.
    let fake = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMpKXV8Haj77LheIxsYi7Za1NIVsqMEvsOUGtoliTK5c";
    std::fs::write(kh.own_file(), format!("[127.0.0.1]:{} {fake}\n", port())).unwrap();
    let err = super::connect(
        params(password(PASSWORD)),
        SIZE,
        kh.clone(),
        accepting_prompt(Arc::default()),
    )
    .await
    .err()
    .expect("changed key must block");
    let AppError::HostKeyChanged(mismatch) = err else {
        panic!("expected HostKeyChanged, got {err:?}");
    };
    assert_eq!(mismatch.new_fingerprint, server_fp);
    assert_ne!(mismatch.old_fingerprint, server_fp);

    kh.replace("127.0.0.1", port(), &mismatch.new_fingerprint)
        .unwrap();
    let t = super::connect(params(password(PASSWORD)), SIZE, kh, rejecting_prompt())
        .await
        .expect("replaced key is trusted");
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start (legacy server on 2223)"]
async fn legacy_server_needs_compatibility_mode() {
    let mut p = params(password(PASSWORD));
    p.port = legacy_port();
    let err = connect(p, SIZE, accepting_prompt(Arc::default()))
        .await
        .err()
        .expect("modern-only must fail");
    assert_eq!(err.code(), "no_common_algorithm");
    let message = err.to_string();
    assert!(
        message.contains("diffie-hellman-group1-sha1"),
        "server offer shown: {message}"
    );
    assert!(
        message.contains("mode compatibilité"),
        "suggestion shown: {message}"
    );

    let mut p = params(password(PASSWORD));
    p.port = legacy_port();
    p.legacy = true;
    let mut t = connect(p, SIZE, accepting_prompt(Arc::default()))
        .await
        .expect("compatibility mode must connect");
    let algorithms = t.info().algorithms.unwrap();
    eprintln!("negotiated: {algorithms:?}");
    assert!(algorithms.legacy.contains(&"ssh-rsa".to_string()));
    assert!(algorithms.legacy.iter().any(|a| a.contains("-cbc")));
    assert!(algorithms.legacy.contains(&"hmac-sha1".to_string()));
    assert!(run(&mut t, "whoami", 60).await.contains(USER));
    t.close().await;
}

#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn modern_server_negotiates_no_legacy_algorithm() {
    let mut p = params(password(PASSWORD));
    p.legacy = true; // even with the mode enabled, modern wins on a modern server
    let t = connect(p, SIZE, accepting_prompt(Arc::default()))
        .await
        .unwrap();
    let algorithms = t.info().algorithms.unwrap();
    assert!(algorithms.legacy.is_empty(), "{algorithms:?}");
    t.close().await;
}

/// Starts a private ssh-agent holding the test key (no passphrase), so the test
/// never touches the user's own agent.
#[tokio::test]
#[ignore = "needs ./scripts/ssh-test-server.sh start and ssh-agent"]
async fn agent_login() {
    let dir = temp_dir("agent");
    let socket = dir.join("agent.sock");
    let mut agent = std::process::Command::new("ssh-agent")
        .arg("-D")
        .arg("-a")
        .arg(&socket)
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("ssh-agent");
    for _ in 0..50 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let added = std::process::Command::new("ssh-add")
        .arg(key_path("id_plain"))
        .env("SSH_AUTH_SOCK", &socket)
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ssh-add");
    assert!(added.success());

    let result = connect(
        params(SshAuth::Agent {
            socket: Some(socket.clone()),
        }),
        SIZE,
        accepting_prompt(Arc::default()),
    )
    .await;
    let _ = agent.kill();
    let _ = agent.wait(); // reap the process: no zombie left behind
    let mut t = result.expect("agent authentication");
    assert!(run(&mut t, "whoami", 70).await.contains(USER));
    t.close().await;

    // Empty agent: clear message, not a generic failure.
    let mut empty = std::process::Command::new("ssh-agent")
        .args(["-D", "-a"])
        .arg(dir.join("empty.sock"))
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("ssh-agent");
    for _ in 0..50 {
        if dir.join("empty.sock").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let err = connect(
        params(SshAuth::Agent {
            socket: Some(dir.join("empty.sock")),
        }),
        SIZE,
        accepting_prompt(Arc::default()),
    )
    .await
    .err()
    .expect("empty agent must fail");
    let _ = empty.kill();
    let _ = empty.wait(); // reap the process: no zombie left behind
    assert_eq!(err.code(), "agent");
}
