// Property-based fuzzing of untrusted input (SRS §12). Case count: SM_FUZZ_CASES (default 512).
use proptest::prelude::*;

fn cases() -> u32 {
    std::env::var("SM_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

use sm_desktop::{cosmic, dbus, focus};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]
    /// Messages from the session bus.
    #[test]
    fn dbus_decode(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        if bytes.len() >= 16 {
            let fixed: [u8; 16] = bytes[..16].try_into().unwrap();
            let _ = dbus::frame_len(&fixed);
        }
        if let Some(m) = dbus::decode(&bytes) {
            let _ = focus::kwin_focus_from_call(&m);
        }
    }

    /// A valid D-Bus header followed by garbage reaches the body decoder.
    #[test]
    fn dbus_decode_after_valid_header(tail in proptest::collection::vec(any::<u8>(), 0..256), flip in 0usize..64, val in any::<u8>()) {
        let m = dbus::Message {
            typ: dbus::METHOD_CALL,
            serial: 7,
            path: Some("/io/github/joshshearer/_3dmouse_linux/Agent".into()),
            interface: Some("io.github.joshshearer._3dmouse_linux.Agent".into()),
            member: Some("ReportFocus".into()),
            body: vec![dbus::Arg::Str("a".into()), dbus::Arg::Str("b".into())],
            ..Default::default()
        };
        let mut b = dbus::encode(&m);
        if flip < b.len() { b[flip] = val; }
        b.extend_from_slice(&tail);
        if let Some(m) = dbus::decode(&b) {
            let _ = focus::kwin_focus_from_call(&m);
        }
    }

    /// Hyprland event socket lines and xprop output.
    #[test]
    fn focus_line_parsers(s in "\\PC{0,300}") {
        let _ = focus::parse_hypr_event(&s);
        let _ = focus::parse_wm_class(&s);
    }

    /// Wayland events from the compositor (COSMIC toplevel tracking).
    #[test]
    fn cosmic_tracker(events in proptest::collection::vec((1u32..12, 0u16..8, proptest::collection::vec(any::<u8>(), 0..96)), 0..60)) {
        let mut t = cosmic::Tracker::new(3, 4, 5);
        for (obj, op, args) in events {
            let _ = t.event(obj, op, &args);
        }
    }
}
