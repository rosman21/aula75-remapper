//! The real keyboard over USB HID (hidapi: IOKit on macOS, hidraw on Linux).
//!
//! The receiver's config collection is usage page `0xFF02` on interface 1, which also
//! carries the keyboard's NKRO and media reports. On macOS it is opened **shared**, never
//! exclusively, so typing keeps working while the app is connected.

use hidapi::{DeviceInfo, HidApi, HidDevice};

use crate::protocol::{self as p, Link};
use crate::session::Wire;

/// What is plugged in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// The 2.4 GHz receiver's config interface.
    pub receiver: bool,
    /// The keyboard on its cable.
    pub wired: bool,
    /// The keyboard over Bluetooth (seen, never configured).
    pub bluetooth: bool,
}

fn is_receiver_config(d: &DeviceInfo) -> bool {
    d.vendor_id() == p::RECEIVER_VID
        && d.product_id() == p::RECEIVER_PID
        && (d.usage_page() == p::VENDOR_USAGE_PAGE
            || (d.usage_page() == 0 && d.interface_number() == 1))
}

fn is_wired_config(d: &DeviceInfo) -> bool {
    d.vendor_id() == p::WIRED_VID
        && d.product_id() == p::WIRED_PID
        && (d.usage_page() >= 0xff00 || (d.usage_page() == 0 && d.interface_number() == 1))
}

/// macOS's Input Monitoring permission, which opening the receiver needs (its interface
/// also carries keystrokes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Allowed (or not needed on this OS).
    Granted,
    /// Refused in System Settings.
    Denied,
    /// Never asked.
    Unknown,
}

#[cfg(target_os = "macos")]
mod tcc {
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        pub fn IOHIDCheckAccess(request: u32) -> u32;
        pub fn IOHIDRequestAccess(request: u32) -> bool;
    }
    /// `kIOHIDRequestTypeListenEvent`.
    pub const LISTEN_EVENT: u32 = 1;
}

/// Whether this process may open the keyboard.
pub fn access() -> Access {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain IOKit query with a constant argument.
        match unsafe { tcc::IOHIDCheckAccess(tcc::LISTEN_EVENT) } {
            0 => Access::Granted,
            1 => Access::Denied,
            _ => Access::Unknown,
        }
    }
    #[cfg(not(target_os = "macos"))]
    Access::Granted
}

/// Ask macOS for Input Monitoring (shows its prompt the first time); true when granted.
pub fn request_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: a plain IOKit request with a constant argument.
        unsafe { tcc::IOHIDRequestAccess(tcc::LISTEN_EVENT) }
    }
    #[cfg(not(target_os = "macos"))]
    true
}

/// The process's one hidapi context.
pub fn api() -> Result<HidApi, String> {
    let api = HidApi::new().map_err(|e| format!("USB HID is unavailable: {e}"))?;
    #[cfg(target_os = "macos")]
    api.set_open_exclusive(false);
    Ok(api)
}

/// Look for the receiver, the cable and Bluetooth.
pub fn scan(api: &mut HidApi) -> Result<Scan, String> {
    api.refresh_devices().map_err(|e| e.to_string())?;
    let mut s = Scan::default();
    for d in api.device_list() {
        s.receiver |= is_receiver_config(d);
        s.wired |= is_wired_config(d);
        s.bluetooth |= d.vendor_id() == p::RECEIVER_VID && p::BLUETOOTH_PIDS.contains(&d.product_id());
    }
    Ok(s)
}

/// Open the config interface: the cable when plugged in (faster), else the receiver.
pub fn open(api: &HidApi) -> Result<Option<HidWire>, String> {
    let pick = |f: fn(&DeviceInfo) -> bool| api.device_list().find(|d| f(d)).cloned();
    let (info, link) = match (pick(is_wired_config), pick(is_receiver_config)) {
        (Some(d), _) => (d, Link::Wired),
        (None, Some(d)) => (d, Link::Receiver),
        (None, None) => return Ok(None),
    };
    let device = info.open_device(api).map_err(|e| denied(&e.to_string()))?;
    Ok(Some(HidWire {
        device,
        link,
        product: info.product_string().unwrap_or("").to_string(),
    }))
}

/// What to do when the OS refuses to open the device.
fn denied(e: &str) -> String {
    if cfg!(target_os = "linux") {
        format!(
            "Linux didn't let the app open the keyboard ({e}). Install the udev rule (udev/70-remap75.rules in this \
             project), then unplug the receiver and plug it back in."
        )
    } else if cfg!(target_os = "macos") {
        format!(
            "macOS didn't let the app open the keyboard ({e}). Allow Remap75 in System Settings → Privacy & \
             Security → Input Monitoring, then reconnect."
        )
    } else {
        format!("The keyboard couldn't be opened: {e}")
    }
}

/// An open config interface.
pub struct HidWire {
    device: HidDevice,
    link: Link,
    /// The USB product string.
    pub product: String,
}

impl Wire for HidWire {
    fn link(&self) -> Link {
        self.link
    }

    fn listens(&self) -> bool {
        self.link == Link::Receiver
    }

    fn send_report(&mut self, packet: &[u8]) -> Result<(), String> {
        self.device
            .write(packet)
            .map(|_| ())
            .map_err(|e| format!("USB write failed: {e}"))
    }

    fn recv_report(&mut self, timeout_ms: i32) -> Result<Option<Vec<u8>>, String> {
        let mut buf = [0u8; 64];
        match self.device.read_timeout(&mut buf, timeout_ms) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(buf[..n].to_vec())),
            Err(e) => Err(format!("USB read failed: {e}")),
        }
    }

    fn send_feature(&mut self, frame: &[u8]) -> Result<(), String> {
        self.device
            .send_feature_report(frame)
            .map_err(|e| e.to_string())
    }

    fn get_feature(&mut self) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; p::WIRED_LEN];
        buf[0] = p::WIRED_REPORT;
        let n = self
            .device
            .get_feature_report(&mut buf)
            .map_err(|e| e.to_string())?;
        buf.truncate(n);
        Ok(buf)
    }
}
