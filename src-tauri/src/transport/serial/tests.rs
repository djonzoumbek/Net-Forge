//! Serial transport against a pseudo-terminal pair: `master` plays the device,
//! the slave side is opened by path, exactly like `/dev/ttyUSB0`.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;
use tokio_serial::SerialPort;

use super::*;

/// Returns the "device" end and the path of the port to open.
fn fake_device() -> (SerialStream, String) {
    let (master, slave) = SerialStream::pair().expect("pty pair");
    let path = slave.name().expect("slave pty path");
    // Closed here, reopened by `connect` (the master keeps the pty alive).
    drop(slave);
    (master, path)
}

async fn next_data(transport: &mut SerialTransport) -> Vec<u8> {
    match timeout(Duration::from_secs(5), transport.recv()).await {
        Ok(TransportEvent::Data(data)) => data,
        other => panic!("expected data, got {other:?}"),
    }
}

#[tokio::test]
async fn data_flows_both_ways_with_custom_line_settings() {
    let (mut device, path) = fake_device();
    let settings = SerialSettings {
        baud_rate: 115_200,
        data_bits: 7,
        parity: Parity::Even,
        stop_bits: 2,
        flow_control: FlowControl::Software,
    };
    let mut port = connect(SerialParams {
        device: path,
        settings,
    })
    .await
    .unwrap();
    assert_eq!(port.info().protocol, "serial");

    // Device -> NetForge, with UTF-8 (the pty is raw, bytes pass untouched).
    device.write_all("Router>é\r\n".as_bytes()).await.unwrap();
    let mut received = Vec::new();
    while received.len() < "Router>é\r\n".len() {
        received.extend(next_data(&mut port).await);
    }
    assert_eq!(received, "Router>é\r\n".as_bytes());

    // NetForge -> device.
    port.send(b"enable\r").await.unwrap();
    let mut buf = [0u8; 16];
    let n = timeout(Duration::from_secs(5), device.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..n], b"enable\r");
    assert!(
        port.resize(TermSize {
            cols: 120,
            rows: 40
        })
        .await
        .is_ok()
    );
    port.close().await;
}

#[tokio::test]
async fn a_port_in_use_is_reported_clearly() {
    let (_device, path) = fake_device();
    let first = connect(SerialParams {
        device: path.clone(),
        settings: SerialSettings::default(),
    })
    .await
    .unwrap();
    let Err(err) = connect(SerialParams {
        device: path,
        settings: SerialSettings::default(),
    })
    .await
    else {
        panic!("the port is opened exclusively");
    };
    assert_eq!(err.code(), "serial");
    assert!(err.to_string().contains("déjà utilisé"), "{err}");
    first.close().await;
}

#[tokio::test]
async fn unplugged_device_is_detected() {
    let (device, path) = fake_device();
    let mut port = connect(SerialParams {
        device: path,
        settings: SerialSettings::default(),
    })
    .await
    .unwrap();
    // Closing the master is what the kernel does when a USB adapter disappears.
    drop(device);
    match timeout(Duration::from_secs(5), port.recv()).await {
        Ok(TransportEvent::Lost(reason)) => assert!(reason.contains("Port série"), "{reason}"),
        other => panic!("expected Lost, got {other:?}"),
    }
}

#[tokio::test]
async fn invalid_and_missing_ports() {
    let missing = connect(SerialParams {
        device: "/dev/ttyUSB-netforge-absent".into(),
        settings: SerialSettings::default(),
    })
    .await;
    assert!(
        matches!(&missing, Err(AppError::Serial(m)) if m.contains("introuvable")),
        "{:?}",
        missing.err()
    );
    for device in ["ttyUSB0", "/etc/passwd/../x", ""] {
        let result = connect(SerialParams {
            device: device.into(),
            settings: SerialSettings::default(),
        })
        .await;
        if !device.starts_with("/dev/") {
            assert!(matches!(result, Err(AppError::InvalidInput(_))), "{device}");
        }
    }
    let bad = SerialSettings {
        data_bits: 9,
        ..SerialSettings::default()
    };
    assert!(bad.validate().is_err());
    let bad = SerialSettings {
        baud_rate: 0,
        ..SerialSettings::default()
    };
    assert!(bad.validate().is_err());
}

/// Needs a real port the user cannot open, e.g. `/dev/ttyS0` (root:dialout)
/// for a user outside `dialout`: `NETFORGE_SERIAL_DENIED=/dev/ttyS0 cargo test -- --ignored denied`.
#[tokio::test]
#[ignore = "needs a serial port without access rights (NETFORGE_SERIAL_DENIED)"]
async fn denied_port_explains_the_dialout_group() {
    let device = std::env::var("NETFORGE_SERIAL_DENIED").unwrap_or_else(|_| "/dev/ttyS0".into());
    assert_eq!(access::accessible(Path::new(&device)), Some(false));
    let err = connect(SerialParams {
        device: device.clone(),
        settings: SerialSettings::default(),
    })
    .await
    .err()
    .expect("access must be denied");
    assert_eq!(err.code(), "serial_permission");
    let message = err.to_string();
    println!("{message}");
    assert!(message.contains("dialout"), "{message}");
}

/// Through the session manager, as the UI uses it: connected, output delivered,
/// unplugging reported as a lost connection, and the session released.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serial_session_end_to_end() {
    use std::sync::{Arc, Mutex};

    use crate::sessions::{Automation, SessionEvent, SessionManager, SessionSinks};

    let (mut device, path) = fake_device();
    let manager = SessionManager::default();
    let output = Arc::new(Mutex::new(Vec::new()));
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let out = Arc::clone(&output);
    let sinks = SessionSinks {
        data: Box::new(move |bytes| out.lock().unwrap().extend_from_slice(&bytes)),
        event: Box::new(move |e| {
            let _ = tx.send(e);
        }),
    };
    let params = SerialParams {
        device: path,
        settings: SerialSettings::default(),
    };
    let id = manager.open_serial(
        params,
        TermSize { cols: 80, rows: 24 },
        Automation::default(),
        sinks,
    );
    match timeout(Duration::from_secs(5), events.recv()).await {
        Ok(Some(SessionEvent::Connected { info, .. })) => assert_eq!(info.protocol, "serial"),
        other => panic!("expected Connected, got {other:?}"),
    }
    device.write_all(b"\r\nSwitch>").await.unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !String::from_utf8_lossy(&output.lock().unwrap()).contains("Switch>") {
        assert!(
            tokio::time::Instant::now() < deadline,
            "output not delivered"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    manager
        .send(
            id,
            crate::sessions::SessionCommand::Write(b"show version\r".to_vec()),
        )
        .await
        .unwrap();
    let mut buf = [0u8; 32];
    let n = timeout(Duration::from_secs(5), device.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf[..n], b"show version\r");

    drop(device);
    match timeout(Duration::from_secs(5), events.recv()).await {
        Ok(Some(SessionEvent::Closed {
            lost, by_remote, ..
        })) => assert!(lost && by_remote),
        other => panic!("expected Closed(lost), got {other:?}"),
    }
    // `Closed` is sent just before the session removes itself: wait for it.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while manager
        .send(id, crate::sessions::SessionCommand::Ack(0))
        .await
        .is_ok()
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "session not released"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
