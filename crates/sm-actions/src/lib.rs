//! Button and command engine (FR-300).
//!
//! Profiles bind logical buttons to [`Binding`]s. The [`ButtonEngine`] turns
//! physical press/release edges into [`Fire`] records according to these
//! deterministic timing rules (FR-303):
//!
//! * Only `tap` bound → fires `Press` on button-down and `Release` on
//!   button-up, so held keys behave like held keys and nothing is delayed.
//! * `hold` bound → the tap is deferred. Released before `hold_ms` → tap
//!   (`Pulse`); still down at `hold_ms` → hold action `Press`, then `Release`
//!   on button-up. Hold has priority once the threshold passes.
//! * `double` bound → after a short release the tap waits `double_ms` for a
//!   second press. A second press inside the window fires `double` (`Pulse`)
//!   and suppresses both taps.
//!
//! [`ButtonEngine::cancel_all`] releases everything currently pressed; the
//! caller invokes it on unplug, profile switch, lock and shutdown (FR-306).

pub mod keys;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_MACRO_STEPS: usize = 64;
pub const MAX_MACRO_DELAY_MS: u32 = 5_000;
pub const MAX_MACRO_TOTAL_MS: u32 = 30_000;
pub const MAX_EXEC_ARGS: usize = 32;
/// Radial menu: 2..=8 slices (one per 45°), no nested menus.
pub const MAX_RADIAL_ITEMS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

impl MouseButton {
    pub fn code(self) -> u16 {
        match self {
            MouseButton::Left => 0x110,
            MouseButton::Right => 0x111,
            MouseButton::Middle => 0x112,
            MouseButton::Back => 0x113,
            MouseButton::Forward => 0x114,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToggleTarget {
    Translation,
    Rotation,
    DominantAxis,
    /// Pause all motion output.
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Top,
    Bottom,
    Left,
    Right,
    Front,
    Back,
    Iso1,
    Iso2,
    Fit,
}

/// A step inside a macro. Deliberately cannot contain another macro
/// (structural recursion protection, FR-304).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MacroStep {
    Key { keys: String },
    KeyDown { keys: String },
    KeyUp { keys: String },
    Mouse { button: MouseButton },
    Scroll { dx: i32, dy: i32 },
    Delay { ms: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    #[default]
    None,
    /// Forward as libspnav button event to native 6-DOF apps (default).
    SpnavButton {
        button: u8,
    },
    /// Key chord, e.g. "ctrl+shift+z".
    Key {
        keys: String,
    },
    Mouse {
        button: MouseButton,
    },
    Scroll {
        dx: i32,
        dy: i32,
    },
    /// QuickView preset; resolved via the active profile's `views` table.
    View {
        view: View,
    },
    Toggle {
        target: ToggleTarget,
    },
    /// Multiply all motion gain (bounded to [0.05, 10]).
    Sensitivity {
        factor: f32,
    },
    SensitivityReset,
    ProfileSwitch {
        profile: String,
    },
    ProfileCycle,
    /// Launch an installed application by desktop-file id (no shell).
    Launch {
        desktop_id: String,
    },
    /// Run argv directly (never through a shell). Only executed when the
    /// user created/confirmed it locally (`confirmed`) AND exec is enabled
    /// in agent settings (FR-305). Imported profiles have it cleared.
    Exec {
        argv: Vec<String>,
        #[serde(default)]
        confirmed: bool,
    },
    Macro {
        steps: Vec<MacroStep>,
    },
    /// Show the button-assignment overlay (FR-308).
    ShowHelp,
    /// Radial menu (FR-302): hold the button, tilt or push the cap towards a
    /// slice, release to run it. Item 0 is at the top, then clockwise.
    Radial {
        items: Vec<RadialItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadialItem {
    pub label: String,
    pub action: Action,
}

/// Which slice a cap direction selects. `x` is right, `y` is away from the
/// user (up on screen). Returns `None` inside the dead zone.
pub fn radial_slice(x: f32, y: f32, n: usize, deadzone: f32) -> Option<usize> {
    if n == 0 || !x.is_finite() || !y.is_finite() || (x * x + y * y).sqrt() < deadzone {
        return None;
    }
    // clockwise angle from 12 o'clock, in [0, 2π)
    let a = x.atan2(y).rem_euclid(std::f32::consts::TAU);
    let w = std::f32::consts::TAU / n as f32;
    Some(((a + w / 2.0) / w) as usize % n)
}

impl Action {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Key { keys } => keys::parse_chord(keys).map(|_| ()),
            Action::Scroll { dx, dy } if dx.abs() > 100 || dy.abs() > 100 => Err("scroll step must be within ±100".into()),
            Action::Sensitivity { factor } if !factor.is_finite() || !(0.1..=10.0).contains(factor) => {
                Err("sensitivity factor must be within [0.1, 10]".into())
            }
            Action::ProfileSwitch { profile } if profile.is_empty() || profile.len() > 128 => Err("bad profile id".into()),
            Action::Launch { desktop_id } => {
                let ok = !desktop_id.is_empty() && desktop_id.len() <= 255 && desktop_id.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
                if ok {
                    Ok(())
                } else {
                    Err("desktop_id may contain only [A-Za-z0-9._-]".into())
                }
            }
            Action::Exec { argv, .. } => {
                if argv.is_empty() || argv.len() > MAX_EXEC_ARGS || argv.iter().any(|a| a.contains('\0') || a.len() > 4096) {
                    Err("exec argv invalid".into())
                } else {
                    Ok(())
                }
            }
            Action::Macro { steps } => validate_macro(steps),
            Action::Radial { items } => {
                if !(2..=MAX_RADIAL_ITEMS).contains(&items.len()) {
                    return Err(format!("a radial menu needs 2 to {MAX_RADIAL_ITEMS} items"));
                }
                for it in items {
                    if it.label.trim().is_empty() || it.label.chars().count() > 48 {
                        return Err("radial item labels must be 1 to 48 characters".into());
                    }
                    if matches!(it.action, Action::Radial { .. }) {
                        return Err("radial menus can't be nested".into());
                    }
                    it.action.validate()?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Whether this action injects synthetic desktop input.
    pub fn injects_input(&self) -> bool {
        matches!(self, Action::Key { .. } | Action::Mouse { .. } | Action::Scroll { .. } | Action::Macro { .. } | Action::View { .. })
    }

    /// Strip anything that must never arrive executable through an import.
    /// Returns true when a confirmed command was disabled.
    pub fn sanitize_import(&mut self) -> bool {
        match self {
            Action::Exec { confirmed, .. } => std::mem::replace(confirmed, false),
            Action::Radial { items } => items.iter_mut().fold(false, |acc, it| it.action.sanitize_import() | acc),
            _ => false,
        }
    }

    /// Whether press/release semantics apply (vs. a one-shot pulse).
    fn holdable(&self) -> bool {
        matches!(self, Action::Key { .. } | Action::Mouse { .. } | Action::SpnavButton { .. } | Action::Radial { .. })
    }
}

pub fn validate_macro(steps: &[MacroStep]) -> Result<(), String> {
    if steps.is_empty() || steps.len() > MAX_MACRO_STEPS {
        return Err(format!("macro must have 1..={MAX_MACRO_STEPS} steps"));
    }
    let mut total = 0u32;
    for s in steps {
        match s {
            MacroStep::Key { keys } | MacroStep::KeyDown { keys } | MacroStep::KeyUp { keys } => {
                keys::parse_chord(keys)?;
            }
            MacroStep::Delay { ms } => {
                if *ms > MAX_MACRO_DELAY_MS {
                    return Err(format!("macro delay over {MAX_MACRO_DELAY_MS} ms"));
                }
                total += ms;
            }
            MacroStep::Scroll { dx, dy } if dx.abs() > 100 || dy.abs() > 100 => return Err("scroll step too large".into()),
            _ => {}
        }
    }
    if total > MAX_MACRO_TOTAL_MS {
        return Err("macro total delay too long".into());
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Binding {
    pub tap: Action,
    pub hold: Option<Action>,
    pub double: Option<Action>,
}

impl Binding {
    pub fn tap(a: Action) -> Self {
        Binding { tap: a, hold: None, double: None }
    }
    pub fn validate(&self) -> Result<(), String> {
        self.tap.validate()?;
        if let Some(h) = &self.hold {
            h.validate()?;
        }
        if let Some(d) = &self.double {
            d.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timing {
    pub hold_ms: u32,
    pub double_ms: u32,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { hold_ms: 450, double_ms: 250 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Press,
    Release,
    /// Press immediately followed by release (one-shot).
    Pulse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fire {
    pub button: u8,
    pub action: Action,
    pub phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum St {
    #[default]
    Idle,
    /// Down, tap fired as Press (simple binding).
    DownImmediate,
    /// Down, waiting to decide tap vs hold.
    DownPending { since: u64 },
    /// Hold action pressed.
    DownHeld,
    /// Released once; waiting for a possible second press.
    WaitDouble { since: u64 },
    /// Second press consumed by double; ignore until release.
    DownSecond,
}

#[derive(Debug, Clone, Default)]
pub struct ButtonEngine {
    bindings: BTreeMap<u8, Binding>,
    timing: Timing,
    st: BTreeMap<u8, St>,
}

impl ButtonEngine {
    pub fn new(bindings: BTreeMap<u8, Binding>, timing: Timing) -> Self {
        ButtonEngine { bindings, timing, st: BTreeMap::new() }
    }

    /// Replace bindings atomically; releases anything held first (FR-404).
    pub fn set_bindings(&mut self, bindings: BTreeMap<u8, Binding>, timing: Timing) -> Vec<Fire> {
        let out = self.cancel_all();
        self.bindings = bindings;
        self.timing = timing;
        out
    }

    fn binding(&self, b: u8) -> Binding {
        // Unbound buttons default to forwarding to native apps.
        self.bindings.get(&b).cloned().unwrap_or(Binding::tap(Action::SpnavButton { button: b }))
    }

    fn fire(button: u8, action: &Action, phase: Phase) -> Fire {
        let phase = if phase == Phase::Press && !action.holdable() { Phase::Pulse } else { phase };
        Fire { button, action: action.clone(), phase }
    }

    pub fn on_button(&mut self, b: u8, pressed: bool, t_us: u64) -> Vec<Fire> {
        let bind = self.binding(b);
        let st = self.st.get(&b).copied().unwrap_or_default();
        let mut out = Vec::new();
        let complex = bind.hold.is_some() || bind.double.is_some();
        let next = match (st, pressed) {
            (St::Idle, true) if !complex => {
                if bind.tap != Action::None {
                    out.push(Self::fire(b, &bind.tap, Phase::Press));
                }
                St::DownImmediate
            }
            (St::Idle, true) => St::DownPending { since: t_us },
            (St::DownImmediate, false) => {
                if bind.tap.holdable() && bind.tap != Action::None {
                    out.push(Fire { button: b, action: bind.tap.clone(), phase: Phase::Release });
                }
                St::Idle
            }
            (St::DownPending { .. }, false) => {
                if bind.double.is_some() {
                    St::WaitDouble { since: t_us }
                } else {
                    out.push(Self::fire(b, &bind.tap, Phase::Pulse));
                    St::Idle
                }
            }
            (St::DownHeld, false) => {
                if let Some(h) = &bind.hold {
                    if h.holdable() {
                        out.push(Fire { button: b, action: h.clone(), phase: Phase::Release });
                    }
                }
                St::Idle
            }
            (St::WaitDouble { .. }, true) => {
                if let Some(d) = &bind.double {
                    out.push(Self::fire(b, d, Phase::Pulse));
                }
                St::DownSecond
            }
            (St::DownSecond, false) => St::Idle,
            // duplicate edges (e.g. after SYN_DROPPED) are ignored
            (s, _) => s,
        };
        out.retain(|f| f.action != Action::None);
        self.st.insert(b, next);
        out
    }

    /// Advance timers. Call at or after [`Self::next_deadline_us`].
    pub fn tick(&mut self, t_us: u64) -> Vec<Fire> {
        let mut out = Vec::new();
        let hold = self.timing.hold_ms as u64 * 1000;
        let dbl = self.timing.double_ms as u64 * 1000;
        let keys: Vec<u8> = self.st.keys().copied().collect();
        for b in keys {
            let bind = self.binding(b);
            let st = self.st[&b];
            let next = match st {
                St::DownPending { since } if t_us.saturating_sub(since) >= hold => match &bind.hold {
                    Some(h) => {
                        out.push(Self::fire(b, h, Phase::Press));
                        St::DownHeld
                    }
                    None => st,
                },
                St::WaitDouble { since } if t_us.saturating_sub(since) >= dbl => {
                    out.push(Self::fire(b, &bind.tap, Phase::Pulse));
                    St::Idle
                }
                s => s,
            };
            self.st.insert(b, next);
        }
        out.retain(|f| f.action != Action::None);
        out
    }

    pub fn next_deadline_us(&self, now: u64) -> Option<u64> {
        let hold = self.timing.hold_ms as u64 * 1000;
        let dbl = self.timing.double_ms as u64 * 1000;
        self.st
            .iter()
            .filter_map(|(b, s)| match s {
                St::DownPending { since } if self.bindings.get(b).map(|x| x.hold.is_some()).unwrap_or(false) => Some((since + hold).saturating_sub(now)),
                St::WaitDouble { since } => Some((since + dbl).saturating_sub(now)),
                _ => None,
            })
            .min()
    }

    /// Release everything held and drop pending decisions (FR-306).
    pub fn cancel_all(&mut self) -> Vec<Fire> {
        let mut out = Vec::new();
        let st = std::mem::take(&mut self.st);
        for (b, s) in st {
            let bind = self.binding(b);
            match s {
                St::DownImmediate if bind.tap.holdable() => out.push(Fire { button: b, action: bind.tap.clone(), phase: Phase::Release }),
                St::DownHeld => {
                    if let Some(h) = bind.hold.filter(|h| h.holdable()) {
                        out.push(Fire { button: b, action: h, phase: Phase::Release })
                    }
                }
                _ => {}
            }
        }
        out.retain(|f| f.action != Action::None);
        out
    }

    pub fn is_idle(&self) -> bool {
        self.st.values().all(|s| *s == St::Idle)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn radial_slices_are_clockwise_from_top() {
        // 4 slices: up, right, down, left
        assert_eq!(radial_slice(0.0, 0.9, 4, 0.3), Some(0));
        assert_eq!(radial_slice(0.9, 0.0, 4, 0.3), Some(1));
        assert_eq!(radial_slice(0.0, -0.9, 4, 0.3), Some(2));
        assert_eq!(radial_slice(-0.9, 0.0, 4, 0.3), Some(3));
        // 8 slices: up-right diagonal is slice 1, just left of top is still 0
        assert_eq!(radial_slice(0.6, 0.6, 8, 0.3), Some(1));
        assert_eq!(radial_slice(-0.1, 0.9, 8, 0.3), Some(0));
        assert_eq!(radial_slice(-0.6, 0.6, 8, 0.3), Some(7));
        assert_eq!(radial_slice(0.1, 0.1, 8, 0.3), None);
        assert_eq!(radial_slice(f32::NAN, 1.0, 8, 0.3), None);
        assert_eq!(radial_slice(1.0, 0.0, 0, 0.3), None);
    }

    #[test]
    fn radial_validation() {
        let it = |l: &str, a: Action| RadialItem { label: l.into(), action: a };
        assert!(Action::Radial { items: vec![it("a", key("a")), it("b", key("b"))] }.validate().is_ok());
        assert!(Action::Radial { items: vec![it("a", key("a"))] }.validate().is_err());
        assert!(Action::Radial { items: (0..9).map(|i| it(&i.to_string(), key("a"))).collect() }.validate().is_err());
        assert!(Action::Radial { items: vec![it("", key("a")), it("b", key("b"))] }.validate().is_err());
        assert!(Action::Radial { items: vec![it("a", key("not-a-key")), it("b", key("b"))] }.validate().is_err());
        let inner = Action::Radial { items: vec![it("a", key("a")), it("b", key("b"))] };
        assert!(Action::Radial { items: vec![it("a", inner), it("b", key("b"))] }.validate().is_err());
        // holding the button keeps the menu open
        let mut e = ButtonEngine::new(
            [(0u8, Binding { tap: Action::Radial { items: vec![it("a", key("a")), it("b", key("b"))] }, hold: None, double: None })].into_iter().collect(),
            Timing::default(),
        );
        assert_eq!(e.on_button(0, true, 0)[0].phase, Phase::Press);
        assert_eq!(e.on_button(0, false, 2_000 * MS)[0].phase, Phase::Release);
    }

    use super::*;

    const MS: u64 = 1000;

    fn key(k: &str) -> Action {
        Action::Key { keys: k.into() }
    }

    #[test]
    fn unbound_buttons_forward_to_spnav() {
        let mut e = ButtonEngine::default();
        let f = e.on_button(1, true, 0);
        assert_eq!(f, vec![Fire { button: 1, action: Action::SpnavButton { button: 1 }, phase: Phase::Press }]);
        let f = e.on_button(1, false, 10);
        assert_eq!(f[0].phase, Phase::Release);
    }

    #[test]
    fn simple_key_holds_like_a_key() {
        let mut m = BTreeMap::new();
        m.insert(0, Binding::tap(key("shift")));
        let mut e = ButtonEngine::new(m, Timing::default());
        assert_eq!(e.on_button(0, true, 0)[0].phase, Phase::Press);
        assert!(e.tick(5_000 * MS).is_empty(), "no timers for simple binding");
        assert_eq!(e.on_button(0, false, 5_000 * MS)[0].phase, Phase::Release);
    }

    #[test]
    fn tap_vs_hold() {
        let mut m = BTreeMap::new();
        m.insert(0, Binding { tap: key("a"), hold: Some(key("b")), double: None });
        let mut e = ButtonEngine::new(m, Timing::default());
        assert!(e.on_button(0, true, 0).is_empty());
        let f = e.on_button(0, false, 100 * MS);
        assert_eq!(f, vec![Fire { button: 0, action: key("a"), phase: Phase::Pulse }]);
        // hold
        assert!(e.on_button(0, true, 1000 * MS).is_empty());
        assert_eq!(e.next_deadline_us(1000 * MS), Some(450 * MS));
        let f = e.tick(1450 * MS);
        assert_eq!(f, vec![Fire { button: 0, action: key("b"), phase: Phase::Press }]);
        let f = e.on_button(0, false, 2000 * MS);
        assert_eq!(f, vec![Fire { button: 0, action: key("b"), phase: Phase::Release }]);
        assert!(e.is_idle());
    }

    #[test]
    fn double_press() {
        let mut m = BTreeMap::new();
        m.insert(0, Binding { tap: key("a"), hold: None, double: Some(key("c")) });
        let mut e = ButtonEngine::new(m, Timing::default());
        e.on_button(0, true, 0);
        assert!(e.on_button(0, false, 50 * MS).is_empty());
        let f = e.on_button(0, true, 150 * MS);
        assert_eq!(f[0].action, key("c"));
        assert!(e.on_button(0, false, 200 * MS).is_empty());
        // single tap times out into tap
        e.on_button(0, true, 1000 * MS);
        e.on_button(0, false, 1050 * MS);
        assert!(e.tick(1200 * MS).is_empty());
        assert_eq!(e.tick(1300 * MS)[0].action, key("a"));
    }

    #[test]
    fn cancel_releases_held_keys() {
        let mut m = BTreeMap::new();
        m.insert(0, Binding::tap(key("ctrl")));
        m.insert(1, Binding { tap: Action::None, hold: Some(key("shift")), double: None });
        let mut e = ButtonEngine::new(m, Timing::default());
        e.on_button(0, true, 0);
        e.on_button(1, true, 0);
        e.tick(500 * MS);
        let f = e.cancel_all();
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|f| f.phase == Phase::Release));
        assert!(e.is_idle());
        // release after cancel does nothing
        assert!(e.on_button(0, false, 600 * MS).is_empty());
    }

    #[test]
    fn non_holdable_actions_pulse() {
        let mut m = BTreeMap::new();
        m.insert(0, Binding::tap(Action::ProfileCycle));
        let mut e = ButtonEngine::new(m, Timing::default());
        assert_eq!(e.on_button(0, true, 0)[0].phase, Phase::Pulse);
        assert!(e.on_button(0, false, 1).is_empty());
    }

    #[test]
    fn validation_limits() {
        assert!(key("ctrl+z").validate().is_ok());
        assert!(key("ctrl+nope").validate().is_err());
        let long = vec![MacroStep::Delay { ms: 10 }; MAX_MACRO_STEPS + 1];
        assert!(Action::Macro { steps: long }.validate().is_err());
        assert!(Action::Macro { steps: vec![MacroStep::Delay { ms: 6000 }] }.validate().is_err());
        assert!(Action::Launch { desktop_id: "org.blender.Blender".into() }.validate().is_ok());
        assert!(Action::Launch { desktop_id: "x; rm -rf /".into() }.validate().is_err());
        let mut r = Action::Radial {
            items: vec![
                RadialItem { label: "run".into(), action: Action::Exec { argv: vec!["x".into()], confirmed: true } },
                RadialItem { label: "undo".into(), action: key("ctrl+z") },
            ],
        };
        assert!(r.sanitize_import(), "a confirmed command inside a radial menu must be reported");
        assert!(matches!(&r, Action::Radial { items } if items[0].action == Action::Exec { argv: vec!["x".into()], confirmed: false }));
        let mut ex = Action::Exec { argv: vec!["notify-send".into(), "hi".into()], confirmed: true };
        ex.sanitize_import();
        assert_eq!(ex, Action::Exec { argv: vec!["notify-send".into(), "hi".into()], confirmed: false });
    }

    #[test]
    fn serde_roundtrip() {
        let b = Binding { tap: key("ctrl+s"), hold: Some(Action::View { view: View::Top }), double: None };
        let t = serde_json::to_string(&b).unwrap();
        let back: Binding = serde_json::from_str(&t).unwrap();
        assert_eq!(b, back);
    }
}
