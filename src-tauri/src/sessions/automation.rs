//! Reactions to the terminal output: pager prompts (`--More--`) and the first
//! command prompt (to send the profile's login commands). Pure logic, no I/O.

use std::time::Duration;

use tokio::time::Instant;

/// Output kept to recognise a prompt split across several network reads.
const TAIL_SIZE: usize = 512;
/// Login commands are abandoned if no prompt shows up within this delay.
const LOGIN_PROMPT_TIMEOUT: Duration = Duration::from_secs(120);

/// Pager prompts of common network devices.
const PAGER_PATTERNS: &[&str] = &[
    "--More--",       // Cisco IOS, Linux `more`
    "---- More ----", // Huawei VRP, H3C
    "-- More --",     // Cisco NX-OS, Aruba
    "<--- More --->", // Cisco ASA
    "---(more",       // Juniper Junos: ---(more)--- / ---(more 42%)---
];

#[derive(Debug, PartialEq, Eq)]
pub enum Reaction {
    /// A pager waits for a key: answer with a space.
    PagerContinue,
    /// First prompt after connecting: time to send the login commands.
    PromptReady,
}

pub struct OutputWatcher {
    tail: Vec<u8>,
    pager_auto_reply: bool,
    /// Waiting for the first prompt, until this deadline.
    login_deadline: Option<Instant>,
}

impl OutputWatcher {
    pub fn new(pager_auto_reply: bool, wait_for_login_prompt: bool) -> Self {
        Self {
            tail: Vec::new(),
            pager_auto_reply,
            login_deadline: wait_for_login_prompt.then(|| Instant::now() + LOGIN_PROMPT_TIMEOUT),
        }
    }

    /// True when there is nothing to watch: the runner can skip the work.
    pub fn idle(&self) -> bool {
        !self.pager_auto_reply && self.login_deadline.is_none()
    }

    pub fn observe(&mut self, data: &[u8]) -> Option<Reaction> {
        if self.idle() {
            return None;
        }
        self.tail.extend_from_slice(data);
        if self.tail.len() > TAIL_SIZE {
            self.tail.drain(..self.tail.len() - TAIL_SIZE);
        }
        let line = last_line(&self.tail);

        if self.pager_auto_reply && is_pager(&line) {
            // Forget what we saw: the same prompt must not be answered twice.
            self.tail.clear();
            return Some(Reaction::PagerContinue);
        }
        if let Some(deadline) = self.login_deadline {
            if Instant::now() > deadline {
                tracing::info!("no prompt detected: login commands not sent");
                self.login_deadline = None;
            } else if is_prompt(&line) {
                self.login_deadline = None;
                return Some(Reaction::PromptReady);
            }
        }
        None
    }
}

/// The current (last, possibly unfinished) line, without ANSI escape sequences,
/// carriage returns and backspaces.
fn last_line(tail: &[u8]) -> String {
    let start = tail.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    strip_controls(&tail[start..])
}

/// Removes ANSI escape sequences (CSI `ESC [ ... final`, OSC `ESC ] ... BEL`)
/// and control characters. Used for detection only, never for display.
pub fn strip_controls(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x1b => {
                i += 1;
                match bytes.get(i) {
                    Some(b'[') => {
                        i += 1;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                    }
                    Some(b']') => {
                        while i < bytes.len() && bytes[i] != 0x07 && bytes[i] != 0x1b {
                            i += 1;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            b if b < 0x20 && b != b'\t' => i += 1,
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_pager(line: &str) -> bool {
    PAGER_PATTERNS.iter().any(|p| line.contains(p))
}

/// Shell or device prompt: `R1#`, `R1>`, `<Huawei>`, `[Huawei]`, `user@host:~$`.
/// Heuristic, deliberately strict: short line ending with a prompt character.
fn is_prompt(line: &str) -> bool {
    let line = line.trim_end();
    let length = line.chars().count();
    (1..=100).contains(&length) && line.ends_with(['#', '>', '$', '%', ']']) && !is_pager(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_ansi_and_controls() {
        assert_eq!(strip_controls(b"\x1b[1;32mR1#\x1b[0m "), "R1# ");
        assert_eq!(strip_controls(b"\x1b]0;title\x07user$ "), "user$ ");
        assert_eq!(strip_controls(b"abc\x08\x08\rdef"), "abcdef");
    }

    #[test]
    fn detects_pagers_of_common_vendors() {
        for prompt in [
            " --More-- ",
            "  ---- More ----",
            "\x1b[7m--More--\x1b[m",
            "---(more 42%)---",
            "<--- More --->",
        ] {
            let mut w = OutputWatcher::new(true, false);
            assert_eq!(
                w.observe(format!("line\r\n{prompt}").as_bytes()),
                Some(Reaction::PagerContinue),
                "{prompt:?}"
            );
        }
    }

    #[test]
    fn pager_split_across_reads_is_answered_once() {
        let mut w = OutputWatcher::new(true, false);
        assert_eq!(w.observe(b"interface Gi0/0\r\n --Mo"), None);
        assert_eq!(w.observe(b"re-- "), Some(Reaction::PagerContinue));
        // Cisco then erases the prompt with backspaces: no second answer.
        assert_eq!(
            w.observe(b"\x08\x08\x08\x08\x08\x08\x08\x08\x08 interface"),
            None
        );
    }

    #[test]
    fn pager_ignored_when_option_disabled() {
        let mut w = OutputWatcher::new(false, false);
        assert!(w.idle());
        assert_eq!(w.observe(b" --More-- "), None);
    }

    #[test]
    fn first_prompt_triggers_login_commands_once() {
        let mut w = OutputWatcher::new(false, true);
        assert_eq!(
            w.observe(b"\r\nUser Access Verification\r\n\r\nPassword: "),
            None
        );
        assert_eq!(w.observe(b"\r\nR1"), None);
        assert_eq!(w.observe(b">"), Some(Reaction::PromptReady));
        assert_eq!(w.observe(b"\r\nR1>"), None, "only the first prompt");
        assert!(w.idle());
    }

    #[test]
    fn recognises_usual_prompts() {
        for p in [
            "R1#",
            "R1>",
            "<Huawei>",
            "[Huawei-GigabitEthernet0/0/1]",
            "user@srv:~$ ",
            "root@srv:~# ",
        ] {
            assert!(is_prompt(p), "{p:?}");
        }
        for p in [
            "Password: ",
            "login: ",
            "Press RETURN to get started!",
            " --More-- ",
            "",
        ] {
            assert!(!is_prompt(p), "{p:?}");
        }
    }
}
