//! `3dmouse-agent` – per-user session agent (SRS §4.1).
//!
//! Owns everything that depends on the graphical session: focus tracking,
//! profile selection, the button/macro engine, desktop emulation, lock
//! detection and the GUI/CLI API. Talks to the system broker over its
//! control socket; never touches device nodes itself.
//! See docs/architecture/overview.md and docs/adr/0002-broker-agent-split.md.

use serde_json::{json, Value};
use sm_actions::{keys, Action, ButtonEngine, Fire, MacroStep, Phase, RadialItem, ToggleTarget};
use sm_core::AppMode;
use sm_desktop::focus::{self, FocusInfo, ProviderKind};
use sm_ipc::{LineBuf, ReadOutcome, WriteBuf};
use sm_profile::{MotionStyle, Profile, SelectedBy, Settings, Store};
use std::collections::{BTreeMap, VecDeque};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BROKER_RETRY_US: u64 = 2_000_000;
const LOCK_POLL_US: u64 = 1_000_000;
const STATUS_POLL_US: u64 = 3_000_000;
const LIVE_MAX_HZ: u64 = 30;
const MAX_CLIENTS: usize = 16;
const MAX_SCROLL_DT_US: u64 = 50_000;

macro_rules! log {
    ($lvl:literal, $($t:tt)*) => {{
        let p = match $lvl { "err" => 3, "warn" => 4, "info" => 6, _ => 7 };
        if p < 7 || debug_enabled() {
            eprintln!("<{}>{}", p, format!($($t)*));
        }
    }};
}

static DEBUG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn debug_enabled() -> bool {
    DEBUG.load(std::sync::atomic::Ordering::Relaxed)
}

fn now() -> u64 {
    sm_core::monotonic_us()
}

fn which(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| PathBuf::from(d).join(cmd)).find(|p| p.is_file())
}

// ------------------------------------------------------------- broker link

struct BrokerConn {
    s: UnixStream,
    lb: LineBuf,
    wb: WriteBuf,
    next_id: u64,
    /// request id -> purpose, for replies we care about
    pending: BTreeMap<u64, &'static str>,
    can_control: bool,
    motion_sub: Option<bool>,
}

impl BrokerConn {
    fn send(&mut self, cmd: &str, args: Value, purpose: &'static str) {
        let id = self.next_id;
        self.next_id += 1;
        let mut v = if args.is_object() { args } else { json!({}) };
        v["v"] = sm_ipc::PROTO_VERSION.into();
        v["id"] = id.into();
        v["cmd"] = cmd.into();
        self.pending.insert(id, purpose);
        self.wb.push_line(&v);
    }
}

// ------------------------------------------------------------- GUI/CLI clients

struct Client {
    id: u64,
    s: UnixStream,
    lb: LineBuf,
    wb: WriteBuf,
    live: bool,
    events: bool,
    /// a registered on-screen overlay (3dmouse-osd) that renders OSD and radial events
    osd: bool,
    last_live_us: u64,
    dead: bool,
}

impl Client {
    fn send(&mut self, v: &Value) {
        if !self.wb.push_line(v) {
            self.dead = true;
        }
    }
}

/// An open radial menu: the button is held, the cap direction picks a slice.
struct RadialState {
    items: Vec<RadialItem>,
    hover: Option<usize>,
}

/// Cap deflection (normalised) needed to pick a radial slice.
const RADIAL_DEADZONE: f32 = 0.35;

/// Session-only adjustments made by buttons (toggle/sensitivity actions).
#[derive(Debug, Clone, Default, serde::Serialize)]
struct Overrides {
    sensitivity: Option<f32>,
    translation: Option<bool>,
    rotation: Option<bool>,
    dominant: Option<bool>,
    motion_paused: bool,
}

struct ScrollState {
    last_t: u64,
    carry: (f32, f32),
    norm: [f32; 6],
    /// pointer mode: sub-pixel remainder
    ptr: (f32, f32),
    /// keys mode: per axis gesture, the held direction and the next repeat time
    gestures: Vec<Gesture>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Gesture {
    dir: i8,
    next_us: u64,
}

struct Agent {
    store: Store,
    settings: Settings,
    profiles: Vec<Profile>,
    load_errors: Vec<String>,
    active: Profile,
    selected_by: SelectedBy,
    session_manual: Option<String>,
    overrides: Overrides,
    engine: ButtonEngine,
    focus: FocusInfo,
    desktop: focus::DesktopInfo,
    focus_rx: Receiver<FocusInfo>,
    wake_r: RawFd,
    broker: Option<BrokerConn>,
    broker_path: PathBuf,
    broker_err: String,
    broker_retry_at: u64,
    broker_status: Value,
    listener: UnixListener,
    socket_path: PathBuf,
    clients: Vec<Client>,
    next_client: u64,
    locked: bool,
    lock_reason: String,
    next_lock_poll: u64,
    next_status_poll: u64,
    macros: VecDeque<(u64, MacroStep)>,
    macro_held: Vec<u16>,
    scroll: ScrollState,
    children: Vec<Child>,
    last_fired: Option<Value>,
    /// focus reports pushed by the GNOME Shell extension
    pushed_focus: u64,
    radial: Option<RadialState>,
    sigfd: RawFd,
    quit: bool,
}

fn make_listener(p: &PathBuf) -> std::io::Result<UnixListener> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
    }
    if p.exists() {
        if UnixStream::connect(p).is_ok() {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, format!("{} is served by another agent", p.display())));
        }
        std::fs::remove_file(p)?;
    }
    let l = UnixListener::bind(p)?;
    l.set_nonblocking(true)?;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
    Ok(l)
}

fn setup_signals() -> RawFd {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::sigaddset(&mut set, s);
        }
        libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        libc::signalfd(-1, &set, libc::SFD_NONBLOCK | libc::SFD_CLOEXEC)
    }
}

impl Agent {
    fn new() -> std::io::Result<Agent> {
        // Block the signals before any thread exists: threads inherit the mask, and a thread
        // that left SIGHUP unblocked would take its default action (exit) instead of a reload.
        let sigfd = setup_signals();
        let store = Store::default_location();
        if let Err(e) = store.ensure_presets() {
            log!("warn", "cannot create preset profiles in {}: {e}", store.dir.display());
        }
        let settings = store.load_settings();
        let (profiles, rep) = store.load_all();
        for q in &rep.quarantined {
            log!("warn", "quarantined unreadable profile: {q}");
        }
        let socket_path = sm_ipc::agent_socket();
        let listener = make_listener(&socket_path)?;
        let (tx, rx) = channel();
        let mut pipe = [0; 2];
        unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) };
        let desktop = focus::detect();
        log!("info", "desktop: {} / {} – focus provider {:?} ({})", desktop.session_type, desktop.current_desktop, desktop.provider, desktop.provider_note);
        focus::spawn(desktop.provider, tx, pipe[1]);
        let default = profiles.iter().find(|p| p.id == sm_profile::DEFAULT_ID).cloned().unwrap_or_else(|| sm_profile::presets().remove(0));
        let mut a = Agent {
            store,
            settings,
            profiles,
            load_errors: rep.errors.into_iter().chain(rep.quarantined.into_iter().map(|q| format!("quarantined {q}"))).collect(),
            engine: ButtonEngine::new(default.binding_map(), default.timing),
            active: default,
            selected_by: SelectedBy::Default,
            session_manual: None,
            overrides: Overrides::default(),
            focus: FocusInfo::default(),
            desktop,
            focus_rx: rx,
            wake_r: pipe[0],
            broker: None,
            broker_path: sm_ipc::broker_socket(),
            broker_err: "not connected yet".into(),
            broker_retry_at: 0,
            broker_status: Value::Null,
            listener,
            socket_path,
            clients: Vec::new(),
            next_client: 1,
            locked: false,
            lock_reason: String::new(),
            next_lock_poll: 0,
            next_status_poll: 0,
            macros: VecDeque::new(),
            macro_held: Vec::new(),
            scroll: ScrollState { last_t: 0, carry: (0.0, 0.0), norm: [0.0; 6], ptr: (0.0, 0.0), gestures: Vec::new() },
            children: Vec::new(),
            last_fired: None,
            pushed_focus: 0,
            radial: None,
            sigfd,
            quit: false,
        };
        a.reselect(true);
        Ok(a)
    }

    // --------------------------------------------------------- profiles

    fn device_usb_id(&self) -> Option<String> {
        self.broker_status["devices"].as_array()?.iter().find(|d| d["state"] == "ready")?["usb_id"].as_str().map(String::from)
    }

    fn manual(&self) -> Option<String> {
        self.session_manual.clone().or_else(|| {
            if self.settings.auto_switch {
                self.settings.manual_profile.clone()
            } else {
                self.settings.manual_profile.clone().or(Some(sm_profile::DEFAULT_ID.into()))
            }
        })
    }

    /// Re-run selection; push config to the broker if anything changed.
    fn reselect(&mut self, force: bool) {
        let usb = self.device_usb_id();
        let manual = self.manual();
        // the focused window's process image, for executable selectors (FR-402); only readable
        // for the user's own processes, which is all the agent needs
        let exe = self.focus.pid.and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok()).and_then(|p| p.to_str().map(str::to_owned));
        let (p, by) = sm_profile::select_window(&self.profiles, manual.as_deref(), &self.focus.app_id, &self.focus.title, exe.as_deref(), usb.as_deref());
        let changed = p != &self.active;
        if changed || force {
            let switched = p.id != self.active.id;
            let p = p.clone();
            if switched {
                log!("info", "profile -> {} ({:?}; app {:?})", p.id, by, self.focus.app_id);
                self.overrides = Overrides::default();
                self.osd(&format!("Profile: {}", p.display_name), &p);
                if p.native_app_mode == AppMode::VirtualJoystick && !self.settings.allow_virtual_joystick {
                    self.osd("The virtual joystick is off: allow it in Settings", &p);
                }
                self.scroll.gestures.clear();
            }
            // a profile switch cancels an open radial menu instead of running a slice
            self.radial_close(true);
            // switching profiles never leaves keys held (FR-404)
            let fires = self.engine.set_bindings(p.binding_map(), p.timing);
            self.active = p;
            self.selected_by = by;
            for f in fires {
                self.execute(f);
            }
            self.release_macro_keys();
            self.push_config();
            self.broadcast_event("profile", json!({"profile": self.active.id, "display_name": self.active.display_name, "selected_by": self.selected_by}));
        }
        self.selected_by = by;
    }

    fn effective_nav(&self) -> sm_pipeline::NavConfig {
        let mut n = self.active.navigation.clone();
        if let Some(s) = self.overrides.sensitivity {
            n.master_gain = (n.master_gain * s).clamp(0.05, 10.0);
        }
        if let Some(t) = self.overrides.translation {
            n.translation_enabled = t;
        }
        if let Some(r) = self.overrides.rotation {
            n.rotation_enabled = r;
        }
        if let Some(d) = self.overrides.dominant {
            n.dominant_axis = d;
        }
        n
    }

    fn effective_mode(&self) -> AppMode {
        // an open radial menu freezes the application's view while the cap picks a slice
        if self.overrides.motion_paused || self.radial.is_some() {
            AppMode::None
        } else if self.active.native_app_mode == AppMode::VirtualJoystick && !self.settings.allow_virtual_joystick {
            // game mode is opt-in (FR-507): without permission the profile just ignores motion
            AppMode::None
        } else {
            self.active.native_app_mode
        }
    }

    /// Device button -> spnav button, for buttons that are plain spnav
    /// forwards. Everything else is handled by the engine here.
    fn spnav_button_map(&self) -> BTreeMap<String, u8> {
        let binds = self.active.binding_map();
        let mut m = BTreeMap::new();
        for b in 0..sm_core::MAX_BUTTONS as u8 {
            match binds.get(&b) {
                None => {
                    m.insert(b.to_string(), b);
                }
                Some(bd) if bd.hold.is_none() && bd.double.is_none() => {
                    if let Action::SpnavButton { button } = bd.tap {
                        m.insert(b.to_string(), button);
                    }
                }
                _ => {}
            }
        }
        m
    }

    fn push_config(&mut self) {
        let msg = json!({
            "nav": self.effective_nav(),
            "app_mode": self.effective_mode(),
            "spnav_buttons": self.spnav_button_map(),
            "profile": self.active.id,
        });
        let want_motion = self.want_motion();
        if let Some(b) = &mut self.broker {
            b.send("set_config", msg, "set_config");
            // refresh the cached broker status so `status` reflects the new config at once
            b.send("status", json!({}), "status");
            if b.motion_sub != Some(want_motion) {
                b.motion_sub = Some(want_motion);
                b.send("subscribe", json!({"motion": want_motion, "max_hz": 60, "buttons": true, "devices": true}), "subscribe");
            }
        }
    }

    fn want_motion(&self) -> bool {
        self.effective_mode() == AppMode::DesktopEmulation || self.radial.is_some() || self.clients.iter().any(|c| c.live)
    }

    fn osd(&mut self, text: &str, p: &Profile) {
        if !(self.settings.osd_enabled && p.osd.enabled) {
            return;
        }
        self.broadcast_event("osd", json!({"text": text, "timeout_ms": p.osd.timeout_ms}));
        if self.has_overlay() {
            return; // the overlay renders it; no duplicate notification
        }
        if let Some(ns) = which("notify-send") {
            let c = Command::new(ns)
                .args(["-a", "3DMouse Linux", "-t", &p.osd.timeout_ms.to_string(), "-h", "string:x-canonical-private-synchronous:3dmouse", text])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(c) = c {
                self.children.push(c);
            }
        }
    }

    fn has_overlay(&self) -> bool {
        self.clients.iter().any(|c| c.osd && !c.dead)
    }

    fn radial_open(&mut self, items: Vec<RadialItem>) {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        self.broadcast_event("radial", json!({"state": "open", "items": labels, "hover": null}));
        if !self.has_overlay() {
            self.osd(&format!("Radial menu: push the cap towards {}", labels.join(" · ")), &self.active.clone());
        }
        self.radial = Some(RadialState { items, hover: None });
        self.push_config();
    }

    fn radial_motion(&mut self, norm: [f32; 6]) {
        // push right / roll right = right; push away / tilt forward (−RX) = up
        let x = norm[0] + norm[5];
        let y = norm[2] - norm[3];
        let Some(r) = &mut self.radial else { return };
        // leaving the dead zone keeps the last slice, so letting go of the cap before the button still works
        let Some(i) = sm_actions::radial_slice(x, y, r.items.len(), RADIAL_DEADZONE) else { return };
        if r.hover == Some(i) {
            return;
        }
        r.hover = Some(i);
        let label = r.items[i].label.clone();
        self.broadcast_event("radial", json!({"state": "hover", "hover": i}));
        if !self.has_overlay() {
            self.osd(&format!("→ {label}"), &self.active.clone());
        }
    }

    /// Close the menu; run the chosen item unless `cancel`.
    fn radial_close(&mut self, cancel: bool) {
        let Some(r) = self.radial.take() else { return };
        let chosen = if cancel { None } else { r.hover };
        self.broadcast_event("radial", json!({"state": "close", "chosen": chosen}));
        self.push_config();
        if let Some(i) = chosen {
            let item = &r.items[i];
            log!("debug", "radial: {}", item.label);
            self.execute(Fire { button: u8::MAX, action: item.action.clone(), phase: Phase::Pulse });
        }
    }

    fn reload_profiles(&mut self) {
        let (p, rep) = self.store.load_all();
        self.profiles = p;
        self.load_errors = rep.errors.into_iter().chain(rep.quarantined.into_iter().map(|q| format!("quarantined {q}"))).collect();
        self.reselect(false);
    }

    // --------------------------------------------------------- actions

    fn inject(&mut self, args: Value) {
        if self.locked {
            return;
        }
        if let Some(b) = &mut self.broker {
            b.send("inject", args, "inject");
        }
    }

    fn inject_keys(&mut self, codes: &[u16], phase: Phase) {
        let down = match phase {
            Phase::Press => Some(true),
            Phase::Release => Some(false),
            Phase::Pulse => None,
        };
        let mut a = json!({"keys": codes});
        if let Some(d) = down {
            a["down"] = json!(d);
        }
        self.inject(a);
    }

    fn execute(&mut self, f: Fire) {
        if self.locked && f.phase != Phase::Release {
            return;
        }
        log!("debug", "button {} {:?}: {:?}", f.button, f.phase, f.action);
        self.last_fired = Some(json!({"button": f.button, "phase": format!("{:?}", f.phase), "action": f.action}));
        match &f.action {
            Action::None | Action::SpnavButton { .. } => {} // forwarded by the broker
            Action::Key { keys: k } => match keys::parse_chord(k) {
                Ok(codes) => self.inject_keys(&codes, f.phase),
                Err(e) => log!("warn", "bad key chord {k:?}: {e}"),
            },
            Action::Mouse { button } => self.inject_keys(&[button.code()], f.phase),
            Action::Scroll { dx, dy } => {
                if f.phase != Phase::Release {
                    self.inject(json!({"scroll": {"v": dy * 120, "h": dx * 120}}));
                }
            }
            Action::View { view } => {
                let name = sm_profile::view_name(*view);
                match self.active.views.get(name).cloned() {
                    Some(chord) => match keys::parse_chord(&chord) {
                        Ok(codes) => self.inject_keys(&codes, Phase::Pulse),
                        Err(e) => log!("warn", "view {name}: {e}"),
                    },
                    None => self.osd(&format!("No '{name}' view defined for {}", self.active.display_name), &self.active.clone()),
                }
            }
            Action::Toggle { target } => {
                let n = self.effective_nav();
                match target {
                    ToggleTarget::Translation => self.overrides.translation = Some(!n.translation_enabled),
                    ToggleTarget::Rotation => self.overrides.rotation = Some(!n.rotation_enabled),
                    ToggleTarget::DominantAxis => self.overrides.dominant = Some(!n.dominant_axis),
                    ToggleTarget::Motion => self.overrides.motion_paused = !self.overrides.motion_paused,
                }
                let t = format!(
                    "{target:?}: {}",
                    match target {
                        ToggleTarget::Translation => on_off(self.overrides.translation.unwrap()),
                        ToggleTarget::Rotation => on_off(self.overrides.rotation.unwrap()),
                        ToggleTarget::DominantAxis => on_off(self.overrides.dominant.unwrap()),
                        ToggleTarget::Motion => on_off(!self.overrides.motion_paused),
                    }
                );
                self.osd(&t, &self.active.clone());
                self.push_config();
            }
            Action::Sensitivity { factor } => {
                let s = (self.overrides.sensitivity.unwrap_or(1.0) * factor).clamp(0.05, 10.0);
                self.overrides.sensitivity = Some(s);
                self.osd(&format!("Sensitivity ×{s:.2}"), &self.active.clone());
                self.push_config();
            }
            Action::SensitivityReset => {
                self.overrides.sensitivity = None;
                self.osd("Sensitivity reset", &self.active.clone());
                self.push_config();
            }
            Action::ProfileSwitch { profile } => {
                if self.profiles.iter().any(|p| &p.id == profile) {
                    self.session_manual = Some(profile.clone());
                    self.reselect(false);
                } else {
                    log!("warn", "profile switch: no profile {profile:?}");
                }
            }
            Action::ProfileCycle => {
                let mut ids: Vec<String> = self.profiles.iter().map(|p| p.id.clone()).collect();
                ids.sort();
                let i = ids.iter().position(|i| *i == self.active.id).map(|i| (i + 1) % ids.len()).unwrap_or(0);
                if let Some(id) = ids.get(i) {
                    self.session_manual = Some(id.clone());
                    self.reselect(false);
                }
            }
            Action::Launch { desktop_id } => {
                if f.phase != Phase::Release {
                    self.launch(desktop_id.clone());
                }
            }
            Action::Exec { argv, confirmed } => {
                if f.phase == Phase::Release {
                    return;
                }
                if !self.settings.allow_exec || !confirmed {
                    log!("warn", "command action blocked (allow_exec={}, confirmed={confirmed})", self.settings.allow_exec);
                    self.osd("Command actions are disabled (enable in Settings)", &self.active.clone());
                    return;
                }
                self.spawn(&argv[0], &argv[1..]);
            }
            Action::Macro { steps } => {
                if f.phase != Phase::Release {
                    let mut t = now();
                    for s in steps {
                        if let MacroStep::Delay { ms } = s {
                            t += *ms as u64 * 1000;
                        }
                        self.macros.push_back((t, s.clone()));
                    }
                }
            }
            Action::ShowHelp => {
                if f.phase != Phase::Release {
                    let text = self.help_text();
                    self.osd(&text, &self.active.clone());
                }
            }
            Action::Radial { items } => match f.phase {
                Phase::Press => self.radial_open(items.clone()),
                // a release while locked closes without running anything
                Phase::Release => self.radial_close(self.locked),
                // a tap resolved on release (the button also has hold/double actions) can't hold a menu open
                Phase::Pulse => self.osd("A radial menu needs a button without hold or double-press actions", &self.active.clone()),
            },
        }
    }

    fn help_text(&self) -> String {
        let labels: Vec<(u8, String)> = self.broker_status["devices"]
            .as_array()
            .and_then(|d| d.first())
            .and_then(|d| d["buttons"].as_array())
            .map(|b| b.iter().filter_map(|b| Some((b["index"].as_u64()? as u8, b["label"].as_str()?.to_string()))).collect())
            .unwrap_or_default();
        let binds = self.active.binding_map();
        let mut out = format!("{} buttons:", self.active.display_name);
        for (i, l) in labels {
            let a = binds.get(&i).map(|b| describe(&b.tap)).unwrap_or_else(|| format!("app button {}", i + 1));
            out += &format!("\n{l}: {a}");
        }
        out
    }

    fn spawn(&mut self, prog: &str, args: &[String]) {
        use std::os::unix::process::CommandExt;
        let mut c = Command::new(prog);
        c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        match c.spawn() {
            Ok(ch) => self.children.push(ch),
            Err(e) => log!("warn", "cannot start {prog}: {e}"),
        }
    }

    /// Launch by desktop-file id without a shell (FR-305).
    fn launch(&mut self, id: String) {
        let id = id.trim_end_matches(".desktop").to_string();
        if let Some(gl) = which("gtk-launch") {
            return self.spawn(gl.to_str().unwrap_or("gtk-launch"), &[id]);
        }
        let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or("/usr/local/share:/usr/share".into());
        let home = std::env::var("XDG_DATA_HOME").unwrap_or(format!("{}/.local/share", std::env::var("HOME").unwrap_or_default()));
        for d in std::iter::once(home.as_str()).chain(dirs.split(':')) {
            let f = PathBuf::from(d).join("applications").join(format!("{id}.desktop"));
            if f.is_file() {
                if let Some(gio) = which("gio") {
                    return self.spawn(gio.to_str().unwrap_or("gio"), &["launch".into(), f.display().to_string()]);
                }
            }
        }
        log!("warn", "cannot launch {id}: need gtk-launch or gio and an installed {id}.desktop");
    }

    fn run_macros(&mut self) {
        let t = now();
        while let Some((due, _)) = self.macros.front() {
            if *due > t {
                break;
            }
            let (_, step) = self.macros.pop_front().unwrap();
            match step {
                MacroStep::Delay { .. } => {}
                MacroStep::Key { keys: k } => {
                    if let Ok(c) = keys::parse_chord(&k) {
                        self.inject_keys(&c, Phase::Pulse)
                    }
                }
                MacroStep::KeyDown { keys: k } => {
                    if let Ok(c) = keys::parse_chord(&k) {
                        self.macro_held.extend(&c);
                        self.inject_keys(&c, Phase::Press)
                    }
                }
                MacroStep::KeyUp { keys: k } => {
                    if let Ok(c) = keys::parse_chord(&k) {
                        self.macro_held.retain(|x| !c.contains(x));
                        self.inject_keys(&c, Phase::Release)
                    }
                }
                MacroStep::Mouse { button } => self.inject_keys(&[button.code()], Phase::Pulse),
                MacroStep::Scroll { dx, dy } => self.inject(json!({"scroll": {"v": dy * 120, "h": dx * 120}})),
            }
        }
        // a macro that ends with keys held releases them (FR-304)
        if self.macros.is_empty() && !self.macro_held.is_empty() {
            self.release_macro_keys();
        }
    }

    fn release_macro_keys(&mut self) {
        self.macros.clear();
        let held: Vec<u16> = std::mem::take(&mut self.macro_held);
        if !held.is_empty() {
            if let Some(b) = &mut self.broker {
                b.send("inject", json!({"keys": held, "down": false}), "inject");
            }
        }
    }

    // --------------------------------------------------------- desktop emulation

    fn desktop_motion(&mut self, norm: [f32; 6], t: u64) {
        let dt = t.saturating_sub(self.scroll.last_t).min(MAX_SCROLL_DT_US);
        self.scroll.last_t = t;
        self.scroll.norm = norm;
        if self.effective_mode() != AppMode::DesktopEmulation || self.locked {
            self.scroll.carry = (0.0, 0.0);
            self.scroll.ptr = (0.0, 0.0);
            self.scroll.gestures.clear();
            return;
        }
        let secs = dt as f32 / 1e6;
        match self.active.desktop.style {
            MotionStyle::Scroll => self.desktop_scroll(norm, secs),
            MotionStyle::Pointer => self.desktop_pointer(norm, secs),
            MotionStyle::Keys => self.desktop_gestures(t),
        }
    }

    fn desktop_scroll(&mut self, norm: [f32; 6], secs: f32) {
        let de = &self.active.desktop;
        let ax = |i: u8| norm.get(i as usize).copied().unwrap_or(0.0);
        // canonical frame: +Z is away from the user, so pushing forward
        // scrolls down unless inverted (docs/architecture/axis-frames.md)
        let mut v = -ax(de.vertical_axis) * de.scroll_speed * secs;
        let mut h = ax(de.horizontal_axis) * de.scroll_speed * secs;
        if de.invert_vertical {
            v = -v;
        }
        if de.invert_horizontal {
            h = -h;
        }
        let zoom = if de.zoom_axis < 6 { ax(de.zoom_axis) * de.scroll_speed * secs } else { 0.0 };
        self.emit_scroll(v, h);
        if zoom.abs() >= 30.0 {
            self.inject(json!({"keys": [29], "down": true}));
            self.inject(json!({"scroll": {"v": zoom as i32}}));
            self.inject(json!({"keys": [29], "down": false}));
        }
    }

    /// Accumulate hi-res scroll units and inject the whole part.
    fn emit_scroll(&mut self, v: f32, h: f32) {
        self.scroll.carry.0 += v;
        self.scroll.carry.1 += h;
        let (vi, hi) = (self.scroll.carry.0.trunc() as i32, self.scroll.carry.1.trunc() as i32);
        if vi != 0 || hi != 0 {
            self.scroll.carry.0 -= vi as f32;
            self.scroll.carry.1 -= hi as f32;
            self.inject(json!({"scroll": {"v": vi, "h": hi}}));
        }
    }

    /// Accessibility navigation (FR-507): the cap moves the pointer, lift/press scrolls.
    fn desktop_pointer(&mut self, norm: [f32; 6], secs: f32) {
        let de = &self.active.desktop;
        let ax = |i: u8| norm.get(i as usize).copied().unwrap_or(0.0);
        let mut x = ax(de.horizontal_axis) * de.pointer_speed * secs;
        // pushing away from the user moves the pointer up the screen (negative Y)
        let mut y = -ax(de.vertical_axis) * de.pointer_speed * secs;
        if de.invert_horizontal {
            x = -x;
        }
        if de.invert_vertical {
            y = -y;
        }
        // lift scrolls up; half the scroll speed, the axis is short on most caps
        let wheel = ax(1) * de.scroll_speed * 0.5 * secs;
        self.scroll.ptr.0 += x;
        self.scroll.ptr.1 += y;
        let (xi, yi) = (self.scroll.ptr.0.trunc() as i32, self.scroll.ptr.1.trunc() as i32);
        if xi != 0 {
            self.scroll.ptr.0 -= xi as f32;
            self.inject(json!({"rel": {"code": 0, "value": xi}}));
        }
        if yi != 0 {
            self.scroll.ptr.1 -= yi as f32;
            self.inject(json!({"rel": {"code": 1, "value": yi}}));
        }
        self.emit_scroll(wheel, 0.0);
    }

    /// Axis gestures (FR-507: media control, window and workspace switching). Runs on motion
    /// and from the main loop, so a held gesture keeps repeating while the cap is still.
    fn desktop_gestures(&mut self, t: u64) {
        let gs = &self.active.desktop.axis_keys;
        let norm = self.scroll.norm;
        self.scroll.gestures.resize(gs.len(), Gesture::default());
        let mut fire: Vec<String> = Vec::new();
        for (g, st) in gs.iter().zip(self.scroll.gestures.iter_mut()) {
            let v = norm.get(g.axis as usize).copied().unwrap_or(0.0);
            let dir: i8 = if v >= g.threshold {
                1
            } else if v <= -g.threshold {
                -1
            } else {
                0
            };
            if st.dir != 0 {
                // hysteresis: back below 60 % of the threshold (or reversed) re-arms the gesture
                if v.abs() < g.threshold * 0.6 || (dir != 0 && dir != st.dir) {
                    st.dir = 0;
                } else {
                    if g.repeat_ms > 0 && t >= st.next_us {
                        fire.push(if st.dir > 0 { g.positive.clone() } else { g.negative.clone() });
                        st.next_us = t + g.repeat_ms as u64 * 1000;
                    }
                    continue;
                }
            }
            if dir != 0 {
                fire.push(if dir > 0 { g.positive.clone() } else { g.negative.clone() });
                st.dir = dir;
                // the first repeat waits a little longer, like keyboard auto-repeat
                st.next_us = t + (g.repeat_ms.max(300) as u64) * 1000;
            }
        }
        for chord in fire {
            if chord.is_empty() {
                continue;
            }
            if let Ok(c) = keys::parse_chord(&chord) {
                self.inject_keys(&c, Phase::Pulse);
            }
        }
    }

    /// When a held gesture next repeats, if one does.
    fn gesture_deadline(&self) -> Option<u64> {
        if self.active.desktop.style != MotionStyle::Keys || self.effective_mode() != AppMode::DesktopEmulation {
            return None;
        }
        self.active.desktop.axis_keys.iter().zip(&self.scroll.gestures).filter(|(g, st)| st.dir != 0 && g.repeat_ms > 0).map(|(_, st)| st.next_us).min()
    }

    // --------------------------------------------------------- broker I/O

    fn connect_broker(&mut self) {
        let t = now();
        if self.broker.is_some() || t < self.broker_retry_at {
            return;
        }
        self.broker_retry_at = t + BROKER_RETRY_US;
        match UnixStream::connect(&self.broker_path) {
            Ok(s) => {
                let _ = s.set_nonblocking(true);
                let mut b = BrokerConn {
                    s,
                    lb: LineBuf::default(),
                    wb: WriteBuf::default(),
                    next_id: 1,
                    pending: BTreeMap::new(),
                    can_control: false,
                    motion_sub: None,
                };
                b.send("hello", json!({"client": "3dmouse-agent"}), "hello");
                b.send("status", json!({}), "status");
                self.broker = Some(b);
                log!("info", "connected to broker at {}", self.broker_path.display());
                self.broker_err.clear();
                // tell the broker our lock state before anything else
                let locked = self.locked;
                if let Some(b) = &mut self.broker {
                    b.send("set_session", json!({"locked": locked}), "set_session");
                }
                self.push_config();
            }
            Err(e) => {
                let msg = format!("{}: {e}", self.broker_path.display());
                if msg != self.broker_err {
                    log!("warn", "broker unavailable: {msg}");
                }
                self.broker_err = msg;
            }
        }
    }

    fn broker_lost(&mut self, why: &str) {
        log!("warn", "broker connection lost ({why})");
        self.broker = None;
        self.broker_err = format!("connection lost: {why}");
        self.broker_status = Value::Null;
        // nothing stays held when the broker restarts
        for f in self.engine.cancel_all() {
            let _ = f;
        }
        self.macros.clear();
        self.macro_held.clear();
        self.broadcast_event("broker", json!({"connected": false}));
    }

    fn handle_broker(&mut self) {
        let Some(b) = &mut self.broker else { return };
        let mut lines = Vec::new();
        let r = b.lb.read_from(&mut b.s, &mut lines);
        for l in lines {
            let Ok(m) = serde_json::from_str::<Value>(&l) else { continue };
            self.broker_message(m);
        }
        if r != ReadOutcome::Open {
            self.broker_lost("closed by broker");
        }
    }

    fn broker_message(&mut self, m: Value) {
        if let Some(ev) = m.get("event").and_then(|e| e.as_str()) {
            match ev {
                "button" => {
                    let b = m["button"].as_u64().unwrap_or(0) as u8;
                    let p = m["pressed"].as_bool().unwrap_or(false);
                    if m.get("source").is_none() {
                        let fires = if self.locked { Vec::new() } else { self.engine.on_button(b, p, now()) };
                        for f in fires {
                            self.execute(f);
                        }
                    }
                    self.forward_live(&m, true);
                }
                "motion" => {
                    let mut norm = [0f32; 6];
                    if let Some(a) = m["norm"].as_array() {
                        for (i, v) in a.iter().take(6).enumerate() {
                            norm[i] = v.as_f64().unwrap_or(0.0) as f32;
                        }
                    }
                    if self.radial.is_some() {
                        self.radial_motion(norm);
                    } else {
                        self.desktop_motion(norm, now());
                    }
                    self.forward_live(&m, norm.iter().all(|v| *v == 0.0));
                }
                "device" | "status" => {
                    if let Some(b) = &mut self.broker {
                        b.send("status", json!({}), "status");
                    }
                    self.broadcast_event("device", m.clone());
                }
                _ => {}
            }
            return;
        }
        let id = m["id"].as_u64().unwrap_or(0);
        let purpose = self.broker.as_mut().and_then(|b| b.pending.remove(&id)).unwrap_or("");
        let ok = m["ok"].as_bool() == Some(true);
        match purpose {
            "hello" if ok => {
                let cc = m["result"]["can_control"].as_bool().unwrap_or(false);
                if let Some(b) = &mut self.broker {
                    b.can_control = cc;
                }
                if !cc {
                    log!("warn", "broker refuses control from this session (not the active seat user); running read-only");
                }
            }
            "status" if ok => {
                let before = self.device_usb_id();
                self.broker_status = m["result"].clone();
                if self.device_usb_id() != before {
                    self.reselect(false);
                }
            }
            "set_config" | "inject" | "subscribe" | "set_session" if !ok => {
                log!("debug", "broker rejected {purpose}: {}", m["error"]);
            }
            _ => {}
        }
    }

    // --------------------------------------------------------- client API

    fn broadcast_event(&mut self, name: &str, body: Value) {
        let ev = sm_ipc::event(name, body);
        for c in &mut self.clients {
            if c.events {
                c.send(&ev);
            }
        }
    }

    fn forward_live(&mut self, m: &Value, force: bool) {
        let t = now();
        for c in &mut self.clients {
            if c.live && (force || t.saturating_sub(c.last_live_us) >= 1_000_000 / LIVE_MAX_HZ) {
                c.last_live_us = t;
                c.send(m);
            }
        }
    }

    fn accept(&mut self) {
        while let Ok((s, _)) = self.listener.accept() {
            // the socket dir is 0700; still verify the peer is us
            let ok = sm_ipc::peer_cred(&s).map(|c| c.uid == sm_ipc::current_uid()).unwrap_or(false);
            if !ok || self.clients.len() >= MAX_CLIENTS || s.set_nonblocking(true).is_err() {
                continue;
            }
            let id = self.next_client;
            self.next_client += 1;
            self.clients.push(Client {
                id,
                s,
                lb: LineBuf::default(),
                wb: WriteBuf::default(),
                live: false,
                events: false,
                osd: false,
                last_live_us: 0,
                dead: false,
            });
        }
    }

    fn handle_client(&mut self, id: u64) {
        let Some(c) = self.clients.iter_mut().find(|c| c.id == id) else { return };
        let mut lines = Vec::new();
        if c.lb.read_from(&mut c.s, &mut lines) != ReadOutcome::Open {
            c.dead = true;
        }
        for l in lines.into_iter().take(256) {
            let reply = match serde_json::from_str::<Value>(&l) {
                Ok(req) => {
                    let rid = req.get("id").cloned().unwrap_or(Value::Null);
                    if req.get("v").and_then(|v| v.as_u64()) != Some(sm_ipc::PROTO_VERSION as u64) {
                        sm_ipc::err(&rid, "version", "protocol version 1 required")
                    } else {
                        match self.api(id, &req) {
                            Ok(v) => sm_ipc::ok(&rid, v),
                            Err((code, msg)) => sm_ipc::err(&rid, code, msg),
                        }
                    }
                }
                Err(e) => sm_ipc::err(&Value::Null, "bad_request", e.to_string()),
            };
            if let Some(c) = self.clients.iter_mut().find(|c| c.id == id) {
                c.send(&reply);
            }
        }
    }

    fn status_json(&self) -> Value {
        json!({
            "agent": {
                "version": VERSION,
                "socket": self.socket_path.display().to_string(),
                "config_dir": self.store.dir.display().to_string(),
                "broker_connected": self.broker.is_some(),
                "broker_error": self.broker_err,
                "can_control": self.broker.as_ref().map(|b| b.can_control).unwrap_or(false),
                "locked": self.locked,
                "lock_reason": self.lock_reason,
                "desktop": self.desktop,
                "focus": {"app_id": self.focus.app_id, "provider": self.focus.provider},
                "capabilities": {
                    "focus_tracking": match self.desktop.provider {
                        ProviderKind::None => false,
                        ProviderKind::GnomeExtension => self.pushed_focus > 0,
                        _ => true,
                    },
                    "osd": self.has_overlay() || which("notify-send").is_some(),
                    "osd_overlay": self.has_overlay(),
                    "launch": which("gtk-launch").is_some() || which("gio").is_some(),
                    "exec_enabled": self.settings.allow_exec,
                    "virtual_joystick": self.settings.allow_virtual_joystick,
                },
                "load_errors": self.load_errors,
            },
            "profile": {
                "id": self.active.id,
                "display_name": self.active.display_name,
                "selected_by": self.selected_by,
                "app_mode": self.effective_mode(),
                "overrides": self.overrides,
                "session_manual": self.session_manual,
            },
            "last_action": self.last_fired,
            "broker": self.broker_status,
        })
    }

    fn api(&mut self, cid: u64, req: &Value) -> Result<Value, (&'static str, String)> {
        let cmd = req["cmd"].as_str().unwrap_or("");
        let bad = |m: String| ("invalid", m);
        let str_arg = |k: &str| req.get(k).and_then(|v| v.as_str()).map(String::from).ok_or_else(|| ("invalid", format!("missing `{k}`")));
        match cmd {
            "hello" => Ok(json!({"daemon": "3dmouse-agent", "version": VERSION, "proto": sm_ipc::PROTO_VERSION})),
            "status" => Ok(self.status_json()),
            "subscribe" => {
                let live = req["live"].as_bool().unwrap_or(false);
                let events = req["events"].as_bool().unwrap_or(true);
                if let Some(c) = self.clients.iter_mut().find(|c| c.id == cid) {
                    c.live = live;
                    c.events = events;
                }
                self.push_config();
                Ok(json!({"subscribed": true}))
            }
            "profiles.list" => Ok(json!(self
                .profiles
                .iter()
                .map(|p| json!({"id": p.id, "display_name": p.display_name, "scope": p.scope, "app_mode": p.native_app_mode, "app_selector": p.app_selector, "device_selector": p.device_selector, "notes": p.notes, "active": p.id == self.active.id}))
                .collect::<Vec<_>>())),
            "profiles.get" => {
                let id = str_arg("profile")?;
                self.profiles.iter().find(|p| p.id == id).map(|p| json!(p)).ok_or(("not_found", id))
            }
            "profiles.save" => {
                let p: Profile = serde_json::from_value(req["profile"].clone()).map_err(|e| bad(e.to_string()))?;
                p.validate().map_err(bad)?;
                let mut p = p;
                p.modified_at = sm_profile::now_rfc3339();
                // a locally saved profile may carry confirmed exec actions;
                // only the GUI's explicit confirmation flow sets `confirmed`.
                self.store.save(&p).map_err(|e| ("io", e.to_string()))?;
                self.reload_profiles();
                Ok(json!({"saved": p.id}))
            }
            "profiles.lint" => {
                // lint an unsaved profile (the GUI sends its working copy) or a stored one by id
                let p: Profile = if req["profile"].is_object() {
                    serde_json::from_value(req["profile"].clone()).map_err(|e| bad(e.to_string()))?
                } else {
                    let id = str_arg("profile")?;
                    self.profiles.iter().find(|p| p.id == id).cloned().ok_or_else(|| ("not_found", format!("no profile {id}")))?
                };
                let ids: Vec<String> = self.profiles.iter().map(|p| p.id.clone()).collect();
                Ok(json!(p.lint(&ids, self.settings.allow_exec)))
            }
            "profiles.delete" => {
                let id = str_arg("profile")?;
                self.store.delete(&id).map_err(|e| ("io", e.to_string()))?;
                if self.session_manual.as_deref() == Some(&id) {
                    self.session_manual = None;
                }
                self.reload_profiles();
                Ok(json!({"deleted": id}))
            }
            "profiles.activate" => {
                // `profile: null` returns to automatic selection
                let id = req.get("profile").and_then(|v| v.as_str()).map(String::from);
                if let Some(i) = &id {
                    if !self.profiles.iter().any(|p| &p.id == i) {
                        return Err(("not_found", i.clone()));
                    }
                }
                if req["persist"].as_bool().unwrap_or(false) {
                    self.settings.manual_profile = id.clone();
                    self.store.save_settings(&self.settings).map_err(|e| ("io", e.to_string()))?;
                    self.session_manual = None;
                } else {
                    self.session_manual = id;
                }
                self.reselect(false);
                Ok(json!({"active": self.active.id, "selected_by": self.selected_by}))
            }
            "profiles.import" => {
                let text = str_arg("text")?;
                let (mut p, preview) = sm_profile::import_untrusted(&text, &self.store).map_err(bad)?;
                if req["apply"].as_bool().unwrap_or(false) {
                    if preview.overwrites_existing && !req["overwrite"].as_bool().unwrap_or(false) {
                        return Err(("exists", format!("profile {} exists; pass overwrite", p.id)));
                    }
                    p.modified_at = sm_profile::now_rfc3339();
                    self.store.save(&p).map_err(|e| ("io", e.to_string()))?;
                    self.reload_profiles();
                }
                Ok(json!({"preview": preview, "applied": req["apply"].as_bool().unwrap_or(false)}))
            }
            "profiles.export" => {
                let id = str_arg("profile")?;
                let p = self.profiles.iter().find(|p| p.id == id).ok_or(("not_found", id))?;
                Ok(json!({"toml": p.to_toml().map_err(bad)?}))
            }
            "profiles.backup" => Ok(json!({"toml": self.store.backup().map_err(|e| ("io", e))?})),
            "profiles.reload" => {
                self.reload_profiles();
                Ok(json!({"profiles": self.profiles.len()}))
            }
            "profiles.reset_presets" => {
                for p in sm_profile::presets() {
                    self.store.save(&p).map_err(|e| ("io", e.to_string()))?;
                }
                self.reload_profiles();
                Ok(json!({"reset": true}))
            }
            "settings.get" => Ok(json!(self.settings)),
            "settings.set" => {
                let mut cur = serde_json::to_value(&self.settings).map_err(|e| bad(e.to_string()))?;
                if let (Some(o), Some(n)) = (cur.as_object_mut(), req["settings"].as_object()) {
                    for (k, v) in n {
                        o.insert(k.clone(), v.clone());
                    }
                }
                let s: Settings = serde_json::from_value(cur).map_err(|e| bad(e.to_string()))?;
                self.store.save_settings(&s).map_err(|e| ("io", e.to_string()))?;
                self.settings = s;
                self.reselect(false);
                Ok(json!(self.settings))
            }
            "osd.register" => {
                // 3dmouse-osd announces it can draw overlays; notifications stop duplicating them
                if let Some(c) = self.clients.iter_mut().find(|c| c.id == cid) {
                    c.osd = true;
                    c.events = true;
                }
                Ok(json!({"registered": true}))
            }
            "focus.report" => {
                // pushed by the GNOME Shell extension (or tests)
                let f = FocusInfo { app_id: str_arg("app_id")?, title: req["title"].as_str().unwrap_or("").to_string(), pid: req["pid"].as_u64().map(|p| p as u32), provider: "gnome-extension".into() };
                self.pushed_focus += 1;
                self.on_focus(f);
                Ok(json!({}))
            }
            "preview_nav" => {
                // live tuning from the GUI without saving (not persisted)
                let n: sm_pipeline::NavConfig = serde_json::from_value(req["nav"].clone()).map_err(|e| bad(e.to_string()))?;
                n.validate().map_err(bad)?;
                self.active.navigation = n;
                self.push_config();
                Ok(json!({"previewing": self.active.id}))
            }
            "test_action" => {
                let a: Action = serde_json::from_value(req["action"].clone()).map_err(|e| bad(e.to_string()))?;
                a.validate().map_err(bad)?;
                // "press"/"release" let a client hold an action (e.g. open and close a radial menu)
                let phase = match req["phase"].as_str() {
                    None | Some("pulse") => Phase::Pulse,
                    Some("press") => Phase::Press,
                    Some("release") => Phase::Release,
                    Some(p) => return Err(bad(format!("unknown phase {p:?}"))),
                };
                self.execute(Fire { button: 255, action: a, phase });
                Ok(json!({}))
            }
            "diagnostics" => Ok(json!({"status": self.status_json(), "keys": keys::KEY_TABLE.iter().map(|(n, _)| *n).collect::<Vec<_>>()})),
            "broker" => {
                // read-only passthrough for the GUI (status/registry)
                let sub = str_arg("broker_cmd")?;
                if !["status", "registry"].contains(&sub.as_str()) {
                    return Err(("denied", "only status/registry are proxied".into()));
                }
                let mut c = sm_ipc::Client::connect(&self.broker_path).map_err(|e| ("unavailable", e.to_string()))?;
                c.call(&sub, json!({})).map_err(|e| ("broker", e))
            }
            _ => Err(("unknown_command", format!("unknown command `{cmd}`"))),
        }
    }

    fn on_focus(&mut self, f: FocusInfo) {
        if f.app_id == self.focus.app_id && f.title == self.focus.title {
            return;
        }
        log!("debug", "focus: {} ({})", f.app_id, f.provider);
        self.focus = f;
        if self.settings.auto_switch {
            self.reselect(false);
        }
        self.broadcast_event("focus", json!({"app_id": self.focus.app_id}));
    }

    // --------------------------------------------------------- loop

    fn poll_lock(&mut self) {
        let (l, reason) = sm_desktop::lock::is_locked();
        if l != self.locked {
            log!("info", "session {} {}", if l { "locked" } else { "unlocked" }, reason);
            self.locked = l;
            self.lock_reason = reason;
            if l {
                for f in self.engine.cancel_all() {
                    self.execute(f); // releases only
                }
                self.release_macro_keys();
            }
            if let Some(b) = &mut self.broker {
                b.send("set_session", json!({"locked": l}), "set_session");
            }
            self.broadcast_event("lock", json!({"locked": l}));
        }
    }

    fn run(&mut self) {
        log!("info", "3dmouse-agent {VERSION} listening on {}", self.socket_path.display());
        while !self.quit {
            let t = now();
            self.connect_broker();
            if t >= self.next_lock_poll {
                self.next_lock_poll = t + LOCK_POLL_US;
                self.poll_lock();
            }
            if t >= self.next_status_poll {
                self.next_status_poll = t + STATUS_POLL_US;
                if let Some(b) = &mut self.broker {
                    b.send("status", json!({}), "status");
                }
                self.children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
            }
            for f in self.engine.tick(t) {
                self.execute(f);
            }
            self.run_macros();
            if self.gesture_deadline().is_some_and(|d| d <= now()) {
                self.desktop_gestures(now());
            }
            if let Some(b) = &mut self.broker {
                if b.wb.flush(&mut b.s).is_err() {
                    self.broker_lost("write failed");
                }
            }
            for c in &mut self.clients {
                if c.wb.flush(&mut c.s).is_err() {
                    c.dead = true;
                }
            }
            let had_live = self.clients.iter().any(|c| c.live);
            self.clients.retain(|c| !c.dead);
            if had_live != self.clients.iter().any(|c| c.live) {
                self.push_config();
            }

            // ---- wait
            let mut fds = vec![
                libc::pollfd { fd: self.sigfd, events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.wake_r, events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: self.listener.as_raw_fd(), events: libc::POLLIN, revents: 0 },
            ];
            let broker_fd = self.broker.as_ref().map(|b| (b.s.as_raw_fd(), !b.wb.is_empty()));
            if let Some((fd, w)) = broker_fd {
                fds.push(libc::pollfd { fd, events: libc::POLLIN | if w { libc::POLLOUT } else { 0 }, revents: 0 });
            }
            let base = fds.len();
            for c in &self.clients {
                fds.push(libc::pollfd { fd: c.s.as_raw_fd(), events: libc::POLLIN | if c.wb.is_empty() { 0 } else { libc::POLLOUT }, revents: 0 });
            }
            let t = now();
            let mut deadline = (self.next_lock_poll.min(self.next_status_poll)).saturating_sub(t);
            if self.broker.is_none() {
                deadline = deadline.min(self.broker_retry_at.saturating_sub(t));
            }
            if let Some(d) = self.engine.next_deadline_us(t) {
                deadline = deadline.min(d);
            }
            if let Some((due, _)) = self.macros.front() {
                deadline = deadline.min(due.saturating_sub(t));
            }
            if let Some(due) = self.gesture_deadline() {
                deadline = deadline.min(due.saturating_sub(t));
            }
            let timeout = deadline.div_ceil(1000).min(5000) as i32;
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            if n <= 0 {
                continue;
            }
            if fds[0].revents != 0 {
                let mut si: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
                unsafe { libc::read(self.sigfd, &mut si as *mut _ as *mut libc::c_void, std::mem::size_of::<libc::signalfd_siginfo>()) };
                if si.ssi_signo == libc::SIGHUP as u32 {
                    log!("info", "SIGHUP: reloading profiles and settings");
                    self.settings = self.store.load_settings();
                    self.reload_profiles();
                } else {
                    self.quit = true;
                }
            }
            if fds[1].revents != 0 {
                let mut b = [0u8; 64];
                while unsafe { libc::read(self.wake_r, b.as_mut_ptr() as *mut libc::c_void, b.len()) } > 0 {}
                let mut last = None;
                while let Ok(f) = self.focus_rx.try_recv() {
                    last = Some(f);
                }
                if let Some(f) = last {
                    self.on_focus(f);
                }
            }
            if fds[2].revents != 0 {
                self.accept();
            }
            if broker_fd.is_some() && fds[3].revents != 0 {
                if fds[3].revents & libc::POLLIN != 0 {
                    self.handle_broker();
                } else if fds[3].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                    self.broker_lost("hangup");
                }
            }
            let ids: Vec<(u64, i16)> = self.clients.iter().zip(fds[base..].iter()).map(|(c, p)| (c.id, p.revents)).collect();
            for (id, rev) in ids {
                if rev & libc::POLLIN != 0 {
                    self.handle_client(id);
                } else if rev & (libc::POLLHUP | libc::POLLERR) != 0 {
                    if let Some(c) = self.clients.iter_mut().find(|c| c.id == id) {
                        c.dead = true;
                    }
                }
            }
        }
        log!("info", "shutting down");
        for f in self.engine.cancel_all() {
            self.execute(f);
        }
        self.release_macro_keys();
        if let Some(b) = &mut self.broker {
            b.send("release_all", json!({}), "release_all");
            let _ = b.wb.flush(&mut b.s);
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

fn describe(a: &Action) -> String {
    match a {
        Action::None => "nothing".into(),
        Action::SpnavButton { button } => format!("app button {}", button + 1),
        Action::Key { keys } => keys.clone(),
        Action::Mouse { button } => format!("mouse {button:?}").to_lowercase(),
        Action::Scroll { dx, dy } => format!("scroll {dx},{dy}"),
        Action::View { view } => format!("view {}", sm_profile::view_name(*view)),
        Action::Toggle { target } => format!("toggle {target:?}").to_lowercase(),
        Action::Sensitivity { factor } => format!("sensitivity ×{factor}"),
        Action::SensitivityReset => "reset sensitivity".into(),
        Action::ProfileSwitch { profile } => format!("profile {profile}"),
        Action::ProfileCycle => "next profile".into(),
        Action::Launch { desktop_id } => format!("launch {desktop_id}"),
        Action::Exec { .. } => "command".into(),
        Action::Macro { steps } => format!("macro ({} steps)", steps.len()),
        Action::ShowHelp => "show this help".into(),
        Action::Radial { items } => format!("radial menu ({})", items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>().join(", ")),
    }
}

fn main() {
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--debug" => DEBUG.store(true, std::sync::atomic::Ordering::Relaxed),
            "--version" => {
                println!("3dmouse-agent {VERSION}");
                return;
            }
            _ => {
                eprintln!("usage: 3dmouse-agent [--debug] [--version]\n  env: THREEDMOUSE_BROKER_SOCKET, THREEDMOUSE_AGENT_SOCKET, THREEDMOUSE_CONFIG_DIR");
                std::process::exit(2);
            }
        }
    }
    if let Some(old) = sm_profile::migrate_legacy_config_dir() {
        log!("info", "moved settings from {} to {}", old.display(), sm_profile::config_dir().display());
    }
    match Agent::new() {
        Ok(mut a) => a.run(),
        Err(e) => {
            log!("err", "startup failed: {e}");
            std::process::exit(1);
        }
    }
}
