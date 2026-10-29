//! Session recording to a file (opt-in per profile).
//!
//! Writing happens on a dedicated thread: the session loop only hands bytes over
//! through a bounded channel and never waits for the disk.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use crate::errors::AppError;
use crate::persistence::profiles::LogMode;

/// Chunks waiting for the writer thread. If the disk cannot keep up, extra
/// output is counted and a marker is written instead of blocking the session.
const QUEUE: usize = 1024;

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub mode: LogMode,
    pub dir: PathBuf,
    /// Used in the file name and the header, e.g. `R1 (10.0.0.1:22)`.
    pub title: String,
}

enum Message {
    Data(Vec<u8>),
    /// Output lost because the queue was full.
    Gap(usize),
}

pub struct SessionLogger {
    sender: Option<SyncSender<Message>>,
    dropped: usize,
    path: PathBuf,
}

impl SessionLogger {
    /// Creates `<dir>/<title>_<date>.log` (mode 0600, directory 0700) and starts
    /// the writer thread.
    pub fn start(config: &LogConfig) -> Result<Self, AppError> {
        let storage = |e: std::io::Error| AppError::Storage(format!("journal de session : {e}"));
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&config.dir)
            .map_err(storage)?;
        let now = chrono::Local::now();
        let name = format!(
            "{}_{}.log",
            safe_file_name(&config.title),
            now.format("%Y-%m-%d_%H-%M-%S")
        );
        let path = config.dir.join(name);
        // create_new: never append to (or overwrite) an existing file.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(storage)?;
        let mut out = BufWriter::new(file);
        writeln!(
            out,
            "# NetForge — {} — début {}",
            config.title,
            now.format("%Y-%m-%d %H:%M:%S %z")
        )
        .map_err(storage)?;

        let (sender, receiver) = sync_channel(QUEUE);
        let mode = config.mode;
        let thread_path = path.clone();
        std::thread::Builder::new()
            .name("session-log".into())
            .spawn(move || {
                if let Err(e) = write_loop(out, receiver, mode) {
                    tracing::warn!(path = %thread_path.display(), "session log stopped: {e}");
                }
            })
            .map_err(storage)?;
        tracing::info!(path = %path.display(), "session log started");
        Ok(Self {
            sender: Some(sender),
            dropped: 0,
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Never blocks: if the writer is behind, the bytes are counted as lost.
    pub fn write(&mut self, data: &[u8]) {
        let Some(sender) = &self.sender else { return };
        if self.dropped > 0 {
            match sender.try_send(Message::Gap(self.dropped)) {
                Ok(()) => self.dropped = 0,
                Err(TrySendError::Full(_)) => {
                    self.dropped += data.len();
                    return;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.sender = None;
                    return;
                }
            }
        }
        match sender.try_send(Message::Data(data.to_vec())) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.dropped += data.len(),
            Err(TrySendError::Disconnected(_)) => self.sender = None,
        }
    }
}

/// Dropping the logger closes the channel: the thread writes the end of the
/// file and exits.
impl Drop for SessionLogger {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take()
            && self.dropped > 0
        {
            let _ = sender.try_send(Message::Gap(self.dropped));
        }
    }
}

fn write_loop(
    mut out: BufWriter<fs::File>,
    receiver: Receiver<Message>,
    mode: LogMode,
) -> std::io::Result<()> {
    let mut cleaner = TextCleaner::default();
    let mut handle = |message: Message, out: &mut BufWriter<fs::File>| match message {
        Message::Data(bytes) => match mode {
            LogMode::Raw => out.write_all(&bytes),
            _ => out.write_all(cleaner.feed(&bytes).as_bytes()),
        },
        Message::Gap(bytes) => writeln!(out, "\n# [NetForge : {bytes} octets non journalisés]"),
    };
    while let Ok(message) = receiver.recv() {
        handle(message, &mut out)?;
        // Write everything already queued, then flush once: little data lost on a
        // crash, few system calls when the output is heavy.
        while let Ok(message) = receiver.try_recv() {
            handle(message, &mut out)?;
        }
        out.flush()?;
    }
    drop(handle);
    if mode != LogMode::Raw {
        out.write_all(cleaner.finish().as_bytes())?;
    }
    writeln!(
        out,
        "\n# Fin de session {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z")
    )?;
    out.flush()
}

/// Keeps letters, digits, `-`, `_`, `.`; everything else becomes `_`.
fn safe_file_name(title: &str) -> String {
    let name: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = name.trim_matches(['_', '.']);
    if name.is_empty() {
        "session".into()
    } else {
        name.chars().take(80).collect()
    }
}

#[derive(Default)]
enum Escape {
    #[default]
    None,
    Esc,
    /// Control Sequence Introducer `ESC [`, with its parameters so far.
    Csi(String),
    /// Operating System Command `ESC ]` (window title...), until BEL or ST.
    Osc,
    OscEsc,
}

/// Turns terminal output into plain text: interprets what changes the current
/// line (CR, backspace, erase-line, horizontal cursor moves) and drops every
/// other escape sequence. Full-screen programs (vim, htop) are not rendered.
#[derive(Default)]
pub struct TextCleaner {
    line: Vec<char>,
    cursor: usize,
    escape: Escape,
    /// Incomplete UTF-8 character at the end of the previous chunk.
    pending: Vec<u8>,
}

impl TextCleaner {
    /// Returns the lines completed by these bytes.
    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut output = String::new();
        let mut input = std::mem::take(&mut self.pending);
        let mut rest: &[u8] = &input;
        loop {
            match std::str::from_utf8(rest) {
                Ok(text) => {
                    self.chars(text, &mut output);
                    break;
                }
                Err(error) => {
                    let (valid, after) = rest.split_at(error.valid_up_to());
                    // `valid` is valid UTF-8 by construction.
                    self.chars(std::str::from_utf8(valid).unwrap_or_default(), &mut output);
                    match error.error_len() {
                        // Truncated character: wait for the next chunk.
                        None => {
                            self.pending = after.to_vec();
                            break;
                        }
                        Some(len) => {
                            self.chars("\u{fffd}", &mut output);
                            rest = &after[len..];
                        }
                    }
                }
            }
        }
        input.clear();
        output
    }

    /// The unfinished last line, at the end of the session.
    pub fn finish(&mut self) -> String {
        let line: String = self.line.iter().collect();
        self.line.clear();
        self.cursor = 0;
        let line = line.trim_end();
        if line.is_empty() {
            String::new()
        } else {
            format!("{line}\n")
        }
    }

    fn chars(&mut self, text: &str, output: &mut String) {
        for c in text.chars() {
            self.char(c, output);
        }
    }

    fn char(&mut self, c: char, output: &mut String) {
        match std::mem::take(&mut self.escape) {
            Escape::Esc => {
                self.escape = match c {
                    '[' => Escape::Csi(String::new()),
                    ']' => Escape::Osc,
                    _ => Escape::None, // two-character sequence: ignored
                };
                return;
            }
            Escape::Csi(mut params) => {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    self.csi(c, &params);
                } else if params.len() < 32 {
                    params.push(c);
                    self.escape = Escape::Csi(params);
                } else {
                    self.escape = Escape::Csi(params);
                }
                return;
            }
            Escape::Osc => {
                self.escape = match c {
                    '\u{7}' => Escape::None,
                    '\u{1b}' => Escape::OscEsc,
                    _ => Escape::Osc,
                };
                return;
            }
            Escape::OscEsc => {
                self.escape = if c == '\\' { Escape::None } else { Escape::Osc };
                return;
            }
            Escape::None => {}
        }
        match c {
            '\u{1b}' => self.escape = Escape::Esc,
            '\r' => self.cursor = 0,
            '\n' => {
                let line: String = self.line.iter().collect();
                output.push_str(line.trim_end());
                output.push('\n');
                self.line.clear();
                self.cursor = 0;
            }
            '\u{8}' => self.cursor = self.cursor.saturating_sub(1),
            c if c.is_control() && c != '\t' => {}
            c => {
                while self.line.len() < self.cursor {
                    self.line.push(' ');
                }
                if self.cursor < self.line.len() {
                    self.line[self.cursor] = c;
                } else {
                    self.line.push(c);
                }
                self.cursor += 1;
            }
        }
    }

    fn csi(&mut self, command: char, params: &str) {
        let n = params
            .split(';')
            .next()
            .and_then(|p| p.parse::<usize>().ok())
            .unwrap_or(0);
        match command {
            // Erase in line: 0 = cursor to end, 1 = start to cursor, 2 = whole line.
            'K' => match n {
                1 => {
                    let end = (self.cursor + 1).min(self.line.len());
                    self.line[..end].iter_mut().for_each(|c| *c = ' ');
                }
                2 => self.line.clear(),
                _ => self.line.truncate(self.cursor),
            },
            'D' => self.cursor = self.cursor.saturating_sub(n.max(1)),
            'C' => self.cursor += n.max(1),
            'G' => self.cursor = n.max(1) - 1,
            // Colours, scrolling, screen moves...: no effect on a text log.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(chunks: &[&[u8]]) -> String {
        let mut cleaner = TextCleaner::default();
        let mut out: String = chunks.iter().map(|c| cleaner.feed(c)).collect();
        out.push_str(&cleaner.finish());
        out
    }

    #[test]
    fn removes_colours_and_titles() {
        assert_eq!(
            clean(&[b"\x1b]0;user@srv: ~\x07\x1b[01;32muser@srv\x1b[00m:~$ ls\r\n\x1b[0m\x1b[01;34mdir\x1b[0m\r\n"]),
            "user@srv:~$ ls\ndir\n"
        );
    }

    #[test]
    fn cisco_pager_erased_with_backspaces_leaves_no_trace() {
        let more = b" --More-- \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08          \x08\x08\x08\x08\x08\x08\x08\x08\x08\x08";
        assert_eq!(
            clean(&[b"interface Gi0/0\r\n", more, b"interface Gi0/1\r\n"]),
            "interface Gi0/0\ninterface Gi0/1\n"
        );
    }

    #[test]
    fn huawei_pager_erased_with_cursor_moves_leaves_no_trace() {
        let more = b"  ---- More ----\x1b[42D                                          \x1b[42D";
        assert_eq!(
            clean(&[b"sysname R1\r\n", more, b"interface GE0/0/0\r\n"]),
            "sysname R1\ninterface GE0/0/0\n"
        );
    }

    #[test]
    fn carriage_return_overwrites_and_erase_line() {
        assert_eq!(clean(&[b"10%\r50%\r100%\r\n"]), "100%\n");
        assert_eq!(clean(&[b"wrong command\r\x1b[Kok\r\n"]), "ok\n");
    }

    #[test]
    fn utf8_split_between_chunks_and_invalid_bytes() {
        let e = "é".as_bytes();
        assert_eq!(clean(&[b"caf", &e[..1], &e[1..], b"\n"]), "café\n");
        assert_eq!(clean(&[b"a\xffb\n"]), "a\u{fffd}b\n");
    }

    #[test]
    fn unfinished_last_line_is_kept() {
        assert_eq!(
            clean(&[b"R1#show clock\r\n*10:00:00\r\nR1#"]),
            "R1#show clock\n*10:00:00\nR1#\n"
        );
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_file_name("R1 (10.0.0.1:22)"), "R1__10.0.0.1_22");
        assert_eq!(safe_file_name("../../etc/passwd"), "etc_passwd");
        assert_eq!(safe_file_name("///"), "session");
    }

    #[test]
    fn writes_a_private_readable_text_log() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("netforge-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let config = LogConfig {
            mode: LogMode::Text,
            dir: dir.clone(),
            title: "R1 test".into(),
        };
        let path = {
            let mut logger = SessionLogger::start(&config).unwrap();
            logger.write(b"\x1b[1mR1#\x1b[0mshow ver\r\n");
            logger.write(b"Cisco IOS\r\nR1#");
            logger.path().to_path_buf()
        };
        // The writer thread finishes after the drop: wait for the footer.
        let mut content = String::new();
        for _ in 0..100 {
            content = fs::read_to_string(&path).unwrap();
            if content.contains("# Fin de session") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            content.starts_with("# NetForge — R1 test — début "),
            "{content}"
        );
        assert!(
            content.contains("R1#show ver\nCisco IOS\nR1#\n"),
            "{content}"
        );
        assert!(
            !content.contains('\u{1b}'),
            "no escape sequence in a text log"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
