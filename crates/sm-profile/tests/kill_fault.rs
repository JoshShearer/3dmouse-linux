//! Kill-fault injection for profile writes (SRS §11 "profiles: atomic").
//!
//! The parent re-runs this test binary as a child that saves the same profile in a tight loop,
//! with a different (and differently sized) notes field each time, then SIGKILLs it at varying
//! moments. After every kill the profile on disk must still parse, validate, and be exactly one
//! of the complete versions the child wrote, never a torn or empty file.

use sm_profile::{Profile, Store};
use std::process::Command;
use std::time::Duration;

const CHILD_ENV: &str = "SM_KILL_FAULT_CHILD_DIR";

fn version(i: u64) -> Profile {
    let mut p = Profile::new("victim", "Victim");
    // vary the size so a torn write would be detectable as a truncated document
    p.notes = format!("v{i}:{}", "x".repeat((i as usize * 37) % 1500));
    p
}

#[test]
fn kill_fault_child() {
    let Some(dir) = std::env::var_os(CHILD_ENV) else { return };
    let store = Store { dir: dir.into() };
    let mut i = 0u64;
    loop {
        store.save(&version(i)).unwrap();
        i += 1;
    }
}

#[test]
fn profile_survives_sigkill_during_save() {
    let dir = std::env::temp_dir().join(format!("sm-kill-fault-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store { dir: dir.clone() };
    store.save(&version(0)).unwrap();
    let exe = std::env::current_exe().unwrap();
    let cycles = 60;
    let mut advanced = 0;
    for n in 0..cycles {
        let mut child = Command::new(&exe)
            .args(["--exact", "kill_fault_child", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, &dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // 5..~65 ms: lands at different points of the write/fsync/rename sequence
        std::thread::sleep(Duration::from_millis(5 + (n * 7919 % 61)));
        unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
        let _ = child.wait();

        let text = std::fs::read_to_string(store.path_for("victim")).unwrap_or_else(|e| panic!("cycle {n}: profile missing after SIGKILL: {e}"));
        let p = Profile::from_toml(&text).unwrap_or_else(|e| panic!("cycle {n}: torn profile after SIGKILL: {e}"));
        let i: u64 = p.notes.strip_prefix('v').and_then(|r| r.split(':').next()).and_then(|v| v.parse().ok()).expect("notes marker");
        assert_eq!(p.notes, version(i).notes, "cycle {n}: content is not a complete version");
        if i > 0 {
            advanced += 1;
        }
        // the lock must not be left stuck either
        store.save(&version(0)).unwrap_or_else(|e| panic!("cycle {n}: save after SIGKILL failed: {e}"));
    }
    let _ = std::fs::remove_dir_all(&dir);
    // the child must actually have been writing when killed, or the test proves nothing
    assert!(advanced > cycles / 2, "only {advanced}/{cycles} kills landed after the child started writing");
}
