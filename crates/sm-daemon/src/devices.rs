//! Device discovery (sysfs), opening and reading (FR-100, FR-201).

use sm_core::{InputState, RawFrame, AXES};
use sm_hid::descriptor;
use sm_hid::{EvdevAssembler, HidrawDecoder, InputEvent, MotionLayout, DEFAULT_EVDEV_WATCHDOG_US};
use sm_registry::{Canonical, DeviceModel, Match, Registry};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// hidraw devices resend reports every ~16 ms while deflected and send an
/// explicit neutral report; this watchdog only guards against a lost final
/// report (FR-208).
pub const HIDRAW_WATCHDOG_US: u64 = 500_000;

#[derive(Debug, Clone)]
pub struct Candidate {
    /// Stable key: sysfs HID device directory name (e.g. 0003:256F:C635.000C).
    pub key: String,
    pub bus: u16,
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub phys: String,
    pub uniq: String,
    pub hidraw: Option<PathBuf>,
    pub evdev: Option<PathBuf>,
    pub descriptor: Vec<u8>,
    pub model: Option<DeviceModel>,
    pub generic: bool,
}

fn parse_uevent(text: &str) -> std::collections::HashMap<String, String> {
    text.lines().filter_map(|l| l.split_once('=')).map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// Parse `HID_ID=0003:0000256F:0000C635`.
pub fn parse_hid_id(s: &str) -> Option<(u16, u16, u16)> {
    let mut it = s.split(':');
    let bus = u16::from_str_radix(it.next()?, 16).ok()?;
    let vid = u32::from_str_radix(it.next()?, 16).ok()? as u16;
    let pid = u32::from_str_radix(it.next()?, 16).ok()? as u16;
    Some((bus, vid, pid))
}

fn find_event_node(hid_dir: &Path, dev_root: &Path) -> Option<PathBuf> {
    for inp in std::fs::read_dir(hid_dir.join("input")).ok()?.flatten() {
        for e in std::fs::read_dir(inp.path()).ok()?.flatten() {
            let n = e.file_name();
            let n = n.to_string_lossy();
            if n.starts_with("event") {
                return Some(dev_root.join("input").join(n.as_ref()));
            }
        }
    }
    None
}

/// Enumerate 6-DOF candidates from sysfs. Never opens device nodes, so it
/// cannot disturb other HID consumers (FR-102). `dev_root` is `/dev`
/// except in tests.
pub fn scan(reg: &Registry, sysfs_root: &Path, dev_root: &Path) -> (Vec<Candidate>, Vec<String>) {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let Ok(rd) = std::fs::read_dir(sysfs_root.join("class/hidraw")) else {
        return (out, notes);
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let hid_dir = match std::fs::canonicalize(e.path().join("device")) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let Ok(ue) = std::fs::read_to_string(hid_dir.join("uevent")) else { continue };
        let ue = parse_uevent(&ue);
        let Some((bus, vid, pid)) = ue.get("HID_ID").and_then(|s| parse_hid_id(s)) else { continue };
        if bus != 0x03 && bus != 0x05 {
            continue; // USB and Bluetooth only
        }
        let (model, generic) = match reg.lookup(vid, pid) {
            Match::Known(m) => (Some(m.clone()), false),
            Match::GenericCandidate => (None, true),
            Match::Blocked(r) => {
                notes.push(format!("{vid:04x}:{pid:04x} ignored: {r}"));
                continue;
            }
            Match::NoMatch => continue,
        };
        let descriptor = std::fs::read(hid_dir.join("report_descriptor")).unwrap_or_default();
        // Interfaces of a known device (or unknown vendor devices) without
        // six axes are skipped: receivers expose mouse/keyboard interfaces.
        let has_axes = descriptor::parse(&descriptor).ok().and_then(|d| MotionLayout::from_descriptor(&d).ok()).is_some();
        if !has_axes {
            if model.is_some() {
                notes.push(format!("{vid:04x}:{pid:04x} interface {} has no 6-DOF axes; skipped", hid_dir.file_name().unwrap_or_default().to_string_lossy()));
            }
            continue;
        }
        out.push(Candidate {
            key: hid_dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            bus,
            vid,
            pid,
            name: ue.get("HID_NAME").cloned().unwrap_or_default(),
            phys: ue.get("HID_PHYS").cloned().unwrap_or_default(),
            uniq: ue.get("HID_UNIQ").cloned().unwrap_or_default(),
            hidraw: Some(dev_root.join(e.file_name())),
            evdev: find_event_node(&hid_dir, dev_root),
            descriptor,
            model,
            generic,
        });
    }
    (out, notes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPref {
    Auto,
    Hidraw,
    Evdev,
}

pub enum Reader {
    Hidraw(HidrawDecoder),
    Evdev(EvdevAssembler),
}

pub struct Device {
    pub id: u32,
    pub cand: Candidate,
    pub file: File,
    pub reader: Reader,
    pub input_path: &'static str,
    pub node: PathBuf,
    pub state: InputState,
    pub canonical: Canonical,
    pub axis_range: i32,
    pub frames: u64,
    pub last_frame_us: u64,
    rate_window_start: u64,
    rate_count: u32,
    pub rate_hz: f32,
}

#[derive(Debug)]
pub enum OpenError {
    Permission(String),
    Other(String),
}

fn open_ro(p: &Path) -> std::io::Result<File> {
    std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(p)
}

impl Device {
    pub fn open(id: u32, cand: Candidate, pref: InputPref) -> Result<Device, OpenError> {
        let layout = descriptor::parse(&cand.descriptor)
            .map_err(|e| OpenError::Other(e.to_string()))
            .and_then(|d| MotionLayout::from_descriptor(&d).map_err(OpenError::Other))?;
        let buttons = cand.model.as_ref().map(|m| m.buttons.clone()).unwrap_or_default();
        // hidraw button decoding needs a verified hid_bit map; models
        // without one use evdev, where the kernel already maps the codes.
        let hid_buttons_known = buttons.is_empty() || buttons.iter().all(|b| b.hid_bit.is_some());
        let canonical = match &cand.model {
            Some(m) => m.canonical,
            // spacenavd applies the same transform to every 256f device
            None if cand.vid == sm_registry::VID_3DCONNEXION => Canonical::SwapYzInvYz,
            None => Canonical::Identity,
        };
        let axis_range = cand.model.as_ref().and_then(|m| m.axis_range).unwrap_or(layout.axis_range);

        let try_hidraw = matches!(pref, InputPref::Hidraw) || (pref == InputPref::Auto && hid_buttons_known);
        let mut errs = Vec::new();
        if try_hidraw {
            if let Some(p) = &cand.hidraw {
                match open_ro(p) {
                    Ok(f) => {
                        let map = buttons.iter().filter_map(|b| b.hid_bit.map(|h| (h, b.index))).collect();
                        return Ok(Device::new(
                            id,
                            cand.clone(),
                            f,
                            Reader::Hidraw(HidrawDecoder::new(layout, map)),
                            "hidraw",
                            p.clone(),
                            canonical,
                            axis_range,
                        ));
                    }
                    Err(e) => errs.push((e.kind(), format!("{}: {e}", p.display()))),
                }
            }
        }
        if pref != InputPref::Hidraw {
            if let Some(p) = &cand.evdev {
                match open_ro(p) {
                    Ok(f) => {
                        let codes = buttons.iter().map(|b| (b.code, b.index)).collect();
                        let asm = EvdevAssembler::new(layout.axis_groups(), codes, DEFAULT_EVDEV_WATCHDOG_US);
                        return Ok(Device::new(id, cand.clone(), f, Reader::Evdev(asm), "evdev", p.clone(), canonical, axis_range));
                    }
                    Err(e) => errs.push((e.kind(), format!("{}: {e}", p.display()))),
                }
            }
        }
        let msg = errs.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>().join("; ");
        if errs.iter().any(|(k, _)| *k == std::io::ErrorKind::PermissionDenied) {
            Err(OpenError::Permission(msg))
        } else {
            Err(OpenError::Other(if msg.is_empty() { "no usable device node".into() } else { msg }))
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new(id: u32, cand: Candidate, file: File, reader: Reader, input_path: &'static str, node: PathBuf, canonical: Canonical, axis_range: i32) -> Device {
        Device {
            id,
            cand,
            file,
            reader,
            input_path,
            node,
            state: InputState::default(),
            canonical,
            axis_range,
            frames: 0,
            last_frame_us: 0,
            rate_window_start: 0,
            rate_count: 0,
            rate_hz: 0.0,
        }
    }

    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    fn note_frame(&mut self, t: u64) {
        self.frames += 1;
        self.last_frame_us = t;
        self.rate_count += 1;
        if t.saturating_sub(self.rate_window_start) >= 1_000_000 {
            let dt = (t - self.rate_window_start).max(1) as f32 / 1e6;
            self.rate_hz = if self.rate_window_start == 0 { 0.0 } else { self.rate_count as f32 / dt };
            self.rate_window_start = t;
            self.rate_count = 0;
        }
    }

    /// Read everything available. Returns frames, or Err(()) if the device
    /// is gone (ENODEV/EIO/EOF).
    #[allow(clippy::result_unit_err)]
    pub fn read(&mut self) -> Result<Vec<RawFrame>, ()> {
        let mut frames = Vec::new();
        loop {
            let mut buf = [0u8; 512];
            match self.file.read(&mut buf) {
                Ok(0) => return Err(()),
                Ok(n) => {
                    let now = sm_core::monotonic_us();
                    match &mut self.reader {
                        Reader::Hidraw(d) => {
                            if let Ok(Some(f)) = d.decode(&buf[..n], now) {
                                frames.push(f);
                            }
                        }
                        Reader::Evdev(a) => {
                            let sz = std::mem::size_of::<libc::input_event>();
                            for chunk in buf[..n].chunks_exact(sz) {
                                // SAFETY: chunk is exactly one input_event.
                                let ev: libc::input_event = unsafe { std::ptr::read_unaligned(chunk.as_ptr() as *const libc::input_event) };
                                if let Some(f) = a.push(InputEvent { t_us: now, typ: ev.type_, code: ev.code, value: ev.value }) {
                                    frames.push(f);
                                }
                            }
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(()),
            }
        }
        for f in &frames {
            self.note_frame(f.t_us);
        }
        Ok(frames)
    }

    pub fn next_deadline_us(&self, now: u64) -> Option<u64> {
        match &self.reader {
            Reader::Evdev(a) => a.next_deadline_us(now),
            Reader::Hidraw(_) => {
                if self.state.is_neutral() {
                    None
                } else {
                    Some((self.state.last_motion_us + HIDRAW_WATCHDOG_US).saturating_sub(now))
                }
            }
        }
    }

    pub fn tick(&mut self, now: u64) -> Option<RawFrame> {
        match &mut self.reader {
            Reader::Evdev(a) => a.tick(now),
            Reader::Hidraw(_) => {
                if !self.state.is_neutral() && now.saturating_sub(self.state.last_motion_us) >= HIDRAW_WATCHDOG_US {
                    Some(RawFrame { t_us: now, axes_present: 0b111111, axes: [0; AXES], ..Default::default() })
                } else {
                    None
                }
            }
        }
    }

    pub fn malformed(&self) -> u64 {
        match &self.reader {
            Reader::Hidraw(d) => d.malformed,
            Reader::Evdev(a) => a.dropped_syncs,
        }
    }

    /// Report rate over the last second; 0 when the device has gone quiet.
    pub fn current_rate_hz(&self, now: u64) -> f32 {
        if now.saturating_sub(self.last_frame_us) > 1_000_000 {
            0.0
        } else {
            self.rate_hz
        }
    }

    pub fn watchdog_resets(&self) -> u64 {
        match &self.reader {
            Reader::Evdev(a) => a.watchdog_resets,
            Reader::Hidraw(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hid_id() {
        assert_eq!(parse_hid_id("0003:0000256F:0000C635"), Some((3, 0x256f, 0xc635)));
        assert_eq!(parse_hid_id("garbage"), None);
    }

    /// Build a fake sysfs tree and make sure scanning matches only the
    /// SpaceMouse and never the unrelated HID devices.
    #[test]
    fn scan_fake_sysfs() {
        let root = std::env::temp_dir().join(format!("sm-sysfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mk = |n: &str, id: &str, name: &str, desc: &[u8]| {
            let dev = root.join("devices").join(n);
            std::fs::create_dir_all(dev.join("input/input1/event7")).unwrap();
            std::fs::write(dev.join("uevent"), format!("HID_ID={id}\nHID_NAME={name}\nHID_PHYS=usb-x/input0\nHID_UNIQ=\n")).unwrap();
            std::fs::write(dev.join("report_descriptor"), desc).unwrap();
            let link = root.join("class/hidraw").join(format!("hidraw{}", n.len()));
            std::fs::create_dir_all(&link).unwrap();
            std::os::unix::fs::symlink(&dev, link.join("device")).unwrap();
        };
        let compact = descriptor::from_hex(include_str!("../../../tests/fixtures/descriptors/256f-c635-usb.hex")).unwrap();
        mk("0003:256F:C635.0001", "0003:0000256F:0000C635", "3Dconnexion SpaceMouse Compact", &compact);
        mk("0003:256F:C650.02", "0003:0000256F:0000C650", "CadMouse", &compact);
        mk("0003:0408:546B.003", "0003:00000408:0000546B", "Camera", &[0x05, 0x01]);
        let (c, notes) = scan(Registry::embedded(), &root, Path::new("/dev"));
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].pid, 0xc635);
        assert!(!c[0].generic);
        assert_eq!(c[0].evdev.as_deref(), Some(Path::new("/dev/input/event7")));
        assert!(notes.iter().any(|n| n.contains("c650")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
