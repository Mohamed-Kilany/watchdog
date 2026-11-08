//! USB transport — enumerate ADB interfaces via nusb (no libusb).
//!
//! ADB devices expose an interface with class 0xFF, subclass 0x42, protocol
//! 0x01. nusb handles discovery on Windows (via SetupAPI), macOS (IOKit) and
//! Linux (sysfs/usbfs). `watchdog device usb-doctor` explains permission
//! problems; on Linux, `watchdog device udev-rules` installs them.

use async_trait::async_trait;
use nusb::descriptors::TransferType;
use nusb::io::{EndpointRead, EndpointWrite};
use nusb::transfer::{Bulk, In, Out};
use nusb::{DeviceInfo, MaybeFuture};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{AdbError, Result};
use crate::transport::{Transport, TransportReader, TransportWriter};

/// USB class/subclass/protocol for the ADB interface.
pub const ADB_USB_CLASS: u8 = 0xFF;
/// ADB interface subclass (`0x42`).
pub const ADB_USB_SUBCLASS: u8 = 0x42;
/// ADB interface protocol (`0x01`).
pub const ADB_USB_PROTOCOL: u8 = 0x01;

/// Transfer buffer size for bulk endpoints (matches ADB's max payload).
const USB_TRANSFER_SIZE: usize = 64 * 1024;

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

fn is_adb_interface(class: u8, subclass: u8, protocol: u8) -> bool {
    class == ADB_USB_CLASS && subclass == ADB_USB_SUBCLASS && protocol == ADB_USB_PROTOCOL
}

fn has_adb_interface(device: &DeviceInfo) -> bool {
    // Most platforms expose interface summaries before opening the device.
    if device
        .interfaces()
        .any(|i| is_adb_interface(i.class(), i.subclass(), i.protocol()))
    {
        return true;
    }
    // Windows composite devices may hide interface info until opened.
    match device.open().wait() {
        Ok(handle) => handle
            .active_configuration()
            .map(|config| {
                config
                    .interface_alt_settings()
                    .any(|i| is_adb_interface(i.class(), i.subclass(), i.protocol()))
            })
            .unwrap_or(false),
        Err(_) => false,
    }
}

/// Enumerate ADB-capable devices (without claiming interfaces).
#[must_use]
pub fn enumerate() -> Vec<UsbDeviceInfo> {
    let Ok(devices) = nusb::list_devices().wait() else {
        return Vec::new();
    };
    devices
        .filter(has_adb_interface)
        .map(|device| UsbDeviceInfo {
            serial: device.serial_number().unwrap_or_default().to_string(),
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            product: device.product_string().unwrap_or_default().to_string(),
        })
        .collect()
}

/// A USB-backed [`Transport`].
pub struct UsbTransport {
    serial: String,
    reader: EndpointRead<Bulk>,
    writer: EndpointWrite<Bulk>,
}

impl std::fmt::Debug for UsbTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UsbTransport")
            .field("serial", &self.serial)
            .finish()
    }
}

fn claim_adb(info: &DeviceInfo) -> Result<UsbTransport> {
    let handle = info
        .open()
        .wait()
        .map_err(|e| AdbError::Transport(format!("open USB device: {e}")))?;
    let config = handle
        .active_configuration()
        .map_err(|e| AdbError::Transport(format!("USB active configuration: {e}")))?;
    let interface_desc = config
        .interface_alt_settings()
        .find(|i| is_adb_interface(i.class(), i.subclass(), i.protocol()))
        .ok_or_else(|| AdbError::Transport("no ADB interface on this device".into()))?;

    let interface_number = interface_desc.interface_number();
    let bulk = |want_in: bool| {
        interface_desc
            .endpoints()
            .find(|e| {
                e.transfer_type() == TransferType::Bulk && (e.address() & 0x80 != 0) == want_in
            })
            .map(|e| e.address())
            .ok_or_else(|| {
                AdbError::Transport(if want_in {
                    "no bulk-in endpoint".into()
                } else {
                    "no bulk-out endpoint".into()
                })
            })
    };
    let in_endpoint = bulk(true)?;
    let out_endpoint = bulk(false)?;

    let interface = handle
        .claim_interface(interface_number)
        .wait()
        .map_err(|e| AdbError::Transport(format!("claim interface {interface_number}: {e}")))?;

    let reader = interface
        .endpoint::<Bulk, In>(in_endpoint)
        .map_err(|e| {
            AdbError::Transport(format!("open bulk-in endpoint 0x{in_endpoint:02x}: {e}"))
        })?
        .reader(USB_TRANSFER_SIZE)
        .with_num_transfers(4);
    let writer = interface
        .endpoint::<Bulk, Out>(out_endpoint)
        .map_err(|e| {
            AdbError::Transport(format!("open bulk-out endpoint 0x{out_endpoint:02x}: {e}"))
        })?
        .writer(USB_TRANSFER_SIZE)
        .with_num_transfers(4);

    Ok(UsbTransport {
        serial: info.serial_number().unwrap_or_default().to_string(),
        reader,
        writer,
    })
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
        let devices = nusb::list_devices()
            .wait()
            .map_err(|e| AdbError::Transport(format!("nusb could not list devices: {e}")))?;

        for device in devices {
            if !has_adb_interface(&device) {
                continue;
            }
            let device_serial = device.serial_number().unwrap_or_default();
            if !serial.is_empty() && device_serial != serial {
                continue;
            }
            return claim_adb(&device);
        }

        Err(AdbError::Transport(format!(
            "no USB ADB device with serial {serial:?} found"
        )))
    }
}

/// Read half of a [`UsbTransport`].
pub struct UsbReader {
    reader: EndpointRead<Bulk>,
}

/// Write half of a [`UsbTransport`].
pub struct UsbWriter {
    writer: EndpointWrite<Bulk>,
}

#[async_trait]
impl TransportReader for UsbTransport {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.reader
            .read_exact(buf)
            .await
            .map(|_| ())
            .map_err(|e| AdbError::Transport(format!("usb read: {e}")))
    }
}

#[async_trait]
impl TransportWriter for UsbTransport {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.writer
            .write_all(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("usb write: {e}")))?;
        self.writer
            .flush()
            .await
            .map_err(|e| AdbError::Transport(format!("usb flush: {e}")))
    }
}

#[async_trait]
impl TransportReader for UsbReader {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        self.reader
            .read_exact(buf)
            .await
            .map(|_| ())
            .map_err(|e| AdbError::Transport(format!("usb read: {e}")))
    }
}

#[async_trait]
impl TransportWriter for UsbWriter {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        self.writer
            .write_all(buf)
            .await
            .map_err(|e| AdbError::Transport(format!("usb write: {e}")))?;
        self.writer
            .flush()
            .await
            .map_err(|e| AdbError::Transport(format!("usb flush: {e}")))
    }
}

#[async_trait]
impl Transport for UsbTransport {
    fn name(&self) -> String {
        format!("usb:{}", self.serial)
    }

    type Reader = UsbReader;
    type Writer = UsbWriter;

    fn split(self) -> (Self::Reader, Self::Writer) {
        (
            UsbReader {
                reader: self.reader,
            },
            UsbWriter {
                writer: self.writer,
            },
        )
    }

    async fn close(&mut self) -> Result<()> {
        let _ = self.writer.flush().await;
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
