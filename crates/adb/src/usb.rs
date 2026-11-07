//! USB transport — enumerate ADB interfaces via nusb (no libusb).
//!
//! ADB devices expose an interface with class 0xFF, subclass 0x42, protocol
//! 0x01. nusb handles discovery on Windows (via SetupAPI), macOS (IOKit) and
//! Linux (sysfs/libusb compatibility shim). `watchdog device usb-doctor`
//! explains permission problems; on Linux, `watchdog device udev-rules`
//! installs them.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use nusb::Interface;
use tokio::sync::mpsc;

use crate::error::{AdbError, Result};
use crate::transport::Transport;

/// USB class/subclass/protocol for the ADB interface.
pub const ADB_USB_CLASS: u8 = 0xFF;
pub const ADB_USB_SUBCLASS: u8 = 0x42;
pub const ADB_USB_PROTOCOL: u8 = 0x01;

/// A connected USB ADB device.
#[derive(Debug, Clone)]
pub struct UsbDeviceInfo {
    /// Serial number (USB descriptor), or empty if the device omits it.
    pub serial: String,
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// Product string from descriptors.
    pub product: String,
}

impl UsbDeviceInfo {
    /// Stable identifier for `device list` (prefers serial).
    #[must_use]
    pub fn id(&self) -> String {
        if self.serial.is_empty() {
            format!("{:04x}:{:04x}", self.vendor_id, self.product_id)
        } else {
            self.serial.clone()
        }
    }
}

fn has_adb_interface(device: &nusb::Device) -> bool {
    device
        .open()
        .ok()
        .and_then(|d| d.active_configuration())
        .map(|config| {
            config.interfaces().any(|i| {
                i.class == ADB_USB_CLASS
                    && i.subclass == ADB_USB_SUBCLASS
                    && i.protocol == ADB_USB_PROTOCOL
            })
        })
        .unwrap_or(false)
}

/// Enumerate ADB-capable devices (without opening them).
#[must_use]
pub fn enumerate() -> Vec<UsbDeviceInfo> {
    let mut out = Vec::new();
    let Some(devices) = nusb::list_devices() else {
        return out;
    };
    for device in devices {
        if !has_adb_interface(&device) {
            continue;
        }
        let serial = device
            .open()
            .ok()
            .and_then(|d| d.serial_number())
            .unwrap_or_default();
        let product = device
            .open()
            .ok()
            .and_then(|d| d.product())
            .unwrap_or_default();
        out.push(UsbDeviceInfo {
            serial,
            vendor_id: device.id().vendor_id,
            product_id: device.id().product_id,
            product,
        });
    }
    out
}

/// A USB-backed [`Transport`].
///
/// A single blocking worker thread owns the `nusb::Interface` and multiplexes
/// an async read channel and an async write channel over the bulk endpoints.
pub struct UsbTransport {
    serial: String,
    read_rx: mpsc::Receiver<Vec<u8>>,
    write_tx: mpsc::Sender<Vec<u8>>,
}

impl std::fmt::Debug for UsbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsbTransport")
            .field("serial", &self.serial)
            .finish()
    }
}

fn claim_adb(device: &nusb::Device) -> Result<(Interface, u8, u8, String)> {
    let handle = device
        .open()
        .map_err(|e| AdbError::Transport(format!("open device: {e}")))?;
    let config = handle
        .active_configuration()
        .ok_or_else(|| AdbError::Transport("device has no active USB configuration".into()))?;
    let interface_desc = config
        .interfaces()
        .find(|i| {
            i.class == ADB_USB_CLASS
                && i.subclass == ADB_USB_SUBCLASS
                && i.protocol == ADB_USB_PROTOCOL
        })
        .ok_or_else(|| AdbError::Transport("no ADB interface on this device".into()))?;

    let interface = handle.claim_interface(interface_desc.number).map_err(|e| {
        AdbError::Transport(format!("claim interface {}: {e}", interface_desc.number))
    })?;

    let in_endpoint = interface_desc
        .endpoints()
        .find(|e| e.address & 0x80 != 0)
        .map(|e| e.address)
        .ok_or_else(|| AdbError::Transport("no bulk-in endpoint".into()))?;
    let out_endpoint = interface_desc
        .endpoints()
        .find(|e| e.address & 0x80 == 0)
        .map(|e| e.address)
        .ok_or_else(|| AdbError::Transport("no bulk-out endpoint".into()))?;

    let serial = handle.serial_number().unwrap_or_default();
    Ok((interface, in_endpoint, out_endpoint, serial))
}

impl UsbTransport {
    /// Open the first USB device exposing an ADB interface.
    pub fn open() -> Result<Self> {
        for info in enumerate() {
            if let Ok(transport) = Self::open_by_serial(&info.serial) {
                return Ok(transport);
            }
        }
        Err(AdbError::Transport(
            "no USB ADB device found (check the cable, USB debugging toggle, and drivers/udev rules; see `watchdog device usb-doctor`)".into(),
        ))
    }

    /// Open the device with the given serial (empty matches the first device).
    pub fn open_by_serial(serial: &str) -> Result<Self> {
        let devices = nusb::list_devices().ok_or_else(|| {
            AdbError::Transport(
                "nusb could not list devices (BIOS/driver blocked enumeration?)".into(),
            )
        })?;

        let mut target = None;
        for device in devices {
            if !has_adb_interface(&device) {
                continue;
            }
            let device_serial = device
                .open()
                .ok()
                .and_then(|d| d.serial_number())
                .unwrap_or_default();
            if !serial.is_empty() && device_serial != *serial {
                continue;
            }
            target = Some(device);
            break;
        }

        let device = target.ok_or_else(|| {
            AdbError::Transport(format!("no USB ADB device with serial {serial:?} found"))
        })?;
        let (interface, in_endpoint, out_endpoint, device_serial) = claim_adb(&device)?;

        let (read_tx, read_rx) = mpsc::channel::<Vec<u8>>(64);
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>(64);
        let interface = Arc::new(interface);

        // Single worker thread: owns the interface, multiplexes channels.
        // Reads use a short timeout so writes never starve behind a blocked read.
        std::thread::spawn(move || {
            let mut write_rx = write_rx;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                // Drain all pending writes first.
                let mut pending: Vec<Vec<u8>> = Vec::new();
                while let Ok(data) = write_rx.try_recv() {
                    pending.push(data);
                }
                for data in pending {
                    if interface
                        .write_bulk(out_endpoint, &data, Duration::from_secs(10))
                        .is_err()
                    {
                        return;
                    }
                }
                match interface.read_bulk(in_endpoint, &mut buf, Duration::from_millis(200)) {
                    Ok(n) if n > 0 => {
                        if read_tx.blocking_send(buf[..n].to_vec()).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {
                        // Timeout with no data: wait for at least one write request
                        // so we never spin hot on an idle device.
                        let _ = write_rx.blocking_recv();
                    }
                    Err(e) => {
                        // On transport errors, back off unless both channels are closed.
                        if read_tx.is_closed() && write_rx.is_closed() {
                            return;
                        }
                        if matches!(
                            e.kind(),
                            nusb::ErrorKind::Timeout
                                | nusb::ErrorKind::TimeoutPreviousTransactionIncomplete
                        ) {
                            std::thread::sleep(Duration::from_millis(2));
                        } else {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                if read_tx.is_closed() && write_rx.is_closed() {
                    return;
                }
            }
        });

        Ok(Self {
            serial: device_serial,
            read_rx,
            write_tx,
        })
    }
}

#[async_trait]
impl Transport for UsbTransport {
    fn name(&self) -> String {
        format!("usb:{}", self.serial)
    }

    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.read_rx.recv().await {
                Some(chunk) => {
                    let take = (buf.len() - filled).min(chunk.len());
                    buf[filled..filled + take].copy_from_slice(&chunk[..take]);
                    filled += take;
                    if take < chunk.len() {
                        return Err(AdbError::Protocol(
                            "USB chunk larger than the buffer we asked to fill".into(),
                        ));
                    }
                }
                None => return Err(AdbError::StreamClosed),
            }
        }
        Ok(())
    }

    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.write_tx
            .send(buf.to_vec())
            .await
            .map_err(|_| AdbError::StreamClosed)
    }

    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Explain USB/permission problems on the current OS.
#[must_use]
pub fn explain_usb_problems() -> Vec<String> {
    let mut tips = Vec::new();
    tips.push(format!(
        "enumerate saw {} ADB device(s); expected your phone listed here",
        enumerate().len()
    ));

    #[cfg(target_os = "linux")]
    {
        tips.push("Linux: run `sudo watchdog device udev-rules --write` then reload udev; or check `lsusb` and your plugdev group".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        tips.push("Windows: bind WinUSB via Zadig (interface MI_01, class ff/42/01) or use the vendor's signed driver".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        tips.push("macOS: no driver needed for most devices; if the device stays unauthorized, tap Allow on the phone".to_string());
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        tips.push(
            "unknown OS: bind a USB driver compatible with class ff/42/01 interfaces".to_string(),
        );
    }
    tips
}
