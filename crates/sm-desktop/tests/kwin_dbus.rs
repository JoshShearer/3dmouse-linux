//! The KWin focus provider against a real, private dbus-daemon. The test
//! plays KWin's part and calls ReportFocus the way the KWin script does.
use sm_desktop::dbus::{Arg, Conn};
use sm_desktop::focus::{self, KWIN_BUS_NAME, KWIN_OBJECT};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn kwin_report_focus_over_dbus() {
    if Command::new("dbus-daemon").arg("--version").stdout(Stdio::null()).status().is_err() {
        eprintln!("dbus-daemon not installed; skipping");
        return;
    }
    let dir = std::env::temp_dir().join(format!("smdb{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("bus");
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", &format!("--address=unix:path={}", sock.display())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if sock.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    let (tx, rx) = mpsc::channel();
    let s2 = sock.clone();
    std::thread::spawn(move || {
        let mut c = Conn::connect(&s2).unwrap();
        let _ = focus::serve_kwin(&mut c, false, |f| {
            let _ = tx.send(f);
        });
    });

    let mut kwin = Conn::connect(&sock).unwrap();
    // wait until the provider owns its name
    let mut owned = false;
    for _ in 0..100 {
        let r =
            kwin.call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "NameHasOwner", vec![Arg::Str(KWIN_BUS_NAME.into())]).unwrap();
        if r.body.first() == Some(&Arg::Bool(true)) {
            owned = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(owned, "provider never claimed {KWIN_BUS_NAME}");

    let args = vec![Arg::Str("blender".into()), Arg::Str("Blender [scene.blend]".into()), Arg::Str("4242".into())];
    kwin.call(KWIN_BUS_NAME, KWIN_OBJECT, KWIN_BUS_NAME, "ReportFocus", args).unwrap();
    let f = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!((f.app_id.as_str(), f.title.as_str(), f.pid, f.provider.as_str()), ("blender", "Blender [scene.blend]", Some(4242), "kwin"));

    // a number argument (as callDBus would send a JS number) is accepted too
    kwin.call(KWIN_BUS_NAME, KWIN_OBJECT, KWIN_BUS_NAME, "ReportFocus", vec![Arg::Str("org.kde.dolphin".into()), Arg::Str("".into()), Arg::F64(17.0)]).unwrap();
    let f = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!((f.app_id.as_str(), f.pid), ("org.kde.dolphin", Some(17)));

    // anything else is refused with an error, not silently accepted
    let e = kwin.call(KWIN_BUS_NAME, KWIN_OBJECT, KWIN_BUS_NAME, "Inject", vec![]).unwrap_err();
    assert!(e.to_string().contains("UnknownMethod"), "{e}");
    // a second provider can't steal the name
    let mut other = Conn::connect(&sock).unwrap();
    assert!(focus::serve_kwin(&mut other, false, |_| {}).is_err());

    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&dir);
}
