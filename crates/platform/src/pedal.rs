//! Foot pedal readers: USB HID devices and serial (COM) ports.
//!
//! A reader runs on its own thread and hands every report to a callback; what the report
//! means is decided by `scribecraft_engine::pedal`. Readers reconnect by themselves when a
//! pedal is unplugged and plugged back in.
//!
//! Privacy: keyboards and mice are never listed or opened, so a pedal reader cannot be
//! pointed at a keyboard (AGENTS.md §5).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use scribecraft_engine::pedal::{PedalSource, SERIAL_CTS, SERIAL_DCD, SERIAL_DSR, SERIAL_RI};

use crate::PlatformError;

/// Vendors of dictation foot pedals and controllers (public USB ID registry). Used only to
/// list likely pedals first; any HID device can be learned.
const PEDAL_VENDORS: &[(u16, &str)] = &[(0x05f3, "P.I. Engineering (Infinity, VEC)"), (0x07b4, "Olympus"), (0x0911, "Philips Speech Processing")];

/// How long to wait before retrying a device that failed to open or was unplugged.
const RETRY: Duration = Duration::from_millis(1000);
/// Serial modem-line polling interval.
const SERIAL_POLL: Duration = Duration::from_millis(10);
/// HID read timeout (also how quickly a stop request is noticed).
const HID_TIMEOUT_MS: i32 = 100;

/// A device a pedal may be connected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PedalDevice {
    pub source: PedalSource,
    /// What to show the user.
    pub label: String,
    /// From a known pedal vendor.
    pub likely_pedal: bool,
}

/// Lists HID devices (except keyboards and mice) and serial ports, likely pedals first.
/// Errors from one kind of device do not hide the other.
pub fn list_devices() -> Vec<PedalDevice> {
    let mut out = Vec::new();
    if let Ok(api) = hidapi::HidApi::new() {
        for d in api.device_list() {
            if is_keyboard_or_mouse(d.usage_page(), d.usage()) {
                continue;
            }
            let (vid, pid) = (d.vendor_id(), d.product_id());
            let maker = d.manufacturer_string().unwrap_or("").trim();
            let product = d.product_string().unwrap_or("").trim();
            let vendor = PEDAL_VENDORS.iter().find(|(v, _)| *v == vid);
            let name = match (maker.is_empty(), product.is_empty()) {
                (_, false) if !maker.is_empty() => format!("{maker} {product}"),
                (_, false) => product.to_owned(),
                (false, true) => maker.to_owned(),
                (true, true) => vendor.map(|(_, n)| (*n).to_owned()).unwrap_or_else(|| "USB device".into()),
            };
            let path = d.path().to_string_lossy().into_owned();
            if out.iter().any(|p: &PedalDevice| matches!(&p.source, PedalSource::Hid { path: Some(q), .. } if *q == path)) {
                continue;
            }
            out.push(PedalDevice {
                label: format!("USB: {name} ({vid:04x}:{pid:04x})"),
                likely_pedal: vendor.is_some(),
                source: PedalSource::Hid { vendor_id: vid, product_id: pid, path: Some(path), name },
            });
        }
    }
    if let Ok(ports) = serialport::available_ports() {
        for p in ports {
            out.push(PedalDevice {
                label: format!("Serial: {}", p.port_name),
                likely_pedal: false,
                source: PedalSource::Serial { port: p.port_name },
            });
        }
    }
    out.sort_by_key(|d| !d.likely_pedal);
    out
}

/// HID usage page 1 (Generic Desktop): 2 = mouse, 6 = keyboard, 7 = keypad.
fn is_keyboard_or_mouse(usage_page: u16, usage: u16) -> bool {
    usage_page == 0x01 && matches!(usage, 0x02 | 0x06 | 0x07)
}

/// What a reader is doing, for the settings window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderStatus {
    Connecting,
    Connected,
    /// Not reachable right now; the reader keeps retrying.
    Waiting(String),
    Stopped,
}

/// A running pedal reader. Dropping it stops the thread.
#[derive(Debug)]
pub struct PedalReader {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<ReaderStatus>>,
    handle: Option<JoinHandle<()>>,
}

impl PedalReader {
    /// Starts reading `source`; `on_report` runs on the reader thread for every report.
    pub fn start(source: &PedalSource, on_report: impl FnMut(&[u8]) + Send + 'static) -> Result<Self, PlatformError> {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(ReaderStatus::Connecting));
        let (s, st) = (stop.clone(), status.clone());
        let source = source.clone();
        if matches!(source, PedalSource::None) {
            return Err(PlatformError::Device("no pedal device is selected".into()));
        }
        let handle = std::thread::Builder::new()
            .name("scribecraft-pedal".into())
            .spawn(move || {
                let mut on_report = on_report;
                let body = AssertUnwindSafe(|| match &source {
                    PedalSource::Hid { vendor_id, product_id, path, .. } => {
                        run_hid(*vendor_id, *product_id, path.as_deref(), &s, &st, &mut on_report)
                    }
                    PedalSource::Serial { port } => run_serial(port, &s, &st, &mut on_report),
                    PedalSource::None => {}
                });
                if catch_unwind(body).is_err() {
                    set(&st, ReaderStatus::Waiting("the pedal reader stopped after an internal error; reopen the pedal settings".into()));
                    return;
                }
                set(&st, ReaderStatus::Stopped);
            })
            .map_err(|e| PlatformError::Device(format!("cannot start the pedal reader: {e}")))?;
        Ok(Self { stop, status, handle: Some(handle) })
    }

    pub fn status(&self) -> ReaderStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }
}

impl Drop for PedalReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            // The thread notices the flag within one read timeout or poll interval.
            let _ = h.join();
        }
    }
}

fn set(status: &Mutex<ReaderStatus>, value: ReaderStatus) {
    match status.lock() {
        Ok(mut s) => *s = value,
        Err(p) => *p.into_inner() = value,
    }
}

/// Sleeps up to `d`, waking early when asked to stop. Returns false when stopping.
fn pause(stop: &AtomicBool, d: Duration) -> bool {
    let step = Duration::from_millis(50);
    let mut left = d;
    while !left.is_zero() {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let s = left.min(step);
        std::thread::sleep(s);
        left = left.saturating_sub(s);
    }
    !stop.load(Ordering::Relaxed)
}

fn run_hid(vid: u16, pid: u16, path: Option<&str>, stop: &AtomicBool, status: &Mutex<ReaderStatus>, on_report: &mut dyn FnMut(&[u8])) {
    let mut buf = [0u8; 64];
    while !stop.load(Ordering::Relaxed) {
        let opened = hidapi::HidApi::new().and_then(|api| match path.and_then(|p| std::ffi::CString::new(p).ok()) {
            Some(p) => api.open_path(&p).or_else(|_| api.open(vid, pid)),
            None => api.open(vid, pid),
        });
        let device = match opened {
            Ok(d) => d,
            Err(e) => {
                set(status, ReaderStatus::Waiting(format!("USB pedal {vid:04x}:{pid:04x} not available ({e}); is it plugged in?")));
                if !pause(stop, RETRY) {
                    return;
                }
                continue;
            }
        };
        set(status, ReaderStatus::Connected);
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            match device.read_timeout(&mut buf, HID_TIMEOUT_MS) {
                Ok(0) => {}
                Ok(n) => on_report(buf.get(..n).unwrap_or(&[])),
                Err(e) => {
                    set(status, ReaderStatus::Waiting(format!("USB pedal disconnected ({e}); waiting for it")));
                    break;
                }
            }
        }
        if !pause(stop, RETRY) {
            return;
        }
    }
}

fn run_serial(port: &str, stop: &AtomicBool, status: &Mutex<ReaderStatus>, on_report: &mut dyn FnMut(&[u8])) {
    while !stop.load(Ordering::Relaxed) {
        let mut p = match serialport::new(port, 9600).timeout(Duration::from_millis(50)).open() {
            Ok(p) => p,
            Err(e) => {
                set(status, ReaderStatus::Waiting(format!("cannot open {port} ({e}); check the port number and that no other program uses it")));
                if !pause(stop, RETRY) {
                    return;
                }
                continue;
            }
        };
        // Pin 4 (DTR) is the pedals' common line; RTS is raised too for pedals powered from it.
        let powered = p.write_data_terminal_ready(true).and_then(|()| p.write_request_to_send(true));
        if let Err(e) = powered {
            set(status, ReaderStatus::Waiting(format!("{port} does not accept modem control ({e})")));
            if !pause(stop, RETRY) {
                return;
            }
            continue;
        }
        set(status, ReaderStatus::Connected);
        let mut last: Option<u8> = None;
        loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            let lines = (|| -> serialport::Result<u8> {
                let mut b = 0u8;
                if p.read_carrier_detect()? {
                    b |= SERIAL_DCD;
                }
                if p.read_data_set_ready()? {
                    b |= SERIAL_DSR;
                }
                if p.read_clear_to_send()? {
                    b |= SERIAL_CTS;
                }
                if p.read_ring_indicator()? {
                    b |= SERIAL_RI;
                }
                Ok(b)
            })();
            match lines {
                Ok(b) => {
                    if last != Some(b) {
                        on_report(&[b]);
                        last = Some(b);
                    }
                }
                Err(e) => {
                    set(status, ReaderStatus::Waiting(format!("{port} stopped responding ({e}); waiting for it")));
                    break;
                }
            }
            std::thread::sleep(SERIAL_POLL);
        }
        if !pause(stop, RETRY) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboards_and_mice_are_never_pedals() {
        assert!(is_keyboard_or_mouse(0x01, 0x06));
        assert!(is_keyboard_or_mouse(0x01, 0x02));
        assert!(!is_keyboard_or_mouse(0x0c, 0x01));
        assert!(!is_keyboard_or_mouse(0xff00, 0x06));
    }

    #[test]
    fn a_missing_device_reports_waiting_and_stops_promptly() {
        let r = PedalReader::start(&PedalSource::Serial { port: "SCRIBECRAFT-NO-SUCH-PORT".into() }, |_| {}).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(matches!(r.status(), ReaderStatus::Waiting(_)), "{:?}", r.status());
        let t = std::time::Instant::now();
        drop(r);
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(PedalReader::start(&PedalSource::None, |_| {}).is_err());
    }

    #[test]
    fn listing_devices_never_fails() {
        // Whatever is attached, listing returns (possibly empty) and never includes keyboards.
        let _ = list_devices();
    }
}
