//! Host key verification against OpenSSH-format `known_hosts` files.
//!
//! NetForge writes only to its own file. `~/.ssh/known_hosts` is read-only: keys the
//! user already trusts with OpenSSH are trusted here too, but never modified.
//! The own file has precedence, so replacing a key always happens there.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use data_encoding::BASE64;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::{HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;

use crate::errors::AppError;

/// Shown to the user when a server presents a different key than the recorded one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyMismatch {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub old_fingerprint: String,
    pub new_fingerprint: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug)]
pub enum HostKeyStatus {
    Trusted,
    Unknown,
    Changed(HostKeyMismatch),
    /// Explicitly marked `@revoked`: never accepted.
    Revoked,
}

pub struct KnownHosts {
    own_file: PathBuf,
    user_file: Option<PathBuf>,
    /// Keys that failed with a mismatch, waiting for an explicit replacement.
    /// Kept here so the UI never has to send key material back to us.
    pending: Mutex<HashMap<String, PublicKey>>,
    /// Serializes read-modify-write of the own file: with several tabs, two
    /// unknown hosts may be accepted at the same time.
    write_lock: Mutex<()>,
}

#[derive(Debug, PartialEq)]
enum Marker {
    Revoked,
    CertAuthority,
}

struct Entry {
    line: usize,
    marker: Option<Marker>,
    key: PublicKey,
}

impl KnownHosts {
    pub fn new(own_file: PathBuf, user_file: Option<PathBuf>) -> Self {
        Self {
            own_file,
            user_file,
            pending: Mutex::new(HashMap::new()),
            write_lock: Mutex::new(()),
        }
    }

    #[cfg(test)]
    pub fn own_file(&self) -> &Path {
        &self.own_file
    }

    pub fn check(&self, host: &str, port: u16, key: &PublicKey) -> Result<HostKeyStatus, AppError> {
        let target = host_pattern(host, port);
        let files: Vec<&Path> = std::iter::once(self.own_file.as_path())
            .chain(self.user_file.as_deref())
            .collect();
        let entries: Vec<(&Path, Vec<Entry>)> = files
            .into_iter()
            .map(|path| Ok((path, read_entries(path, &target)?)))
            .collect::<Result<_, AppError>>()?;

        // A revoked key is refused whatever any other line says.
        let revoked = entries
            .iter()
            .flat_map(|(_, e)| e)
            .any(|e| e.marker == Some(Marker::Revoked) && e.key.key_data() == key.key_data());
        if revoked {
            return Ok(HostKeyStatus::Revoked);
        }

        // First file with a key of the same type decides (own file first).
        for (path, file_entries) in &entries {
            let same_type: Vec<&Entry> = file_entries
                .iter()
                .filter(|e| e.marker.is_none() && e.key.algorithm() == key.algorithm())
                .collect();
            if same_type.iter().any(|e| e.key.key_data() == key.key_data()) {
                return Ok(HostKeyStatus::Trusted);
            }
            if let Some(old) = same_type.first() {
                self.remember_pending(&target, key);
                return Ok(HostKeyStatus::Changed(HostKeyMismatch {
                    host: host.to_string(),
                    port,
                    algorithm: key.algorithm().as_str().to_string(),
                    old_fingerprint: old.key.fingerprint(HashAlg::Sha256).to_string(),
                    new_fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
                    file: path.display().to_string(),
                    line: old.line,
                }));
            }
        }
        Ok(HostKeyStatus::Unknown)
    }

    /// Records a key the user just accepted for an unknown host.
    pub fn add(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().unwrap_or_else(|p| p.into_inner());
        let target = host_pattern(host, port);
        let already_known = read_entries(&self.own_file, &target)?
            .iter()
            .any(|e| e.marker.is_none() && e.key.key_data() == key.key_data());
        if already_known {
            return Ok(());
        }
        let mut content = read_to_string_or_empty(&self.own_file)?;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&entry_line(host, port, key)?);
        write_private_file(&self.own_file, &content)
    }

    /// Replaces the recorded key of a host by the one that was refused, after the
    /// user explicitly confirmed it. `expected_fingerprint` must match the key we
    /// refused: it proves the user saw this exact key.
    pub fn replace(
        &self,
        host: &str,
        port: u16,
        expected_fingerprint: &str,
    ) -> Result<(), AppError> {
        let target = host_pattern(host, port);
        let key = {
            let mut pending = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            match pending.get(&target) {
                Some(key)
                    if key.fingerprint(HashAlg::Sha256).to_string() == expected_fingerprint =>
                {
                    pending.remove(&target)
                }
                _ => None,
            }
        }
        .ok_or_else(|| {
            AppError::InvalidInput("aucune clé en attente de remplacement pour cet hôte.".into())
        })?;

        let _guard = self.write_lock.lock().unwrap_or_else(|p| p.into_inner());
        let content = read_to_string_or_empty(&self.own_file)?;
        let mut kept = String::new();
        for (index, line) in content.lines().enumerate() {
            let replaced = parse_line(line, index + 1, &target)
                .is_some_and(|e| e.marker.is_none() && e.key.algorithm() == key.algorithm());
            if !replaced {
                kept.push_str(line);
                kept.push('\n');
            }
        }
        kept.push_str(&entry_line(host, port, &key)?);
        write_private_file(&self.own_file, &kept)?;
        tracing::info!(host, port, "host key replaced after user confirmation");
        Ok(())
    }

    fn remember_pending(&self, target: &str, key: &PublicKey) {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(target.to_string(), key.clone());
    }
}

/// OpenSSH writes `host` for port 22 and `[host]:port` otherwise.
fn host_pattern(host: &str, port: u16) -> String {
    let host = host.to_lowercase();
    if port == 22 {
        host
    } else {
        format!("[{host}]:{port}")
    }
}

fn entry_line(host: &str, port: u16, key: &PublicKey) -> Result<String, AppError> {
    let key = key
        .to_openssh()
        .map_err(|e| AppError::Storage(format!("encodage de la clé d'hôte : {e}")))?;
    Ok(format!("{} {key}\n", host_pattern(host, port)))
}

fn read_to_string_or_empty(path: &Path) -> Result<String, AppError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(AppError::Storage(format!(
            "lecture de {} : {e}",
            path.display()
        ))),
    }
}

fn read_entries(path: &Path, target: &str) -> Result<Vec<Entry>, AppError> {
    let content = read_to_string_or_empty(path)?;
    Ok(content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| parse_line(line, index + 1, target))
        .collect())
}

/// Returns the entry if the line is valid and applies to `target`.
fn parse_line(line: &str, line_number: usize, target: &str) -> Option<Entry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut fields = line.split_whitespace();
    let mut first = fields.next()?;
    let marker = match first {
        "@revoked" => Some(Marker::Revoked),
        "@cert-authority" => Some(Marker::CertAuthority),
        _ => None,
    };
    if marker.is_some() {
        first = fields.next()?;
    }
    if !hosts_match(first, target) {
        return None;
    }
    let (key_type, key_base64) = (fields.next()?, fields.next()?);
    // Unknown key types (e.g. hardware keys) are skipped, not fatal.
    let key = PublicKey::from_openssh(&format!("{key_type} {key_base64}")).ok()?;
    Some(Entry {
        line: line_number,
        marker,
        key,
    })
}

/// OpenSSH host list: comma-separated patterns, `*`/`?` wildcards, `!` negation,
/// and hashed entries `|1|salt|hash` (HMAC-SHA1 of the host, base64).
fn hosts_match(patterns: &str, target: &str) -> bool {
    let mut matched = false;
    for pattern in patterns.split(',') {
        if let Some(hashed) = pattern.strip_prefix("|1|") {
            matched |= hashed_matches(hashed, target);
            continue;
        }
        let (negated, pattern) = match pattern.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        if wildcard_match(&pattern.to_lowercase(), target) {
            if negated {
                return false;
            }
            matched = true;
        }
    }
    matched
}

fn hashed_matches(hashed: &str, target: &str) -> bool {
    let Some((salt, hash)) = hashed.split_once('|') else {
        return false;
    };
    let (Ok(salt), Ok(hash)) = (
        BASE64.decode(salt.as_bytes()),
        BASE64.decode(hash.as_bytes()),
    ) else {
        return false;
    };
    let Ok(mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    // `verify_slice` compares in constant time.
    mac.chain_update(target.as_bytes())
        .verify_slice(&hash)
        .is_ok()
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    // Classic dynamic programming: dp[j] = pattern[..i] matches text[..j].
    let mut dp = vec![false; t.len() + 1];
    dp[0] = true;
    for &pc in &p {
        let mut next = vec![false; t.len() + 1];
        next[0] = dp[0] && pc == '*';
        for j in 1..=t.len() {
            next[j] = match pc {
                '*' => next[j - 1] || dp[j],
                '?' => dp[j - 1],
                c => dp[j - 1] && c == t[j - 1],
            };
        }
        dp = next;
    }
    dp[t.len()]
}

/// Writes through a temporary file + rename, so a crash never leaves a truncated
/// file. Mode 0600: the list of hosts you administer is nobody else's business.
fn write_private_file(path: &Path, content: &str) -> Result<(), AppError> {
    let storage_error =
        |e: std::io::Error| AppError::Storage(format!("écriture de {} : {e}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(storage_error)?;
    }
    let tmp = path.with_extension("tmp");
    let mut file =
        crate::platform::private_file(OpenOptions::new().write(true).create(true).truncate(true))
            .open(&tmp)
            .map_err(storage_error)?;
    file.write_all(content.as_bytes()).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)?;
    fs::rename(&tmp, path).map_err(storage_error)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const ED_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";
    const ED_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMpKXV8Haj77LheIxsYi7Za1NIVsqMEvsOUGtoliTK5c";

    fn key(s: &str) -> PublicKey {
        PublicKey::from_openssh(s).unwrap()
    }

    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "netforge-kh-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn store(own: &str, user: Option<&str>) -> (KnownHosts, PathBuf) {
        let dir = temp_dir();
        let own_path = dir.join("known_hosts");
        fs::write(&own_path, own).unwrap();
        let user_path = user.map(|content| {
            let p = dir.join("user_known_hosts");
            fs::write(&p, content).unwrap();
            p
        });
        (KnownHosts::new(own_path, user_path), dir)
    }

    #[test]
    fn unknown_then_trusted_after_add() {
        let (kh, _dir) = store("", None);
        assert!(matches!(
            kh.check("r1", 22, &key(ED_A)).unwrap(),
            HostKeyStatus::Unknown
        ));
        kh.add("r1", 22, &key(ED_A)).unwrap();
        assert!(matches!(
            kh.check("R1", 22, &key(ED_A)).unwrap(),
            HostKeyStatus::Trusted
        ));
        // Same host on another port is another entry.
        assert!(matches!(
            kh.check("r1", 2222, &key(ED_A)).unwrap(),
            HostKeyStatus::Unknown
        ));
    }

    #[test]
    fn changed_key_is_reported_with_both_fingerprints() {
        let (kh, _dir) = store(&format!("# comment\n\n[r1]:2222 {ED_A}\n"), None);
        match kh.check("r1", 2222, &key(ED_B)).unwrap() {
            HostKeyStatus::Changed(m) => {
                assert_eq!(m.line, 3, "comments and blank lines count");
                assert_eq!(
                    m.old_fingerprint,
                    key(ED_A).fingerprint(HashAlg::Sha256).to_string()
                );
                assert_eq!(
                    m.new_fingerprint,
                    key(ED_B).fingerprint(HashAlg::Sha256).to_string()
                );
            }
            other => panic!("expected Changed, got {other:?}"),
        }
    }

    #[test]
    fn replace_requires_the_refused_fingerprint() {
        let (kh, _dir) = store(&format!("other {ED_A}\nr1 {ED_A}\n"), None);
        assert!(matches!(
            kh.check("r1", 22, &key(ED_B)).unwrap(),
            HostKeyStatus::Changed(_)
        ));
        assert!(kh.replace("r1", 22, "SHA256:wrong").is_err());
        let fp = key(ED_B).fingerprint(HashAlg::Sha256).to_string();
        kh.replace("r1", 22, &fp).unwrap();
        assert!(matches!(
            kh.check("r1", 22, &key(ED_B)).unwrap(),
            HostKeyStatus::Trusted
        ));
        // Other hosts are untouched.
        assert!(matches!(
            kh.check("other", 22, &key(ED_A)).unwrap(),
            HostKeyStatus::Trusted
        ));
        // The pending key is consumed: a second replace is refused.
        assert!(kh.replace("r1", 22, &fp).is_err());
    }

    #[test]
    fn user_file_is_trusted_but_own_file_wins() {
        let (kh, _dir) = store("", Some(&format!("r1 {ED_A}\n")));
        assert!(matches!(
            kh.check("r1", 22, &key(ED_A)).unwrap(),
            HostKeyStatus::Trusted
        ));
        assert!(matches!(
            kh.check("r1", 22, &key(ED_B)).unwrap(),
            HostKeyStatus::Changed(_)
        ));
        let fp = key(ED_B).fingerprint(HashAlg::Sha256).to_string();
        kh.replace("r1", 22, &fp).unwrap();
        assert!(matches!(
            kh.check("r1", 22, &key(ED_B)).unwrap(),
            HostKeyStatus::Trusted
        ));
    }

    #[test]
    fn revoked_key_is_always_refused() {
        let (kh, _dir) = store(
            &format!("@revoked * {ED_A}\n"),
            Some(&format!("r1 {ED_A}\n")),
        );
        assert!(matches!(
            kh.check("r1", 22, &key(ED_A)).unwrap(),
            HostKeyStatus::Revoked
        ));
    }

    #[test]
    fn hashed_wildcard_and_negated_patterns() {
        // Hashed entry for "example.com" (from OpenSSH's own test vectors used by russh).
        let hashed = "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak=";
        assert!(hosts_match(hashed, "example.com"));
        assert!(!hosts_match(hashed, "example.org"));
        assert!(hosts_match("*.lab,!bad.lab", "r1.lab"));
        assert!(!hosts_match("*.lab,!bad.lab", "bad.lab"));
        assert!(hosts_match("10.0.0.?", "10.0.0.7"));
        assert!(!hosts_match("10.0.0.?", "10.0.0.17"));
    }

    #[test]
    fn concurrent_adds_lose_nothing_and_never_duplicate() {
        let (kh, _dir) = store("", None);
        let kh = std::sync::Arc::new(kh);
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let kh = kh.clone();
                std::thread::spawn(move || {
                    kh.add(&format!("r{i}"), 22, &key(ED_A)).unwrap();
                    kh.add(&format!("r{i}"), 22, &key(ED_A)).unwrap();
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let content = fs::read_to_string(kh.own_file()).unwrap();
        assert_eq!(content.lines().count(), 8, "{content}");
        for i in 0..8 {
            assert!(matches!(
                kh.check(&format!("r{i}"), 22, &key(ED_A)).unwrap(),
                HostKeyStatus::Trusted
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn written_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let (kh, _dir) = store("", None);
        fs::remove_file(kh.own_file()).unwrap();
        kh.add("r1", 22, &key(ED_A)).unwrap();
        let mode = fs::metadata(kh.own_file()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
