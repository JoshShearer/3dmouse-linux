//! Desktop integration: virtual input (broker side), focus and lock
//! detection (session side). See docs/compatibility/desktop-matrix.md.

pub mod cosmic;
pub mod dbus;
pub mod focus;
pub mod uinput;

/// Screen-lock detection (FR-506, §4.6 rule 4).
///
/// Combines logind's `LockedHint` (set by GNOME Shell, KDE and lockers that
/// call SetLockedHint) with the presence of a known screen locker process
/// owned by this user (hyprlock, swaylock, …), because not every Wayland
/// locker sets the hint.
pub mod lock {
    pub const LOCKERS: &[&str] = &["hyprlock", "swaylock", "gtklock", "waylock", "i3lock", "xsecurelock", "slock", "swaylock-effects"];

    /// A known locker process owned by this user. Runs every second, so it stays cheap on hosts
    /// with many processes: ownership comes from the /proc entry's metadata (one stat), and only
    /// this user's processes have their `comm` read.
    pub fn locker_running() -> Option<String> {
        use std::io::Read;
        use std::os::unix::fs::MetadataExt;
        let uid = unsafe { libc::getuid() };
        let mut buf = [0u8; 32];
        for e in std::fs::read_dir("/proc").ok()?.flatten() {
            let name = e.file_name();
            if !name.as_encoded_bytes().iter().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let p = e.path();
            if !std::fs::metadata(&p).is_ok_and(|m| m.uid() == uid) {
                continue;
            }
            let Ok(mut f) = std::fs::File::open(p.join("comm")) else { continue };
            let n = f.read(&mut buf).unwrap_or(0);
            let comm = std::str::from_utf8(&buf[..n]).unwrap_or("").trim_end();
            if LOCKERS.contains(&comm) {
                return Some(comm.to_string());
            }
        }
        None
    }

    /// Desktops whose own lock screen always sets logind's LockedHint (GNOME Shell, KDE).
    /// There the hint is authoritative and the /proc walk is skipped.
    pub fn desktop_sets_locked_hint(current_desktop: &str) -> bool {
        current_desktop.split(':').any(|d| matches!(d.to_ascii_uppercase().as_str(), "GNOME" | "KDE"))
    }

    pub fn logind_locked_hint() -> Option<bool> {
        let sid = std::env::var("XDG_SESSION_ID").ok();
        let mut cmd = std::process::Command::new("loginctl");
        match sid {
            Some(s) if !s.is_empty() => cmd.args(["show-session", &s, "-p", "LockedHint", "--value"]),
            _ => cmd.args(["show-session", "auto", "-p", "LockedHint", "--value"]),
        };
        let o = cmd.output().ok()?;
        if !o.status.success() {
            return None;
        }
        match String::from_utf8_lossy(&o.stdout).trim() {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        }
    }

    /// Hyprland exposes no lock state directly, but while any client holds an
    /// ext-session-lock every monitor reports `LOCK` in `solitaryBlockedBy`.
    /// This catches in-process lockers (e.g. Omarchy's Quickshell lock) that
    /// neither run a known locker binary nor set logind's LockedHint.
    /// Returns None when undetermined (not Hyprland, or no readable monitor).
    pub fn hyprland_session_locked(monitors: &serde_json::Value) -> Option<bool> {
        let mons = monitors.as_array()?;
        let blockers = |m: &serde_json::Value| -> Vec<String> {
            m["solitaryBlockedBy"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
        };
        if mons.iter().any(|m| blockers(m).iter().any(|b| b == "LOCK")) {
            return Some(true);
        }
        // Hyprland stops at the first reason; a monitor without a workspace
        // never reaches the lock check, so only other monitors are conclusive.
        if mons.iter().any(|m| m.get("solitaryBlockedBy").is_some() && !blockers(m).iter().any(|b| b == "WORKSPACE")) {
            return Some(false);
        }
        None
    }

    /// Returns (locked, reason).
    pub fn is_locked() -> (bool, String) {
        // On Hyprland every locker (hyprlock, swaylock, Quickshell…) goes through
        // ext-session-lock, so a conclusive answer there is authoritative and
        // spares the /proc walk and the loginctl fork on every poll.
        if let Some(m) = crate::focus::hypr_request("j/monitors") {
            match hyprland_session_locked(&m) {
                Some(true) => return (true, "Hyprland session lock".into()),
                Some(false) => return (false, String::new()),
                None => {}
            }
        }
        let hint = logind_locked_hint();
        if hint.is_some() && desktop_sets_locked_hint(&std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()) {
            return if hint == Some(true) { (true, "logind LockedHint".into()) } else { (false, String::new()) };
        }
        if let Some(l) = locker_running() {
            return (true, format!("locker process {l}"));
        }
        if hint == Some(true) {
            return (true, "logind LockedHint".into());
        }
        (false, String::new())
    }
}

#[cfg(test)]
mod lock_tests {
    use super::lock::{desktop_sets_locked_hint, hyprland_session_locked, locker_running};
    use serde_json::json;

    #[test]
    fn locked_hint_desktops() {
        assert!(desktop_sets_locked_hint("ubuntu:GNOME"));
        assert!(desktop_sets_locked_hint("KDE"));
        assert!(desktop_sets_locked_hint("pop:GNOME"));
        assert!(!desktop_sets_locked_hint("Hyprland"));
        assert!(!desktop_sets_locked_hint("sway"));
        assert!(!desktop_sets_locked_hint("COSMIC"));
        assert!(!desktop_sets_locked_hint(""));
    }

    #[test]
    fn finds_a_locker_owned_by_this_user() {
        // a process whose comm is a known locker name: copy `sleep` under that name
        let dir = std::env::temp_dir().join(format!("sm-locker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("swaylock");
        std::fs::copy("/bin/sleep", &fake).unwrap();
        assert_ne!(locker_running().as_deref(), Some("swaylock"), "a real swaylock is running; test is inconclusive");
        let mut child = std::process::Command::new(&fake).arg("5").spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let found = locker_running();
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.as_deref(), Some("swaylock"));
    }

    #[test]
    fn hyprland_lock_from_monitors() {
        assert_eq!(hyprland_session_locked(&json!([{"solitaryBlockedBy": ["LOCK"]}])), Some(true));
        assert_eq!(hyprland_session_locked(&json!([{"solitaryBlockedBy": ["WINDOWED", "CANDIDATE"]}])), Some(false));
        assert_eq!(hyprland_session_locked(&json!([{"solitaryBlockedBy": ["WORKSPACE"]}])), None);
        assert_eq!(hyprland_session_locked(&json!([{"solitaryBlockedBy": ["WORKSPACE"]}, {"solitaryBlockedBy": ["LOCK"]}])), Some(true));
        assert_eq!(hyprland_session_locked(&json!([{"name": "old-hyprland"}])), None);
        assert_eq!(hyprland_session_locked(&json!({})), None);
    }
}
