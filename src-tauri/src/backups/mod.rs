//! Configuration backups: one text file per capture, in
//! `<base>/<device>/<YYYY-mm-dd_HH-MM-SS>.txt` (base: ~/Documents/NetForge/backups).
//!
//! Plain files rather than SQLite: they can be opened, diffed, versioned with git
//! or copied by the user with any tool. A configuration often contains secrets
//! (password hashes, SNMP communities, keys): files are private (0600, 0700).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::errors::AppError;
use crate::sessions::logger::safe_file_name;

/// Reading a backup back: bigger than any real configuration.
const MAX_READ: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// File name, also the identifier given back to `read`.
    pub name: String,
    /// `YYYY-mm-dd HH:MM:SS`, local time.
    pub taken_at: String,
    pub bytes: u64,
    pub path: String,
}

fn storage(e: std::io::Error) -> AppError {
    AppError::Storage(format!("sauvegarde de configuration : {e}"))
}

/// Folder of one device; `device` is sanitized (no path traversal possible).
pub fn device_dir(base: &Path, device: &str) -> PathBuf {
    base.join(safe_file_name(device))
}

/// `2026-11-01_09-30-00.txt` or, for several backups in the same second,
/// `2026-11-01_09-30-00_02.txt`. With `_` (after `.` in ASCII) and two digits,
/// alphabetical order is chronological order. Anything else is refused by `read`.
fn valid_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".txt") else {
        return false;
    };
    let bytes = stem.as_bytes();
    (bytes.len() == 19 || bytes.len() == 22)
        && bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 | 13 | 16 => *b == b'-',
            10 | 19 => *b == b'_',
            _ => b.is_ascii_digit(),
        })
}

fn taken_at(name: &str) -> String {
    // 2026-11-01_09-30-00 -> 2026-11-01 09:30:00
    let stem = &name[..19.min(name.len())];
    let (date, time) = stem.split_at(10.min(stem.len()));
    format!("{date} {}", time.trim_start_matches('_').replace('-', ":"))
}

fn entry(path: &Path) -> Option<BackupEntry> {
    let name = path.file_name()?.to_str()?.to_string();
    if !valid_name(&name) {
        return None;
    }
    let bytes = fs::metadata(path).ok()?.len();
    Some(BackupEntry {
        taken_at: taken_at(&name),
        bytes,
        path: path.display().to_string(),
        name,
    })
}

/// Writes a new backup; never overwrites an existing one.
pub fn save(base: &Path, device: &str, content: &str) -> Result<BackupEntry, AppError> {
    let dir = device_dir(base, device);
    crate::platform::private_dir(fs::DirBuilder::new().recursive(true))
        .create(&dir)
        .map_err(storage)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    for attempt in 1..100 {
        let name = if attempt == 1 {
            format!("{stamp}.txt")
        } else {
            format!("{stamp}_{attempt:02}.txt")
        };
        let path = dir.join(&name);
        let opened =
            crate::platform::private_file(fs::OpenOptions::new().write(true).create_new(true))
                .open(&path);
        match opened {
            Ok(mut file) => {
                file.write_all(content.as_bytes()).map_err(storage)?;
                file.sync_all().map_err(storage)?;
                return entry(&path).ok_or_else(|| {
                    AppError::Storage("sauvegarde illisible après écriture.".into())
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(storage(e)),
        }
    }
    Err(AppError::Storage(
        "trop de sauvegardes dans la même seconde.".into(),
    ))
}

/// The device's backups, newest first. No folder yet: an empty list.
pub fn list(base: &Path, device: &str) -> Result<Vec<BackupEntry>, AppError> {
    let dir = device_dir(base, device);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(storage(e)),
    };
    let mut backups: Vec<BackupEntry> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| entry(&e.path()))
        .collect();
    // The names sort chronologically (see `valid_name`).
    backups.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(backups)
}

pub fn read(base: &Path, device: &str, name: &str) -> Result<String, AppError> {
    if !valid_name(name) {
        return Err(AppError::InvalidInput("nom de sauvegarde invalide.".into()));
    }
    let path = device_dir(base, device).join(name);
    let size = fs::metadata(&path).map_err(storage)?.len();
    if size > MAX_READ {
        return Err(AppError::Storage(
            "fichier de sauvegarde trop volumineux.".into(),
        ));
    }
    let bytes = fs::read(&path).map_err(storage)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("netforge-backups-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn saves_lists_and_reads_back() {
        let base = base("roundtrip");
        assert!(list(&base, "R1").unwrap().is_empty(), "no folder yet");
        let first = save(&base, "R1 (10.0.0.1:22)", "hostname R1\n").unwrap();
        let second = save(&base, "R1 (10.0.0.1:22)", "hostname R1-new\n").unwrap();
        assert_ne!(
            first.name, second.name,
            "same second: suffixed, never overwritten"
        );
        let all = list(&base, "R1 (10.0.0.1:22)").unwrap();
        assert_eq!(all, vec![second.clone(), first.clone()], "newest first");
        assert_eq!(
            read(&base, "R1 (10.0.0.1:22)", &first.name).unwrap(),
            "hostname R1\n"
        );
        assert_eq!(first.taken_at.len(), 19);
        assert_eq!(&first.taken_at[10..11], " ");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&first.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn names_cannot_escape_the_folder() {
        let base = base("traversal");
        for bad in [
            "../../etc/passwd",
            "x.txt",
            "2026-11-01_09-30-00.cfg",
            "2026-11-01_09-30-00/../a.txt",
        ] {
            assert!(read(&base, "R1", bad).is_err(), "{bad}");
        }
        assert!(valid_name("2026-11-01_09-30-00.txt"));
        assert!(valid_name("2026-11-01_09-30-00_12.txt"));
        assert!(!valid_name("2026-11-01_09-30-00-12.txt"));
        assert!("2026-11-01_09-30-00_02.txt" > "2026-11-01_09-30-00.txt");
        // The device name is sanitized: it stays one folder under the base.
        let dir = device_dir(&base, "../../etc");
        assert_eq!(dir.parent(), Some(base.as_path()));
    }
}
