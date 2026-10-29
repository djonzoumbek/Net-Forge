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
