//! Core types shared by every 3DMouse Linux component.
//!
//! Axis order everywhere is `[TX, TY, TZ, RX, RY, RZ]`.
//! Two frames exist (see `docs/architecture/axis-frames.md`):
//! * **device frame** – the order/sign the HID report delivers;
//! * **canonical frame** – the spacenavd/libspnav convention that native
//!   applications (Blender, FreeCAD, OpenSCAD) already expect.
//!   The registry stores the per-model device->canonical transform.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const AXES: usize = 6;
pub const AXIS_NAMES: [&str; AXES] = ["tx", "ty", "tz", "rx", "ry", "rz"];
/// Upper bound on the number of buttons any adapter may report.
pub const MAX_BUTTONS: usize = 64;

/// Monotonic time in microseconds (CLOCK_MONOTONIC).
pub fn monotonic_us() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: valid pointer to a timespec; CLOCK_MONOTONIC always exists on Linux.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000 + ts.tv_nsec as u64 / 1_000
}

/// Raw decoded axis/button sample as delivered by one physical report
/// (or one evdev SYN frame), in the **device frame**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RawFrame {
    /// Raw axis values. Only entries whose bit is set in `axes_present` were
    /// carried by this report; the rest must be taken from the previous state.
    pub axes: [i32; AXES],
    pub axes_present: u8,
    /// Logical (contiguous, registry-mapped) button state bitmask.
    pub buttons: u64,
    pub buttons_present: bool,
    pub t_us: u64,
}

impl RawFrame {
    pub fn has_axis(&self, i: usize) -> bool {
        self.axes_present & (1 << i) != 0
    }
}

/// Accumulated per-device input state. Merges partial frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputState {
    pub axes: [i32; AXES],
    pub buttons: u64,
    pub last_motion_us: u64,
    pub last_nonzero_us: u64,
}

impl InputState {
    /// Merge a frame. Returns `(axes_changed, button_edges)` where
    /// `button_edges` is a bitmask of buttons whose state toggled.
    pub fn apply(&mut self, f: &RawFrame) -> (bool, u64) {
        let mut changed = false;
        for i in 0..AXES {
            if f.has_axis(i) && self.axes[i] != f.axes[i] {
                self.axes[i] = f.axes[i];
                changed = true;
            }
        }
        if f.axes_present != 0 {
            self.last_motion_us = f.t_us;
            if self.axes.iter().any(|v| *v != 0) {
                self.last_nonzero_us = f.t_us;
            }
        }
        let mut edges = 0;
        if f.buttons_present {
            edges = self.buttons ^ f.buttons;
            self.buttons = f.buttons;
        }
        (changed, edges)
    }

    pub fn is_neutral(&self) -> bool {
        self.axes.iter().all(|v| *v == 0)
    }

    /// Zero everything (disconnect / watchdog). Returns the buttons that were
    /// held so callers can emit synthetic releases (FR-104, FR-306).
    pub fn reset(&mut self) -> u64 {
        let held = self.buttons;
        *self = InputState::default();
        held
    }
}

/// Device lifecycle state machine (SRS §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Absent,
    Discovered,
    Probing,
    Ready,
    Suspended,
    Disconnected,
    Reconnecting,
    Error,
    Unsupported,
}

impl DeviceState {
    /// Allowed transitions; anything else is a programming error.
    pub fn can_transition(self, to: DeviceState) -> bool {
        use DeviceState::*;
        matches!(
            (self, to),
            (Absent, Discovered)
                | (Discovered, Probing)
                | (Probing, Ready)
                | (Probing, Error)
                | (Probing, Unsupported)
                | (Ready, Suspended)
                | (Ready, Disconnected)
                | (Ready, Error)
                | (Suspended, Ready)
                | (Suspended, Disconnected)
                | (Disconnected, Reconnecting)
                | (Disconnected, Absent)
                | (Reconnecting, Probing)
                | (Reconnecting, Absent)
                | (Error, Absent)
                | (Error, Reconnecting)
                | (Unsupported, Absent)
        )
    }
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = serde_json::to_value(self).ok().and_then(|v| v.as_str().map(String::from));
        write!(f, "{}", s.unwrap_or_default())
    }
}

/// Which component owns the hardware (FR-107).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnershipMode {
    /// We read the device and serve the spacenavd-compatible socket.
    NativeOwner,
    /// spacenavd owns the device; we are one of its clients.
    IntegratedSpacenav,
    /// Read-only observation; no outputs of any kind.
    PassiveMonitor,
}

impl fmt::Display for OwnershipMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OwnershipMode::NativeOwner => "native-owner",
            OwnershipMode::IntegratedSpacenav => "integrated-spacenav",
            OwnershipMode::PassiveMonitor => "passive-monitor",
        })
    }
}

impl std::str::FromStr for OwnershipMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "native-owner" => Ok(Self::NativeOwner),
            "integrated-spacenav" => Ok(Self::IntegratedSpacenav),
            "passive-monitor" => Ok(Self::PassiveMonitor),
            "auto" => Err("auto must be resolved by the caller".into()),
            o => Err(format!("unknown ownership mode `{o}`")),
        }
    }
}

/// How the active application receives 6-DOF motion (FR-408).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppMode {
    /// Motion goes to libspnav clients only.
    #[default]
    Native6dof,
    /// App consumes motion through its own plugin (still via spnav socket).
    ApplicationPlugin,
    /// Motion is translated into scroll/zoom/pan through the virtual input device.
    DesktopEmulation,
    /// Device motion is ignored entirely for this app.
    None,
    /// Motion and buttons drive a virtual game controller (FR-507 game mode). Opt-in: the agent
    /// only requests it when the user allowed it in Settings.
    VirtualJoystick,
}

/// Redact an identifier for logs/support bundles (FR-106, FR-900).
pub fn redact(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    // FNV-1a: stable, non-reversible enough for support correlation.
    let mut h: u32 = 0x811c9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    format!("redacted-{h:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_frames_merge() {
        let mut st = InputState::default();
        let f1 = RawFrame { axes: [10, 20, 30, 0, 0, 0], axes_present: 0b000111, t_us: 1, ..Default::default() };
        let f2 = RawFrame { axes: [0, 0, 0, 4, 5, 6], axes_present: 0b111000, t_us: 2, ..Default::default() };
        assert!(st.apply(&f1).0);
        assert!(st.apply(&f2).0);
        assert_eq!(st.axes, [10, 20, 30, 4, 5, 6]);
    }

    #[test]
    fn button_edges_and_reset() {
        let mut st = InputState::default();
        let f = RawFrame { buttons: 0b10, buttons_present: true, ..Default::default() };
        assert_eq!(st.apply(&f).1, 0b10);
        assert_eq!(st.apply(&f).1, 0);
        assert_eq!(st.reset(), 0b10);
        assert!(st.is_neutral());
        assert_eq!(st.buttons, 0);
    }

    #[test]
    fn state_machine() {
        use DeviceState::*;
        assert!(Absent.can_transition(Discovered));
        assert!(Ready.can_transition(Disconnected));
        assert!(!Absent.can_transition(Ready));
        assert_eq!(Ready.to_string(), "ready");
    }

    #[test]
    fn redaction_is_stable_and_opaque() {
        assert_eq!(redact("ABC123"), redact("ABC123"));
        assert!(!redact("ABC123").contains("ABC"));
        assert_eq!(redact(""), "");
    }
}
