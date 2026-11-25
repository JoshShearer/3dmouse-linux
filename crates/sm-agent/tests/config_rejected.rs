//! A profile the broker refuses must be visible in the log at the default level. `denied` is the
//! exception: a read-only session is reported once at connect, not on every profile change.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Run the agent against a broker that refuses `set_config` with `code`; returns the agent's log.
fn agent_log(code: &str) -> String {
    let dir = std::env::temp_dir().join(format!("sm-rejected-{code}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("config")).unwrap();
    let bsock = dir.join("b.sock");
    let listener = UnixListener::bind(&bsock).unwrap();
    let mut agent = Command::new(env!("CARGO_BIN_EXE_3dmouse-agent"))
        .env("THREEDMOUSE_AGENT_SOCKET", dir.join("a.sock"))
        .env("THREEDMOUSE_CONFIG_DIR", dir.join("config"))
        .env("THREEDMOUSE_BROKER_SOCKET", &bsock)
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .env_remove("SWAYSOCK")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (s, _) = listener.accept().unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut reply = s.try_clone().unwrap();
    let mut rejected = false;
    for line in BufReader::new(s).lines() {
        let Ok(line) = line else { break };
        let req: Value = serde_json::from_str(&line).unwrap();
        let r = match req["cmd"].as_str() {
            Some("hello") => json!({"v": 1, "id": req["id"], "ok": true, "result": {"can_control": code != "denied"}}),
            Some("set_config") => {
                rejected = true;
                json!({"v": 1, "id": req["id"], "ok": false, "error": {"code": code, "message": "test refusal"}})
            }
            _ => json!({"v": 1, "id": req["id"], "ok": true, "result": {}}),
        };
        writeln!(reply, "{r}").unwrap();
        if rejected {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(500)); // let the agent read the refusal
    let _ = agent.kill();
    let mut log = String::new();
    agent.stderr.take().unwrap().read_to_string(&mut log).unwrap();
    let _ = agent.wait();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(rejected, "the agent never sent set_config: {log}");
    log
}

#[test]
fn rejected_config_is_logged_at_warn() {
    let log = agent_log("invalid");
    assert!(log.lines().any(|l| l.starts_with("<4>") && l.contains("set_config") && l.contains("test refusal")), "{log}");
}

#[test]
fn denied_config_is_not_repeated_in_the_log() {
    let log = agent_log("denied");
    assert!(log.contains("running read-only"), "{log}");
    assert!(!log.contains("test refusal"), "{log}");
}
