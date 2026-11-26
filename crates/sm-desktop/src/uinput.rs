//! Virtual input device via /dev/uinput (FR-503 "explicitly permitted
//! uinput backend"). Works identically on X11 and every Wayland compositor
//! because events enter the kernel input stack like a real keyboard/mouse.
//!
//! Safety properties:
//! * Only codes from the static allowlist (`sm_actions::keys`) plus five
//!   mouse buttons and wheel/pointer axes are registered; the kernel rejects
//!   anything else on this device.
//! * Pressed keys are tracked; [`Uinput::release_all`] emits releases.
//! * When the process exits or crashes the kernel destroys the device and
//!   the input core releases every key it still reports as down, so a
//!   crash cannot leave a modifier stuck (FR-306).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
const UI_SET_RELBIT: libc::c_ulong = 0x4004_5566;
const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
const UI_DEV_CREATE: libc::c_ulong = 0x5501;
const UI_DEV_DESTROY: libc::c_ulong = 0x5502;

pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;
pub const EV_REL: u16 = 2;
pub const REL_X: u16 = 0;
pub const REL_Y: u16 = 1;
pub const REL_HWHEEL: u16 = 6;
pub const REL_WHEEL: u16 = 8;
pub const REL_WHEEL_HI_RES: u16 = 11;
pub const REL_HWHEEL_HI_RES: u16 = 12;
pub const MOUSE_CODES: [u16; 5] = [0x110, 0x111, 0x112, 0x113, 0x114];
pub const REL_CODES: [u16; 6] = [REL_X, REL_Y, REL_HWHEEL, REL_WHEEL, REL_WHEEL_HI_RES, REL_HWHEEL_HI_RES];

pub const DEVICE_NAME: &str = "3DMouse Linux virtual input";

#[repr(C)]
struct UinputSetup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; 80],
    ff_effects_max: u32,
}

pub fn allowed_key(code: u16) -> bool {
    MOUSE_CODES.contains(&code) || sm_actions::keys::all_codes().any(|c| c == code)
}

pub struct Uinput {
    f: File,
    pressed: Vec<u16>,
    pub events_sent: u64,
}

impl Uinput {
    pub fn open(path: &str) -> std::io::Result<Uinput> {
        let f = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)?;
        let fd = f.as_raw_fd();
        let io = |req, arg: libc::c_int| -> std::io::Result<()> {
            // SAFETY: ioctls with integer arguments on a uinput fd.
            if unsafe { libc::ioctl(fd, req, arg) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        };
        io(UI_SET_EVBIT, EV_KEY as i32)?;
        io(UI_SET_EVBIT, EV_REL as i32)?;
        io(UI_SET_EVBIT, EV_SYN as i32)?;
        for c in sm_actions::keys::all_codes().chain(MOUSE_CODES) {
            io(UI_SET_KEYBIT, c as i32)?;
        }
        for r in REL_CODES {
            io(UI_SET_RELBIT, r as i32)?;
        }
        let mut setup = UinputSetup { bustype: 0x06 /* BUS_VIRTUAL */, vendor: 0x1d6b, product: 0x5350, version: 1, name: [0; 80], ff_effects_max: 0 };
        setup.name[..DEVICE_NAME.len()].copy_from_slice(DEVICE_NAME.as_bytes());
        // SAFETY: struct layout matches linux/uinput.h `struct uinput_setup`.
        if unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        io(UI_DEV_CREATE, 0)?;
        Ok(Uinput { f, pressed: Vec::new(), events_sent: 0 })
    }

    fn emit(&mut self, typ: u16, code: u16, value: i32) -> std::io::Result<()> {
        let ev = libc::input_event { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, type_: typ, code, value };
        // SAFETY: input_event is plain old data.
        let bytes = unsafe { std::slice::from_raw_parts(&ev as *const _ as *const u8, std::mem::size_of::<libc::input_event>()) };
        self.f.write_all(bytes)?;
        self.events_sent += 1;
        Ok(())
    }

    fn syn(&mut self) -> std::io::Result<()> {
        self.emit(EV_SYN, 0, 0)
    }

    pub fn key(&mut self, code: u16, down: bool) -> std::io::Result<()> {
        if !allowed_key(code) {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("key code {code} not allowlisted")));
        }
        if down == self.pressed.contains(&code) {
            return Ok(()); // no duplicate press/release
        }
        if down {
            // never form Alt+SysRq across separate inject calls either
            let mut would = self.pressed.clone();
            would.push(code);
            if sm_actions::keys::is_sysrq_combo(&would) {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "alt+sysrq refused (kernel magic SysRq)"));
            }
        }
        self.emit(EV_KEY, code, down as i32)?;
        self.syn()?;
        if down {
            self.pressed.push(code);
        } else {
            self.pressed.retain(|c| *c != code);
        }
        Ok(())
    }

    /// Press a chord in order, or release it in reverse order. A press is all or nothing: a
    /// chord with a refused key presses none of it, and a write error releases what it pressed.
    pub fn chord(&mut self, codes: &[u16], down: bool) -> std::io::Result<()> {
        if down {
            if let Some(c) = codes.iter().find(|c| !allowed_key(**c)) {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("key code {c} not allowlisted")));
            }
            let mut would = self.pressed.clone();
            would.extend_from_slice(codes);
            if sm_actions::keys::is_sysrq_combo(&would) {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "alt+sysrq refused (kernel magic SysRq)"));
            }
            let mut pressed_here = Vec::new();
            for c in codes {
                let was_down = self.pressed.contains(c);
                if let Err(e) = self.key(*c, true) {
                    for k in pressed_here.iter().rev() {
                        let _ = self.key(*k, false);
                    }
                    return Err(e);
                }
                if !was_down {
                    pressed_here.push(*c);
                }
            }
        } else {
            for c in codes.iter().rev() {
                self.key(*c, false)?;
            }
        }
        Ok(())
    }

    pub fn rel(&mut self, code: u16, value: i32) -> std::io::Result<()> {
        if !REL_CODES.contains(&code) || value.abs() > 10_000 {
            return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "rel axis not allowed"));
        }
        if value == 0 {
            return Ok(());
        }
        self.emit(EV_REL, code, value)?;
        self.syn()
    }

    /// Smooth scrolling: hi-res units (120 = one detent) plus legacy detents.
    pub fn scroll_hires(&mut self, v120: i32, h120: i32, carry: &mut (i32, i32)) -> std::io::Result<()> {
        if v120 != 0 {
            self.emit(EV_REL, REL_WHEEL_HI_RES, v120)?;
            carry.0 += v120;
            let det = carry.0 / 120;
            if det != 0 {
                self.emit(EV_REL, REL_WHEEL, det)?;
                carry.0 -= det * 120;
            }
        }
        if h120 != 0 {
            self.emit(EV_REL, REL_HWHEEL_HI_RES, h120)?;
            carry.1 += h120;
            let det = carry.1 / 120;
            if det != 0 {
                self.emit(EV_REL, REL_HWHEEL, det)?;
                carry.1 -= det * 120;
            }
        }
        if v120 != 0 || h120 != 0 {
            self.syn()?;
        }
        Ok(())
    }

    pub fn pressed(&self) -> &[u16] {
        &self.pressed
    }

    pub fn release_all(&mut self) {
        let held: Vec<u16> = self.pressed.iter().rev().copied().collect();
        for c in held {
            let _ = self.key(c, false);
        }
    }
}

impl Drop for Uinput {
    fn drop(&mut self) {
        self.release_all();
        // SAFETY: valid fd.
        unsafe { libc::ioctl(self.f.as_raw_fd(), UI_DEV_DESTROY, 0) };
    }
}

// ------------------------------------------------------------ virtual joystick

const UI_SET_ABSBIT: libc::c_ulong = 0x4004_5567;
/// _IOW('U', 4, struct uinput_abs_setup), 28 bytes
const UI_ABS_SETUP: libc::c_ulong = 0x401c_5504;
pub const EV_ABS: u16 = 3;
/// BTN_TRIGGER: the first of 16 joystick buttons (BTN_JOYSTICK range, 0x120..0x12f)
pub const BTN_JOYSTICK: u16 = 0x120;
pub const GAMEPAD_BUTTONS: u8 = 16;
pub const GAMEPAD_AXIS_MAX: i32 = 32767;
pub const GAMEPAD_NAME: &str = "3DMouse Linux virtual joystick";

#[repr(C)]
struct InputAbsinfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    absinfo: InputAbsinfo,
}

/// A six-axis game controller (FR-507 game mode, opt-in). ABS_X..ABS_RZ carry the processed
/// axes, BTN_TRIGGER.. the device buttons. The broker creates it only while a virtual-joystick
/// profile is active and destroys it afterwards, so games never see a stale controller.
pub struct Gamepad {
    f: File,
    axes: [i32; 6],
    buttons: u16,
}

impl Gamepad {
    pub fn open(path: &str) -> std::io::Result<Gamepad> {
        let f = OpenOptions::new().write(true).custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC).open(path)?;
        let fd = f.as_raw_fd();
        let io = |req, arg: libc::c_int| -> std::io::Result<()> {
            // SAFETY: ioctls with integer arguments on a uinput fd.
            if unsafe { libc::ioctl(fd, req, arg) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        };
        io(UI_SET_EVBIT, EV_KEY as i32)?;
        io(UI_SET_EVBIT, EV_ABS as i32)?;
        io(UI_SET_EVBIT, EV_SYN as i32)?;
        for b in 0..GAMEPAD_BUTTONS as u16 {
            io(UI_SET_KEYBIT, (BTN_JOYSTICK + b) as i32)?;
        }
        for code in 0..6u16 {
            io(UI_SET_ABSBIT, code as i32)?;
            let abs = UinputAbsSetup {
                code,
                absinfo: InputAbsinfo { value: 0, minimum: -GAMEPAD_AXIS_MAX, maximum: GAMEPAD_AXIS_MAX, fuzz: 0, flat: 0, resolution: 0 },
            };
            // SAFETY: struct layout matches linux/uinput.h `struct uinput_abs_setup`.
            if unsafe { libc::ioctl(fd, UI_ABS_SETUP, &abs as *const UinputAbsSetup) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        let mut setup = UinputSetup { bustype: 0x06 /* BUS_VIRTUAL */, vendor: 0x1d6b, product: 0x5351, version: 1, name: [0; 80], ff_effects_max: 0 };
        setup.name[..GAMEPAD_NAME.len()].copy_from_slice(GAMEPAD_NAME.as_bytes());
        // SAFETY: struct layout matches linux/uinput.h `struct uinput_setup`.
        if unsafe { libc::ioctl(fd, UI_DEV_SETUP, &setup as *const UinputSetup) } < 0 {
            return Err(std::io::Error::last_os_error());
        }
        io(UI_DEV_CREATE, 0)?;
        Ok(Gamepad { f, axes: [0; 6], buttons: 0 })
    }

    fn emit(&mut self, typ: u16, code: u16, value: i32) -> std::io::Result<()> {
        let ev = libc::input_event { time: libc::timeval { tv_sec: 0, tv_usec: 0 }, type_: typ, code, value };
        // SAFETY: input_event is plain old data.
        let bytes = unsafe { std::slice::from_raw_parts(&ev as *const _ as *const u8, std::mem::size_of::<libc::input_event>()) };
        self.f.write_all(bytes)
    }

    /// Axes in -1..1 (the processed, normalised motion); only changed axes are written.
    pub fn set_axes(&mut self, norm: [f32; 6]) -> std::io::Result<()> {
        let mut any = false;
        for (i, n) in norm.iter().enumerate() {
            let v = (n.clamp(-1.0, 1.0) * GAMEPAD_AXIS_MAX as f32).round() as i32;
            if v != self.axes[i] {
                self.emit(EV_ABS, i as u16, v)?;
                self.axes[i] = v;
                any = true;
            }
        }
        if any {
            self.emit(EV_SYN, 0, 0)?;
        }
        Ok(())
    }

    pub fn set_button(&mut self, index: u8, down: bool) -> std::io::Result<()> {
        if index >= GAMEPAD_BUTTONS || down == (self.buttons & (1 << index) != 0) {
            return Ok(());
        }
        self.emit(EV_KEY, BTN_JOYSTICK + index as u16, down as i32)?;
        self.emit(EV_SYN, 0, 0)?;
        if down {
            self.buttons |= 1 << index;
        } else {
            self.buttons &= !(1 << index);
        }
        Ok(())
    }

    /// Centre every axis and release every button (lock, mode change, device loss).
    pub fn neutral(&mut self) {
        let _ = self.set_axes([0.0; 6]);
        for b in 0..GAMEPAD_BUTTONS {
            let _ = self.set_button(b, false);
        }
    }
}

impl Drop for Gamepad {
    fn drop(&mut self) {
        self.neutral();
        // SAFETY: valid fd.
        unsafe { libc::ioctl(self.f.as_raw_fd(), UI_DEV_DESTROY, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Uinput` that writes its events to a plain file instead of a kernel device.
    fn to_file(name: &str) -> (Uinput, std::path::PathBuf) {
        let p = std::env::temp_dir().join(format!("sm-uinput-{name}-{}", std::process::id()));
        let f = OpenOptions::new().create(true).truncate(true).write(true).open(&p).unwrap();
        (Uinput { f, pressed: Vec::new(), events_sent: 0 }, p)
    }

    fn key_events(p: &std::path::Path) -> Vec<(u16, i32)> {
        std::fs::read(p)
            .unwrap()
            .chunks(24)
            .map(|e| (u16::from_ne_bytes([e[16], e[17]]), u16::from_ne_bytes([e[18], e[19]]), i32::from_ne_bytes([e[20], e[21], e[22], e[23]])))
            .filter(|(t, _, _)| *t == EV_KEY)
            .map(|(_, c, v)| (c, v))
            .collect()
    }

    /// A chord that cannot be pressed in full presses nothing: a refused key later in the chord
    /// must not leave the keys before it held down, untracked by the caller.
    #[test]
    fn refused_chord_presses_no_key() {
        let (shift, alt, sysrq, a) = (42, 56, sm_actions::keys::KEY_SYSRQ, 30);
        assert!(allowed_key(shift) && allowed_key(a) && !allowed_key(9999));
        let (mut u, p) = to_file("chord");
        assert!(u.chord(&[shift, 9999], true).is_err(), "a key outside the allowlist is refused");
        if allowed_key(sysrq) {
            assert!(u.chord(&[alt, sysrq], true).is_err(), "alt+sysrq is refused");
        }
        assert_eq!(key_events(&p), vec![], "nothing was pressed");
        assert!(u.pressed.is_empty());
        u.chord(&[shift, a], true).unwrap();
        u.chord(&[shift, a], false).unwrap();
        assert_eq!(key_events(&p), vec![(shift, 1), (a, 1), (a, 0), (shift, 0)]);
        drop(u);
        let _ = std::fs::remove_file(p);
    }

    /// Creates a real controller; needs write access to /dev/uinput, so it only runs with
    /// SM_UINPUT_TEST=1 (e.g. as root in a container with --device /dev/uinput).
    #[test]
    fn gamepad_is_a_kernel_joystick() {
        if std::env::var_os("SM_UINPUT_TEST").is_none() {
            return;
        }
        let mut g = Gamepad::open("/dev/uinput").expect("create gamepad");
        g.set_axes([0.5, -0.5, 1.0, 0.0, 0.25, -1.0]).unwrap();
        g.set_button(0, true).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let devs = std::fs::read_to_string("/proc/bus/input/devices").unwrap();
        let block = devs.split("\n\n").find(|b| b.contains(GAMEPAD_NAME)).expect("device listed");
        println!("{block}");
        // joydev binds a js node only to devices with joystick axes and buttons
        assert!(block.contains(" js"), "not classed as a joystick: {block}");
        let hold = std::env::var("SM_UINPUT_HOLD_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        std::thread::sleep(std::time::Duration::from_millis(hold));
        g.neutral();
    }

    #[test]
    fn abs_setup_struct_size_matches_kernel() {
        // the ioctl number encodes the size; a mismatch would make UI_ABS_SETUP fail with ENOTTY
        assert_eq!(std::mem::size_of::<UinputAbsSetup>(), 28);
        assert_eq!(UI_ABS_SETUP, (1 << 30) | (28 << 16) | (0x55 << 8) | 4);
        assert_eq!(UI_SET_ABSBIT, (1 << 30) | (4 << 16) | (0x55 << 8) | 103);
    }
    #[test]
    fn allowlist() {
        assert!(allowed_key(29)); // ctrl
        assert!(allowed_key(0x110));
        assert!(!allowed_key(0x2ff));
        assert!(!allowed_key(116)); // KEY_POWER is deliberately not injectable
    }
    #[test]
    fn setup_struct_size_matches_kernel() {
        assert_eq!(std::mem::size_of::<UinputSetup>(), 92);
        assert_eq!(std::mem::size_of::<libc::input_event>(), 24);
    }
}
