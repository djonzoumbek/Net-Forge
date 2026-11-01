//! The loop that drives one connected session, whatever its transport.

use std::collections::VecDeque;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};

use super::automation::{OutputWatcher, Reaction};
use super::logger::{LogConfig, SessionLogger};
use super::{SessionCommand, SessionEvent, SessionId, SessionSinks};
use crate::transport::{Transport, TransportEvent};

/// Output is grouped for at most this long before being sent to the UI.
/// ~one frame at 120 Hz: invisible for the user, but divides IPC messages by a lot
/// when a command prints thousands of small fragments.
const FLUSH_DELAY: Duration = Duration::from_millis(8);
/// A batch is sent immediately once it reaches this size.
const MAX_BATCH: usize = 64 * 1024;
/// Maximum amount of output sent to the UI but not yet acknowledged as rendered.
/// Above it we stop reading the transport: the remote side is then slowed down by
/// SSH/TCP flow control instead of letting memory grow without limit.
pub(crate) const MAX_UNACKED: usize = 1024 * 1024;
/// Pause between login commands: lets slow devices print their prompt.
const LOGIN_COMMAND_DELAY: Duration = Duration::from_millis(300);

/// Automatic behaviours and recording configured per profile.
#[derive(Debug, Clone, Default)]
pub struct Automation {
    /// Answer `--More--`-like pager prompts with a space.
    pub pager_auto_reply: bool,
    /// Sent once, when the first prompt appears (e.g. `terminal length 0`).
    pub login_commands: Vec<String>,
    /// Record the session output in a file.
    pub log: Option<LogConfig>,
}

/// Lines waiting to be sent: each one after its own delay.
#[derive(Default)]
struct SendQueue {
    lines: VecDeque<(Vec<u8>, Duration)>,
    next_at: Option<Instant>,
}

impl SendQueue {
    /// Each line is typed followed by Enter (`\r`); `delay` separates the lines
    /// of this sequence (the first one leaves right after what is already queued).
    fn push(&mut self, lines: &[String], delay: Duration) {
        for (index, line) in lines.iter().enumerate() {
            let mut bytes = line.as_bytes().to_vec();
            bytes.push(b'\r');
            self.lines
                .push_back((bytes, if index == 0 { Duration::ZERO } else { delay }));
        }
        if self.next_at.is_none()
            && let Some((_, first_delay)) = self.lines.front()
        {
            self.next_at = Some(Instant::now() + *first_delay);
        }
    }

    fn pop(&mut self) -> Option<Vec<u8>> {
        let (bytes, _) = self.lines.pop_front()?;
        self.next_at = self.lines.front().map(|(_, delay)| Instant::now() + *delay);
        Some(bytes)
    }
}

pub(crate) async fn run_session<T: Transport>(
    id: SessionId,
    mut transport: T,
    mut commands: mpsc::Receiver<SessionCommand>,
    sinks: &SessionSinks,
    automation: Automation,
    mut logger: Option<SessionLogger>,
) {
    let mut batch: Vec<u8> = Vec::new();
    let mut flush_at: Option<Instant> = None;
    let mut unacked: usize = 0;
    let mut queue = SendQueue::default();
    let mut watcher = OutputWatcher::new(
        automation.pager_auto_reply,
        !automation.login_commands.is_empty(),
    );

    let flush = |batch: &mut Vec<u8>, flush_at: &mut Option<Instant>, unacked: &mut usize| {
        if !batch.is_empty() {
            *unacked += batch.len();
            (sinks.data)(std::mem::take(batch));
        }
        *flush_at = None;
    };

    // `Some((reason, lost))` when the remote side ended the session.
    let remote_end: Option<(String, bool)> = loop {
        let can_read = unacked < MAX_UNACKED;
        tokio::select! {
            // `biased`: check user commands first, so that Ctrl+C or "close" are never
            // starved by a remote command that floods the terminal.
            biased;

            cmd = commands.recv() => match cmd {
                Some(SessionCommand::Write(data)) => {
                    if let Err(e) = transport.send(&data).await {
                        break Some((format!("Envoi impossible : {e}"), true));
                    }
                }
                Some(SessionCommand::Sequence { lines, delay }) => queue.push(&lines, delay),
                Some(SessionCommand::Resize(size)) => {
                    if let Err(e) = transport.resize(size).await {
                        tracing::warn!(session = id, "resize failed: {e}");
                    }
                }
                Some(SessionCommand::Ack(bytes)) => unacked = unacked.saturating_sub(bytes),
                // `None`: every sender is gone (manager dropped), treat as a close.
                Some(SessionCommand::Close) | None => break None,
            },

            // `next_at` is always Some when this branch is enabled.
            _ = sleep_until(queue.next_at.unwrap_or_else(Instant::now)), if queue.next_at.is_some() => {
                if let Some(bytes) = queue.pop()
                    && let Err(e) = transport.send(&bytes).await
                {
                    break Some((format!("Envoi impossible : {e}"), true));
                }
            }

            event = transport.recv(), if can_read => match event {
                TransportEvent::Data(data) => {
                    if let Some(logger) = &mut logger {
                        logger.write(&data);
                    }
                    match watcher.observe(&data) {
                        Some(Reaction::PagerContinue) => {
                            if let Err(e) = transport.send(b" ").await {
                                break Some((format!("Envoi impossible : {e}"), true));
                            }
                        }
                        Some(Reaction::PromptReady) => {
                            tracing::info!(session = id, "prompt detected, sending login commands");
                            queue.push(&automation.login_commands, LOGIN_COMMAND_DELAY);
                        }
                        None => {}
                    }
                    if batch.is_empty() {
                        flush_at = Some(Instant::now() + FLUSH_DELAY);
                    }
                    batch.extend_from_slice(&data);
                    if batch.len() >= MAX_BATCH {
                        flush(&mut batch, &mut flush_at, &mut unacked);
                    }
                }
                TransportEvent::Closed(reason) => break Some((reason, false)),
                TransportEvent::Lost(reason) => break Some((reason, true)),
            },

            // `flush_at` is always Some when this branch is enabled.
            _ = sleep_until(flush_at.unwrap_or_else(Instant::now)), if flush_at.is_some() => {
                flush(&mut batch, &mut flush_at, &mut unacked);
            }
        }
    };

    flush(&mut batch, &mut flush_at, &mut unacked);
    let lost = remote_end.as_ref().is_some_and(|(_, lost)| *lost);
    tracing::info!(
        session = id,
        by_remote = remote_end.is_some(),
        lost,
        "session closing"
    );
    transport.close().await;
    (sinks.event)(SessionEvent::Closed {
        by_remote: remote_end.is_some(),
        lost,
        reason: remote_end.map_or_else(|| "Session fermée.".into(), |(reason, _)| reason),
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::errors::AppError;
    use crate::transport::TermSize;

    /// Fake transport: replays scripted events and records what it receives.
    struct MockTransport {
        incoming: mpsc::Receiver<TransportEvent>,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Transport for MockTransport {
        fn info(&self) -> crate::transport::ConnectionInfo {
            crate::transport::ConnectionInfo {
                protocol: "test",
                encrypted: true,
                algorithms: None,
                via: Vec::new(),
            }
        }
        async fn recv(&mut self) -> TransportEvent {
            match self.incoming.recv().await {
                Some(event) => event,
                None => TransportEvent::Closed("mock closed".into()),
            }
        }
        async fn send(&mut self, data: &[u8]) -> Result<(), AppError> {
            self.log
                .lock()
                .unwrap()
                .push(format!("send {}", String::from_utf8_lossy(data)));
            Ok(())
        }
        async fn resize(&mut self, size: TermSize) -> Result<(), AppError> {
            self.log
                .lock()
                .unwrap()
                .push(format!("resize {}x{}", size.cols, size.rows));
            Ok(())
        }
        async fn close(self) {
            self.log.lock().unwrap().push("close".into());
        }
    }

    struct Harness {
        remote: mpsc::Sender<TransportEvent>,
        commands: mpsc::Sender<SessionCommand>,
        log: Arc<Mutex<Vec<String>>>,
        output: Arc<Mutex<Vec<Vec<u8>>>>,
        events: Arc<Mutex<Vec<SessionEvent>>>,
        task: tokio::task::JoinHandle<()>,
    }

    fn start() -> Harness {
        start_with(Automation::default())
    }

    fn start_with(automation: Automation) -> Harness {
        let (remote, incoming) = mpsc::channel(1024);
        let (commands, commands_rx) = mpsc::channel(16);
        let log = Arc::new(Mutex::new(Vec::new()));
        let output = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let transport = MockTransport {
            incoming,
            log: log.clone(),
        };
        let (out, ev) = (output.clone(), events.clone());
        let sinks = SessionSinks {
            data: Box::new(move |d| out.lock().unwrap().push(d)),
            event: Box::new(move |e| ev.lock().unwrap().push(e)),
        };
        let task = tokio::spawn(async move {
            run_session(1, transport, commands_rx, &sinks, automation, None).await
        });
        Harness {
            remote,
            commands,
            log,
            output,
            events,
            task,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn batches_small_fragments_into_one_message() {
        let h = start();
        for chunk in [&b"he"[..], b"llo", b" wor", b"ld"] {
            h.remote
                .send(TransportEvent::Data(chunk.to_vec()))
                .await
                .unwrap();
        }
        tokio::time::sleep(FLUSH_DELAY * 2).await;
        assert_eq!(*h.output.lock().unwrap(), vec![b"hello world".to_vec()]);
        h.commands.send(SessionCommand::Close).await.unwrap();
        h.task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn keeps_a_split_utf8_character_intact() {
        let h = start();
        let bytes = "é".as_bytes(); // 2 bytes, sent separately
        h.remote
            .send(TransportEvent::Data(vec![bytes[0]]))
            .await
            .unwrap();
        h.remote
            .send(TransportEvent::Data(vec![bytes[1]]))
            .await
            .unwrap();
        // Let the batch be flushed: a close request skips output not yet read.
        tokio::time::sleep(FLUSH_DELAY * 2).await;
        h.commands.send(SessionCommand::Close).await.unwrap();
        h.task.await.unwrap();
        let all: Vec<u8> = h.output.lock().unwrap().concat();
        assert_eq!(String::from_utf8(all).unwrap(), "é");
    }

    #[tokio::test(start_paused = true)]
    async fn stops_reading_when_ui_does_not_acknowledge() {
        let h = start();
        let chunk = vec![b'x'; MAX_BATCH];
        let chunks_to_fill = MAX_UNACKED / MAX_BATCH;
        for _ in 0..chunks_to_fill + 5 {
            h.remote
                .send(TransportEvent::Data(chunk.clone()))
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
        let sent: usize = h.output.lock().unwrap().iter().map(Vec::len).sum();
        assert_eq!(sent, MAX_UNACKED, "must stop at the limit");

        h.commands.send(SessionCommand::Ack(sent)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let sent: usize = h.output.lock().unwrap().iter().map(Vec::len).sum();
        assert_eq!(
            sent,
            (chunks_to_fill + 5) * MAX_BATCH,
            "must resume after ack"
        );

        h.commands.send(SessionCommand::Close).await.unwrap();
        h.task.await.unwrap();
    }

    #[tokio::test]
    async fn forwards_input_and_resize_then_closes_transport() {
        let h = start();
        h.commands
            .send(SessionCommand::Write(b"ls\r".to_vec()))
            .await
            .unwrap();
        h.commands
            .send(SessionCommand::Resize(TermSize {
                cols: 120,
                rows: 40,
            }))
            .await
            .unwrap();
        h.commands.send(SessionCommand::Close).await.unwrap();
        h.task.await.unwrap();
        assert_eq!(
            *h.log.lock().unwrap(),
            vec!["send ls\r", "resize 120x40", "close"]
        );
        assert!(matches!(
            h.events.lock().unwrap().last(),
            Some(SessionEvent::Closed {
                by_remote: false,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn remote_exit_is_reported_and_transport_closed() {
        let h = start();
        h.remote
            .send(TransportEvent::Data(b"bye\r\n".to_vec()))
            .await
            .unwrap();
        h.remote
            .send(TransportEvent::Closed("exit 0".into()))
            .await
            .unwrap();
        h.task.await.unwrap();
        assert_eq!(h.output.lock().unwrap().concat(), b"bye\r\n");
        assert_eq!(*h.log.lock().unwrap(), vec!["close"]);
        assert!(matches!(
            h.events.lock().unwrap().last(),
            Some(SessionEvent::Closed { by_remote: true, lost: false, reason }) if reason == "exit 0"
        ));
    }

    #[tokio::test]
    async fn lost_connection_is_flagged_as_lost() {
        let h = start();
        h.remote
            .send(TransportEvent::Lost("cut".into()))
            .await
            .unwrap();
        h.task.await.unwrap();
        assert!(matches!(
            h.events.lock().unwrap().last(),
            Some(SessionEvent::Closed { lost: true, .. })
        ));
    }

    fn sent(h: &Harness) -> Vec<String> {
        h.log.lock().unwrap().clone()
    }

    #[tokio::test(start_paused = true)]
    async fn sequence_is_sent_line_by_line_with_delay() {
        let h = start();
        let lines: Vec<String> = (1..=5).map(|n| format!("cmd{n}")).collect();
        h.commands
            .send(SessionCommand::Sequence {
                lines,
                delay: Duration::from_millis(500),
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(sent(&h), vec!["send cmd1\r"], "first line immediately");
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(sent(&h).len(), 2);
        // Typing during a sequence is not blocked.
        h.commands
            .send(SessionCommand::Write(b"x".to_vec()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(2000)).await;
        assert_eq!(
            sent(&h),
            vec![
                "send cmd1\r",
                "send cmd2\r",
                "send x",
                "send cmd3\r",
                "send cmd4\r",
                "send cmd5\r"
            ]
        );
        h.commands.send(SessionCommand::Close).await.unwrap();
        h.task.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn pager_is_answered_and_login_commands_follow_the_prompt() {
        let h = start_with(Automation {
            pager_auto_reply: true,
            login_commands: vec!["terminal length 0".into(), "show clock".into()],
            log: None,
        });
        h.remote
            .send(TransportEvent::Data(b"\r\nR1>".to_vec()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1000)).await;
        assert_eq!(
            sent(&h),
            vec!["send terminal length 0\r", "send show clock\r"]
        );

        h.remote
            .send(TransportEvent::Data(b"line 1\r\n --More-- ".to_vec()))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(sent(&h).last().map(String::as_str), Some("send  "));
        h.commands.send(SessionCommand::Close).await.unwrap();
        h.task.await.unwrap();
    }
}
