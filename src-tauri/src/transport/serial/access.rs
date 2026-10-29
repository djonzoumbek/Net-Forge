//! Why a serial port cannot be opened, explained for this machine.
//!
//! On Ubuntu, `/dev/ttyUSB0`, `/dev/ttyACM0` and `/dev/ttyS0` belong to the
//! `dialout` group (mode 0660). A user outside that group gets "permission
//! denied"; a user just added to it must open a new login session first, because
//! the groups of a process are fixed when the session starts.
//!
//! Everything is read from `/etc/group` and `/proc/self/status`: no extra
//! dependency, and the pure functions below are unit-tested.

use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Uid and groups of the current process, from `/proc/self/status`.
#[derive(Debug, Default, PartialEq)]
pub struct ProcessIds {
    pub uid: Option<u32>,
    pub groups: Vec<u32>,
}

pub fn parse_proc_status(status: &str) -> ProcessIds {
    let mut ids = ProcessIds::default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("Uid:") {
            // Real, effective, saved, filesystem: the effective one decides.
            ids.uid = rest.split_whitespace().nth(1).and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.strip_prefix("Groups:") {
            ids.groups = rest
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
        }
    }
    ids
}

/// Name and members of the group `gid`, from the content of `/etc/group`.
pub fn group_entry(group_file: &str, gid: u32) -> Option<(String, Vec<String>)> {
    group_file.lines().find_map(|line| {
        let mut fields = line.split(':');
        let name = fields.next()?;
        let _password = fields.next()?;
        let id: u32 = fields.next()?.parse().ok()?;
        let members = fields.next().unwrap_or("");
        (id == gid).then(|| {
            (
                name.to_string(),
                members
                    .split(',')
                    .filter(|m| !m.is_empty())
                    .map(str::to_string)
                    .collect(),
            )
        })
    })
}

/// What the device file allows the current process (read + write needed).
pub fn can_open(mode: u32, owner: u32, group: u32, ids: &ProcessIds) -> bool {
    let rw = |shift: u32| (mode >> shift) & 0o6 == 0o6;
    if ids.uid == Some(0) || (ids.uid == Some(owner) && rw(6)) {
        return true;
    }
    (ids.groups.contains(&group) && rw(3)) || rw(0)
}

/// The explanation shown after "permission denied".
pub fn diagnose(
    device: &str,
    group: &str,
    members: &[String],
    user: &str,
    in_group_now: bool,
) -> String {
    if in_group_now {
        format!(
            "Tu fais déjà partie du groupe « {group} » : vérifie les droits de {device} \
             (`ls -l {device}`)."
        )
    } else if members.iter().any(|m| m == user) {
        format!(
            "Ton compte est bien dans le groupe « {group} », mais ta session actuelle a été \
             ouverte avant cet ajout : déconnecte-toi puis reconnecte-toi (ou redémarre), \
             puis relance NetForge."
        )
    } else {
        format!(
            "{device} appartient au groupe « {group} ». Ajoute ton compte à ce groupe (une \
             seule fois) : `sudo usermod -aG {group} $USER`, puis déconnecte-toi et \
             reconnecte-toi pour que ce soit pris en compte."
        )
    }
}

fn read_ids() -> ProcessIds {
    std::fs::read_to_string("/proc/self/status")
        .map(|s| parse_proc_status(&s))
        .unwrap_or_default()
}

/// `None` when the file cannot be inspected (it may not exist).
pub fn accessible(device: &Path) -> Option<bool> {
    let meta = std::fs::metadata(device).ok()?;
    Some(can_open(meta.mode(), meta.uid(), meta.gid(), &read_ids()))
}

/// Builds the explanation for `device` from the real system files.
pub fn permission_message(device: &Path) -> String {
    let display = device.display().to_string();
    let gid = std::fs::metadata(device).map(|m| m.gid()).ok();
    let group_file = std::fs::read_to_string("/etc/group").unwrap_or_default();
    let (group, members) = gid
        .and_then(|gid| group_entry(&group_file, gid))
        .unwrap_or_else(|| ("dialout".into(), Vec::new()));
    let user = std::env::var("USER").unwrap_or_default();
    let in_group_now = gid.is_some_and(|gid| read_ids().groups.contains(&gid));
    diagnose(&display, &group, &members, &user, in_group_now)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROUPS: &str =
        "root:x:0:\nadm:x:4:syslog,kamis\ndialout:x:20:alice,bob\nplugdev:x:46:kamis\n";

    #[test]
    fn reads_process_groups_and_group_file() {
        let status = "Name:\tnetforge\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\nGroups:\t4 24 27 46 \n";
        assert_eq!(
            parse_proc_status(status),
            ProcessIds {
                uid: Some(1000),
                groups: vec![4, 24, 27, 46]
            }
        );
        assert_eq!(
            group_entry(GROUPS, 20),
            Some(("dialout".into(), vec!["alice".into(), "bob".into()]))
        );
        assert_eq!(group_entry(GROUPS, 0), Some(("root".into(), vec![])));
        assert_eq!(group_entry(GROUPS, 99), None);
    }

    #[test]
    fn device_access_follows_unix_permissions() {
        let user = ProcessIds {
            uid: Some(1000),
            groups: vec![4, 46],
        };
        let dialout_member = ProcessIds {
            uid: Some(1000),
            groups: vec![20],
        };
        // crw-rw---- root:dialout
        assert!(!can_open(0o20660, 0, 20, &user));
        assert!(can_open(0o20660, 0, 20, &dialout_member));
        // crw-rw-rw- (some USB adapters with a udev rule)
        assert!(can_open(0o20666, 0, 20, &user));
        // crw------- owned by the user (a pty) / by someone else
        assert!(can_open(0o20600, 1000, 5, &user));
        assert!(!can_open(0o20600, 0, 5, &user));
        assert!(can_open(
            0o20660,
            0,
            20,
            &ProcessIds {
                uid: Some(0),
                groups: vec![]
            }
        ));
    }

    #[test]
    fn explains_the_three_situations() {
        let members = vec!["alice".to_string(), "kamis".to_string()];
        let not_member = diagnose("/dev/ttyUSB0", "dialout", &members, "bob", false);
        assert!(
            not_member.contains("sudo usermod -aG dialout $USER"),
            "{not_member}"
        );
        let not_yet = diagnose("/dev/ttyUSB0", "dialout", &members, "kamis", false);
        assert!(not_yet.contains("reconnecte-toi"), "{not_yet}");
        assert!(!not_yet.contains("usermod"));
        let other = diagnose("/dev/ttyUSB0", "dialout", &members, "kamis", true);
        assert!(other.contains("ls -l /dev/ttyUSB0"), "{other}");
    }
}
