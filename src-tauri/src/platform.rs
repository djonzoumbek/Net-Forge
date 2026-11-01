//! The few places where Linux and Windows differ, kept together.
//!
//! Private files: on Linux the mode bits (0600 / 0700) keep other users out. On
//! Windows the app data directory (`%APPDATA%`) is already private to the user
//! through its inherited ACL, so there is nothing to add.

use std::fs::{DirBuilder, OpenOptions};
use std::path::Path;

/// Readable and writable by the current user only (Linux: 0600).
pub fn private_file(options: &mut OpenOptions) -> &mut OpenOptions {
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(options, 0o600);
    options
}

/// Same as [`private_file`] for tokio's asynchronous `OpenOptions`.
pub fn private_file_async(options: &mut tokio::fs::OpenOptions) -> &mut tokio::fs::OpenOptions {
    #[cfg(unix)]
    options.mode(0o600);
    options
}

/// Directory usable by the current user only (Linux: 0700).
pub fn private_dir(builder: &mut DirBuilder) -> &mut DirBuilder {
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(builder, 0o700);
    builder
}

/// Restricts an existing file or directory (Linux: 0600 / 0700; Windows: no-op).
pub fn restrict(path: &Path, is_dir: bool) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if is_dir { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, is_dir);
        Ok(())
    }
}

/// Example shown when a serial port name is rejected.
#[cfg(windows)]
pub const SERIAL_EXAMPLE: &str = "COM3";
#[cfg(not(windows))]
pub const SERIAL_EXAMPLE: &str = "/dev/ttyUSB0";

/// Linux: a device file under `/dev/`. Windows: `COM3`, or `\\.\COM10`
/// (the form Windows requires from COM10 on).
pub fn is_serial_device(name: &str) -> bool {
    if name.len() > 255 {
        return false;
    }
    if cfg!(windows) {
        let com = name.strip_prefix(r"\\.\").unwrap_or(name);
        // `get`, not `[..3]`: slicing inside a multi-byte character would panic.
        let (prefix, number) = (com.get(..3), com.get(3..));
        prefix.is_some_and(|p| p.eq_ignore_ascii_case("COM"))
            && number.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    } else {
        name.starts_with("/dev/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_device_names_follow_the_os() {
        if cfg!(windows) {
            assert!(is_serial_device("COM3"));
            assert!(is_serial_device(r"\\.\COM12"));
            assert!(!is_serial_device("COM"));
            assert!(!is_serial_device("/dev/ttyUSB0"));
        } else {
            assert!(is_serial_device("/dev/ttyUSB0"));
            assert!(!is_serial_device("COM3"));
            assert!(!is_serial_device("ttyUSB0"));
        }
    }
}
