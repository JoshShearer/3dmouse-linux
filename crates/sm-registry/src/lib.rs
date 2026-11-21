//! Versioned hardware registry (SRS §2.2, §4.4).
//!
//! Device ids and mappings live in `data/devices.toml`, never in code
//! branches. A system override file may extend (not silently replace) it.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const EMBEDDED: &str = include_str!("../data/devices.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportStatus {
    Unknown,
    Detected,
    AxisVerified,
    ButtonsVerified,
    OutputsVerified,
    FullVerified,
    Partial,
    Blocked,
}

impl SupportStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Detected => "detected",
            Self::AxisVerified => "axis_verified",
            Self::ButtonsVerified => "buttons_verified",
            Self::OutputsVerified => "outputs_verified",
            Self::FullVerified => "full_verified",
            Self::Partial => "partial",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Canonical {
    #[default]
    Identity,
    /// spacenavd DF_SWAPYZ | DF_INVYZ.
    SwapYzInvYz,
}

impl Canonical {
    /// Map a device-frame vector into the canonical (libspnav) frame.
    pub fn apply(&self, v: [i32; 6]) -> [i32; 6] {
        match self {
            Canonical::Identity => v,
            // out[ty] = -in[tz], out[tz] = -in[ty]; same for rotations.
            Canonical::SwapYzInvYz => [v[0], -v[2], -v[1], v[3], -v[5], -v[4]],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonDef {
    /// Linux evdev key code.
    pub code: u16,
    /// Bit in the HID button bitfield (hidraw path), if known.
    #[serde(default)]
    pub hid_bit: Option<u16>,
    /// Logical index exposed to clients.
    pub index: u8,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceModel {
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub family: String,
    pub transport: String,
    pub spnav_type: i32,
    /// Raw magnitude that maps to 1.0. None = take from report descriptor.
    #[serde(default)]
    pub axis_range: Option<i32>,
    #[serde(default)]
    pub canonical: Canonical,
    pub status: SupportStatus,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub buttons: Vec<ButtonDef>,
    /// Reuse the button table of another family.
    #[serde(default)]
    pub button_profile: Option<String>,
}

impl DeviceModel {
    pub fn usb_id(&self) -> String {
        format!("{:04x}:{:04x}", self.vid, self.pid)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockedRange {
    pub vid: u16,
    pub pids: Vec<u16>,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Registry {
    pub schema_version: u32,
    #[serde(default, rename = "device")]
    pub devices: Vec<DeviceModel>,
    #[serde(default, rename = "blocked")]
    pub blocked: Vec<BlockedRange>,
}

/// Result of matching a candidate input device.
#[derive(Debug, Clone, PartialEq)]
pub enum Match<'a> {
    Known(&'a DeviceModel),
    /// Same vendor family, not in registry: generic descriptor-based support only.
    GenericCandidate,
    Blocked(&'a str),
    NoMatch,
}

pub const VID_LOGITECH_3DX: u16 = 0x046d;
pub const VID_3DCONNEXION: u16 = 0x256f;

impl Registry {
    pub fn parse(text: &str) -> Result<Registry, String> {
        let mut r: Registry = toml::from_str(text).map_err(|e| e.to_string())?;
        if r.schema_version != 1 {
            return Err(format!("unsupported registry schema {}", r.schema_version));
        }
        r.resolve_button_profiles()?;
        r.validate()?;
        Ok(r)
    }

    pub fn embedded() -> &'static Registry {
        static R: OnceLock<Registry> = OnceLock::new();
        R.get_or_init(|| Registry::parse(EMBEDDED).expect("embedded registry is valid"))
    }

    fn resolve_button_profiles(&mut self) -> Result<(), String> {
        let snapshot = self.devices.clone();
        for d in &mut self.devices {
            if let Some(fam) = &d.button_profile {
                let src =
                    snapshot.iter().find(|s| &s.family == fam && !s.buttons.is_empty()).ok_or_else(|| format!("{}: unknown button_profile {fam}", d.name))?;
                d.buttons = src.buttons.clone();
                if d.axis_range.is_none() {
                    d.axis_range = src.axis_range;
                }
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for d in &self.devices {
            if !seen.insert((d.vid, d.pid, d.transport.clone())) {
                return Err(format!("duplicate registry entry {}", d.usb_id()));
            }
            if self.blocked_reason(d.vid, d.pid).is_some() {
                return Err(format!("{} is both supported and blocked", d.usb_id()));
            }
            let mut idx = std::collections::HashSet::new();
            let mut codes = std::collections::HashSet::new();
            for b in &d.buttons {
                if b.index as usize >= sm_core::MAX_BUTTONS {
                    return Err(format!("{}: button index {} too large", d.name, b.index));
                }
                if !idx.insert(b.index) || !codes.insert(b.code) {
                    return Err(format!("{}: duplicate button {}", d.name, b.label));
                }
            }
            if let Some(r) = d.axis_range {
                if r <= 0 {
                    return Err(format!("{}: axis_range must be positive", d.name));
                }
            }
        }
        Ok(())
    }

    fn blocked_reason(&self, vid: u16, pid: u16) -> Option<&str> {
        self.blocked.iter().find(|b| b.vid == vid && b.pids.contains(&pid)).map(|b| b.reason.as_str())
    }

    pub fn lookup(&self, vid: u16, pid: u16) -> Match<'_> {
        if let Some(r) = self.blocked_reason(vid, pid) {
            return Match::Blocked(r);
        }
        if let Some(d) = self.devices.iter().find(|d| d.vid == vid && d.pid == pid) {
            return Match::Known(d);
        }
        if vid == VID_3DCONNEXION {
            return Match::GenericCandidate;
        }
        Match::NoMatch
    }

    /// All (vid, pid) pairs explicitly supported; used to generate udev rules.
    pub fn usb_ids(&self) -> Vec<(u16, u16)> {
        let mut v: Vec<_> = self.devices.iter().map(|d| (d.vid, d.pid)).collect();
        v.sort();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_parses() {
        let r = Registry::embedded();
        assert!(r.devices.len() >= 15);
    }

    #[test]
    fn compact_lookup() {
        let r = Registry::embedded();
        match r.lookup(0x256f, 0xc635) {
            Match::Known(d) => {
                assert_eq!(d.name, "SpaceMouse Compact");
                assert_eq!(d.buttons.len(), 2);
                assert_eq!(d.axis_range, Some(350));
                assert_eq!(d.canonical, Canonical::SwapYzInvYz);
            }
            m => panic!("unexpected {m:?}"),
        }
    }

    #[test]
    fn cadmouse_and_keyboards_are_blocked() {
        let r = Registry::embedded();
        for pid in [0xc650, 0xc664, 0xc665] {
            assert!(matches!(r.lookup(0x256f, pid), Match::Blocked(_)));
        }
    }

    #[test]
    fn unrelated_devices_never_match() {
        let r = Registry::embedded();
        assert_eq!(r.lookup(0x046d, 0xc52b), Match::NoMatch); // Logitech Unifying receiver
        assert_eq!(r.lookup(0x0408, 0x546b), Match::NoMatch);
        assert_eq!(r.lookup(0x256f, 0xc6ff), Match::GenericCandidate);
    }

    #[test]
    fn button_profiles_resolve() {
        let r = Registry::embedded();
        let Match::Known(d) = r.lookup(0x256f, 0xc632) else { panic!() };
        assert_eq!(d.buttons.len(), 15);
    }

    #[test]
    fn canonical_transform_matches_spacenavd() {
        // spacenavd: idx swap {0,2,1,3,5,4}, then negate everything except 0 and 3.
        let v = [1, 2, 3, 4, 5, 6];
        assert_eq!(Canonical::SwapYzInvYz.apply(v), [1, -3, -2, 4, -6, -5]);
    }

    #[test]
    fn duplicate_rejected() {
        let t = r#"schema_version = 1
[[device]]
vid = 1
pid = 2
name = "a"
family = "a"
transport = "usb"
spnav_type = 0
status = "unknown"
[[device]]
vid = 1
pid = 2
name = "b"
family = "b"
transport = "usb"
spnav_type = 0
status = "unknown"
"#;
        assert!(Registry::parse(t).is_err());
    }

    /// The shipped udev rules must cover exactly the registry (FR-702);
    /// regenerate with tools/gen-udev-rules.py.
    #[test]
    fn udev_rules_match_registry() {
        let rules = include_str!("../../../packaging/udev/70-3dmouse-linux.rules");
        let reg = Registry::embedded();
        for d in &reg.devices {
            let k = format!(":{:04X}:{:04X}.*", d.vid, d.pid);
            assert!(rules.contains(&k), "udev rules missing {}", d.usb_id());
        }
        let n = rules.lines().filter(|l| l.starts_with("SUBSYSTEM==\"hidraw\", KERNELS")).count();
        let mut ids = reg.usb_ids();
        ids.sort();
        ids.dedup();
        assert_eq!(n, ids.len(), "udev rules contain devices not in the registry");
        assert!(!rules.contains("MODE=\"0666\""), "no world-writable device nodes");
        for l in rules.lines().filter(|l| l.starts_with("SUBSYSTEM==")) {
            assert!(l.contains("KERNELS==\"000"), "blanket rule without exact id: {l}");
        }
    }
}
