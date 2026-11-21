//! Profiles, presets, storage and selection (FR-400, FR-809, SRS §5).
//!
//! **Selection precedence (FR-403)** – exactly one *complete* profile is
//! active; fields are never merged, so behaviour is always exactly what the
//! profile shows in the UI:
//! 1. manual override (user picked a profile, until cleared);
//! 2. application profile whose `app_selector` matches the focused window
//!    and whose `device_selector` matches the device;
//! 3. application profile without a device selector that matches;
//! 4. device profile for the device;
//! 5. the global `default` profile.
//!
//! Ties inside one level resolve by profile id (lexicographic) so the
//! result is deterministic.
//!
//! **Storage** – `$XDG_CONFIG_HOME/3dmouse-linux/profiles/<id>.toml`,
//! mode 0600, written atomically (temp file + fsync + rename + dir fsync)
//! under an advisory `flock`, previous version kept as `<id>.toml.bak`.
//! Unparseable files are quarantined as `<id>.toml.corrupt-<unix>` and
//! reported; they never crash the agent.

use serde::{Deserialize, Serialize};
use sm_actions::{Action, Binding, Timing, View};
use sm_core::AppMode;
use sm_pipeline::NavConfig;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_ID: &str = "default";
pub const MAX_PROFILE_BYTES: usize = 256 * 1024;
pub const MAX_PROFILES: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Global,
    Device,
    Application,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppSelector {
    /// Case-insensitive exact matches against Wayland app_id / X11 WM_CLASS /
    /// Flatpak app id / desktop-file id.
    pub app_ids: Vec<String>,
    /// Executables: a file name (`blender`) or an absolute path (`/opt/blender/blender`),
    /// compared with the focused window's process image (FR-402).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub executables: Vec<String>,
    /// Opt-in: substring that must appear in the window title (FR-402).
    pub title_contains: Option<String>,
    /// Opt-in: regular expression the window title must match (FR-402). Linear-time engine;
    /// titles are only matched, never stored or logged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_regex: Option<String>,
}

pub const MAX_TITLE_REGEX: usize = 256;

fn compile_title_regex(re: &str) -> Result<regex::Regex, String> {
    if re.len() > MAX_TITLE_REGEX {
        return Err(format!("title regex longer than {MAX_TITLE_REGEX} characters"));
    }
    regex::RegexBuilder::new(re).case_insensitive(true).size_limit(1 << 16).dfa_size_limit(1 << 16).build().map_err(|e| format!("title regex: {e}"))
}

impl AppSelector {
    pub fn matches(&self, app_id: &str, title: &str) -> bool {
        self.matches_window(app_id, title, None)
    }

    /// `exe` is the focused window's process image (`/proc/<pid>/exe`), when known.
    pub fn matches_window(&self, app_id: &str, title: &str, exe: Option<&str>) -> bool {
        let id_ok = !app_id.is_empty() && self.app_ids.iter().any(|a| a.eq_ignore_ascii_case(app_id));
        let exe_ok = exe.is_some_and(|e| {
            let base = e.rsplit('/').next().unwrap_or(e);
            self.executables.iter().any(|x| if x.starts_with('/') { x == e } else { x == base })
        });
        if !(id_ok || exe_ok) {
            return false;
        }
        let substr_ok = match &self.title_contains {
            Some(t) if !t.is_empty() => title.to_lowercase().contains(&t.to_lowercase()),
            _ => true,
        };
        let regex_ok = match &self.title_regex {
            Some(r) if !r.is_empty() => compile_title_regex(r).map(|re| re.is_match(title)).unwrap_or(false),
            _ => true,
        };
        substr_ok && regex_ok
    }

    /// How specific the title condition is: regex and substring 3, regex 2, substring 1,
    /// none 0. A higher value wins when several application profiles match.
    pub fn title_specificity(&self) -> u8 {
        let sub = self.title_contains.as_deref().is_some_and(|t| !t.is_empty());
        let re = self.title_regex.as_deref().is_some_and(|t| !t.is_empty());
        (re as u8) * 2 + sub as u8
    }
}

/// Desktop-emulation mapping (FR-507), used when `native_app_mode` is
/// `desktop-emulation`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopEmulation {
    /// Canonical axis index driving vertical scroll (default: TZ push/pull).
    pub vertical_axis: u8,
    /// Canonical axis driving horizontal scroll (default: TX).
    pub horizontal_axis: u8,
    /// Hi-res scroll units (120 = one notch) per second at full deflection.
    pub scroll_speed: f32,
    pub invert_vertical: bool,
    pub invert_horizontal: bool,
    /// Canonical axis that zooms (ctrl+scroll), 255 = disabled.
    pub zoom_axis: u8,
    /// What motion does: scroll (default), move the pointer, or press keys.
    pub style: MotionStyle,
    /// Pointer mode: pixels per second at full deflection.
    pub pointer_speed: f32,
    /// Keys mode: key chords fired when an axis is pushed past a threshold.
    pub axis_keys: Vec<AxisKeys>,
}

impl Default for DesktopEmulation {
    fn default() -> Self {
        DesktopEmulation {
            vertical_axis: 2,
            horizontal_axis: 0,
            scroll_speed: 2400.0,
            invert_vertical: false,
            invert_horizontal: false,
            zoom_axis: 255,
            style: MotionStyle::Scroll,
            pointer_speed: 1200.0,
            axis_keys: Vec::new(),
        }
    }
}

/// How desktop emulation turns motion into input (FR-507).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MotionStyle {
    /// Push/pull and left/right scroll; an optional zoom axis.
    #[default]
    Scroll,
    /// The cap moves the pointer (horizontal and vertical axes); lift/press scrolls.
    /// For people who find a mouse hard to use (accessibility navigation).
    Pointer,
    /// Axis gestures press key chords: media keys, window or workspace switching.
    Keys,
}

pub const MAX_AXIS_KEYS: usize = 12;

/// One axis gesture: pushing `axis` past `threshold` (0..1 of full deflection) fires `positive`
/// or `negative`, once, or every `repeat_ms` while held. The axis must come back below 60 % of
/// the threshold before the gesture fires again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisKeys {
    pub axis: u8,
    #[serde(default)]
    pub positive: String,
    #[serde(default)]
    pub negative: String,
    #[serde(default = "default_axis_threshold")]
    pub threshold: f32,
    #[serde(default)]
    pub repeat_ms: u32,
}

fn default_axis_threshold() -> f32 {
    0.5
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OsdPrefs {
    pub enabled: bool,
    pub timeout_ms: u32,
}

impl Default for OsdPrefs {
    fn default() -> Self {
        OsdPrefs { enabled: true, timeout_ms: 1500 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default)]
    pub scope: Scope,
    #[serde(default)]
    pub device_selector: Option<String>,
    #[serde(default)]
    pub app_selector: Option<AppSelector>,
    #[serde(default)]
    pub native_app_mode: AppMode,
    #[serde(default)]
    pub navigation: NavConfig,
    /// Keys are logical button indices as strings ("0", "1", ...).
    #[serde(default)]
    pub bindings: BTreeMap<String, Binding>,
    #[serde(default)]
    pub timing: Timing,
    /// QuickView preset -> key chord for this application (FR-307).
    #[serde(default)]
    pub views: BTreeMap<String, String>,
    #[serde(default)]
    pub desktop: DesktopEmulation,
    #[serde(default)]
    pub osd: OsdPrefs,
    /// Free-text provenance/compatibility note (e.g. "untested preset").
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub modified_at: String,
    /// Unknown future fields survive a load/save round-trip (SRS §5).
    #[serde(flatten)]
    pub extra: toml::Table,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

pub fn view_name(v: View) -> &'static str {
    match v {
        View::Top => "top",
        View::Bottom => "bottom",
        View::Left => "left",
        View::Right => "right",
        View::Front => "front",
        View::Back => "back",
        View::Iso1 => "iso1",
        View::Iso2 => "iso2",
        View::Fit => "fit",
    }
}

const VIEW_NAMES: [&str; 9] = ["top", "bottom", "left", "right", "front", "back", "iso1", "iso2", "fit"];

impl Profile {
    pub fn new(id: &str, name: &str) -> Profile {
        Profile {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            display_name: name.into(),
            scope: Scope::Global,
            device_selector: None,
            app_selector: None,
            native_app_mode: AppMode::Native6dof,
            navigation: NavConfig::default(),
            bindings: BTreeMap::new(),
            timing: Timing::default(),
            views: BTreeMap::new(),
            desktop: DesktopEmulation::default(),
            osd: OsdPrefs::default(),
            notes: String::new(),
            modified_at: String::new(),
            extra: toml::Table::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!("schema_version {} unsupported (expected {SCHEMA_VERSION})", self.schema_version));
        }
        if !valid_id(&self.id) {
            return Err("id must be 1-64 chars of [a-z0-9_-]".into());
        }
        if self.display_name.trim().is_empty() || self.display_name.len() > 128 {
            return Err("display_name must be 1-128 chars".into());
        }
        self.navigation.validate()?;
        if self.bindings.len() > sm_core::MAX_BUTTONS {
            return Err("too many bindings".into());
        }
        for (k, b) in &self.bindings {
            let idx: usize = k.parse().map_err(|_| format!("binding key `{k}` is not a button index"))?;
            if idx >= sm_core::MAX_BUTTONS {
                return Err(format!("button index {idx} out of range"));
            }
            b.validate().map_err(|e| format!("button {k}: {e}"))?;
            for a in [Some(&b.tap), b.hold.as_ref(), b.double.as_ref()].into_iter().flatten() {
                if let Action::View { view } = a {
                    let _ = view; // resolved at runtime; missing mapping => inactive with message
                }
            }
        }
        for (v, chord) in &self.views {
            if !VIEW_NAMES.contains(&v.as_str()) {
                return Err(format!("unknown view `{v}`"));
            }
            sm_actions::keys::parse_chord(chord).map_err(|e| format!("view {v}: {e}"))?;
        }
        if !(1.0..=20000.0).contains(&self.desktop.scroll_speed) || self.desktop.vertical_axis > 5 || self.desktop.horizontal_axis > 5 {
            return Err("desktop emulation parameters out of range".into());
        }
        if self.desktop.zoom_axis > 5 && self.desktop.zoom_axis != 255 {
            return Err("desktop.zoom_axis must be 0..5 or 255".into());
        }
        if !(50.0..=5000.0).contains(&self.desktop.pointer_speed) {
            return Err("desktop.pointer_speed must be 50..5000".into());
        }
        if self.desktop.axis_keys.len() > MAX_AXIS_KEYS {
            return Err(format!("at most {MAX_AXIS_KEYS} axis gestures"));
        }
        for (i, g) in self.desktop.axis_keys.iter().enumerate() {
            let n = i + 1;
            if g.axis > 5 {
                return Err(format!("axis gesture {n}: axis must be 0..5"));
            }
            if !(0.2..=0.95).contains(&g.threshold) {
                return Err(format!("axis gesture {n}: threshold must be 0.2..0.95"));
            }
            if g.repeat_ms != 0 && !(100..=2000).contains(&g.repeat_ms) {
                return Err(format!("axis gesture {n}: repeat must be 0 (once) or 100..2000 ms"));
            }
            if g.positive.is_empty() && g.negative.is_empty() {
                return Err(format!("axis gesture {n}: needs a key for at least one direction"));
            }
            for c in [&g.positive, &g.negative] {
                if !c.is_empty() {
                    sm_actions::keys::parse_chord(c).map_err(|e| format!("axis gesture {n}: {e}"))?;
                }
            }
        }
        if let Some(sel) = &self.app_selector {
            if sel.app_ids.len() > 32
                || sel.app_ids.iter().any(|a| a.len() > 255)
                || sel.executables.len() > 32
                || sel.executables.iter().any(|a| a.is_empty() || a.len() > 4096)
            {
                return Err("app_selector too large".into());
            }
            if let Some(r) = sel.title_regex.as_deref().filter(|r| !r.is_empty()) {
                compile_title_regex(r)?;
            }
        }
        if self.scope == Scope::Application && self.app_selector.as_ref().map(|s| s.app_ids.is_empty() && s.executables.is_empty()).unwrap_or(true) {
            return Err("application profiles need at least one app id or executable".into());
        }
        Ok(())
    }

    pub fn binding_map(&self) -> BTreeMap<u8, Binding> {
        self.bindings.iter().filter_map(|(k, v)| k.parse::<u8>().ok().map(|i| (i, v.clone()))).collect()
    }

    pub fn to_toml(&self) -> Result<String, String> {
        toml::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn from_toml(s: &str) -> Result<Profile, String> {
        if s.len() > MAX_PROFILE_BYTES {
            return Err("profile file too large".into());
        }
        let p: Profile = toml::from_str(s).map_err(|e| e.to_string())?;
        p.validate()?;
        Ok(p)
    }

    pub fn matches_device(&self, usb_id: Option<&str>) -> bool {
        match (&self.device_selector, usb_id) {
            (None, _) => true,
            (Some(s), Some(u)) => s.eq_ignore_ascii_case(u),
            (Some(_), None) => false,
        }
    }
}

// ----------------------------------------------------------- presets

fn app(id: &str, name: &str, ids: &[&str], mode: AppMode, notes: &str) -> Profile {
    let mut p = Profile::new(id, name);
    p.scope = Scope::Application;
    p.app_selector = Some(AppSelector { app_ids: ids.iter().map(|s| s.to_string()).collect(), ..Default::default() });
    p.native_app_mode = mode;
    p.notes = notes.into();
    p
}

/// Window classes / Wayland app ids of common browsers (X11 classes match case-insensitively).
const BROWSER_IDS: &[&str] = &[
    "firefox",
    "org.mozilla.firefox",
    "firefox-esr",
    "librewolf",
    "chromium",
    "chromium-browser",
    "org.chromium.Chromium",
    "google-chrome",
    "brave-browser",
    "com.brave.Browser",
    "vivaldi-stable",
    "microsoft-edge",
    "zen",
];

/// Shipped presets (FR-405). Notes state test status honestly; see
/// docs/compatibility/application-matrix.md.
pub fn presets() -> Vec<Profile> {
    let mut default = Profile::new(DEFAULT_ID, "Default (native 6-DOF apps)");
    default.notes = "Motion and buttons go to libspnav applications; no desktop input is injected.".into();

    let mut desktop = Profile::new("desktop", "Desktop navigation (scroll)");
    desktop.native_app_mode = AppMode::DesktopEmulation;
    desktop.notes = "Push/pull scrolls vertically, left/right scrolls horizontally. Left button = Back, right button = Forward.".into();
    desktop.bindings.insert("0".into(), Binding::tap(Action::Mouse { button: sm_actions::MouseButton::Back }));
    desktop.bindings.insert("1".into(), Binding::tap(Action::Mouse { button: sm_actions::MouseButton::Forward }));

    let mut blender = app(
        "blender",
        "Blender",
        &["blender", "org.blender.Blender"],
        AppMode::Native6dof,
        "Blender reads the libspnav socket natively (NDOF). Buttons are forwarded as NDOF buttons.",
    );
    blender.views = [("top", "kp7"), ("front", "kp1"), ("right", "kp3"), ("bottom", "ctrl+kp7"), ("back", "ctrl+kp1"), ("left", "ctrl+kp3"), ("fit", "home")]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

    let mut freecad = app(
        "freecad",
        "FreeCAD",
        &["freecad", "org.freecad.FreeCAD", "FreeCAD"],
        AppMode::Native6dof,
        "FreeCAD uses libspnav when built with spacenav support (Arch/Ubuntu packages are).",
    );
    freecad.views = [("top", "2"), ("front", "1"), ("right", "3"), ("bottom", "5"), ("back", "4"), ("left", "6"), ("iso1", "0"), ("fit", "v")]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();

    let openscad = app(
        "openscad",
        "OpenSCAD",
        &["openscad", "OpenSCAD", "org.openscad.OpenSCAD"],
        AppMode::Native6dof,
        "OpenSCAD reads the libspnav socket natively, but its SpaceNav driver is off by default: tick Edit > Preferences > Axes > SpaceNav (or add `[input] enableDriverSPNAV=true` to ~/.config/OpenSCAD/OpenSCAD.conf) and restart OpenSCAD.",
    );
    let mut kicad = app(
        "kicad",
        "KiCad",
        &["kicad", "org.kicad.kicad", "pcbnew", "eeschema"],
        AppMode::Native6dof,
        "KiCad links libspnav and navigates natively (2D canvas pan/zoom and 3D viewer); verified with KiCad 10.0.7. Restart KiCad after 3dmoused restarts – libspnav does not reconnect. For older KiCad builds without libspnav, switch Output mode to Desktop emulation.",
    );
    kicad.desktop = DesktopEmulation { vertical_axis: 2, horizontal_axis: 0, scroll_speed: 1800.0, zoom_axis: 255, ..Default::default() };
    let prusa = app(
        "prusaslicer",
        "PrusaSlicer",
        &["PrusaSlicer", "prusa-slicer", "com.prusa3d.PrusaSlicer"],
        AppMode::None,
        "PrusaSlicer reads the device itself through hidapi/hidraw; 3DMouse Linux stays out of its way (mode none) to avoid double motion.",
    );
    let mut browsers = app(
        "web-browsers",
        "Web browsers (scroll)",
        BROWSER_IDS,
        AppMode::DesktopEmulation,
        "Push/pull scrolls vertically, left/right scrolls horizontally. Left button = Back, right button = Forward. Onshape tabs use their own profile.",
    );
    browsers.bindings = desktop.bindings.clone();
    let mut browser = app(
        "browser-onshape",
        "Browser CAD (Onshape) – emulation",
        BROWSER_IDS,
        AppMode::None,
        "Web CAD needs a browser bridge for native 6-DOF (not shipped). Switch to desktop-emulation manually if wanted.",
    );
    browser.app_selector.as_mut().unwrap().title_contains = Some("Onshape".into());

    // Opt-in desktop modes (FR-507): no application selector, so they are only used when picked.
    let mut media = Profile::new("media-control", "Media control");
    media.native_app_mode = AppMode::DesktopEmulation;
    media.desktop.style = MotionStyle::Keys;
    let g = |axis, positive: &str, negative: &str, threshold, repeat_ms| AxisKeys {
        axis,
        positive: positive.into(),
        negative: negative.into(),
        threshold,
        repeat_ms,
    };
    media.desktop.axis_keys = vec![
        // twist clockwise is RY negative: clockwise turns the volume up, like a knob
        g(4, "volumedown", "volumeup", 0.35, 150),
        g(0, "nextsong", "previoussong", 0.6, 0),
        g(1, "", "playpause", 0.6, 0),
    ];
    media.bindings.insert("0".into(), Binding::tap(Action::Key { keys: "playpause".into() }));
    media.bindings.insert("1".into(), Binding::tap(Action::Key { keys: "mute".into() }));
    media.notes = "Twist for volume (clockwise is louder), push right/left for next/previous track, press down to play or pause. Left button = play/pause, right button = mute.".into();

    let mut pointer = Profile::new("accessibility-pointer", "Accessibility: pointer control");
    pointer.native_app_mode = AppMode::DesktopEmulation;
    pointer.desktop.style = MotionStyle::Pointer;
    pointer.bindings.insert("0".into(), Binding::tap(Action::Mouse { button: sm_actions::MouseButton::Left }));
    pointer.bindings.insert("1".into(), Binding::tap(Action::Mouse { button: sm_actions::MouseButton::Right }));
    pointer.notes = "Push the cap to move the pointer (away = up), lift or press to scroll. Left button = click, right button = right-click. Tune the speed under Navigation.".into();

    let mut joystick = Profile::new("game-joystick", "Game controller (virtual joystick)");
    joystick.native_app_mode = AppMode::VirtualJoystick;
    joystick.notes = "Six axes and the buttons appear as a game controller (\"3DMouse Linux virtual joystick\"). Needs Settings → Allow virtual joystick. The controller exists only while this profile is active.".into();

    vec![default, desktop, blender, freecad, openscad, kicad, prusa, browser, browsers, media, pointer, joystick]
}

// ----------------------------------------------------------- binding lint

/// A binding that can't work as configured, or probably isn't what the user meant (GUI §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lint {
    /// Logical button index, or `None` for profile-wide findings.
    pub button: Option<u8>,
    /// `tap`, `hold`, `double` or empty.
    pub slot: String,
    /// `error` (won't work) or `warning` (works, but probably unintended).
    pub severity: &'static str,
    pub message: String,
}

impl Profile {
    /// Check the bindings against each other, the profile's mode and the agent settings.
    /// `profile_ids` are the profiles that exist; `allow_exec` is the agent setting.
    pub fn lint(&self, profile_ids: &[String], allow_exec: bool) -> Vec<Lint> {
        use sm_actions::Action;
        let mut out = Vec::new();
        let mut push =
            |button: Option<u8>, slot: &str, severity: &'static str, message: String| out.push(Lint { button, slot: slot.into(), severity, message });
        let mut chords: BTreeMap<String, Vec<(u8, String)>> = BTreeMap::new();
        for (k, b) in &self.bindings {
            let Ok(bi) = k.parse::<u8>() else { continue };
            let slots = [("tap", Some(&b.tap)), ("hold", b.hold.as_ref()), ("double", b.double.as_ref())];
            for (slot, a) in slots {
                let Some(a) = a else { continue };
                // radial items run like taps; check them with their own slot label
                let mut acts: Vec<(String, &Action)> = vec![(slot.to_string(), a)];
                if let Action::Radial { items } = a {
                    acts.extend(items.iter().map(|it| (format!("{slot} › {}", it.label), &it.action)));
                }
                for (sl, a) in acts {
                    match a {
                        Action::Radial { .. } if slot != "tap" => push(Some(bi), &sl, "error", "a radial menu only works as the press action".into()),
                        Action::Radial { .. } if b.hold.is_some() || b.double.is_some() => {
                            push(Some(bi), &sl, "error", "a radial menu needs the whole press: remove the hold and double-press actions on this button".into())
                        }
                        Action::SpnavButton { .. } if self.native_app_mode == AppMode::None => {
                            push(Some(bi), &sl, "warning", "output mode is Off, so applications never see this button".into())
                        }
                        Action::Exec { confirmed: false, .. } => push(Some(bi), &sl, "error", "the command isn't confirmed, so it never runs".into()),
                        Action::Exec { .. } if !allow_exec => {
                            push(Some(bi), &sl, "warning", "command actions are disabled in Settings, so this does nothing".into())
                        }
                        Action::Exec { argv, .. } if argv.is_empty() => push(Some(bi), &sl, "error", "the command is empty".into()),
                        Action::ProfileSwitch { profile } if !profile_ids.iter().any(|p| p == profile) => {
                            push(Some(bi), &sl, "error", format!("profile '{profile}' doesn't exist"))
                        }
                        Action::View { view } if !self.views.contains_key(view_name(*view)) => {
                            push(Some(bi), &sl, "warning", format!("this profile has no '{}' view shortcut, so nothing happens", view_name(*view)))
                        }
                        Action::Key { keys } => match sm_actions::keys::parse_chord(keys) {
                            Err(e) => push(Some(bi), &sl, "error", format!("key chord: {e}")),
                            Ok(_) => chords.entry(keys.to_lowercase().replace(' ', "")).or_default().push((bi, sl.clone())),
                        },
                        Action::None if slot == "tap" && b.hold.is_none() && b.double.is_none() => {
                            push(Some(bi), &sl, "warning", "this button does nothing (not even forwarded to applications)".into())
                        }
                        _ => {}
                    }
                }
            }
        }
        for (chord, uses) in chords {
            if uses.len() > 1 {
                let list: Vec<String> = uses.iter().map(|(b, s)| format!("button {} {s}", b + 1)).collect();
                for (b, s) in &uses {
                    push(
                        Some(*b),
                        s,
                        "warning",
                        format!(
                            "'{chord}' is also bound to {}",
                            list.iter().filter(|l| **l != format!("button {} {s}", b + 1)).cloned().collect::<Vec<_>>().join(", ")
                        ),
                    );
                }
            }
        }
        out
    }
}

// ----------------------------------------------------------- store

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("THREEDMOUSE_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(".config"));
    base.join("3dmouse-linux")
}

/// Pre-release builds were called "SpaceMouse Linux" and kept settings in
/// `~/.config/spacemouse-linux`. Move that directory to the current location once, if the new one
/// doesn't exist yet. Returns the old path when it moved something.
pub fn migrate_legacy_config_dir() -> Option<PathBuf> {
    if std::env::var_os("THREEDMOUSE_CONFIG_DIR").is_some() {
        return None;
    }
    let new = config_dir();
    let old = new.parent()?.join("spacemouse-linux");
    migrate_dir(&old, &new).then_some(old)
}

fn migrate_dir(old: &Path, new: &Path) -> bool {
    !new.exists() && old.is_dir() && std::fs::rename(old, new).is_ok()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub auto_switch: bool,
    pub manual_profile: Option<String>,
    /// FR-305: user-created exec actions only run when this is true.
    pub allow_exec: bool,
    pub osd_enabled: bool,
    pub log_level: String,
    /// FR-507: profiles in virtual-joystick mode only create the game controller when this is true.
    pub allow_virtual_joystick: bool,
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: 1,
            auto_switch: true,
            manual_profile: None,
            allow_exec: false,
            osd_enabled: true,
            log_level: "info".into(),
            allow_virtual_joystick: false,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct LoadReport {
    pub loaded: usize,
    pub quarantined: Vec<String>,
    pub errors: Vec<String>,
}

pub struct Store {
    pub dir: PathBuf,
}

/// RFC 3339 UTC timestamp without external crates.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil_from_days (H. Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Atomic, durable file replace with 0600 permissions.
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no parent dir"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp-{}", path.file_name().and_then(|n| n.to_str()).unwrap_or("f"), std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

struct Lock(std::fs::File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

impl Store {
    pub fn new(dir: PathBuf) -> Store {
        Store { dir }
    }

    pub fn default_location() -> Store {
        Store::new(config_dir())
    }

    fn profiles_dir(&self) -> PathBuf {
        self.dir.join("profiles")
    }

    fn lock(&self) -> std::io::Result<Lock> {
        std::fs::create_dir_all(&self.dir)?;
        let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600).open(self.dir.join(".lock"))?;
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Lock(f))
    }

    pub fn path_for(&self, id: &str) -> PathBuf {
        self.profiles_dir().join(format!("{id}.toml"))
    }

    /// Write presets that do not exist yet (first run / new version).
    pub fn ensure_presets(&self) -> std::io::Result<usize> {
        let mut n = 0;
        for p in presets() {
            if !self.path_for(&p.id).exists() {
                self.save(&p)?;
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn load_all(&self) -> (Vec<Profile>, LoadReport) {
        let mut rep = LoadReport::default();
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(self.profiles_dir()) else {
            return (out, rep);
        };
        let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false)).collect();
        paths.sort();
        for p in paths.into_iter().take(MAX_PROFILES) {
            let r = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| Profile::from_toml(&s));
            match r {
                Ok(prof) if p.file_stem().and_then(|s| s.to_str()) == Some(prof.id.as_str()) => {
                    out.push(prof);
                    rep.loaded += 1;
                }
                Ok(prof) => rep.errors.push(format!("{}: id `{}` does not match file name; ignored", p.display(), prof.id)),
                Err(e) => {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    let q = p.with_extension(format!("toml.corrupt-{ts}"));
                    let _ = std::fs::rename(&p, &q);
                    rep.quarantined.push(format!("{} -> {}: {e}", p.display(), q.display()));
                }
            }
        }
        if !out.iter().any(|p| p.id == DEFAULT_ID) {
            out.push(presets().remove(0));
        }
        (out, rep)
    }

    pub fn load(&self, id: &str) -> Result<Profile, String> {
        if !valid_id(id) {
            return Err("invalid profile id".into());
        }
        let s = std::fs::read_to_string(self.path_for(id)).map_err(|e| e.to_string())?;
        Profile::from_toml(&s)
    }

    pub fn save(&self, p: &Profile) -> std::io::Result<()> {
        p.validate().map_err(std::io::Error::other)?;
        let _l = self.lock()?;
        let path = self.path_for(&p.id);
        let mut p = p.clone();
        p.modified_at = now_rfc3339();
        let text = p.to_toml().map_err(std::io::Error::other)?;
        if path.exists() {
            let _ = std::fs::copy(&path, path.with_extension("toml.bak"));
        }
        atomic_write(&path, text.as_bytes())
    }

    pub fn delete(&self, id: &str) -> std::io::Result<()> {
        if !valid_id(id) || id == DEFAULT_ID {
            return Err(std::io::Error::other("the default profile cannot be deleted"));
        }
        let _l = self.lock()?;
        let path = self.path_for(id);
        std::fs::rename(&path, path.with_extension("toml.deleted"))
    }

    pub fn load_settings(&self) -> Settings {
        std::fs::read_to_string(self.dir.join("settings.toml")).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save_settings(&self, s: &Settings) -> std::io::Result<()> {
        let _l = self.lock()?;
        let t = toml::to_string_pretty(s).map_err(std::io::Error::other)?;
        atomic_write(&self.dir.join("settings.toml"), t.as_bytes())
    }

    /// Backup all profiles + settings into one TOML document (FR-407).
    pub fn backup(&self) -> Result<String, String> {
        let (profiles, _) = self.load_all();
        let mut t = toml::Table::new();
        t.insert("backup_version".into(), toml::Value::Integer(1));
        t.insert("created_at".into(), toml::Value::String(now_rfc3339()));
        let arr: Result<Vec<toml::Value>, String> = profiles.iter().map(|p| toml::Value::try_from(p).map_err(|e| e.to_string())).collect();
        t.insert("profile".into(), toml::Value::Array(arr?));
        toml::to_string_pretty(&t).map_err(|e| e.to_string())
    }
}

// ----------------------------------------------------------- import

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImportPreview {
    pub id: String,
    pub display_name: String,
    pub overwrites_existing: bool,
    pub bindings: usize,
    pub warnings: Vec<String>,
}

/// Parse an untrusted profile (TOML or JSON), validate, and neutralise
/// anything executable (FR-305, FR-406). Never writes anything.
pub fn import_untrusted(text: &str, store: &Store) -> Result<(Profile, ImportPreview), String> {
    if text.len() > MAX_PROFILE_BYTES {
        return Err("file too large".into());
    }
    let mut p: Profile = if text.trim_start().starts_with('{') {
        serde_json::from_str(text).map_err(|e| format!("JSON: {e}"))?
    } else {
        // migration: schema_version missing => assume 1
        let mut t: toml::Table = toml::from_str(text).map_err(|e| format!("TOML: {e}"))?;
        t.entry("schema_version").or_insert(toml::Value::Integer(1));
        if let Some(v) = t.get("schema_version").and_then(|v| v.as_integer()) {
            if v > SCHEMA_VERSION as i64 {
                return Err(format!("profile schema {v} is newer than this version supports ({SCHEMA_VERSION})"));
            }
        }
        toml::Value::Table(t).try_into().map_err(|e: toml::de::Error| e.to_string())?
    };
    let mut warnings = Vec::new();
    for b in p.bindings.values_mut() {
        for a in [Some(&mut b.tap), b.hold.as_mut(), b.double.as_mut()].into_iter().flatten() {
            if a.sanitize_import() {
                warnings.push("contains command actions; they were disabled and must be re-confirmed".into());
            }
        }
    }
    if !p.extra.is_empty() {
        warnings.push(format!("unknown fields preserved: {}", p.extra.keys().cloned().collect::<Vec<_>>().join(", ")));
    }
    p.validate()?;
    warnings.dedup();
    let preview = ImportPreview {
        id: p.id.clone(),
        display_name: p.display_name.clone(),
        overwrites_existing: store.path_for(&p.id).exists(),
        bindings: p.bindings.len(),
        warnings,
    };
    Ok((p, preview))
}

// ----------------------------------------------------------- selection

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectedBy {
    Manual,
    ApplicationDevice,
    Application,
    Device,
    Default,
}

pub fn select<'a>(profiles: &'a [Profile], manual: Option<&str>, app_id: &str, title: &str, usb_id: Option<&str>) -> (&'a Profile, SelectedBy) {
    select_window(profiles, manual, app_id, title, None, usb_id)
}

/// As [`select`], with the focused window's executable (`/proc/<pid>/exe`) when known.
pub fn select_window<'a>(
    profiles: &'a [Profile],
    manual: Option<&str>,
    app_id: &str,
    title: &str,
    exe: Option<&str>,
    usb_id: Option<&str>,
) -> (&'a Profile, SelectedBy) {
    let mut sorted: Vec<&Profile> = profiles.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(m) = manual {
        if let Some(p) = sorted.iter().find(|p| p.id == m) {
            return (p, SelectedBy::Manual);
        }
    }
    let app_match = |p: &&&Profile| p.scope == Scope::Application && p.app_selector.as_ref().map(|s| s.matches_window(app_id, title, exe)).unwrap_or(false);
    if let Some(p) = sorted.iter().filter(app_match).find(|p| p.device_selector.is_some() && p.matches_device(usb_id)) {
        return (p, SelectedBy::ApplicationDevice);
    }
    // a profile that also matches the window title (e.g. Onshape in a browser) is more specific
    // than one matching the application alone
    let spec = |p: &&&Profile| p.app_selector.as_ref().map(|s| s.title_specificity()).unwrap_or(0);
    let app_only: Vec<&&Profile> = sorted.iter().filter(app_match).filter(|p| p.device_selector.is_none()).collect();
    // most specific title condition wins; ties keep id order (the first one)
    let best = app_only.iter().fold(None::<&&&Profile>, |acc, p| match acc {
        Some(a) if spec(a) >= spec(p) => Some(a),
        _ => Some(p),
    });
    if let Some(p) = best {
        return (p, SelectedBy::Application);
    }
    if let Some(p) = sorted.iter().find(|p| p.scope == Scope::Device && p.device_selector.is_some() && p.matches_device(usb_id)) {
        return (p, SelectedBy::Device);
    }
    if let Some(p) = sorted.iter().find(|p| p.id == DEFAULT_ID) {
        return (p, SelectedBy::Default);
    }
    (sorted[0], SelectedBy::Default)
}

#[cfg(test)]
mod tests;
