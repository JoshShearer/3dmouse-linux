//! `3dmoused` – input broker / device owner (SRS §4.1).
//!
//! One authoritative reader per device (FR-107). Single-threaded poll(2)
//! loop: no busy polling, bounded queues, monotonic timers (§4.3).
//! See docs/adr/0001-device-ownership.md and docs/architecture/overview.md.

use devices::{Candidate, Device, InputPref, OpenError};
use serde_json::{json, Value};
use sm_core::{AppMode, DeviceState, OwnershipMode, AXES};
use sm_daemon::devices;
use sm_desktop::uinput::{Gamepad, Uinput};
use sm_ipc::{BrokerStatus, ButtonLabel, DeviceStatus, LineBuf, PeerCred, ReadOutcome, WriteBuf};
use sm_pipeline::{NavConfig, Pipeline};
use sm_registry::Registry;
use sm_spnav::{DeviceInfo, SpnavClient, SpnavServer};
use std::collections::{BTreeMap, HashMap};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const SMOOTH_TICK_US: u64 = 8_000;
const PERIODIC_US: u64 = 1_000_000;
const RETRY_US: u64 = 5_000_000;
const MAX_CTL_CLIENTS: usize = 32;
const MAX_INJECT_PER_SEC: u32 = 2000;

// ------------------------------------------------------------- logging

static mut LOG_DEBUG: bool = false;

macro_rules! log {
    ($lvl:literal, $($t:tt)*) => {{
        // journald understands <N> priority prefixes on stderr.
        let p = match $lvl { "err" => 3, "warn" => 4, "info" => 6, _ => 7 };
        #[allow(static_mut_refs)]
        if p < 7 || unsafe { LOG_DEBUG } {
            eprintln!("<{}>{}", p, format!($($t)*));
        }
    }};
}

// ------------------------------------------------------------- args

struct Args {
    mode: Option<OwnershipMode>,
    spnav_socket: PathBuf,
    control_socket: PathBuf,
    input: InputPref,
    uinput: Option<String>,
    sysfs: PathBuf,
    dev_root: PathBuf,
}

fn usage() -> ! {
    eprintln!(
        "3dmoused {VERSION} – 3DMouse Linux input broker

USAGE: 3dmoused [OPTIONS]
  --mode auto|native-owner|integrated-spacenav|passive-monitor   (default auto)
  --spnav-socket PATH     libspnav-compatible socket (default /run/spnav.sock)
  --control-socket PATH   control socket (default /run/3dmouse/broker.sock)
  --input auto|hidraw|evdev
  --uinput PATH|none      virtual input device (default /dev/uinput)
  --sysfs PATH            sysfs root (testing; default /sys)
  --dev-root PATH         device node root (testing; default /dev)
  --debug
  --version"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let mut a = Args {
        mode: None,
        spnav_socket: sm_spnav::DEFAULT_SOCKET.into(),
        control_socket: sm_ipc::DEFAULT_BROKER_SOCKET.into(),
        input: InputPref::Auto,
        uinput: Some("/dev/uinput".into()),
        sysfs: "/sys".into(),
        dev_root: "/dev".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut val = || it.next().unwrap_or_else(|| usage());
        match k.as_str() {
            "--mode" => {
                let v = val();
                a.mode = if v == "auto" {
                    None
                } else {
                    Some(v.parse().unwrap_or_else(|e: String| {
                        eprintln!("{e}");
                        usage()
                    }))
                }
            }
            "--spnav-socket" => a.spnav_socket = val().into(),
            "--control-socket" => a.control_socket = val().into(),
            "--input" => {
                a.input = match val().as_str() {
                    "auto" => InputPref::Auto,
                    "hidraw" => InputPref::Hidraw,
                    "evdev" => InputPref::Evdev,
                    _ => usage(),
                }
            }
            "--uinput" => {
                let v = val();
                a.uinput = if v == "none" { None } else { Some(v) }
            }
            "--sysfs" => a.sysfs = val().into(),
            "--dev-root" => a.dev_root = val().into(),
            "--debug" => unsafe { LOG_DEBUG = true },
            "--version" => {
                println!("3dmoused {VERSION}");
                std::process::exit(0)
            }
            _ => usage(),
        }
    }
    a
}

// ------------------------------------------------------------- helpers

fn sd_notify(msg: &str) {
    let Some(p) = std::env::var_os("NOTIFY_SOCKET") else { return };
    let p = p.to_string_lossy().into_owned();
    let s = match std::os::unix::net::UnixDatagram::unbound() {
        Ok(s) => s,
        Err(_) => return,
    };
    if let Some(abs) = p.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt;
        if let Ok(addr) = std::os::unix::net::SocketAddr::from_abstract_name(abs.as_bytes()) {
            let _ = s.send_to_addr(msg.as_bytes(), &addr);
        }
    } else {
        let _ = s.send_to(msg.as_bytes(), &p);
    }
}

fn spacenavd_running() -> bool {
    std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten().any(|e| {
                e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit())
                    && std::fs::read_to_string(e.path().join("comm")).map(|c| c.trim() == "spacenavd").unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn socket_live(p: &Path) -> bool {
    p.exists() && UnixStream::connect(p).is_ok()
}

fn inode(p: &Path) -> Option<u64> {
    std::fs::symlink_metadata(p).ok().map(|m| m.ino())
}

// ------------------------------------------------------------- control clients

struct Ctl {
    id: u64,
    s: UnixStream,
    cred: PeerCred,
    lb: LineBuf,
    wb: WriteBuf,
    motion_hz: Option<u32>,
    last_motion_us: u64,
    last_motion_zero: bool,
    buttons: bool,
    devices: bool,
    pressed: Vec<u16>,
    inject_window_us: u64,
    inject_count: u32,
    dead: bool,
}

impl Ctl {
    fn send(&mut self, v: &Value) {
        if !self.wb.push_line(v) {
            self.dead = true; // peer stopped reading
        }
    }
}

#[derive(Clone)]
struct ActiveConfig {
    nav: NavConfig,
    app_mode: AppMode,
    /// device button index -> spnav button number; None = identity.
    spnav_buttons: Option<BTreeMap<u8, u8>>,
    profile: String,
    owner_conn: Option<u64>,
    owner_uid: Option<u32>,
}

impl Default for ActiveConfig {
    fn default() -> Self {
        ActiveConfig {
            nav: NavConfig::default(),
            app_mode: AppMode::Native6dof,
            spnav_buttons: None,
            profile: "builtin-default".into(),
            owner_conn: None,
            owner_uid: None,
        }
    }
}

impl ActiveConfig {
    fn spnav_motion(&self) -> bool {
        matches!(self.app_mode, AppMode::Native6dof | AppMode::ApplicationPlugin)
    }
    fn spnav_button(&self, idx: u8) -> Option<u8> {
        match &self.spnav_buttons {
            None => Some(idx),
            Some(m) => m.get(&idx).copied(),
        }
    }
}

struct Failed {
    reason: String,
    state: DeviceState,
    retry_at: u64,
    cand: Candidate,
}

// ------------------------------------------------------------- broker

struct Broker {
    args: Args,
    reg: &'static Registry,
    mode: OwnershipMode,
    mode_reason: String,
    devices: Vec<Device>,
    pipelines: HashMap<u32, Pipeline>,
    last_spnav_zero: HashMap<u32, bool>,
    failed: HashMap<String, Failed>,
    scan_notes: Vec<String>,
    known_keys: HashMap<String, u32>,
    next_dev_id: u32,
    spnav: Option<SpnavServer>,
    spnav_ino: Option<u64>,
    upstream: Option<SpnavClient>,
    upstream_devices: Vec<Candidate>,
    ctl_listener: UnixListener,
    ctls: Vec<Ctl>,
    next_ctl: u64,
    uinput: Option<Uinput>,
    uinput_reason: String,
    /// virtual game controller, present only while a virtual-joystick profile is active (FR-507)
    gamepad: Option<Gamepad>,
    /// why the controller could not be created (logged once per activation)
    gamepad_err: Option<String>,
    cfg: ActiveConfig,
    locked: bool,
    inotify: RawFd,
    sigfd: RawFd,
    rescan_at: Option<u64>,
    next_periodic: u64,
    start_us: u64,
    last_motion_t: HashMap<u32, u64>,
    quit: bool,
}

fn make_control_socket(p: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if p.exists() {
        if socket_live(p) {
            return Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, format!("{} already served (another 3dmoused?)", p.display())));
        }
        std::fs::remove_file(p)?;
    }
    let l = UnixListener::bind(p)?;
    l.set_nonblocking(true)?;
    // Any local user may connect; commands are authorised per peer uid.
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o666))?;
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

fn setup_inotify() -> RawFd {
    unsafe {
        let fd = libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC);
        if fd >= 0 {
            for d in [&b"/dev\0"[..], &b"/dev/input\0"[..]] {
                libc::inotify_add_watch(fd, d.as_ptr() as *const libc::c_char, libc::IN_CREATE | libc::IN_DELETE | libc::IN_ATTRIB);
            }
        }
        fd
    }
}

fn drain(fd: RawFd) {
    let mut b = [0u8; 4096];
    while unsafe { libc::read(fd, b.as_mut_ptr() as *mut libc::c_void, b.len()) } > 0 {}
}

#[derive(Clone, Copy)]
enum Tag {
    Sig,
    Inotify,
    CtlListen,
    Ctl(u64),
    SpnavListen,
    SpnavClient,
    Upstream,
    Dev(u32),
}

impl Broker {
    fn new(args: Args) -> std::io::Result<Broker> {
        let ctl_listener = make_control_socket(&args.control_socket)?;
        let now = sm_core::monotonic_us();
        let mut b = Broker {
            reg: Registry::embedded(),
            mode: OwnershipMode::PassiveMonitor,
            mode_reason: String::new(),
            devices: Vec::new(),
            pipelines: HashMap::new(),
            last_spnav_zero: HashMap::new(),
            failed: HashMap::new(),
            scan_notes: Vec::new(),
            known_keys: HashMap::new(),
            next_dev_id: 0,
            spnav: None,
            spnav_ino: None,
            upstream: None,
            upstream_devices: Vec::new(),
            ctl_listener,
            ctls: Vec::new(),
            next_ctl: 1,
            uinput: None,
            uinput_reason: "not opened".into(),
            gamepad: None,
            gamepad_err: None,
            cfg: ActiveConfig::default(),
            locked: false,
            inotify: setup_inotify(),
            sigfd: setup_signals(),
            rescan_at: Some(now),
            next_periodic: now + PERIODIC_US,
            start_us: now,
            last_motion_t: HashMap::new(),
            quit: false,
            args,
        };
        b.resolve_mode(b.args.mode);
        b.open_uinput();
        Ok(b)
    }

    /// Deterministic ownership negotiation (FR-107, ADR-0001).
    fn resolve_mode(&mut self, forced: Option<OwnershipMode>) {
        let sock = self.args.spnav_socket.clone();
        let (mode, reason) = match forced {
            Some(m) => (m, "selected with --mode".to_string()),
            None if socket_live(&sock) => (OwnershipMode::IntegratedSpacenav, format!("{} is served by another daemon (spacenavd)", sock.display())),
            None if spacenavd_running() => (OwnershipMode::IntegratedSpacenav, "a spacenavd process is running".into()),
            None => (OwnershipMode::NativeOwner, "no other 6-DOF daemon detected".into()),
        };
        self.set_mode(mode, reason);
    }

    fn set_mode(&mut self, mode: OwnershipMode, reason: String) {
        log!("info", "ownership mode: {mode} ({reason})");
        // tear down whatever the previous mode owned
        for id in self.devices.iter().map(|d| d.id).collect::<Vec<_>>() {
            self.remove_device(id, "ownership mode change");
        }
        self.spnav = None;
        self.spnav_ino = None;
        self.upstream = None;
        self.mode = mode;
        self.mode_reason = reason;
        match mode {
            OwnershipMode::NativeOwner => match SpnavServer::bind(&self.args.spnav_socket) {
                Ok(s) => {
                    self.spnav_ino = inode(&self.args.spnav_socket);
                    log!("info", "serving libspnav protocol on {}", self.args.spnav_socket.display());
                    self.spnav = Some(s);
                }
                Err(e) => {
                    log!("err", "cannot serve {}: {e}", self.args.spnav_socket.display());
                    self.mode_reason += &format!("; spnav socket unavailable: {e}");
                }
            },
            OwnershipMode::IntegratedSpacenav => self.connect_upstream(),
            OwnershipMode::PassiveMonitor => {}
        }
        self.rescan_at = Some(sm_core::monotonic_us());
        self.broadcast_status_event();
    }

    fn connect_upstream(&mut self) {
        match SpnavClient::connect(&self.args.spnav_socket, "3dmouse-linux", sm_spnav::EVMASK_MOTION | sm_spnav::EVMASK_BUTTON | sm_spnav::EVMASK_DEV) {
            Ok(c) => {
                log!("info", "connected to spacenavd (protocol v{})", c.proto);
                self.upstream = Some(c);
            }
            Err(e) => log!("warn", "spacenavd not reachable yet: {e}"),
        }
    }

    fn open_uinput(&mut self) {
        self.uinput = None;
        if self.mode == OwnershipMode::PassiveMonitor {
            self.uinput_reason = "disabled in passive-monitor mode".into();
            return;
        }
        let Some(p) = self.args.uinput.clone() else {
            self.uinput_reason = "disabled with --uinput none".into();
            return;
        };
        match Uinput::open(&p) {
            Ok(u) => {
                self.uinput = Some(u);
                self.uinput_reason = format!("virtual device on {p}");
            }
            Err(e) => {
                self.uinput_reason = format!("{p}: {e}");
                log!("warn", "desktop input injection unavailable: {}", self.uinput_reason);
            }
        }
    }

    // --------------------------------------------------------- devices

    fn rescan(&mut self) {
        let (cands, notes) = devices::scan(self.reg, &self.args.sysfs, &self.args.dev_root);
        if notes != self.scan_notes {
            for n in &notes {
                log!("info", "{n}");
            }
            self.scan_notes = notes;
        }
        let present: Vec<String> = cands.iter().map(|c| c.key.clone()).collect();
        // removed devices
        for id in self.devices.iter().filter(|d| !present.contains(&d.cand.key)).map(|d| d.id).collect::<Vec<_>>() {
            self.remove_device(id, "unplugged");
        }
        self.failed.retain(|k, _| present.contains(k));
        if self.mode == OwnershipMode::IntegratedSpacenav {
            self.upstream_devices = cands;
            return;
        }
        let now = sm_core::monotonic_us();
        for c in cands {
            if self.devices.iter().any(|d| d.cand.key == c.key) {
                continue;
            }
            if let Some(f) = self.failed.get(&c.key) {
                if now < f.retry_at {
                    continue;
                }
            }
            let id = *self.known_keys.entry(c.key.clone()).or_insert_with(|| {
                self.next_dev_id += 1;
                self.next_dev_id - 1
            });
            match Device::open(id, c.clone(), self.args.input) {
                Ok(d) => {
                    log!(
                        "info",
                        "device {} ready: {} [{:04x}:{:04x}] via {} {} (status: {})",
                        id,
                        d.cand.name,
                        d.cand.vid,
                        d.cand.pid,
                        d.input_path,
                        d.node.display(),
                        d.cand.model.as_ref().map(|m| m.status.as_str()).unwrap_or("generic")
                    );
                    self.failed.remove(&c.key);
                    self.pipelines.insert(id, Pipeline::new(self.cfg.nav.clone(), d.axis_range));
                    let info = Self::spnav_info(&d);
                    if let Some(s) = &mut self.spnav {
                        s.devices.retain(|x| x.id != info.id);
                        s.devices.push(info.clone());
                        s.send_dev(0, &info);
                    }
                    self.devices.push(d);
                    let st = self.device_status(self.devices.last().unwrap());
                    self.broadcast(|c| c.devices, &sm_ipc::event("device", json!({"op": "added", "device": st})));
                }
                Err(e) => {
                    let (state, reason) = match e {
                        OpenError::Permission(m) => {
                            (DeviceState::Error, format!("permission denied ({m}); install the udev rules / run as the system service"))
                        }
                        OpenError::Other(m) => (DeviceState::Unsupported, m),
                    };
                    if self.failed.get(&c.key).map(|f| f.reason != reason).unwrap_or(true) {
                        log!("warn", "device {:04x}:{:04x} not opened: {reason}", c.vid, c.pid);
                    }
                    self.failed.insert(c.key.clone(), Failed { reason, state, retry_at: now + RETRY_US, cand: c });
                }
            }
        }
    }

    fn spnav_info(d: &Device) -> DeviceInfo {
        DeviceInfo {
            id: d.id as i32,
            name: d.cand.name.clone(),
            path: d.node.display().to_string(),
            naxes: AXES as i32,
            nbuttons: d.cand.model.as_ref().map(|m| m.buttons.len() as i32).unwrap_or(2),
            vid: d.cand.vid,
            pid: d.cand.pid,
            devtype: d.cand.model.as_ref().map(|m| m.spnav_type).unwrap_or(0),
        }
    }

    /// Disconnect handling: release held buttons, zero motion (FR-104).
    fn remove_device(&mut self, id: u32, why: &str) {
        let Some(pos) = self.devices.iter().position(|d| d.id == id) else { return };
        let mut d = self.devices.remove(pos);
        log!("info", "device {id} removed ({why})");
        let held = d.state.reset();
        for b in 0..64u8 {
            if held & (1 << b) != 0 {
                self.emit_button(id, b, false);
            }
        }
        if let Some(p) = self.pipelines.get_mut(&id) {
            p.reset();
        }
        if let Some(g) = &mut self.gamepad {
            g.neutral();
        }
        if let Some(s) = &mut self.spnav {
            if self.cfg.spnav_motion() {
                s.send_motion([0; 6], 0);
            }
            let info = Self::spnav_info(&d);
            s.send_dev(1, &info);
            s.devices.retain(|x| x.id != info.id);
        }
        let (zi, zf) = ([0i32; 6], [0.0f32; 6]);
        self.broadcast(
            |c| c.motion_hz.is_some(),
            &sm_ipc::event("motion", json!({"dev": id, "raw": zi, "norm": zf, "spnav": zi, "t": sm_core::monotonic_us()})),
        );
        self.broadcast(|c| c.devices, &sm_ipc::event("device", json!({"op": "removed", "id": id})));
        self.pipelines.remove(&id);
    }

    fn handle_device_readable(&mut self, id: u32) {
        let Some(d) = self.devices.iter_mut().find(|d| d.id == id) else { return };
        match d.read() {
            Ok(frames) => {
                for f in frames {
                    self.process_frame(id, f);
                }
            }
            Err(()) => {
                self.remove_device(id, "read error / device gone");
                self.rescan_at = Some(sm_core::monotonic_us() + 200_000);
            }
        }
    }

    fn process_frame(&mut self, id: u32, f: sm_core::RawFrame) {
        let Some(d) = self.devices.iter_mut().find(|d| d.id == id) else { return };
        let (changed, edges) = d.state.apply(&f);
        let cur_buttons = d.state.buttons;
        let raw_c = d.canonical.apply(d.state.axes);
        if f.axes_present != 0 {
            if changed {
                if let Some(s) = &mut self.spnav {
                    for (i, v) in raw_c.iter().enumerate() {
                        s.send_raw_axis(i as i32, *v);
                    }
                }
            }
            self.emit_motion(id, raw_c, f.t_us);
        }
        for b in 0..64u8 {
            if edges & (1 << b) != 0 {
                self.emit_button(id, b, cur_buttons & (1 << b) != 0);
            }
        }
    }

    fn emit_motion(&mut self, id: u32, raw_c: [i32; 6], t: u64) {
        let Some(p) = self.pipelines.get_mut(&id) else { return };
        let m = p.process(raw_c, t);
        let prev = self.last_motion_t.insert(id, t).unwrap_or(t);
        let period = ((t.saturating_sub(prev)) / 1000).min(1000) as i32;
        let zero = m.is_zero();
        let was_zero = self.last_spnav_zero.insert(id, zero).unwrap_or(true);
        if self.cfg.spnav_motion() && !(zero && was_zero) {
            if let Some(s) = &mut self.spnav {
                s.send_motion(m.spnav, period);
            }
        }
        if !(zero && was_zero) {
            if let Some(g) = self.joystick() {
                let _ = g.set_axes(m.norm);
            }
        }
        if zero && was_zero {
            return; // nothing new for subscribers either
        }
        let ev = sm_ipc::event("motion", json!({"dev": id, "raw": raw_c, "norm": m.norm, "spnav": m.spnav, "t": t}));
        for c in &mut self.ctls {
            let Some(hz) = c.motion_hz else { continue };
            let min_dt = 1_000_000 / hz.clamp(1, 1000) as u64;
            // always deliver transitions to neutral so subscribers never stick
            if zero || t.saturating_sub(c.last_motion_us) >= min_dt || c.last_motion_zero {
                c.last_motion_us = t;
                c.last_motion_zero = zero;
                c.send(&ev);
            }
        }
    }

    /// The virtual game controller, created on first use while a virtual-joystick profile is
    /// active and the session is unlocked. The agent only asks for this mode when the user
    /// allowed it, so the broker never creates a controller on its own.
    fn joystick(&mut self) -> Option<&mut Gamepad> {
        if self.cfg.app_mode != AppMode::VirtualJoystick || self.locked {
            return None;
        }
        if self.gamepad.is_none() && self.gamepad_err.is_none() {
            let path = match (&self.args.uinput, self.mode) {
                (_, OwnershipMode::PassiveMonitor) => Err("disabled in passive-monitor mode".to_string()),
                (None, _) => Err("disabled with --uinput none".to_string()),
                (Some(p), _) => Ok(p.clone()),
            };
            match path.and_then(|p| Gamepad::open(&p).map_err(|e| format!("{p}: {e}"))) {
                Ok(g) => {
                    log!("info", "virtual joystick created for profile {}", self.cfg.profile);
                    self.gamepad = Some(g);
                }
                Err(e) => {
                    log!("warn", "virtual joystick unavailable: {e}");
                    self.gamepad_err = Some(e);
                }
            }
        }
        self.gamepad.as_mut()
    }

    fn emit_button(&mut self, id: u32, b: u8, pressed: bool) {
        if let Some(g) = self.joystick() {
            let _ = g.set_button(b, pressed);
        }
        if let Some(s) = &mut self.spnav {
            s.send_raw_button(b as i32, pressed);
            if let Some(n) = self.cfg.spnav_button(b) {
                s.send_button(n as i32, pressed);
            }
        }
        self.broadcast(|c| c.buttons, &sm_ipc::event("button", json!({"dev": id, "button": b, "pressed": pressed, "t": sm_core::monotonic_us()})));
    }

    fn handle_upstream(&mut self) {
        let Some(u) = &mut self.upstream else { return };
        match u.read_events() {
            Ok(evs) => {
                for e in evs {
                    match e {
                        sm_spnav::ClientEvent::Motion(m, _) => {
                            // already processed by spacenavd: forward as-is,
                            // never run our pipeline on top (FR-210)
                            let norm: Vec<f32> = m.iter().map(|v| *v as f32 / sm_pipeline::SPNAV_SCALE).collect();
                            self.broadcast(
                                |c| c.motion_hz.is_some(),
                                &sm_ipc::event(
                                    "motion",
                                    json!({"dev": 0, "raw": m, "norm": norm, "spnav": m, "t": sm_core::monotonic_us(), "source": "spacenavd"}),
                                ),
                            );
                        }
                        sm_spnav::ClientEvent::Button(b, p) => {
                            self.broadcast(|c| c.buttons, &sm_ipc::event("button", json!({"dev": 0, "button": b, "pressed": p, "source": "spacenavd"})));
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                log!("warn", "spacenavd connection lost: {e}");
                self.upstream = None;
            }
        }
    }

    // --------------------------------------------------------- control

    fn broadcast(&mut self, pred: impl Fn(&Ctl) -> bool, v: &Value) {
        for c in &mut self.ctls {
            if pred(c) {
                c.send(v);
            }
        }
    }

    fn broadcast_status_event(&mut self) {
        let st = serde_json::to_value(self.status()).unwrap_or(Value::Null);
        self.broadcast(|c| c.devices, &sm_ipc::event("status", json!({"status": st})));
    }

    fn device_status(&self, d: &Device) -> DeviceStatus {
        DeviceStatus {
            id: d.id,
            name: d.cand.name.clone(),
            usb_id: format!("{:04x}:{:04x}", d.cand.vid, d.cand.pid),
            transport: if d.cand.bus == 5 { "bluetooth".into() } else { d.cand.model.as_ref().map(|m| m.transport.clone()).unwrap_or("usb".into()) },
            path: d.node.display().to_string(),
            input_path: d.input_path.into(),
            state: DeviceState::Ready,
            support_status: d.cand.model.as_ref().map(|m| m.status.as_str().to_string()).unwrap_or("generic".into()),
            generic: d.cand.generic,
            buttons: d
                .cand
                .model
                .as_ref()
                .map(|m| m.buttons.iter().map(|b| ButtonLabel { index: b.index, label: b.label.clone() }).collect())
                .unwrap_or_else(|| (0..2).map(|i| ButtonLabel { index: i, label: format!("Button {}", i + 1) }).collect()),
            axis_range: d.axis_range,
            serial: sm_core::redact(&d.cand.uniq),
            frames: d.frames,
            malformed: d.malformed(),
            watchdog_resets: d.watchdog_resets(),
            event_rate_hz: d.current_rate_hz(sm_core::monotonic_us()),
            battery: None,
        }
    }

    fn status(&self) -> BrokerStatus {
        let mut devs: Vec<DeviceStatus> = self.devices.iter().map(|d| self.device_status(d)).collect();
        let mut extra: Vec<&Candidate> = self.failed.values().map(|f| &f.cand).collect();
        if self.mode == OwnershipMode::IntegratedSpacenav {
            extra.extend(self.upstream_devices.iter());
        }
        for c in extra {
            let f = self.failed.get(&c.key);
            devs.push(DeviceStatus {
                id: self.known_keys.get(&c.key).copied().unwrap_or(u32::MAX),
                name: c.name.clone(),
                usb_id: format!("{:04x}:{:04x}", c.vid, c.pid),
                transport: if c.bus == 5 { "bluetooth".into() } else { "usb".into() },
                path: String::new(),
                input_path: match (self.mode, f) {
                    (OwnershipMode::IntegratedSpacenav, _) => "owned by spacenavd".into(),
                    (_, Some(f)) => f.reason.clone(),
                    _ => String::new(),
                },
                state: f.map(|f| f.state).unwrap_or(DeviceState::Discovered),
                support_status: c.model.as_ref().map(|m| m.status.as_str().to_string()).unwrap_or("generic".into()),
                generic: c.generic,
                buttons: c
                    .model
                    .as_ref()
                    .map(|m| m.buttons.iter().map(|b| ButtonLabel { index: b.index, label: b.label.clone() }).collect())
                    .unwrap_or_default(),
                axis_range: c.model.as_ref().and_then(|m| m.axis_range).unwrap_or(0),
                serial: sm_core::redact(&c.uniq),
                frames: 0,
                malformed: 0,
                watchdog_resets: 0,
                event_rate_hz: 0.0,
                battery: None,
            });
        }
        BrokerStatus {
            version: VERSION.into(),
            mode: self.mode,
            mode_reason: self.mode_reason.clone(),
            devices: devs,
            spnav_socket: self.spnav.as_ref().map(|_| self.args.spnav_socket.display().to_string()),
            spnav_clients: self.spnav.as_ref().map(|s| s.client_names()).unwrap_or_default(),
            spnav_motion_events: self.spnav.as_ref().map(|s| s.motion_events).unwrap_or(0),
            injection_available: self.uinput.is_some(),
            injection_reason: self.uinput_reason.clone(),
            virtual_joystick: self.gamepad.is_some(),
            locked: self.locked,
            app_mode: self.cfg.app_mode,
            profile: self.cfg.profile.clone(),
            config_owner_uid: self.cfg.owner_uid,
            active_uid: sm_ipc::active_seat_uid("seat0"),
            uptime_s: (sm_core::monotonic_us() - self.start_us) / 1_000_000,
        }
    }

    /// May this peer change state or inject input? (§8: permission-bound session control)
    fn authorized(&self, cred: &PeerCred) -> Result<(), String> {
        if cred.uid == 0 {
            return Ok(());
        }
        match sm_ipc::active_seat_uid("seat0") {
            Some(u) if u == cred.uid => Ok(()),
            Some(u) => Err(format!("uid {} is not the active seat user (uid {u})", cred.uid)),
            // no logind seat (container / headless dev): allow our own uid only
            None if cred.uid == sm_ipc::current_uid() => Ok(()),
            None => Err("no active logind seat".into()),
        }
    }

    fn accept_ctl(&mut self) {
        while let Ok((s, _)) = self.ctl_listener.accept() {
            if self.ctls.len() >= MAX_CTL_CLIENTS || s.set_nonblocking(true).is_err() {
                continue;
            }
            let Ok(cred) = sm_ipc::peer_cred(&s) else { continue };
            let id = self.next_ctl;
            self.next_ctl += 1;
            log!("debug", "control client {id} connected (uid {} pid {})", cred.uid, cred.pid);
            self.ctls.push(Ctl {
                id,
                s,
                cred,
                lb: LineBuf::default(),
                wb: WriteBuf::default(),
                motion_hz: None,
                last_motion_us: 0,
                last_motion_zero: true,
                buttons: false,
                devices: false,
                pressed: Vec::new(),
                inject_window_us: 0,
                inject_count: 0,
                dead: false,
            });
        }
    }

    fn handle_ctl(&mut self, id: u64) {
        let Some(c) = self.ctls.iter_mut().find(|c| c.id == id) else { return };
        let mut lines = Vec::new();
        let r = c.lb.read_from(&mut c.s, &mut lines);
        if r != ReadOutcome::Open {
            c.dead = true;
        }
        for l in lines.into_iter().take(256) {
            let reply = match serde_json::from_str::<Value>(&l) {
                Ok(req) => self.dispatch(id, &req),
                Err(e) => sm_ipc::err(&Value::Null, "bad_request", e.to_string()),
            };
            if let Some(c) = self.ctls.iter_mut().find(|c| c.id == id) {
                c.send(&reply);
            }
        }
    }

    fn dispatch(&mut self, cid: u64, req: &Value) -> Value {
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let cmd = req.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        let Some(cred) = self.ctls.iter().find(|c| c.id == cid).map(|c| c.cred) else { return Value::Null };
        if req.get("v").and_then(|v| v.as_u64()) != Some(sm_ipc::PROTO_VERSION as u64) {
            return sm_ipc::err(&id, "version", format!("protocol version {} required", sm_ipc::PROTO_VERSION));
        }
        match cmd {
            "hello" => sm_ipc::ok(
                &id,
                json!({"daemon": "3dmoused", "version": VERSION, "proto": sm_ipc::PROTO_VERSION, "uid": cred.uid, "can_control": self.authorized(&cred).is_ok()}),
            ),
            "status" => sm_ipc::ok(&id, serde_json::to_value(self.status()).unwrap_or(Value::Null)),
            "registry" => sm_ipc::ok(&id, serde_json::to_value(self.reg).unwrap_or(Value::Null)),
            "subscribe" => {
                let c = self.ctls.iter_mut().find(|c| c.id == cid).unwrap();
                c.motion_hz = req
                    .get("motion")
                    .and_then(|v| v.as_bool())
                    .filter(|b| *b)
                    .map(|_| req.get("max_hz").and_then(|h| h.as_u64()).unwrap_or(60).clamp(1, 1000) as u32);
                c.buttons = req.get("buttons").and_then(|v| v.as_bool()).unwrap_or(false);
                c.devices = req.get("devices").and_then(|v| v.as_bool()).unwrap_or(false);
                sm_ipc::ok(&id, json!({"subscribed": true}))
            }
            "set_config" | "clear_config" | "set_session" | "inject" | "release_all" | "set_mode" => {
                if let Err(e) = self.authorized(&cred) {
                    return sm_ipc::err(&id, "denied", e);
                }
                self.dispatch_control(cid, cred, cmd, req, &id)
            }
            _ => sm_ipc::err(&id, "unknown_command", format!("unknown command `{cmd}`")),
        }
    }

    fn apply_config(&mut self, cfg: ActiveConfig) {
        let was_motion = self.cfg.spnav_motion();
        self.cfg = cfg;
        if self.cfg.app_mode != AppMode::VirtualJoystick {
            // leaving game mode removes the controller (dropping it centres and destroys it)
            if self.gamepad.take().is_some() {
                log!("info", "virtual joystick removed");
            }
            self.gamepad_err = None;
        } else {
            // create the controller now, not on the first touch: games usually look for
            // controllers when they start
            let _ = self.joystick();
        }
        for p in self.pipelines.values_mut() {
            p.set_config(self.cfg.nav.clone());
        }
        // route change during motion: never leave a native app moving (FR-404)
        if was_motion && !self.cfg.spnav_motion() {
            if let Some(s) = &mut self.spnav {
                s.send_motion([0; 6], 0);
            }
        }
        log!("info", "active profile: {} ({:?})", self.cfg.profile, self.cfg.app_mode);
    }

    fn release_ctl_keys(&mut self, cid: u64) {
        let Some(c) = self.ctls.iter_mut().find(|c| c.id == cid) else { return };
        let keys: Vec<u16> = c.pressed.drain(..).rev().collect();
        if let Some(u) = &mut self.uinput {
            for k in keys {
                let _ = u.key(k, false);
            }
        }
    }

    fn dispatch_control(&mut self, cid: u64, cred: PeerCred, cmd: &str, req: &Value, id: &Value) -> Value {
        match cmd {
            "set_config" => {
                let nav: NavConfig = match req.get("nav").map(|n| serde_json::from_value(n.clone())) {
                    Some(Ok(n)) => n,
                    Some(Err(e)) => return sm_ipc::err(id, "invalid", e.to_string()),
                    None => NavConfig::default(),
                };
                if let Err(e) = nav.validate() {
                    return sm_ipc::err(id, "invalid", e);
                }
                // only an absent field means the default: a value we don't understand (e.g. a mode
                // from a newer agent) must not silently become another mode
                let app_mode: AppMode = match req.get("app_mode").map(|m| serde_json::from_value(m.clone())) {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => return sm_ipc::err(id, "invalid", format!("app_mode: {e}")),
                    None => AppMode::default(),
                };
                let spnav_buttons = match req.get("spnav_buttons") {
                    Some(Value::Object(m)) => {
                        let mut out = BTreeMap::new();
                        for (k, v) in m {
                            match (k.parse::<u8>(), v.as_u64()) {
                                (Ok(a), Some(b)) if (a as usize) < 64 && b < 64 => {
                                    out.insert(a, b as u8);
                                }
                                _ => return sm_ipc::err(id, "invalid", "spnav_buttons must map 0..63 -> 0..63"),
                            }
                        }
                        Some(out)
                    }
                    Some(_) => return sm_ipc::err(id, "invalid", "spnav_buttons must be an object mapping 0..63 -> 0..63"),
                    None => None,
                };
                let profile = req.get("profile").and_then(|p| p.as_str()).unwrap_or("unnamed").chars().take(64).collect();
                if let Some(prev) = self.cfg.owner_conn.filter(|p| *p != cid) {
                    self.release_ctl_keys(prev);
                }
                self.apply_config(ActiveConfig { nav, app_mode, spnav_buttons, profile, owner_conn: Some(cid), owner_uid: Some(cred.uid) });
                sm_ipc::ok(id, json!({"applied": true}))
            }
            "clear_config" => {
                // same rule as disconnect: a stale client must not clear a profile set after its own
                if self.cfg.owner_conn != Some(cid) {
                    return sm_ipc::err(id, "not_owner", "this connection does not own the active profile");
                }
                self.apply_config(ActiveConfig::default());
                sm_ipc::ok(id, json!({"applied": true}))
            }
            "set_session" => {
                // anything but a real boolean is refused: defaulting to `false` would unlock
                let Some(locked) = req.get("locked").and_then(|v| v.as_bool()) else {
                    return sm_ipc::err(id, "invalid", "locked must be true or false");
                };
                if locked && !self.locked {
                    log!("info", "session locked: input injection suspended");
                    if let Some(g) = &mut self.gamepad {
                        g.neutral();
                    }
                    for c in self.ctls.iter().map(|c| c.id).collect::<Vec<_>>() {
                        self.release_ctl_keys(c);
                    }
                }
                self.locked = locked;
                // game mode chosen while locked: the controller appears on unlock
                let _ = self.joystick();
                sm_ipc::ok(id, json!({"locked": locked}))
            }
            "release_all" => {
                self.release_ctl_keys(cid);
                sm_ipc::ok(id, json!({}))
            }
            "set_mode" => {
                let m: Option<OwnershipMode> = req.get("mode").and_then(|m| m.as_str()).and_then(|m| if m == "auto" { None } else { m.parse().ok() });
                if cred.uid != 0 && req.get("mode").and_then(|m| m.as_str()) != Some("auto") {
                    // only root may force a mode; users may only re-run auto
                    return sm_ipc::err(id, "denied", "forcing an ownership mode requires root");
                }
                self.resolve_mode(m);
                sm_ipc::ok(id, json!({"mode": self.mode}))
            }
            "inject" => self.inject(cid, req, id),
            _ => sm_ipc::err(id, "unknown_command", cmd.to_string()),
        }
    }

    fn inject(&mut self, cid: u64, req: &Value, id: &Value) -> Value {
        if self.locked {
            return sm_ipc::err(id, "locked", "session is locked; synthetic input is disabled (FR-506)");
        }
        let Some(u) = &mut self.uinput else {
            return sm_ipc::err(id, "unavailable", self.uinput_reason.clone());
        };
        let now = sm_core::monotonic_us();
        let c = self.ctls.iter_mut().find(|c| c.id == cid).unwrap();
        if now.saturating_sub(c.inject_window_us) >= 1_000_000 {
            c.inject_window_us = now;
            c.inject_count = 0;
        }
        c.inject_count += 1;
        if c.inject_count > MAX_INJECT_PER_SEC {
            return sm_ipc::err(id, "rate_limited", "too many injected events");
        }
        let r = if let Some(keys) = req.get("keys").and_then(|k| k.as_array()) {
            let codes: Vec<u16> = keys.iter().filter_map(|k| k.as_u64()).map(|k| k as u16).collect();
            if codes.len() != keys.len() || codes.is_empty() || codes.len() > 8 {
                return sm_ipc::err(id, "invalid", "keys must be 1..8 key codes");
            }
            let down = req.get("down").and_then(|d| d.as_bool());
            let res = match down {
                Some(d) => u.chord(&codes, d),
                None => u.chord(&codes, true).and_then(|_| u.chord(&codes, false)),
            };
            if res.is_ok() {
                match down {
                    Some(true) => {
                        for k in &codes {
                            if !c.pressed.contains(k) {
                                c.pressed.push(*k);
                            }
                        }
                    }
                    Some(false) => c.pressed.retain(|k| !codes.contains(k)),
                    None => {}
                }
            }
            res
        } else if let Some(v) = req.get("scroll") {
            let v120 = v.get("v").and_then(|x| x.as_i64()).unwrap_or(0).clamp(-12000, 12000) as i32;
            let h120 = v.get("h").and_then(|x| x.as_i64()).unwrap_or(0).clamp(-12000, 12000) as i32;
            let mut carry = (0, 0);
            u.scroll_hires(v120, h120, &mut carry)
        } else if let Some(r) = req.get("rel") {
            let code = r.get("code").and_then(|x| x.as_u64()).unwrap_or(999) as u16;
            let val = r.get("value").and_then(|x| x.as_i64()).unwrap_or(0) as i32;
            u.rel(code, val)
        } else {
            return sm_ipc::err(id, "invalid", "inject needs keys, scroll or rel");
        };
        match r {
            Ok(()) => sm_ipc::ok(id, json!({})),
            Err(e) => sm_ipc::err(id, "inject_failed", e.to_string()),
        }
    }

    fn reap_ctls(&mut self) {
        let dead: Vec<u64> = self.ctls.iter().filter(|c| c.dead).map(|c| c.id).collect();
        for d in dead {
            // crash-safety: a dying agent never leaves keys down (FR-306)
            self.release_ctl_keys(d);
            if self.cfg.owner_conn == Some(d) {
                log!("info", "config owner disconnected; reverting to built-in defaults");
                self.apply_config(ActiveConfig::default());
            }
            self.ctls.retain(|c| c.id != d);
        }
    }

    // --------------------------------------------------------- timers

    fn periodic(&mut self) {
        // user switch: revert config owned by an inactive user
        if let (Some(owner), Some(active)) = (self.cfg.owner_uid, sm_ipc::active_seat_uid("seat0")) {
            if owner != 0 && owner != active {
                log!("info", "active seat user changed ({owner} -> {active}); reverting profile and releasing keys");
                if let Some(c) = self.cfg.owner_conn {
                    self.release_ctl_keys(c);
                }
                self.apply_config(ActiveConfig::default());
            }
        }
        // spacenavd appeared and took over our socket path
        if self.mode == OwnershipMode::NativeOwner && self.args.mode.is_none() && self.spnav.is_some() && inode(&self.args.spnav_socket) != self.spnav_ino {
            log!("warn", "{} was replaced by another process", self.args.spnav_socket.display());
            self.set_mode(OwnershipMode::IntegratedSpacenav, "spacenavd started and took over the libspnav socket".into());
        }
        if self.mode == OwnershipMode::IntegratedSpacenav && self.upstream.is_none() {
            if socket_live(&self.args.spnav_socket) {
                self.connect_upstream();
            } else if self.args.mode.is_none() && !spacenavd_running() {
                self.set_mode(OwnershipMode::NativeOwner, "spacenavd is no longer running".into());
            }
        }
        if !self.failed.is_empty() {
            self.rescan_at.get_or_insert(sm_core::monotonic_us());
        }
        if std::env::var_os("WATCHDOG_USEC").is_some() {
            sd_notify("WATCHDOG=1");
        }
    }

    fn timeout_ms(&self, now: u64) -> i32 {
        let mut d = self.next_periodic.saturating_sub(now);
        if let Some(r) = self.rescan_at {
            d = d.min(r.saturating_sub(now));
        }
        for dev in &self.devices {
            if let Some(x) = dev.next_deadline_us(now) {
                d = d.min(x);
            }
            if self.pipelines.get(&dev.id).map(|p| !p.is_settled()).unwrap_or(false) {
                d = d.min(SMOOTH_TICK_US);
            }
        }
        (d.div_ceil(1000)).min(60_000) as i32
    }

    fn timers(&mut self) {
        let now = sm_core::monotonic_us();
        if self.rescan_at.map(|r| now >= r).unwrap_or(false) {
            self.rescan_at = None;
            self.rescan();
        }
        if now >= self.next_periodic {
            self.next_periodic = now + PERIODIC_US;
            self.periodic();
        }
        let ids: Vec<u32> = self.devices.iter().map(|d| d.id).collect();
        for id in ids {
            let f = self.devices.iter_mut().find(|d| d.id == id).and_then(|d| d.tick(now));
            if let Some(f) = f {
                log!("debug", "device {id}: watchdog zeroed stale axes");
                self.process_frame(id, f);
            }
            // keep the smoothing filter decaying when the device is silent
            let unsettled = self.pipelines.get(&id).map(|p| !p.is_settled()).unwrap_or(false);
            if unsettled {
                let d = self.devices.iter().find(|d| d.id == id).unwrap();
                if now.saturating_sub(d.last_frame_us) >= SMOOTH_TICK_US {
                    let raw_c = d.canonical.apply(d.state.axes);
                    self.emit_motion(id, raw_c, now);
                }
            }
        }
    }

    // --------------------------------------------------------- loop

    fn run(&mut self) {
        sd_notify("READY=1\nSTATUS=running");
        log!("info", "3dmoused {VERSION} started; control socket {}", self.args.control_socket.display());
        while !self.quit {
            let mut fds: Vec<libc::pollfd> = Vec::new();
            let mut tags: Vec<Tag> = Vec::new();
            let mut add = |fd: RawFd, ev: i16, t: Tag| {
                if fd >= 0 {
                    fds.push(libc::pollfd { fd, events: ev, revents: 0 });
                    tags.push(t);
                }
            };
            add(self.sigfd, libc::POLLIN, Tag::Sig);
            add(self.inotify, libc::POLLIN, Tag::Inotify);
            add(self.ctl_listener.as_raw_fd(), libc::POLLIN, Tag::CtlListen);
            for c in &self.ctls {
                let ev = libc::POLLIN | if c.wb.is_empty() { 0 } else { libc::POLLOUT };
                add(c.s.as_raw_fd(), ev, Tag::Ctl(c.id));
            }
            if let Some(s) = &self.spnav {
                add(s.listen_fd(), libc::POLLIN, Tag::SpnavListen);
                for (fd, w) in s.client_fds() {
                    add(fd, libc::POLLIN | if w { libc::POLLOUT } else { 0 }, Tag::SpnavClient);
                }
            }
            if let Some(u) = &self.upstream {
                add(u.fd(), libc::POLLIN, Tag::Upstream);
            }
            for d in &self.devices {
                add(d.fd(), libc::POLLIN, Tag::Dev(d.id));
            }
            let timeout = self.timeout_ms(sm_core::monotonic_us());
            let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
            if n < 0 {
                let e = std::io::Error::last_os_error();
                if e.kind() != std::io::ErrorKind::Interrupted {
                    log!("err", "poll: {e}");
                }
                continue;
            }
            for (pfd, tag) in fds.iter().zip(tags.iter()) {
                if pfd.revents == 0 {
                    continue;
                }
                match *tag {
                    Tag::Sig => {
                        let mut si: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
                        unsafe { libc::read(self.sigfd, &mut si as *mut _ as *mut libc::c_void, std::mem::size_of::<libc::signalfd_siginfo>()) };
                        if si.ssi_signo == libc::SIGHUP as u32 {
                            log!("info", "SIGHUP: rescanning devices and re-negotiating ownership");
                            self.resolve_mode(self.args.mode);
                        } else {
                            self.quit = true;
                        }
                    }
                    Tag::Inotify => {
                        drain(self.inotify);
                        // udev applies permissions shortly after node creation
                        self.rescan_at = Some(sm_core::monotonic_us() + 150_000);
                    }
                    Tag::CtlListen => self.accept_ctl(),
                    Tag::Ctl(id) => {
                        if pfd.revents & (libc::POLLHUP | libc::POLLERR) != 0 && pfd.revents & libc::POLLIN == 0 {
                            if let Some(c) = self.ctls.iter_mut().find(|c| c.id == id) {
                                c.dead = true;
                            }
                        } else {
                            self.handle_ctl(id);
                        }
                    }
                    Tag::SpnavListen => {
                        if let Some(s) = &mut self.spnav {
                            s.accept();
                        }
                    }
                    Tag::SpnavClient => {
                        if let Some(s) = &mut self.spnav {
                            s.service(pfd.fd);
                        }
                    }
                    Tag::Upstream => self.handle_upstream(),
                    Tag::Dev(id) => self.handle_device_readable(id),
                }
            }
            self.timers();
            for c in &mut self.ctls {
                if c.wb.flush(&mut c.s).is_err() {
                    c.dead = true;
                }
            }
            self.reap_ctls();
        }
        log!("info", "shutting down");
        sd_notify("STOPPING=1");
        for id in self.devices.iter().map(|d| d.id).collect::<Vec<_>>() {
            self.remove_device(id, "shutdown");
        }
        if let Some(u) = &mut self.uinput {
            u.release_all();
        }
        let _ = std::fs::remove_file(&self.args.control_socket);
    }
}

fn main() {
    let args = parse_args();
    match Broker::new(args) {
        Ok(mut b) => b.run(),
        Err(e) => {
            log!("err", "startup failed: {e}");
            std::process::exit(1);
        }
    }
}
