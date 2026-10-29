//! Serial transport: console cable (RS-232, USB-serial adapter) of a network
//! device, for its initial configuration.
//!
//! `tokio-serial` gives an asynchronous port registered with tokio's reactor:
//! reads are cancel-safe, as `Transport::recv` requires, and no thread is
//! blocked waiting for the device.

pub mod access;

use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

use super::{ConnectionInfo, TermSize, Transport, TransportEvent};
use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlowControl {
    None,
    /// XON/XOFF.
    Software,
    /// RTS/CTS.
    Hardware,
}

/// Line settings. Cisco and Huawei consoles: 9600 8N1, no flow control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialSettings {
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
    pub flow_control: FlowControl,
}

impl Default for SerialSettings {
    fn default() -> Self {
        Self {
            baud_rate: 9600,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow_control: FlowControl::None,
        }
    }
}

impl SerialSettings {
    pub fn validate(self) -> Result<Self, AppError> {
        let invalid = |m: &str| Err(AppError::InvalidInput(m.into()));
        if !(50..=4_000_000).contains(&self.baud_rate) {
            return invalid("vitesse série : entre 50 et 4 000 000 bauds.");
        }
        if !(5..=8).contains(&self.data_bits) {
            return invalid("bits de données : 5, 6, 7 ou 8.");
        }
        if !(1..=2).contains(&self.stop_bits) {
            return invalid("bits de stop : 1 ou 2.");
        }
        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct SerialParams {
    /// e.g. `/dev/ttyUSB0`.
    pub device: String,
    pub settings: SerialSettings,
}

/// A port found on the machine, for the port picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortEntry {
    pub path: String,
    /// e.g. "USB — FTDI FT232R USB UART".
    pub description: String,
    /// `false`: opening it will be refused (typically: not in `dialout`).
    pub accessible: bool,
}

/// Lists the serial ports. Blocking (udev): call it on the blocking pool.
pub fn list_ports() -> Result<Vec<SerialPortEntry>, AppError> {
    let ports = tokio_serial::available_ports()
        .map_err(|e| AppError::Serial(format!("liste des ports impossible : {e}")))?;
    let mut entries: Vec<SerialPortEntry> = ports
        .into_iter()
        .map(|port| {
            let description = match port.port_type {
                tokio_serial::SerialPortType::UsbPort(usb) => {
                    let name = [usb.manufacturer, usb.product]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if name.is_empty() {
                        format!("USB {:04x}:{:04x}", usb.vid, usb.pid)
                    } else {
                        format!("USB — {name}")
                    }
                }
                tokio_serial::SerialPortType::PciPort => "port série intégré".into(),
                tokio_serial::SerialPortType::BluetoothPort => "Bluetooth".into(),
                tokio_serial::SerialPortType::Unknown => String::new(),
            };
            let accessible = access::accessible(Path::new(&port.port_name)).unwrap_or(true);
            SerialPortEntry {
                path: port.port_name,
                description,
                accessible,
            }
        })
        .collect();
    // USB adapters (the usual console cables) first.
    entries.sort_by(|a, b| {
        (!a.description.starts_with("USB"))
            .cmp(&!b.description.starts_with("USB"))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(entries)
}

pub struct SerialTransport {
    stream: SerialStream,
    device: String,
    buf: Vec<u8>,
}

pub async fn connect(params: SerialParams) -> Result<SerialTransport, AppError> {
    let settings = params.settings.validate()?;
    let device = params.device.trim().to_string();
    if !device.starts_with("/dev/") {
        return Err(AppError::InvalidInput(format!(
            "port série invalide : « {device} » (attendu : /dev/ttyUSB0, /dev/ttyS0…)."
        )));
    }
    let builder = tokio_serial::new(&device, settings.baud_rate)
        .data_bits(match settings.data_bits {
            5 => tokio_serial::DataBits::Five,
            6 => tokio_serial::DataBits::Six,
            7 => tokio_serial::DataBits::Seven,
            _ => tokio_serial::DataBits::Eight,
        })
        .parity(match settings.parity {
            Parity::None => tokio_serial::Parity::None,
            Parity::Odd => tokio_serial::Parity::Odd,
            Parity::Even => tokio_serial::Parity::Even,
        })
        .stop_bits(if settings.stop_bits == 2 {
            tokio_serial::StopBits::Two
        } else {
            tokio_serial::StopBits::One
        })
        .flow_control(match settings.flow_control {
            FlowControl::None => tokio_serial::FlowControl::None,
            FlowControl::Software => tokio_serial::FlowControl::Software,
            FlowControl::Hardware => tokio_serial::FlowControl::Hardware,
        });
    // Opening only configures the line (non-blocking open, a few ioctls).
    let stream = builder
        .open_native_async()
        .map_err(|e| open_error(&device, e))?;
    tracing::info!(device = %device, baud = settings.baud_rate, "serial port opened");
    Ok(SerialTransport {
        stream,
        device,
        buf: vec![0; 4096],
    })
}

fn open_error(device: &str, e: tokio_serial::Error) -> AppError {
    use std::io::ErrorKind as Io;
    use tokio_serial::ErrorKind as K;
    match e.kind {
        K::Io(Io::PermissionDenied) => {
            AppError::SerialPermission(access::permission_message(Path::new(device)))
        }
        K::Io(Io::NotFound) => AppError::Serial(format!(
            "{device} introuvable : l'adaptateur USB est-il branché ? (`ls /dev/ttyUSB* /dev/ttyACM*`)"
        )),
        K::NoDevice => AppError::Serial(format!(
            "{device} est déjà utilisé par un autre programme (minicom, screen, une autre \
             session NetForge ?)."
        )),
        _ => AppError::Serial(format!(
            "ouverture de {device} impossible : {}",
            e.description
        )),
    }
}

impl Transport for SerialTransport {
    fn info(&self) -> ConnectionInfo {
        ConnectionInfo {
            protocol: "serial",
            // A local cable, not a network: "encrypted" does not apply, the UI shows
            // a neutral badge for this protocol.
            encrypted: false,
            algorithms: None,
        }
    }

    async fn recv(&mut self) -> TransportEvent {
        // `read` on a tokio AsyncFd-based stream is cancel-safe.
        match self.stream.read(&mut self.buf).await {
            Ok(0) => TransportEvent::Lost(format!("Port série {} fermé.", self.device)),
            Ok(n) => TransportEvent::Data(self.buf[..n].to_vec()),
            // Unplugging a USB adapter gives EIO here.
            Err(e) => TransportEvent::Lost(format!(
                "Port série {} perdu (adaptateur débranché ?) : {e}.",
                self.device
            )),
        }
    }

    async fn send(&mut self, data: &[u8]) -> Result<(), AppError> {
        self.stream
            .write_all(data)
            .await
            .map_err(|e| AppError::Serial(format!("écriture sur {} : {e}", self.device)))
    }

    async fn resize(&mut self, _size: TermSize) -> Result<(), AppError> {
        // A serial line has no window size: the device assumes 80x24 (Cisco:
        // `terminal width`, Huawei: `screen-width`).
        Ok(())
    }

    async fn close(self) {
        tracing::info!(device = %self.device, "serial port closed");
        // Dropping the stream closes the file descriptor and releases the lock.
    }
}

#[cfg(test)]
mod tests;
