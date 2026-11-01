//! SFTP tests against the Docker OpenSSH server (`./scripts/ssh-test-server.sh start`).

use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::mpsc;
use zeroize::Zeroizing;

use super::*;
use crate::security::known_hosts::KnownHosts;
use crate::sessions::{Automation, SessionCommand, SessionEvent, SessionManager, SessionSinks};
use crate::transport::TermSize;
use crate::transport::ssh::{SshAuth, SshParams};

const SIZE: TermSize = TermSize { cols: 80, rows: 24 };

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("netforge-sftp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A connected SSH session in a `SessionManager`, host key accepted explicitly.
async fn ssh_session(tag: &str) -> (SessionManager, u64) {
    let manager = SessionManager::default();
    let known_hosts = Arc::new(KnownHosts::new(temp_dir(tag).join("known_hosts"), None));
    let (tx, mut events) = mpsc::unbounded_channel();
    let sinks = SessionSinks {
        data: Box::new(|_| {}),
        event: Box::new(move |e| {
            let _ = tx.send(e);
        }),
    };
    let params = SshParams {
        host: "127.0.0.1".into(),
        port: 2222,
        username: "netforge".into(),
        auth: SshAuth::Password(Zeroizing::new("netforge-test".into())),
        legacy: false,
        keepalive: None,
    };
    let id = manager.open_ssh(params, SIZE, known_hosts, Automation::default(), sinks);
    loop {
        match tokio::time::timeout(Duration::from_secs(20), events.recv()).await {
            Ok(Some(SessionEvent::HostKeyPrompt { .. })) => {
                manager.answer_host_key(id, true).unwrap()
            }
            Ok(Some(SessionEvent::Connected { .. })) => return (manager, id),
            other => panic!("expected a connection, got {other:?}"),
        }
    }
}

/// Records every event; optionally sets `cancel` on the first progress event.
fn recorder(
    cancel: Option<Arc<AtomicBool>>,
) -> (
    Arc<Mutex<Vec<TransferEvent>>>,
    impl Fn(TransferEvent) + Send + Sync,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&events);
    let sink = move |e: TransferEvent| {
        if let (Some(flag), TransferEvent::Progress { .. }) = (&cancel, &e) {
            flag.store(true, Ordering::Relaxed);
        }
        seen.lock().unwrap().push(e);
    };
    (events, sink)
}

/// Pseudo-random but reproducible content: a wrong offset would be detected.
fn sample(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn listing_starts_in_home_and_sorts_directories_first() {
    let (manager, id) = ssh_session("list").await;
    let sftp = manager.sftp(id).await.unwrap();
    let home = list_dir(&sftp, "").await.unwrap();
    assert_eq!(home.path, "/home/netforge");
    let big = home
        .entries
        .iter()
        .find(|e| e.name == "big.txt")
        .expect("big.txt listed");
    assert_eq!(big.kind, EntryKind::File);
    assert!(big.size.unwrap() > 50_000);
    assert!(big.permissions.as_deref().unwrap().starts_with("rw"));

    let root = list_dir(&sftp, "/home/netforge/..").await.unwrap();
    assert_eq!(root.path, "/home", "`..` resolved by the server");
    let etc = list_dir(&sftp, "/etc").await.unwrap();
    let first_file = etc
        .entries
        .iter()
        .position(|e| e.kind != EntryKind::Dir)
        .unwrap();
    assert!(
        etc.entries[first_file..]
            .iter()
            .all(|e| e.kind != EntryKind::Dir)
    );

    let missing = list_dir(&sftp, "/nope").await.unwrap_err();
    assert_eq!(missing.code(), "sftp");
    assert!(missing.to_string().contains("introuvable"), "{missing}");
    manager.send(id, SessionCommand::Close).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn upload_then_download_round_trip_with_progress() {
    let (manager, id) = ssh_session("roundtrip").await;
    let sftp = manager.sftp(id).await.unwrap();
    let local = temp_dir("roundtrip");
    let remote_dir = make_dir(&sftp, "/tmp", &format!("nf-{}", std::process::id()))
        .await
        .unwrap();
    let content = sample(3 * 1024 * 1024 + 123);
    let source = local.join("image.bin");
    std::fs::write(&source, &content).unwrap();

    let no_cancel = AtomicBool::new(false);
    let (events, sink) = recorder(None);
    let sent = upload(&sftp, &source, &remote_dir, false, &no_cancel, &sink)
        .await
        .unwrap();
    assert_eq!(sent, content.len() as u64);
    let first = events.lock().unwrap()[0].clone();
    assert_eq!(
        first,
        TransferEvent::Progress {
            done: 0,
            total: Some(content.len() as u64)
        }
    );

    // Same name again without `overwrite`: refused, the remote file is intact.
    let again = upload(&sftp, &source, &remote_dir, false, &no_cancel, &sink)
        .await
        .unwrap_err();
    assert_eq!(again.code(), "remote_exists", "{again}");

    let back = local.join("back.bin");
    let received = download(
        &sftp,
        &remote_join(&remote_dir, "image.bin"),
        &back,
        &no_cancel,
        &sink,
    )
    .await
    .unwrap();
    assert_eq!(received, content.len() as u64);
    assert!(
        std::fs::read(&back).unwrap() == content,
        "identical content"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&back).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(!local.join("back.bin.netforge-part").exists());

    sftp.remove_file(remote_join(&remote_dir, "image.bin"))
        .await
        .unwrap();
    sftp.remove_dir(remote_dir).await.unwrap();
    manager.send(id, SessionCommand::Close).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn cancelled_transfers_leave_no_partial_file() {
    let (manager, id) = ssh_session("cancel").await;
    let sftp = manager.sftp(id).await.unwrap();
    let local = temp_dir("cancel");
    let source = local.join("upload.bin");
    std::fs::write(&source, sample(1024 * 1024)).unwrap();

    let cancel = Arc::new(AtomicBool::new(false));
    let (_, sink) = recorder(Some(Arc::clone(&cancel)));
    let err = upload(&sftp, &source, "/tmp", true, &cancel, &sink)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Cancelled));
    assert!(
        !sftp.try_exists("/tmp/upload.bin").await.unwrap(),
        "incomplete upload removed"
    );

    let cancel = Arc::new(AtomicBool::new(false));
    let (_, sink) = recorder(Some(Arc::clone(&cancel)));
    let target = local.join("big.txt");
    let err = download(&sftp, "/home/netforge/big.txt", &target, &cancel, &sink)
        .await
        .unwrap_err();
    assert!(matches!(err, AppError::Cancelled));
    assert!(!target.exists());
    assert!(!local.join("big.txt.netforge-part").exists());
    manager.send(id, SessionCommand::Close).await.unwrap();
}

/// Through the `TransferManager`, as the Tauri commands use it: the final event
/// is always sent, and the transfer is forgotten afterwards.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn manager_reports_the_outcome_and_forgets_the_transfer() {
    let (manager, id) = ssh_session("manager").await;
    let sftp = manager.sftp(id).await.unwrap();
    let transfers = TransferManager::default();
    let target = temp_dir("manager").join("big.txt");
    let (tx, mut rx) = mpsc::unbounded_channel();
    let job_target = target.clone();
    let transfer = transfers.start(
        Box::new(move |e| {
            let _ = tx.send(e);
        }),
        move |cancel, sink| async move {
            download(
                &sftp,
                "/home/netforge/big.txt",
                &job_target,
                &cancel,
                &*sink,
            )
            .await
        },
    );
    let last = loop {
        let event = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .unwrap()
            .unwrap();
        if !matches!(event, TransferEvent::Progress { .. }) {
            break event;
        }
    };
    assert!(
        matches!(last, TransferEvent::Done { bytes } if bytes > 50_000),
        "{last:?}"
    );
    assert!(
        std::fs::read_to_string(&target)
            .unwrap()
            .contains("ligne numéro 5000")
    );
    assert_eq!(transfers.len(), 0);
    assert!(transfers.cancel(transfer).is_err(), "already finished");
    manager.send(id, SessionCommand::Close).await.unwrap();
}

/// Closing the session releases the SFTP channel with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs ./scripts/ssh-test-server.sh start"]
async fn sftp_ends_with_its_session() {
    let (manager, id) = ssh_session("close").await;
    let sftp = manager.sftp(id).await.unwrap();
    assert!(
        Arc::ptr_eq(&sftp, &manager.sftp(id).await.unwrap()),
        "one channel per session"
    );
    manager.send(id, SessionCommand::Close).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while manager.sftp(id).await.is_ok() {
        assert!(Instant::now() < deadline, "session still listed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(matches!(
        manager.sftp(id).await,
        Err(AppError::SessionNotFound(_))
    ));
    assert!(
        list_dir(&sftp, "").await.is_err(),
        "the channel died with the connection"
    );
}

#[test]
fn names_and_paths() {
    assert_eq!(permission_string(0o100755), "rwxr-xr-x");
    assert_eq!(permission_string(0o640), "rw-r-----");
    assert_eq!(remote_join("/", "a"), "/a");
    assert_eq!(remote_join("/home/x", "a"), "/home/x/a");
    for bad in ["", ".", "..", "a/b", "x\0"] {
        assert!(valid_name(bad).is_err(), "{bad:?}");
    }
    assert!(valid_name("backup R1.cfg").is_ok());
}
