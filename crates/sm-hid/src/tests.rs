use super::*;
use proptest::prelude::*;

const COMPACT_DESC: &str = include_str!("../../../tests/fixtures/descriptors/256f-c635-usb.hex");
const COMPACT_CAPTURE: &str = include_str!("../../../tests/fixtures/evdev/256f-c635-usb-capture1.txt");

fn compact_layout() -> MotionLayout {
    let bytes = descriptor::from_hex(COMPACT_DESC).unwrap();
    let d = descriptor::parse(&bytes).unwrap();
    MotionLayout::from_descriptor(&d).unwrap()
}

#[test]
fn compact_descriptor_golden() {
    let l = compact_layout();
    assert!(l.uses_report_ids);
    assert!(l.is_multi_axis);
    assert_eq!(l.axis_range, 350);
    // translation in report 1, rotation in report 2, 16-bit signed LE
    for i in 0..3 {
        let f = l.axes[i].as_ref().unwrap();
        assert_eq!((f.report_id, f.bit_offset, f.bit_size), (1, i as u32 * 16, 16));
        assert_eq!((f.logical_min, f.logical_max), (-350, 350));
    }
    for i in 3..6 {
        let f = l.axes[i].as_ref().unwrap();
        assert_eq!((f.report_id, f.bit_offset), (2, (i as u32 - 3) * 16));
    }
    assert_eq!(l.buttons.len(), 2);
    assert!(l.buttons.iter().all(|(f, _)| f.report_id == 3));
    assert_eq!(l.axis_groups(), [1, 1, 1, 2, 2, 2]);
}

fn rep(id: u8, v: [i16; 3]) -> Vec<u8> {
    let mut r = vec![id];
    for x in v {
        r.extend_from_slice(&x.to_le_bytes());
    }
    r
}

#[test]
fn compact_hidraw_decode() {
    let mut d = HidrawDecoder::new(compact_layout(), vec![(0, 0), (1, 1)]);
    let f = d.decode(&rep(1, [100, -200, 350]), 5).unwrap().unwrap();
    assert_eq!(f.axes_present, 0b000111);
    assert_eq!(&f.axes[..3], &[100, -200, 350]);
    let f = d.decode(&rep(2, [-1, 0, 7]), 6).unwrap().unwrap();
    assert_eq!(f.axes_present, 0b111000);
    assert_eq!(&f.axes[3..], &[-1, 0, 7]);
    let f = d.decode(&[3, 0b10, 0], 7).unwrap().unwrap();
    assert!(f.buttons_present);
    assert_eq!(f.buttons, 0b10);
    // explicit neutral report arrives on hidraw (unlike evdev)
    let f = d.decode(&rep(1, [0, 0, 0]), 8).unwrap().unwrap();
    assert_eq!(f.axes_present, 0b000111);
    assert_eq!(f.axes, [0; 6]);
}

#[test]
fn hidraw_rejects_malformed() {
    let mut d = HidrawDecoder::new(compact_layout(), vec![]);
    assert_eq!(d.decode(&[], 0), Err(DecodeError::Empty));
    assert!(matches!(d.decode(&[1, 0, 0], 0), Err(DecodeError::Truncated { .. })));
    assert!(matches!(d.decode(&rep(1, [400, 0, 0]), 0), Err(DecodeError::OutOfRange { axis: 0, value: 400 })));
    assert_eq!(d.decode(&[0x55, 1, 2], 0), Ok(None));
    assert_eq!(d.malformed, 3);
}

fn compact_assembler() -> EvdevAssembler {
    EvdevAssembler::new([1, 1, 1, 2, 2, 2], vec![(256, 0), (257, 1)], DEFAULT_EVDEV_WATCHDOG_US)
}

#[test]
fn evdev_zero_fill_within_group() {
    let mut a = compact_assembler();
    let ev = |typ, code, value| InputEvent { t_us: 1000, typ, code, value };
    a.push(ev(EV_REL, 0, 10));
    a.push(ev(EV_REL, 1, 20));
    let f = a.push(ev(EV_SYN, 0, 0)).unwrap();
    assert_eq!(f.axes, [10, 20, 0, 0, 0, 0]);
    // next frame only carries Y: X must be inferred as zero, rotation untouched
    a.push(ev(EV_REL, 4, 9));
    a.push(ev(EV_SYN, 0, 0));
    a.push(ev(EV_REL, 1, 5));
    let f = a.push(ev(EV_SYN, 0, 0)).unwrap();
    assert_eq!(f.axes, [0, 5, 0, 0, 9, 0]);
    assert_eq!(f.axes_present, 0b000111);
}

#[test]
fn evdev_watchdog_zeroes_stale_group() {
    let mut a = compact_assembler();
    a.push(InputEvent { t_us: 0, typ: EV_REL, code: 4, value: -45 });
    a.push(InputEvent { t_us: 0, typ: EV_SYN, code: 0, value: 0 });
    assert!(a.tick(10_000).is_none());
    assert_eq!(a.next_deadline_us(10_000), Some(DEFAULT_EVDEV_WATCHDOG_US - 10_000));
    let f = a.tick(DEFAULT_EVDEV_WATCHDOG_US).unwrap();
    assert_eq!(f.axes, [0; 6]);
    assert_eq!(a.next_deadline_us(DEFAULT_EVDEV_WATCHDOG_US), None);
}

#[test]
fn evdev_buttons() {
    let mut a = compact_assembler();
    a.push(InputEvent { t_us: 0, typ: EV_KEY, code: 257, value: 1 });
    let f = a.push(InputEvent { t_us: 0, typ: EV_SYN, code: 0, value: 0 }).unwrap();
    assert!(f.buttons_present && f.buttons == 0b10 && f.axes_present == 0);
    a.push(InputEvent { t_us: 1, typ: EV_KEY, code: 300, value: 1 }); // unknown code ignored
    assert!(a.push(InputEvent { t_us: 1, typ: EV_SYN, code: 0, value: 0 }).is_none());
}

/// Replay the real capture: values stay within the descriptor range, the
/// stream never ends with residual motion once the watchdog has run.
#[test]
fn evdev_capture_replay_never_sticks() {
    let evs = parse_evdev_fixture(COMPACT_CAPTURE);
    assert!(evs.len() > 1000);
    let mut a = compact_assembler();
    let mut st = sm_core::InputState::default();
    let mut frames = 0;
    let mut watchdog = 0;
    let mut now = 0;
    for ev in &evs {
        // run the watchdog as the event loop would
        while let Some(d) = a.next_deadline_us(now) {
            if now + d > ev.t_us {
                break;
            }
            now += d;
            if let Some(f) = a.tick(now) {
                st.apply(&f);
                watchdog += 1;
            }
        }
        now = ev.t_us;
        if let Some(f) = a.push(*ev) {
            frames += 1;
            assert!(f.axes.iter().all(|v| v.abs() <= 350), "{f:?}");
            st.apply(&f);
        }
    }
    let end = now + DEFAULT_EVDEV_WATCHDOG_US;
    if let Some(f) = a.tick(end) {
        st.apply(&f);
    }
    assert!(frames > 500);
    assert!(watchdog >= 1, "capture contains a dropped-zero gap the watchdog must catch");
    assert!(st.is_neutral(), "residual motion {:?}", st.axes);
}

proptest! {
    #[test]
    fn descriptor_parser_never_panics(bytes in proptest::collection::vec(any::<u8>(), 1..512)) {
        let _ = descriptor::parse(&bytes);
    }

    #[test]
    fn hidraw_decoder_never_panics_and_bounds(report in proptest::collection::vec(any::<u8>(), 0..64)) {
        let mut d = HidrawDecoder::new(compact_layout(), vec![(0, 0), (1, 1)]);
        if let Ok(Some(f)) = d.decode(&report, 0) {
            prop_assert!(f.axes.iter().all(|v| v.abs() <= 350));
            prop_assert!(f.buttons < 4);
        }
    }

    #[test]
    fn evdev_assembler_bounded(evs in proptest::collection::vec((0u16..4, 0u16..300, -500i32..500), 0..200)) {
        let mut a = compact_assembler();
        let mut t = 0;
        for (typ, code, value) in evs {
            t += 1000;
            let _ = a.push(InputEvent { t_us: t, typ, code, value });
        }
        // after the watchdog, everything is neutral regardless of history
        let _ = a.push(InputEvent { t_us: t, typ: EV_SYN, code: SYN_REPORT, value: 0 });
        let _ = a.tick(t + DEFAULT_EVDEV_WATCHDOG_US);
        prop_assert_eq!(a.next_deadline_us(t + DEFAULT_EVDEV_WATCHDOG_US), None);
    }
}
