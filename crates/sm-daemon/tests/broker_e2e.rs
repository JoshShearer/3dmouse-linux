//! End-to-end broker test without hardware: a fake sysfs tree describes a
//! SpaceMouse Compact whose evdev node is a FIFO we write `input_event`s
//! into. Exercises discovery, the evdev assembler + watchdog, the pipeline,
//! the libspnav server, control-socket auth/config, and disconnect safety.

use serde_json::json;
use sm_spnav::{ClientEvent, SpnavClient};
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;

struct Env {
    dir: PathBuf,
    broker: Child,
    fifo: Option<File>,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = self.broker.kill();
        let _ = self.broker.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn ev(typ: u16, code: u16, value: i32) -> [u8; 24] {
    let mut b = [0u8; 24];
    b[16..18].copy_from_slice(&typ.to_ne_bytes());
    b[18..20].copy_from_slice(&code.to_ne_bytes());
    b[20..24].copy_from_slice(&value.to_ne_bytes());
    b
}

fn wait_for(what: &str, timeout_ms: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(timeout_ms) {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn setup(name: &str) -> Env {
    setup_with(name, "none")
}

fn setup_with(name: &str, uinput: &str) -> Env {
    let dir = std::env::temp_dir().join(format!("sm-e2e-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let hid = dir.join("sys/devices/0003:256F:C635.0001");
    std::fs::create_dir_all(hid.join("input/input9/event9")).unwrap();
    std::fs::write(hid.join("uevent"), "HID_ID=0003:0000256F:0000C635\nHID_NAME=3Dconnexion SpaceMouse Compact\nHID_PHYS=usb-test/input0\nHID_UNIQ=\n")
        .unwrap();
    let desc = sm_hid::descriptor::from_hex(include_str!("../../../tests/fixtures/descriptors/256f-c635-usb.hex")).unwrap();
    std::fs::write(hid.join("report_descriptor"), desc).unwrap();
    std::fs::create_dir_all(dir.join("sys/class/hidraw/hidraw9")).unwrap();
    std::os::unix::fs::symlink(&hid, dir.join("sys/class/hidraw/hidraw9/device")).unwrap();
    std::fs::create_dir_all(dir.join("dev/input")).unwrap();
    let fifo = dir.join("dev/input/event9");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    // O_RDWR keeps the FIFO open without blocking and without EOF for the broker
    let w = std::fs::OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(&fifo).unwrap();
    let broker = Command::new(env!("CARGO_BIN_EXE_3dmoused"))
        .args(["--mode", "native-owner", "--input", "evdev", "--uinput", uinput])
        .arg("--sysfs")
        .arg(dir.join("sys"))
        .arg("--dev-root")
        .arg(dir.join("dev"))
        .arg("--spnav-socket")
        .arg(dir.join("spnav.sock"))
        .arg("--control-socket")
        .arg(dir.join("broker.sock"))
        .spawn()
        .expect("spawn 3dmoused");
    let e = Env { dir, broker, fifo: Some(w) };
    wait_for("control socket", 5000, || e.dir.join("broker.sock").exists());
    wait_for("device ready", 5000, || status(&e)["devices"].as_array().map(|d| d.iter().any(|d| d["state"] == "ready")).unwrap_or(false));
    e
}

fn ctl(e: &Env) -> sm_ipc::Client {
    sm_ipc::Client::connect(&e.dir.join("broker.sock")).unwrap()
}

fn status(e: &Env) -> serde_json::Value {
    sm_ipc::Client::connect(&e.dir.join("broker.sock")).and_then(|mut c| c.call("status", json!({})).map_err(std::io::Error::other)).unwrap_or_default()
}

fn write(e: &mut Env, evs: &[[u8; 24]]) {
    let f = e.fifo.as_mut().unwrap();
    for x in evs {
        f.write_all(x).unwrap();
    }
}

fn collect(c: &mut SpnavClient, ms: u64) -> Vec<ClientEvent> {
    let mut out = Vec::new();
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(ms) {
        out.extend(c.read_events().unwrap().into_iter().filter(|e| *e != ClientEvent::Other));
        std::thread::sleep(Duration::from_millis(5));
    }
    out
}

fn spnav(e: &Env) -> SpnavClient {
    let c = SpnavClient::connect(&e.dir.join("spnav.sock"), "e2e", sm_spnav::EVMASK_MOTION | sm_spnav::EVMASK_BUTTON).unwrap();
    assert_eq!(c.proto, 1, "protocol v1 negotiated");
    c
}

#[test]
fn motion_buttons_watchdog_and_unplug() {
    let mut e = setup("main");
    let mut c = spnav(&e);
    // push right (X), HID +Y toward user (canonical -Z): report 1 group
    write(&mut e, &[ev(EV_REL, 0, 200), ev(EV_REL, 1, 100), ev(EV_SYN, 0, 0)]);
    let evs = collect(&mut c, 150);
    let m = evs
        .iter()
        .find_map(|e| {
            if let ClientEvent::Motion(m, _) = e {
                if m[0] != 0 {
                    Some(*m)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("motion delivered");
    assert!(m[0] > 150, "x ≈ 200 after default 2% deadzone rescale: {m:?}");
    assert!(m[2] < -50, "HID +Y maps to canonical -Z: {m:?}");
    // the kernel never sends the zero; the evdev watchdog must
    let last = evs.iter().rev().find_map(|e| if let ClientEvent::Motion(m, _) = e { Some(*m) } else { None }).unwrap();
    assert_eq!(last, [0; 6], "watchdog returned motion to neutral (never stuck)");

    // buttons
    write(&mut e, &[ev(EV_KEY, 256, 1), ev(EV_SYN, 0, 0)]);
    assert!(collect(&mut c, 80).contains(&ClientEvent::Button(0, true)));
    write(&mut e, &[ev(EV_KEY, 256, 0), ev(EV_SYN, 0, 0)]);
    assert!(collect(&mut c, 80).contains(&ClientEvent::Button(0, false)));

    // unplug while a button is held and the cap is deflected
    write(&mut e, &[ev(EV_KEY, 257, 1), ev(EV_REL, 3, 120), ev(EV_SYN, 0, 0)]);
    collect(&mut c, 30);
    e.fifo = None; // writer closed -> EOF on the broker side
    let evs = collect(&mut c, 300);
    assert!(evs.contains(&ClientEvent::Button(1, false)), "held button released on disconnect: {evs:?}");
    assert!(evs.iter().any(|e| matches!(e, ClientEvent::Motion(m, _) if *m == [0; 6])), "motion zeroed on disconnect");
}

#[test]
fn control_socket_config_routing_and_owner_revert() {
    let mut e = setup("ctl");
    let mut c = spnav(&e);
    let mut owner = ctl(&e);
    let hello = owner.call("hello", json!({})).unwrap();
    assert_eq!(hello["can_control"], true, "test user is the active seat user or no seat exists");
    // invalid config is rejected, not applied
    assert!(owner.call("set_config", json!({"nav": {"master_gain": 1e9}})).is_err());
    assert!(owner.call("set_config", json!({"spnav_buttons": {"99": 0}})).is_err());
    // desktop-emulation: no spnav motion, buttons follow the map
    owner.call("set_config", json!({"app_mode": "desktop-emulation", "profile": "desktop", "spnav_buttons": {}})).unwrap();
    assert_eq!(status(&e)["profile"], "desktop");
    write(&mut e, &[ev(EV_REL, 0, 300), ev(EV_SYN, 0, 0), ev(EV_KEY, 256, 1), ev(EV_SYN, 0, 0), ev(EV_KEY, 256, 0), ev(EV_SYN, 0, 0)]);
    let evs = collect(&mut c, 150);
    assert!(!evs.iter().any(|e| matches!(e, ClientEvent::Motion(m, _) if m[0] != 0)), "no native motion in desktop mode: {evs:?}");
    assert!(!evs.iter().any(|e| matches!(e, ClientEvent::Button(..))), "unmapped button not forwarded");
    // gain is applied
    owner.call("set_config", json!({"app_mode": "native6dof", "nav": {"master_gain": 0.5, "deadzone": [0, 0, 0, 0, 0, 0]}})).unwrap();
    write(&mut e, &[ev(EV_REL, 0, 300), ev(EV_SYN, 0, 0)]);
    let evs = collect(&mut c, 40);
    let m = evs
        .iter()
        .find_map(|e| {
            if let ClientEvent::Motion(m, _) = e {
                if m[0] != 0 {
                    Some(*m)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("motion");
    assert!((m[0] - 150).abs() <= 2, "half gain: {m:?}");
    // the owning agent dies -> broker reverts to built-in defaults
    drop(owner);
    wait_for("revert", 2000, || status(&e)["profile"] == "builtin-default");
    // inject without uinput reports a clear error
    let mut o2 = ctl(&e);
    let r = o2.call("inject", json!({"keys": [30]})).unwrap_err();
    assert!(r.contains("--uinput none"), "{r}");
    assert!(o2.call("frobnicate", json!({})).unwrap_err().contains("unknown"));
}

/// An optional field that is supplied but not understood (e.g. a mode from a newer agent) is
/// rejected before it touches the active profile; it is never treated as absent (issue #1).
#[test]
fn set_config_rejects_unknown_optional_fields_and_keeps_the_profile() {
    let mut e = setup("badcfg");
    let mut c = spnav(&e);
    let mut owner = ctl(&e);
    owner.call("set_config", json!({"app_mode": "desktop-emulation", "profile": "desktop", "spnav_buttons": {}})).unwrap();
    for bad in [
        json!({"app_mode": "future_mode", "profile": "bad"}),
        json!({"app_mode": 5, "profile": "bad"}),
        json!({"app_mode": null, "profile": "bad"}),
        json!({"app_mode": "desktop-emulation", "spnav_buttons": 5, "profile": "bad"}),
        json!({"app_mode": "desktop-emulation", "spnav_buttons": [0, 1], "profile": "bad"}),
        json!({"app_mode": "desktop-emulation", "spnav_buttons": null, "profile": "bad"}),
    ] {
        let r = owner.call("set_config", bad.clone());
        assert!(r.as_ref().is_err_and(|m| m.starts_with("invalid: ")), "{bad} must be rejected as invalid, got {r:?}");
        assert_eq!(status(&e)["profile"], "desktop", "{bad} must not replace the active profile");
    }
    // the previous profile still routes input: no native motion, no unmapped buttons
    write(&mut e, &[ev(EV_REL, 0, 300), ev(EV_SYN, 0, 0), ev(EV_KEY, 256, 1), ev(EV_SYN, 0, 0), ev(EV_KEY, 256, 0), ev(EV_SYN, 0, 0)]);
    let evs = collect(&mut c, 150);
    assert!(!evs.iter().any(|e| matches!(e, ClientEvent::Motion(m, _) if *m != [0; 6])), "still desktop mode: {evs:?}");
    assert!(!evs.iter().any(|e| matches!(e, ClientEvent::Button(..))), "still the empty button map: {evs:?}");
    // absent fields still mean the defaults
    owner.call("set_config", json!({"profile": "plain"})).unwrap();
    assert_eq!(status(&e)["profile"], "plain");
    write(&mut e, &[ev(EV_REL, 0, 300), ev(EV_SYN, 0, 0), ev(EV_KEY, 256, 1), ev(EV_SYN, 0, 0)]);
    let evs = collect(&mut c, 150);
    assert!(evs.iter().any(|e| matches!(e, ClientEvent::Motion(m, _) if m[0] != 0)), "native motion by default: {evs:?}");
    assert!(evs.contains(&ClientEvent::Button(0, true)), "identity button map by default: {evs:?}");
}

/// A stale client cannot clear a profile another client set after it (issue #2).
#[test]
fn clear_config_only_clears_the_callers_own_profile() {
    let e = setup("clear");
    let mut a = ctl(&e);
    let mut b = ctl(&e);
    a.call("set_config", json!({"profile": "from-a"})).unwrap();
    b.call("set_config", json!({"profile": "from-b"})).unwrap();
    let r = a.call("clear_config", json!({}));
    assert!(r.as_ref().is_err_and(|m| m.starts_with("not_owner: ")), "stale clear must be refused, got {r:?}");
    assert_eq!(status(&e)["profile"], "from-b");
    // the refusal leaves ownership alone: B may still clear
    b.call("clear_config", json!({})).unwrap();
    assert_eq!(status(&e)["profile"], "builtin-default");
}

/// The virtual joystick exists as soon as game mode is active, before the cap is touched,
/// waits for unlock when chosen on a locked session, and goes away with the mode.
/// Needs a writable /dev/uinput, so it runs only with SM_UINPUT_TEST=1.
#[test]
fn virtual_joystick_follows_the_mode_without_input() {
    if std::env::var_os("SM_UINPUT_TEST").is_none() {
        return;
    }
    let e = setup_with("joy", "/dev/uinput");
    let mut owner = ctl(&e);
    owner.call("hello", json!({})).unwrap();
    let listed = || std::fs::read_to_string("/proc/bus/input/devices").unwrap().contains("3DMouse Linux virtual joystick");
    owner.call("set_config", json!({"app_mode": "virtual-joystick", "profile": "game"})).unwrap();
    wait_for("joystick created without input", 2000, || status(&e)["virtual_joystick"] == true && listed());
    owner.call("set_config", json!({"app_mode": "native6dof"})).unwrap();
    wait_for("joystick removed", 2000, || status(&e)["virtual_joystick"] == false && !listed());
    owner.call("set_session", json!({"locked": true})).unwrap();
    owner.call("set_config", json!({"app_mode": "virtual-joystick", "profile": "game"})).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(status(&e)["virtual_joystick"], false, "no controller while locked");
    owner.call("set_session", json!({"locked": false})).unwrap();
    wait_for("joystick created on unlock", 2000, || status(&e)["virtual_joystick"] == true);
}

#[test]
fn second_instance_refuses_and_stale_socket_is_reclaimed() {
    let e = setup("dup");
    let out = Command::new(env!("CARGO_BIN_EXE_3dmoused"))
        .args(["--mode", "native-owner", "--uinput", "none"])
        .arg("--sysfs")
        .arg(e.dir.join("nosys"))
        .arg("--spnav-socket")
        .arg(e.dir.join("spnav2.sock"))
        .arg("--control-socket")
        .arg(e.dir.join("broker.sock"))
        .output()
        .unwrap();
    assert!(!out.status.success(), "a second broker must not steal a live control socket");
    // a live foreign spnav socket is never stolen either
    let mut c = ctl(&e);
    let st = c.call("status", json!({})).unwrap();
    assert!(st["spnav_socket"].as_str().unwrap().ends_with("spnav.sock"));

    // stale (dead) socket file is reclaimed
    let stale = e.dir.join("stale.sock");
    drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());
    assert!(Path::new(&stale).exists());
    let mut b = Command::new(env!("CARGO_BIN_EXE_3dmoused"))
        .args(["--mode", "native-owner", "--uinput", "none"])
        .arg("--sysfs")
        .arg(e.dir.join("nosys"))
        .arg("--spnav-socket")
        .arg(e.dir.join("spnav3.sock"))
        .arg("--control-socket")
        .arg(&stale)
        .spawn()
        .unwrap();
    wait_for("reclaimed", 3000, || sm_ipc::Client::connect(&stale).is_ok());
    let _ = b.kill();
    let _ = b.wait();
}

#[test]
fn auto_mode_yields_to_existing_spnav_server() {
    // something (spacenavd) already serves the libspnav socket
    let dir = std::env::temp_dir().join(format!("sm-e2e-{}-auto", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let theirs = sm_spnav::SpnavServer::bind(&dir.join("spnav.sock")).unwrap();
    let mut b = Command::new(env!("CARGO_BIN_EXE_3dmoused"))
        .args(["--uinput", "none"])
        .arg("--sysfs")
        .arg(dir.join("nosys"))
        .arg("--spnav-socket")
        .arg(dir.join("spnav.sock"))
        .arg("--control-socket")
        .arg(dir.join("broker.sock"))
        .spawn()
        .unwrap();
    wait_for("control socket", 3000, || dir.join("broker.sock").exists());
    let st = sm_ipc::Client::connect(&dir.join("broker.sock")).unwrap().call("status", json!({})).unwrap();
    assert_eq!(st["mode"], "integrated-spacenav", "{st}");
    assert!(st["spnav_socket"].is_null(), "did not take over the socket");
    drop(theirs);
    let _ = b.kill();
    let _ = b.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
