//! Focused-application identity providers (FR-401, FR-505).
//!
//! | Desktop            | Provider          | Mechanism                                    |
//! |--------------------|-------------------|----------------------------------------------|
//! | Hyprland           | `hyprland`        | `.socket2.sock` event stream + `j/activewindow` |
//! | Sway / i3-ipc      | `sway`            | i3-ipc SUBSCRIBE ["window"]                  |
//! | GNOME (Wayland)    | `gnome-extension` | bundled GNOME Shell extension pushes focus to the agent socket |
//! | X11 (any WM)       | `x11`             | `_NET_ACTIVE_WINDOW` via `xprop`, 2 Hz         |
//! | COSMIC             | `cosmic`          | Wayland `ext_foreign_toplevel_list_v1` + `zcosmic_toplevel_info_v1` (activated state) |
//! | KDE Plasma (KWin)  | `kwin`            | agent loads a KWin script that calls `ReportFocus` on the agent's D-Bus name |
//!
//! Window titles can contain document names: they are kept in memory for
//! opt-in title rules only and are never logged or exported (FR-402, FR-900).

use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct FocusInfo {
    /// Wayland app_id or X11 WM_CLASS (class part).
    pub app_id: String,
    #[serde(skip_serializing)]
    pub title: String,
    pub pid: Option<u32>,
    pub provider: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Hyprland,
    Sway,
    GnomeExtension,
    Kwin,
    Cosmic,
    X11,
    None,
}

#[derive(Debug, Clone, Serialize)]
pub struct DesktopInfo {
    pub session_type: String,
    pub current_desktop: String,
    pub provider: ProviderKind,
    pub provider_note: String,
}

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_default()
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
}

/// Locate the Hyprland instance directory even when the agent was started
/// by systemd without HYPRLAND_INSTANCE_SIGNATURE in its environment.
pub fn hyprland_dir() -> Option<PathBuf> {
    let base = runtime_dir().join("hypr");
    let sig = env("HYPRLAND_INSTANCE_SIGNATURE");
    if !sig.is_empty() && base.join(&sig).join(".socket2.sock").exists() {
        return Some(base.join(sig));
    }
    // newest instance with a live socket
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in std::fs::read_dir(&base).ok()?.flatten() {
        let p = e.path();
        let s2 = p.join(".socket2.sock");
        if s2.exists() && UnixStream::connect(&s2).is_ok() {
            let m = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(t, _)| m > *t).unwrap_or(true) {
                best = Some((m, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

pub fn detect() -> DesktopInfo {
    let session_type = env("XDG_SESSION_TYPE");
    let current_desktop = env("XDG_CURRENT_DESKTOP");
    let (provider, note) = if hyprland_dir().is_some() {
        (ProviderKind::Hyprland, "Hyprland IPC event socket".to_string())
    } else if !env("SWAYSOCK").is_empty() {
        (ProviderKind::Sway, "sway i3-ipc window events".into())
    } else if current_desktop.to_ascii_uppercase().contains("GNOME") && (session_type == "wayland" || !env("WAYLAND_DISPLAY").is_empty()) {
        (ProviderKind::GnomeExtension, "requires the bundled GNOME Shell extension 3dmouse-linux@joshshearer.github.io (gnome-extensions enable …)".into())
    } else if current_desktop.to_ascii_uppercase().contains("COSMIC") {
        (ProviderKind::Cosmic, "COSMIC toplevel-info Wayland protocol".into())
    } else if current_desktop.to_ascii_uppercase().contains("KDE") {
        // KWin runs both Wayland and X11 Plasma sessions; the script works in either.
        (ProviderKind::Kwin, "KWin script reporting focus over D-Bus (loaded automatically)".into())
    } else if !env("DISPLAY").is_empty() && env("WAYLAND_DISPLAY").is_empty() {
        (ProviderKind::X11, "X11 _NET_ACTIVE_WINDOW (requires xprop)".into())
    } else {
        (ProviderKind::None, "no supported focus provider for this session".into())
    };
    DesktopInfo { session_type, current_desktop, provider, provider_note: note }
}

/// Spawn the provider thread. Each focus change is sent on `tx`; `wake` is
/// written to so a poll(2) loop notices immediately.
pub fn spawn(kind: ProviderKind, tx: Sender<FocusInfo>, wake: std::os::fd::RawFd) {
    let notify = move |f: FocusInfo, tx: &Sender<FocusInfo>| {
        let _ = tx.send(f);
        // SAFETY: writing one byte to a pipe fd owned by the agent.
        unsafe { libc::write(wake, b"f".as_ptr() as *const libc::c_void, 1) };
    };
    std::thread::Builder::new()
        .name(format!("focus-{kind:?}"))
        .spawn(move || loop {
            let r = match kind {
                ProviderKind::Hyprland => run_hyprland(&tx, &notify),
                ProviderKind::Sway => run_sway(&tx, &notify),
                ProviderKind::X11 => run_x11(&tx, &notify),
                ProviderKind::Kwin => run_kwin(&tx, &notify),
                ProviderKind::Cosmic => crate::cosmic::run(|f| notify(f, &tx)),
                _ => return,
            };
            if let Err(e) = r {
                eprintln!("focus provider {kind:?}: {e}; retrying in 2s");
            }
            std::thread::sleep(Duration::from_secs(2));
        })
        .ok();
}

/// Raw JSON reply to a Hyprland request socket command such as `j/monitors`.
pub fn hypr_request(cmd: &str) -> Option<serde_json::Value> {
    let dir = hyprland_dir()?;
    let mut s = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    s.write_all(cmd.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    serde_json::from_str(&out).ok()
}

type Notify<'a> = &'a dyn Fn(FocusInfo, &Sender<FocusInfo>);

fn hypr_query(dir: &std::path::Path) -> Option<FocusInfo> {
    let mut s = UnixStream::connect(dir.join(".socket.sock")).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
    s.write_all(b"j/activewindow").ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    let v: serde_json::Value = serde_json::from_str(&out).ok()?;
    Some(FocusInfo {
        app_id: v["class"].as_str().unwrap_or("").to_string(),
        title: v["title"].as_str().unwrap_or("").to_string(),
        pid: v["pid"].as_u64().map(|p| p as u32),
        provider: "hyprland".into(),
    })
}

/// Parse one socket2 line; returns (class, title) for `activewindow>>`.
pub fn parse_hypr_event(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("activewindow>>")?;
    let (class, title) = rest.split_once(',').unwrap_or((rest, ""));
    Some((class.to_string(), title.to_string()))
}

fn run_hyprland(tx: &Sender<FocusInfo>, notify: Notify) -> std::io::Result<()> {
    let dir = hyprland_dir().ok_or_else(|| std::io::Error::other("hyprland socket not found"))?;
    if let Some(f) = hypr_query(&dir) {
        notify(f, tx);
    }
    let s = UnixStream::connect(dir.join(".socket2.sock"))?;
    for line in BufReader::new(s).lines() {
        let line = line?;
        if let Some((class, title)) = parse_hypr_event(&line) {
            // query for pid (cheap, local); fall back to the event data
            let f = hypr_query(&dir).filter(|f| f.app_id == class).unwrap_or(FocusInfo { app_id: class, title, pid: None, provider: "hyprland".into() });
            notify(f, tx);
        }
    }
    Err(std::io::Error::other("hyprland event socket closed"))
}

fn i3_msg(s: &mut UnixStream, typ: u32, payload: &[u8]) -> std::io::Result<()> {
    let mut m = b"i3-ipc".to_vec();
    m.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    m.extend_from_slice(&typ.to_ne_bytes());
    m.extend_from_slice(payload);
    s.write_all(&m)
}

fn i3_read(s: &mut UnixStream) -> std::io::Result<(u32, serde_json::Value)> {
    let mut h = [0u8; 14];
    s.read_exact(&mut h)?;
    if &h[..6] != b"i3-ipc" {
        return Err(std::io::Error::other("bad i3-ipc magic"));
    }
    let len = u32::from_ne_bytes([h[6], h[7], h[8], h[9]]) as usize;
    let typ = u32::from_ne_bytes([h[10], h[11], h[12], h[13]]);
    if len > 16 << 20 {
        return Err(std::io::Error::other("i3-ipc message too large"));
    }
    let mut b = vec![0u8; len];
    s.read_exact(&mut b)?;
    Ok((typ, serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null)))
}

pub fn sway_focus_from_container(c: &serde_json::Value) -> FocusInfo {
    let app = c["app_id"].as_str().filter(|s| !s.is_empty()).or_else(|| c["window_properties"]["class"].as_str()).unwrap_or("");
    FocusInfo {
        app_id: app.to_string(),
        title: c["name"].as_str().unwrap_or("").to_string(),
        pid: c["pid"].as_u64().map(|p| p as u32),
        provider: "sway".into(),
    }
}

fn run_sway(tx: &Sender<FocusInfo>, notify: Notify) -> std::io::Result<()> {
    let mut s = UnixStream::connect(env("SWAYSOCK"))?;
    i3_msg(&mut s, 2, br#"["window"]"#)?; // SUBSCRIBE
    loop {
        let (typ, v) = i3_read(&mut s)?;
        if typ & 0x8000_0000 != 0 && v["change"] == "focus" {
            notify(sway_focus_from_container(&v["container"]), tx);
        }
    }
}

fn xprop(args: &[&str]) -> Option<String> {
    let o = std::process::Command::new("xprop").args(args).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Parse `WM_CLASS(STRING) = "blender", "Blender"` -> "Blender" (class part).
pub fn parse_wm_class(s: &str) -> Option<String> {
    let line = s.lines().find(|l| l.starts_with("WM_CLASS"))?;
    let parts: Vec<&str> = line.split('"').collect();
    parts.get(3).or(parts.get(1)).map(|s| s.to_string())
}

fn run_x11(tx: &Sender<FocusInfo>, notify: Notify) -> std::io::Result<()> {
    let mut last = FocusInfo::default();
    loop {
        if let Some(root) = xprop(&["-root", "_NET_ACTIVE_WINDOW"]) {
            if let Some(id) = root.split_whitespace().last().filter(|s| s.starts_with("0x")) {
                if let Some(w) = xprop(&["-id", id, "WM_CLASS", "_NET_WM_PID", "_NET_WM_NAME"]) {
                    let pid = w.lines().find(|l| l.starts_with("_NET_WM_PID")).and_then(|l| l.split('=').nth(1)).and_then(|p| p.trim().parse().ok());
                    let title = w
                        .lines()
                        .find(|l| l.starts_with("_NET_WM_NAME"))
                        .and_then(|l| l.split_once('='))
                        .map(|(_, t)| t.trim().trim_matches('"').to_string())
                        .unwrap_or_default();
                    let f = FocusInfo { app_id: parse_wm_class(&w).unwrap_or_default(), title, pid, provider: "x11".into() };
                    if f != last {
                        last = f.clone();
                        notify(f, tx);
                    }
                }
            }
        } else {
            return Err(std::io::Error::other("xprop unavailable or no X display"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

// ---------- KDE Plasma / KWin ----------

pub const KWIN_BUS_NAME: &str = "io.github.joshshearer._3dmouse_linux.Agent";
pub const KWIN_OBJECT: &str = "/io/github/joshshearer/_3dmouse_linux/Agent";
const KWIN_PLUGIN: &str = "3dmouse-linux-focus";

/// Runs inside KWin (5.27 and 6.x APIs). Arguments are strings so the JS
/// number marshalling of `callDBus` can't change the signature.
pub const KWIN_SCRIPT: &str = r#"// 3dmouse-linux focus bridge, loaded by 3dmouse-agent
function smReport(w) {
    if (!w) return;
    callDBus("io.github.joshshearer._3dmouse_linux.Agent", "/io/github/joshshearer/_3dmouse_linux/Agent",
             "io.github.joshshearer._3dmouse_linux.Agent", "ReportFocus",
             String(w.resourceClass || w.desktopFileName || ""), String(w.caption || ""), String(w.pid || 0));
}
if (workspace.windowActivated) workspace.windowActivated.connect(smReport);
else workspace.clientActivated.connect(smReport);
smReport(workspace.activeWindow || workspace.activeClient);
"#;

/// Turn a `ReportFocus(s app, s title, s pid)` call into focus info.
pub fn kwin_focus_from_call(m: &crate::dbus::Message) -> Option<FocusInfo> {
    if m.member.as_deref() != Some("ReportFocus") || m.path.as_deref() != Some(KWIN_OBJECT) {
        return None;
    }
    let app_id = m.body.first()?.as_str()?.to_string();
    let title = m.body.get(1).and_then(|a| a.as_str()).unwrap_or("").to_string();
    let pid = m.body.get(2).and_then(|a| a.as_u64()).filter(|p| *p > 0 && *p <= u32::MAX as u64).map(|p| p as u32);
    Some(FocusInfo { app_id, title, pid, provider: "kwin".into() })
}

fn kwin_load_script(c: &mut crate::dbus::Conn) -> std::io::Result<()> {
    use crate::dbus::Arg;
    let dir = runtime_dir().join("3dmouse");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("kwin-focus.js");
    std::fs::write(&path, KWIN_SCRIPT)?;
    let s = "org.kde.kwin.Scripting";
    // a previous agent instance may have left it loaded
    let _ = c.call("org.kde.KWin", "/Scripting", s, "unloadScript", vec![Arg::Str(KWIN_PLUGIN.into())]);
    c.call("org.kde.KWin", "/Scripting", s, "loadScript", vec![Arg::Str(path.display().to_string()), Arg::Str(KWIN_PLUGIN.into())])?;
    c.call("org.kde.KWin", "/Scripting", s, "start", vec![])?;
    Ok(())
}

/// Serve `ReportFocus` on the session bus. `load_script` is false in tests
/// (no KWin there); the caller then plays KWin's part.
pub fn serve_kwin(c: &mut crate::dbus::Conn, load_script: bool, mut on_focus: impl FnMut(FocusInfo)) -> std::io::Result<()> {
    use crate::dbus::{Arg, METHOD_CALL, SIGNAL};
    if !c.request_name(KWIN_BUS_NAME)? {
        return Err(std::io::Error::other(format!("{KWIN_BUS_NAME} is owned by another process (second agent?)")));
    }
    // reload the script whenever KWin (re)starts
    c.call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "AddMatch",
        vec![Arg::Str("type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged',arg0='org.kde.KWin'".into())],
    )?;
    if load_script {
        if let Err(e) = kwin_load_script(c) {
            eprintln!("focus provider kwin: loading the KWin script failed: {e}; waiting for KWin");
        }
    }
    c.set_read_timeout(None)?;
    loop {
        let m = c.read()?;
        match m.typ {
            METHOD_CALL => {
                if let Some(f) = kwin_focus_from_call(&m) {
                    c.reply(&m, vec![])?;
                    on_focus(f);
                } else if m.member.as_deref() == Some("Ping") {
                    c.reply(&m, vec![])?;
                } else {
                    c.reply_error(&m, "org.freedesktop.DBus.Error.UnknownMethod", "only ReportFocus is supported")?;
                }
            }
            SIGNAL if m.member.as_deref() == Some("NameOwnerChanged") => {
                let new_owner = m.body.get(2).and_then(Arg::as_str).unwrap_or("");
                if load_script && !new_owner.is_empty() {
                    c.set_read_timeout(Some(Duration::from_secs(5)))?;
                    if let Err(e) = kwin_load_script(c) {
                        eprintln!("focus provider kwin: reloading the KWin script failed: {e}");
                    }
                    c.set_read_timeout(None)?;
                }
            }
            _ => {}
        }
    }
}

fn run_kwin(tx: &Sender<FocusInfo>, notify: Notify) -> std::io::Result<()> {
    let mut c = crate::dbus::Conn::session()?;
    serve_kwin(&mut c, true, |f| notify(f, tx))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hypr_events() {
        assert_eq!(parse_hypr_event("activewindow>>blender,Blender [scene.blend]"), Some(("blender".into(), "Blender [scene.blend]".into())));
        assert_eq!(parse_hypr_event("activewindow>>,"), Some(("".into(), "".into())));
        assert_eq!(parse_hypr_event("workspace>>2"), None);
    }
    #[test]
    fn wm_class() {
        assert_eq!(parse_wm_class("WM_CLASS(STRING) = \"blender\", \"Blender\"\n").as_deref(), Some("Blender"));
        assert_eq!(parse_wm_class("WM_CLASS(STRING) = \"xterm\"\n").as_deref(), Some("xterm"));
    }
    #[test]
    fn sway_container() {
        let v = serde_json::json!({"app_id": null, "window_properties": {"class": "FreeCAD"}, "name": "x", "pid": 42});
        let f = sway_focus_from_container(&v);
        assert_eq!(f.app_id, "FreeCAD");
        assert_eq!(f.pid, Some(42));
    }
}
