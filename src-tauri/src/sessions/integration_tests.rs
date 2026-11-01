//! Many real sessions at once through the `SessionManager` (Docker test servers).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{Instant, sleep};
use zeroize::Zeroizing;

use super::*;
use crate::transport::ssh::SshAuth;

const SIZE: TermSize = TermSize { cols: 80, rows: 24 };
const SESSIONS: usize = 10;

/// Indices 0..=6 are SSH sessions, 7..=9 Telnet sessions.
fn is_telnet(index: usize) -> bool {
    index >= 7
}

struct Harness {
    manager: SessionManager,
    ids: Vec<SessionId>,
    outputs: Vec<Arc<Mutex<Vec<u8>>>>,
    events: mpsc::UnboundedReceiver<(usize, SessionEvent)>,
}

impl Harness {
    fn output(&self, index: usize) -> String {
        String::from_utf8_lossy(&self.outputs[index].lock().unwrap()).into_owned()
    }

    async fn wait_output(&self, index: usize, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !self.output(index).contains(needle) {
            assert!(
                Instant::now() < deadline,
                "session {index}: {needle:?} not received"
            );
            sleep(Duration::from_millis(20)).await;
        }
    }

    async fn write(&self, index: usize, text: &str) {
        self.manager
            .send(
                self.ids[index],
                SessionCommand::Write(text.as_bytes().to_vec()),
            )
            .await
            .unwrap();
    }

    /// Waits for an event of `index` matching `pred`, answering host key prompts.
    async fn wait_event(&mut self, mut pred: impl FnMut(usize, &SessionEvent) -> bool) -> usize {
        loop {
            let (index, event) = tokio::time::timeout(Duration::from_secs(20), self.events.recv())
                .await
                .expect("event timeout")
                .expect("events channel closed");
            if let SessionEvent::HostKeyPrompt { .. } = event {
                self.manager.answer_host_key(self.ids[index], true).unwrap();
            }
            if let SessionEvent::Failed { message, .. } = &event {
                panic!("session {index} failed: {message}");
            }
            if pred(index, &event) {
                return index;
            }
        }
    }
}

fn open_all() -> Harness {
    let manager = SessionManager::default();
    let dir = std::env::temp_dir().join(format!("netforge-multi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join("known_hosts"));
    let known_hosts = Arc::new(KnownHosts::new(dir.join("known_hosts"), None));
    let (events_tx, events) = mpsc::unbounded_channel();
    let mut ids = Vec::new();
    let mut outputs = Vec::new();

    for index in 0..SESSIONS {
        let output = Arc::new(Mutex::new(Vec::new()));
        let (out, tx) = (output.clone(), events_tx.clone());
        let sinks = SessionSinks {
            data: Box::new(move |bytes| out.lock().unwrap().extend_from_slice(&bytes)),
            event: Box::new(move |event| {
                let _ = tx.send((index, event));
            }),
        };
        let id = if is_telnet(index) {
            let params = TelnetParams {
                host: "127.0.0.1".into(),
                port: 2323,
                keepalive: Some(Duration::from_secs(15)),
            };
            manager.open_telnet(params, SIZE, Automation::default(), sinks)
        } else {
            let params = SshParams {
                host: "127.0.0.1".into(),
                port: 2222,
                username: "netforge".into(),
                auth: SshAuth::Password(Zeroizing::new("netforge-test".into())),
                legacy: false,
                keepalive: Some(Duration::from_secs(15)),
                jump: None,
            };
            manager.open_ssh(
                params,
                SIZE,
                known_hosts.clone(),
                Automation::default(),
                sinks,
            )
        };
        ids.push(id);
        outputs.push(output);
    }
    Harness {
        manager,
        ids,
        outputs,
        events,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn ten_sessions_are_isolated_and_independent() {
    let mut h = open_all();
    let mut connected = 0;
    while connected < SESSIONS {
        h.wait_event(|_, e| matches!(e, SessionEvent::Connected { .. }))
            .await;
        connected += 1;
    }
    assert_eq!(h.manager.len(), SESSIONS);

    for index in (0..SESSIONS).filter(|&i| is_telnet(i)) {
        h.wait_output(index, "login:").await;
        h.write(index, "netforge\r").await;
        h.wait_output(index, "Password:").await;
        h.write(index, "netforge-test\r").await;
    }

    // Every session prints its own marker at the same time.
    for index in 0..SESSIONS {
        let command = format!(
            "for n in $(seq 1 2000); do echo MARK-{index}-$n; done; echo DONE-$(({index}+1000))\r"
        );
        h.write(index, &command).await;
    }
    for index in 0..SESSIONS {
        h.wait_output(index, &format!("DONE-{}", index + 1000))
            .await;
    }
    for index in 0..SESSIONS {
        let output = h.output(index);
        assert!(output.contains(&format!("MARK-{index}-2000")));
        for other in (0..SESSIONS).filter(|&o| o != index) {
            assert!(
                !output.contains(&format!("MARK-{other}-")),
                "output of session {other} leaked into session {index}"
            );
        }
    }

    // Closing one session (SSH #3) does not disturb the others.
    h.manager
        .send(h.ids[3], SessionCommand::Close)
        .await
        .unwrap();
    let closed = h
        .wait_event(|i, e| i == 3 && matches!(e, SessionEvent::Closed { .. }))
        .await;
    assert_eq!(closed, 3);
    for index in (0..SESSIONS).filter(|&i| i != 3) {
        h.write(index, &format!("echo AFTER-$(({index}+2000))\r"))
            .await;
    }
    for index in (0..SESSIONS).filter(|&i| i != 3) {
        h.wait_output(index, &format!("AFTER-{}", index + 2000))
            .await;
    }
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        h.manager.len(),
        SESSIONS - 1,
        "only the closed session is released"
    );

    for index in (0..SESSIONS).filter(|&i| i != 3) {
        h.manager
            .send(h.ids[index], SessionCommand::Close)
            .await
            .unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while h.manager.len() > 0 {
        assert!(Instant::now() < deadline, "sessions not released");
        sleep(Duration::from_millis(20)).await;
    }
}

// ---------- Phase 5: keepalive, sequences, pager, login commands ----------

/// One session through the manager, recording output and events.
struct Single {
    manager: SessionManager,
    id: SessionId,
    output: Arc<Mutex<Vec<u8>>>,
    events: mpsc::UnboundedReceiver<SessionEvent>,
}

impl Single {
    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    async fn wait_output(&self, needle: &str, within: Duration) {
        let deadline = Instant::now() + within;
        while !self.output().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "{needle:?} not received: {:?}",
                self.output()
            );
            sleep(Duration::from_millis(20)).await;
        }
    }

    async fn write(&self, text: &str) {
        self.manager
            .send(self.id, SessionCommand::Write(text.as_bytes().to_vec()))
            .await
            .unwrap();
    }

    /// Next event that is not a host key prompt (prompts are accepted).
    async fn next_event(&mut self, within: Duration) -> Option<SessionEvent> {
        loop {
            let event = tokio::time::timeout(within, self.events.recv())
                .await
                .ok()??;
            if let SessionEvent::HostKeyPrompt { .. } = event {
                self.manager.answer_host_key(self.id, true).unwrap();
                continue;
            }
            return Some(event);
        }
    }

    async fn wait_connected(&mut self) {
        match self.next_event(Duration::from_secs(20)).await {
            Some(SessionEvent::Connected { .. }) => {}
            other => panic!("expected Connected, got {other:?}"),
        }
    }
}

fn single(open: impl FnOnce(&SessionManager, SessionSinks) -> SessionId) -> Single {
    let manager = SessionManager::default();
    let output = Arc::new(Mutex::new(Vec::new()));
    let (tx, events) = mpsc::unbounded_channel();
    let out = output.clone();
    let sinks = SessionSinks {
        data: Box::new(move |bytes| out.lock().unwrap().extend_from_slice(&bytes)),
        event: Box::new(move |event| {
            let _ = tx.send(event);
        }),
    };
    let id = open(&manager, sinks);
    Single {
        manager,
        id,
        output,
        events,
    }
}

fn ssh_params(port: u16, keepalive: Option<Duration>) -> SshParams {
    SshParams {
        host: "127.0.0.1".into(),
        port,
        username: "netforge".into(),
        auth: SshAuth::Password(Zeroizing::new("netforge-test".into())),
        legacy: false,
        keepalive,
        jump: None,
    }
}

fn fresh_known_hosts(tag: &str) -> Arc<KnownHosts> {
    let dir = std::env::temp_dir().join(format!("netforge-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join("known_hosts"));
    Arc::new(KnownHosts::new(dir.join("known_hosts"), None))
}

async fn open_ssh_session(
    port: u16,
    keepalive: Option<Duration>,
    automation: Automation,
) -> Single {
    let known_hosts = fresh_known_hosts(&format!("ssh{port}"));
    let mut s = single(|m, sinks| {
        m.open_ssh(
            ssh_params(port, keepalive),
            SIZE,
            known_hosts,
            automation,
            sinks,
        )
    });
    s.wait_connected().await;
    s
}

async fn open_telnet_session(
    port: u16,
    keepalive: Option<Duration>,
    automation: Automation,
) -> Single {
    let params = TelnetParams {
        host: "127.0.0.1".into(),
        port,
        keepalive,
    };
    let mut s = single(|m, sinks| m.open_telnet(params, SIZE, automation, sinks));
    s.wait_connected().await;
    s.wait_output("login:", Duration::from_secs(5)).await;
    s.write("netforge\r").await;
    s.wait_output("Password:", Duration::from_secs(5)).await;
    s.write("netforge-test\r").await;
    s
}

const IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const KEEPALIVE: Duration = Duration::from_secs(1);

/// Scaled-down version of "an idle session stays open 30 min with keepalive":
/// a firewall-like proxy drops connections idle for 3 s; the session stays idle
/// for 3 × that time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start; ~20 s"]
async fn keepalive_keeps_idle_ssh_session_through_idle_firewall() {
    let port =
        crate::transport::idle_proxy::start("127.0.0.1:2222".parse().unwrap(), IDLE_TIMEOUT).await;

    let s = open_ssh_session(port, Some(KEEPALIVE), Automation::default()).await;
    sleep(IDLE_TIMEOUT * 3).await;
    s.write("echo ALIVE-$((40+2))\r").await;
    s.wait_output("ALIVE-42", Duration::from_secs(5)).await;

    // Control: without keepalive the same firewall kills the session.
    let mut s = open_ssh_session(port, None, Automation::default()).await;
    match s.next_event(IDLE_TIMEOUT * 4).await {
        Some(SessionEvent::Closed { lost: true, .. }) => {}
        other => panic!("idle session without keepalive should be lost, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start; ~15 s"]
async fn keepalive_keeps_idle_telnet_session_through_idle_firewall() {
    let port =
        crate::transport::idle_proxy::start("127.0.0.1:2323".parse().unwrap(), IDLE_TIMEOUT).await;
    let s = open_telnet_session(port, Some(KEEPALIVE), Automation::default()).await;
    s.write("true\r").await;
    sleep(IDLE_TIMEOUT * 3).await;
    s.write("echo ALIVE-$((40+3))\r").await;
    s.wait_output("ALIVE-43", Duration::from_secs(5)).await;
    assert!(
        !s.output().contains('\u{fffd}'),
        "NOP must not show up as garbage"
    );
}

/// "A button sends a sequence of 5 commands correctly", over SSH and Telnet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn sequence_of_five_commands_runs_in_order() {
    let lines: Vec<String> = (1..=5).map(|n| format!("echo STEP-$(({n}*10))")).collect();
    for telnet in [false, true] {
        let s = if telnet {
            open_telnet_session(2323, Some(KEEPALIVE), Automation::default()).await
        } else {
            open_ssh_session(2222, Some(KEEPALIVE), Automation::default()).await
        };
        s.manager
            .send(
                s.id,
                SessionCommand::Sequence {
                    lines: lines.clone(),
                    delay: Duration::from_millis(200),
                },
            )
            .await
            .unwrap();
        s.wait_output("STEP-50", Duration::from_secs(10)).await;
        let output = s.output();
        let positions: Vec<usize> = (1..=5)
            .map(|n| {
                output
                    .find(&format!("STEP-{}", n * 10))
                    .expect("each step ran")
            })
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "order kept (telnet: {telnet})"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn pager_is_answered_and_login_commands_run_at_first_prompt() {
    let automation = Automation {
        pager_auto_reply: true,
        login_commands: vec!["echo LOGIN-$((40+4))".into()],
        log: None,
    };
    // busybox `more` shows "--More--" on a 24-line terminal: 5000 lines need ~200 answers.
    for telnet in [false, true] {
        let s = if telnet {
            open_telnet_session(2323, None, automation.clone()).await
        } else {
            open_ssh_session(2222, None, automation.clone()).await
        };
        s.wait_output("LOGIN-44", Duration::from_secs(10)).await;
        s.write("more big.txt; echo PAGED-$((40+5))\r").await;
        s.wait_output("PAGED-45", Duration::from_secs(30)).await;
        let output = s.output();
        assert!(output.contains("ligne numéro 5000"), "telnet: {telnet}");
        let pages = output.matches("--More--").count();
        assert!(
            pages > 100,
            "the pager really paused ({pages} times), telnet: {telnet}"
        );
    }
}

// ---------- Phase 6: session log ----------

/// Criterion: "the log file is readable and has no stray escape sequences
/// (plain text option)", with colours, a pager and its erasures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn text_session_log_is_clean_and_readable() {
    use crate::persistence::profiles::LogMode;
    use crate::sessions::logger::LogConfig;

    let dir = std::env::temp_dir().join(format!("netforge-session-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let automation = Automation {
        pager_auto_reply: true,
        login_commands: vec![],
        log: Some(LogConfig {
            mode: LogMode::Text,
            dir: dir.clone(),
            title: "test-server".into(),
        }),
    };
    let mut s = {
        let known_hosts = fresh_known_hosts("log");
        single(|m, sinks| m.open_ssh(ssh_params(2222, None), SIZE, known_hosts, automation, sinks))
    };
    let log_file = match s.next_event(Duration::from_secs(20)).await {
        Some(SessionEvent::Connected {
            log_file: Some(path),
            ..
        }) => path,
        other => panic!("expected Connected with a log file, got {other:?}"),
    };
    s.write("ls --color=always /etc | head -3; printf '\\033[1;31mrouge\\033[0m\\n'; more big.txt; echo FIN-$((40+6))\r")
        .await;
    s.wait_output("FIN-46", Duration::from_secs(20)).await;
    s.manager.send(s.id, SessionCommand::Close).await.unwrap();
    while s.manager.len() > 0 {
        sleep(Duration::from_millis(20)).await;
    }

    let mut content = String::new();
    for _ in 0..200 {
        content = std::fs::read_to_string(&log_file).unwrap_or_default();
        if content.contains("# Fin de session") {
            break;
        }
        sleep(Duration::from_millis(10)).await;
    }
    assert!(
        content.starts_with("# NetForge — test-server — début"),
        "{content}"
    );
    assert!(
        content.contains("\nrouge\n"),
        "coloured text kept as plain text"
    );
    assert!(
        content.contains("ligne numéro 5000\n"),
        "whole file paged through"
    );
    assert!(
        !content.contains("--More--"),
        "erased pager prompts must not remain in the text log"
    );
    assert!(content.contains("FIN-46"));
    let control = content
        .chars()
        .find(|c| c.is_control() && *c != '\n' && *c != '\t');
    assert_eq!(
        control, None,
        "no escape sequence or control character in the text log"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

// ---------- Port forwarding ----------

/// Reads the first bytes the forwarded service sends.
async fn read_banner(stream: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buf))
        .await
        .expect("banner in time")
        .unwrap();
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn local_and_socks_forwards_go_through_the_ssh_server() {
    use crate::forwarding::{ForwardKind, ForwardSpec};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut s = open_ssh_session(2222, None, Automation::default()).await;
    // Target seen from *inside* the container: its own sshd on port 22. Its
    // banner proves that the bytes crossed the tunnel.
    let local = s
        .manager
        .start_forward(
            s.id,
            ForwardSpec {
                kind: ForwardKind::Local,
                bind_address: "127.0.0.1".into(),
                bind_port: 0,
                target_host: "127.0.0.1".into(),
                target_port: 22,
            },
        )
        .await
        .unwrap();
    let mut client = TcpStream::connect(("127.0.0.1", local.local_port))
        .await
        .unwrap();
    assert!(read_banner(&mut client).await.starts_with("SSH-2.0-"));

    let socks = s
        .manager
        .start_forward(
            s.id,
            ForwardSpec {
                kind: ForwardKind::Dynamic,
                bind_address: "127.0.0.1".into(),
                bind_port: 0,
                target_host: String::new(),
                target_port: 0,
            },
        )
        .await
        .unwrap();
    let mut proxied = TcpStream::connect(("127.0.0.1", socks.local_port))
        .await
        .unwrap();
    proxied.write_all(&[5, 1, 0]).await.unwrap();
    let mut method = [0u8; 2];
    proxied.read_exact(&mut method).await.unwrap();
    // "localhost" is resolved by the SSH server, as with `curl --socks5-hostname`.
    let mut request = vec![5, 1, 0, 3, 9];
    request.extend_from_slice(b"localhost");
    request.extend_from_slice(&22u16.to_be_bytes());
    proxied.write_all(&request).await.unwrap();
    let mut reply = [0u8; 10];
    proxied.read_exact(&mut reply).await.unwrap();
    assert_eq!(reply[1], 0, "SOCKS CONNECT accepted");
    assert!(read_banner(&mut proxied).await.starts_with("SSH-2.0-"));

    // A target the server cannot reach: the error is recorded on the forward.
    let refused = s
        .manager
        .start_forward(
            s.id,
            ForwardSpec {
                kind: ForwardKind::Local,
                bind_address: "127.0.0.1".into(),
                bind_port: 0,
                target_host: "127.0.0.1".into(),
                target_port: 9,
            },
        )
        .await
        .unwrap();
    let mut dead = TcpStream::connect(("127.0.0.1", refused.local_port))
        .await
        .unwrap();
    let mut buf = [0u8; 1];
    let _ = tokio::time::timeout(Duration::from_secs(10), dead.read(&mut buf)).await;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let list = s.manager.list_forwards(s.id).unwrap();
        let info = list.iter().find(|f| f.id == refused.id).unwrap();
        if let Some(error) = &info.last_error {
            assert!(error.contains("refusé par le serveur SSH"), "{error}");
            break;
        }
        assert!(Instant::now() < deadline, "error not recorded");
        sleep(Duration::from_millis(50)).await;
    }

    // Stopping one forward frees its port; closing the session frees the others.
    s.manager.stop_forward(s.id, refused.id).unwrap();
    assert_eq!(s.manager.list_forwards(s.id).unwrap().len(), 2);
    s.manager.send(s.id, SessionCommand::Close).await.unwrap();
    while s
        .next_event(Duration::from_secs(10))
        .await
        .is_some_and(|e| !matches!(e, SessionEvent::Closed { .. }))
    {}
    let deadline = Instant::now() + Duration::from_secs(10);
    for port in [local.local_port, socks.local_port, refused.local_port] {
        while std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "port {port} still in use");
            sleep(Duration::from_millis(50)).await;
        }
    }
}

// ---------- Jump hosts (ssh -J) ----------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn session_through_jump_hosts_checks_every_host_key() {
    // Bastion: the container's sshd from here (127.0.0.1:2222). Target: the same
    // sshd seen from *inside* the container (127.0.0.1:22), so only reachable
    // through the bastion. A second jump makes a chain of two.
    let target = |jump: Option<Box<SshParams>>| SshParams {
        host: "127.0.0.1".into(),
        port: 22,
        jump,
        ..ssh_params(2222, None)
    };
    for chain in [1usize, 2] {
        let mut params = target(Some(Box::new(ssh_params(2222, None))));
        if chain == 2 {
            params = target(Some(Box::new(params)));
        }
        let known_hosts = fresh_known_hosts(&format!("jump{chain}"));
        let mut s =
            single(|m, sinks| m.open_ssh(params, SIZE, known_hosts, Automation::default(), sinks));
        let mut prompted = Vec::new();
        let via = loop {
            let event = tokio::time::timeout(Duration::from_secs(20), s.events.recv())
                .await
                .expect("event in time")
                .expect("events open");
            match event {
                SessionEvent::HostKeyPrompt { info } => {
                    prompted.push(format!("{}:{}", info.host, info.port));
                    s.manager.answer_host_key(s.id, true).unwrap();
                }
                SessionEvent::Connected { info, .. } => break info.via,
                other => panic!("unexpected {other:?}"),
            }
        };
        // Farthest first: the bastion is contacted (and verified) before the target.
        assert_eq!(prompted[0], "127.0.0.1:2222", "chain {chain}: {prompted:?}");
        assert_eq!(prompted.last().unwrap(), "127.0.0.1:22");
        assert_eq!(prompted.len(), 2, "same key for both hops of the chain");
        assert_eq!(via.len(), chain, "{via:?}");
        assert_eq!(via[0], "netforge@127.0.0.1:2222");
        s.write("echo JUMP-$((6*7))\n").await;
        s.wait_output("JUMP-42", Duration::from_secs(10)).await;
        s.manager.send(s.id, SessionCommand::Close).await.unwrap();
    }

    // The bastion cannot reach the target: a jump error, not "unreachable".
    let params = SshParams {
        port: 9,
        ..target(Some(Box::new(ssh_params(2222, None))))
    };
    let known_hosts = fresh_known_hosts("jump-refused");
    let mut s =
        single(|m, sinks| m.open_ssh(params, SIZE, known_hosts, Automation::default(), sinks));
    match s.next_event(Duration::from_secs(20)).await {
        Some(SessionEvent::Failed { code, message }) => {
            assert_eq!(code, "jump");
            assert!(
                message.contains("n'a pas pu joindre 127.0.0.1:9"),
                "{message}"
            );
        }
        other => panic!("expected a jump failure, got {other:?}"),
    }
}
