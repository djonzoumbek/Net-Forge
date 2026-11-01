//! Captures the output of one command typed in the session (configuration
//! backup): from the echo of the command to the return of the prompt.
//!
//! Pure logic, driven by the runner: `feed` for each piece of output, `finish`
//! once the prompt has been stable for `SETTLE` (or at the deadline). Works on
//! every transport (SSH, Telnet, serial), unlike an SSH `exec` channel.

use std::time::Duration;

use tokio::time::Instant;

use super::automation::{is_pager, is_prompt, strip_controls};
use super::logger::TextCleaner;
use crate::errors::AppError;

/// The prompt must stay unchanged this long: a long configuration may pause.
pub const SETTLE: Duration = Duration::from_millis(1000);
/// Give up if the prompt never comes back.
pub const TIMEOUT: Duration = Duration::from_secs(180);
/// Bigger than any real configuration: protects the memory.
const MAX_BYTES: usize = 32 * 1024 * 1024;
const TAIL_SIZE: usize = 256;

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    /// A pager waits: the runner answers with a space.
    AnswerPager,
}

pub struct Capture {
    command: String,
    cleaner: TextCleaner,
    /// Completed lines after the echo of the command.
    lines: Vec<String>,
    /// `Some` once the echo line is seen: what precedes the command on it.
    prompt: Option<String>,
    bytes: usize,
    tail: Vec<u8>,
    pub deadline: Instant,
    /// When the prompt has been seen and nothing came since.
    pub settle_at: Option<Instant>,
}

impl Capture {
    pub fn new(command: &str, now: Instant) -> Self {
        Self {
            command: command.trim().to_string(),
            cleaner: TextCleaner::default(),
            lines: Vec::new(),
            prompt: None,
            bytes: 0,
            tail: Vec::new(),
            deadline: now + TIMEOUT,
            settle_at: None,
        }
    }

    /// Earliest instant the runner must wake up for.
    pub fn wake_at(&self) -> Instant {
        self.settle_at
            .map_or(self.deadline, |s| s.min(self.deadline))
    }

    pub fn feed(&mut self, data: &[u8], now: Instant) -> Result<Step, AppError> {
        self.bytes += data.len();
        if self.bytes > MAX_BYTES {
            return Err(AppError::InvalidInput(
                "sortie trop volumineuse (32 Mio) : est-ce bien une configuration ?".into(),
            ));
        }
        for line in self.cleaner.feed(data).lines() {
            match &self.prompt {
                None => {
                    // The echo: "R1#show running-config" -> prompt "R1#".
                    if let Some(position) = line.find(&self.command) {
                        self.prompt = Some(line[..position].trim_end().to_string());
                    }
                }
                Some(_) => self.lines.push(line.to_string()),
            }
        }

        self.tail.extend_from_slice(data);
        if self.tail.len() > TAIL_SIZE {
            self.tail.drain(..self.tail.len() - TAIL_SIZE);
        }
        let start = self
            .tail
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        if is_pager(&strip_controls(&self.tail[start..])) {
            self.tail.clear();
            self.settle_at = None;
            return Ok(Step::AnswerPager);
        }

        self.settle_at = match &self.prompt {
            Some(prompt) if self.prompt_is_back(prompt) => Some(now + SETTLE),
            _ => None,
        };
        Ok(Step::Continue)
    }

    fn prompt_is_back(&self, prompt: &str) -> bool {
        let current = self.cleaner.current_line();
        let current = current.trim_end();
        if prompt.is_empty() {
            // Echo without a visible prompt before it: fall back to the heuristic.
            is_prompt(current)
        } else {
            current == prompt
        }
    }

    /// The captured text, once the prompt has settled. `Err` at the deadline.
    pub fn finish(self, now: Instant) -> Result<String, AppError> {
        if self.settle_at.is_none_or(|s| now < s) {
            return Err(AppError::Timeout(if self.prompt.is_none() {
                "la commande n'a pas été renvoyée en écho : la session était-elle à l'invite ?"
                    .into()
            } else {
                "l'invite n'est pas revenue après la commande (pager non reconnu ?).".into()
            }));
        }
        let mut lines: Vec<String> = self
            .lines
            .into_iter()
            // Pager prompts erased by the device leave blank-only remnants.
            .filter(|line| !is_pager(line))
            .map(|line| line.trim_end().to_string())
            .collect();
        while lines.first().is_some_and(|l| l.is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            return Err(AppError::InvalidInput(
                "la commande n'a rien affiché (droits insuffisants, commande inconnue ?).".into(),
            ));
        }
        let mut text = lines.join("\n");
        text.push('\n');
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&[u8]]) -> (Result<String, AppError>, usize) {
        let start = Instant::now();
        let mut capture = Capture::new("show running-config", start);
        let mut pagers = 0;
        for chunk in chunks {
            if capture.feed(chunk, start).unwrap() == Step::AnswerPager {
                pagers += 1;
            }
        }
        (capture.finish(start + SETTLE), pagers)
    }

    #[test]
    fn cisco_running_config_with_pager() {
        let (text, pagers) = run(&[
            b"show running-config\r\n",
            b"Building configuration...\r\n\r\nhostname R1\r\n",
            b" --More-- ",
            // Cisco erases the pager prompt with backspaces, then continues.
            b"\x08\x08\x08\x08\x08\x08\x08\x08\x08\x08        \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08",
            b"interface Gi0/0\r\n ip address 10.0.0.1 255.255.255.0\r\nend\r\n\r\nR1#",
        ]);
        assert_eq!(pagers, 1);
        assert_eq!(
            text.unwrap(),
            "Building configuration...\n\nhostname R1\ninterface Gi0/0\n ip address 10.0.0.1 255.255.255.0\nend\n"
        );
    }

    #[test]
    fn huawei_with_its_pager_and_prompt() {
        let (text, pagers) = run(&[
            b"<R1>show running-config\r\n#\r\nsysname R1\r\n",
            b"  ---- More ----",
            b"\x1b[42D                                          \x1b[42D",
            b"#\r\ninterface GE0/0/1\r\n#\r\nreturn\r\n<R1>",
        ]);
        assert_eq!(pagers, 1);
        assert_eq!(
            text.unwrap(),
            "#\nsysname R1\n#\ninterface GE0/0/1\n#\nreturn\n"
        );
    }

    #[test]
    fn a_line_ending_with_the_prompt_char_is_not_the_end() {
        let start = Instant::now();
        let mut capture = Capture::new("show running-config", start);
        capture
            .feed(b"R1#show running-config\r\nbanner motd #", start)
            .unwrap();
        assert!(capture.settle_at.is_none(), "not the exact prompt R1#");
        capture.feed(b"Lab#\r\n", start).unwrap();
        capture.feed(b"end\r\nR1#", start).unwrap();
        assert!(capture.settle_at.is_some());
        // More output after the prompt (e.g. a log message) restarts the wait.
        capture.feed(b"\r\n%LINK-3-UPDOWN\r\nR1", start).unwrap();
        assert!(capture.settle_at.is_none());
    }

    #[test]
    fn errors_are_explained() {
        let start = Instant::now();
        let silent = Capture::new("show running-config", start);
        let err = silent.finish(start + TIMEOUT).unwrap_err();
        assert!(err.to_string().contains("écho"), "{err}");

        let (empty, _) = run(&[b"R1>show running-config\r\nR1>"]);
        assert!(empty.unwrap_err().to_string().contains("rien affiché"));

        let start = Instant::now();
        let mut stuck = Capture::new("show running-config", start);
        stuck
            .feed(b"R1#show running-config\r\nhostname R1\r\n", start)
            .unwrap();
        let err = stuck.finish(start + TIMEOUT).unwrap_err();
        assert!(err.to_string().contains("invite"), "{err}");
    }
}
