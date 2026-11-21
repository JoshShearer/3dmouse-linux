//! SIGHUP must reload profiles, never terminate the agent. Regression test: the focus thread
//! used to start before the signal mask was set, so SIGHUP killed the process (status 129) and
//! systemd treated that as a clean exit and did not restart it.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn sighup_reloads_and_keeps_running() {
    let dir = std::env::temp_dir().join(format!("sm-agent-hup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("a.sock");
    let mut child = Command::new(env!("CARGO_BIN_EXE_3dmouse-agent"))
        .env("THREEDMOUSE_AGENT_SOCKET", &sock)
        .env("THREEDMOUSE_CONFIG_DIR", dir.join("config"))
        .env("THREEDMOUSE_BROKER_SOCKET", dir.join("no-broker.sock"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while !sock.exists() {
        assert!(t0.elapsed() < Duration::from_secs(10), "agent did not create its socket");
        std::thread::sleep(Duration::from_millis(20));
    }
    for _ in 0..3 {
        unsafe { libc::kill(child.id() as i32, libc::SIGHUP) };
        std::thread::sleep(Duration::from_millis(150));
    }
    let status = child.try_wait().unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(status.is_none(), "agent exited on SIGHUP: {status:?}");
}
