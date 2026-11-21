//! Opt-in desktop modes (FR-507) end to end through the agent, against a fake broker:
//! axis gestures (media control), pointer control (accessibility) and the virtual-joystick gate.
//! The fake broker answers every request, feeds motion events, and records what the agent asks
//! it to inject.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct Harness {
    dir: PathBuf,
    agent: Child,
    out: UnixStream,
    seen: Arc<Mutex<Vec<Value>>>,
}

impl Harness {
    fn start(name: &str, settings: &str) -> Harness {
        let dir = std::env::temp_dir().join(format!("sm-modes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/settings.toml"), settings).unwrap();
        let bsock = dir.join("b.sock");
        let listener = UnixListener::bind(&bsock).unwrap();
        let agent = Command::new(env!("CARGO_BIN_EXE_3dmouse-agent"))
            .env("THREEDMOUSE_AGENT_SOCKET", dir.join("a.sock"))
            .env("THREEDMOUSE_CONFIG_DIR", dir.join("config"))
            .env("THREEDMOUSE_BROKER_SOCKET", &bsock)
            // no desktop: focus tracking stays off, so the pinned profile is what's active
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
            .env_remove("SWAYSOCK")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let (s, _) = listener.accept().unwrap();
        let out = s.try_clone().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let rec = seen.clone();
        let mut reply = s.try_clone().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(s).lines() {
                let Ok(line) = line else { break };
                let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
                let result = match req["cmd"].as_str() {
                    Some("hello") => json!({"daemon": "fake", "can_control": true}),
                    _ => json!({}),
                };
                let _ = writeln!(reply, "{}", json!({"v": 1, "id": req["id"], "ok": true, "result": result}));
                rec.lock().unwrap().push(req);
            }
        });
        Harness { dir, agent, out, seen }
    }

    fn wait_for(&self, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Some(v) = self.seen.lock().unwrap().iter().find(|v| f(v)) {
                return v.clone();
            }
            assert!(t0.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn motion(&mut self, norm: [f32; 6]) {
        writeln!(self.out, "{}", json!({"v": 1, "event": "motion", "dev": 1, "norm": norm, "raw": [0, 0, 0, 0, 0, 0], "spnav": [0, 0, 0, 0, 0, 0]})).unwrap();
    }

    /// Hold `norm` for `ms`, one frame every 10 ms, then return to neutral.
    fn hold(&mut self, norm: [f32; 6], ms: u64) {
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(ms) {
            self.motion(norm);
            std::thread::sleep(Duration::from_millis(10));
        }
        self.motion([0.0; 6]);
        std::thread::sleep(Duration::from_millis(120));
    }

    fn injects(&self) -> Vec<Value> {
        self.seen.lock().unwrap().iter().filter(|v| v["cmd"] == "inject").cloned().collect()
    }

    fn clear(&self) {
        self.seen.lock().unwrap().clear();
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.agent.kill();
        let _ = self.agent.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn key_injects(h: &Harness, code: u64) -> usize {
    h.injects().iter().filter(|v| v["keys"] == json!([code])).count()
}

#[test]
fn media_control_gestures() {
    let mut h = Harness::start("media", "manual_profile = \"media-control\"\n");
    h.wait_for("media-control config", |v| v["cmd"] == "set_config" && v["profile"] == "media-control");
    std::thread::sleep(Duration::from_millis(200));
    h.clear();

    // twist clockwise (RY negative) for 700 ms: volume up now, again after 300 ms, then every 150 ms
    h.hold([0.0, 0.0, 0.0, 0.0, -0.8, 0.0], 700);
    let up = key_injects(&h, 115);
    assert!((3..=6).contains(&up), "volumeup fired {up} times");
    assert_eq!(key_injects(&h, 114), 0, "no volumedown");

    // a single push right: next track exactly once (no repeat configured)
    h.clear();
    h.hold([0.8, 0.0, 0.0, 0.0, 0.0, 0.0], 500);
    assert_eq!(key_injects(&h, 163), 1);

    // below the threshold nothing fires
    h.clear();
    h.hold([0.4, 0.0, 0.0, 0.0, 0.0, 0.0], 300);
    assert!(h.injects().is_empty(), "sub-threshold push injected {:?}", h.injects());

    // a held twist keeps repeating from the main loop even when the device sends no new frames
    h.clear();
    h.motion([0.0, 0.0, 0.0, 0.0, -0.8, 0.0]);
    std::thread::sleep(Duration::from_millis(700));
    h.motion([0.0; 6]);
    std::thread::sleep(Duration::from_millis(100));
    let up = key_injects(&h, 115);
    assert!(up >= 3, "held gesture repeated only {up} times without frames");
}

#[test]
fn pointer_control_moves_the_pointer() {
    let mut h = Harness::start("pointer", "manual_profile = \"accessibility-pointer\"\n");
    h.wait_for("pointer config", |v| v["cmd"] == "set_config" && v["profile"] == "accessibility-pointer");
    std::thread::sleep(Duration::from_millis(200));
    h.clear();
    // full right for 500 ms at 1200 px/s: about 600 px to the right, nothing vertical
    h.hold([1.0, 0.0, 0.0, 0.0, 0.0, 0.0], 500);
    let rel = |h: &Harness, code: u64| -> i64 { h.injects().iter().filter(|v| v["rel"]["code"] == code).map(|v| v["rel"]["value"].as_i64().unwrap()).sum() };
    let x = rel(&h, 0);
    assert!((350..=800).contains(&x), "moved {x} px right");
    assert_eq!(rel(&h, 1), 0);
    // pushing away moves the pointer up (negative Y)
    h.clear();
    h.hold([0.0, 0.0, 1.0, 0.0, 0.0, 0.0], 300);
    let y = rel(&h, 1);
    assert!(y < -150, "moved {y} px vertically");
}

#[test]
fn virtual_joystick_needs_permission() {
    let h = Harness::start("joy-off", "manual_profile = \"game-joystick\"\n");
    let cfg = h.wait_for("game-joystick config", |v| v["cmd"] == "set_config" && v["profile"] == "game-joystick");
    assert_eq!(cfg["app_mode"], "none", "virtual joystick requested without permission");
    drop(h);

    let h = Harness::start("joy-on", "manual_profile = \"game-joystick\"\nallow_virtual_joystick = true\n");
    let cfg = h.wait_for("game-joystick config", |v| v["cmd"] == "set_config" && v["profile"] == "game-joystick");
    assert_eq!(cfg["app_mode"], "virtual-joystick");
}
