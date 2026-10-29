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
            };
            manager.open_telnet(params, SIZE, sinks)
        } else {
            let params = SshParams {
                host: "127.0.0.1".into(),
                port: 2222,
                username: "netforge".into(),
                auth: SshAuth::Password(Zeroizing::new("netforge-test".into())),
                legacy: false,
            };
            manager.open_ssh(params, SIZE, known_hosts.clone(), sinks)
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
