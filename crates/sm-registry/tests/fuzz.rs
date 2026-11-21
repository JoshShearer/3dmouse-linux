// Property-based fuzzing of untrusted input (SRS §12). Case count: SM_FUZZ_CASES (default 512).
use proptest::prelude::*;

fn cases() -> u32 {
    std::env::var("SM_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]
    /// Registry overrides are user-editable TOML.
    #[test]
    fn registry_parse(s in "\\PC{0,600}") {
        let _ = sm_registry::Registry::parse(&s);
    }
}
