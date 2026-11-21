// Property-based fuzzing of untrusted input (SRS §12). Case count: SM_FUZZ_CASES (default 512).
use proptest::prelude::*;

fn cases() -> u32 {
    std::env::var("SM_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)
}

use sm_ipc::{LineBuf, ReadOutcome, MAX_LINE};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]
    /// Peers on the agent/broker sockets send arbitrary bytes: lines are split correctly,
    /// never contain a newline, and the buffer never exceeds MAX_LINE without reporting overflow.
    #[test]
    fn line_reader(data in proptest::collection::vec(any::<u8>(), 0..4096), nl_every in 0usize..200) {
        let mut data = data;
        if nl_every > 0 {
            for i in (0..data.len()).step_by(nl_every) { data[i] = b'\n'; }
        }
        let mut lb = LineBuf::default();
        let mut out = Vec::new();
        let r = lb.read_from(&mut std::io::Cursor::new(&data), &mut out);
        prop_assert!(r == ReadOutcome::Closed || r == ReadOutcome::Overflow);
        for l in &out {
            prop_assert!(!l.contains('\n'));
            prop_assert!(!l.is_empty());
            prop_assert!(l.len() <= MAX_LINE);
            // what the daemons do next: JSON parse must not panic either
            let _ = serde_json::from_str::<serde_json::Value>(l);
        }
    }
}

#[test]
fn line_reader_overflow_is_reported() {
    let big = vec![b'a'; MAX_LINE + 10];
    let mut lb = LineBuf::default();
    let mut out = Vec::new();
    assert_eq!(lb.read_from(&mut std::io::Cursor::new(&big), &mut out), ReadOutcome::Overflow);
    assert!(out.is_empty());
}
