// Property-based fuzzing of untrusted input (SRS §12). Case count: SM_FUZZ_CASES (default 512).
use proptest::prelude::*;

fn cases() -> u32 {
    std::env::var("SM_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]
    /// Key chords come from imported profiles.
    #[test]
    fn chord_parse(s in "\\PC{0,80}") {
        if let Ok(codes) = sm_actions::keys::parse_chord(&s) {
            prop_assert!(codes.len() <= 16);
        }
    }

    /// Imported actions: arbitrary JSON objects tagged with a known type must deserialize,
    /// validate and sanitize without panicking.
    #[test]
    fn action_json(t in prop_oneof![Just("key"), Just("macro"), Just("radial"), Just("exec"), Just("launch"), Just("scroll"), Just("view")], v in "\\PC{0,200}") {
        let doc = format!("{{\"type\":\"{t}\",\"keys\":{v:?},\"steps\":[{{\"type\":\"delay\",\"ms\":5}}],\"items\":[],\"argv\":[{v:?}],\"desktop_id\":{v:?}}}");
        if let Ok(mut a) = serde_json::from_str::<sm_actions::Action>(&doc) {
            let _ = a.validate();
            let _ = a.sanitize_import();
        }
    }
}
