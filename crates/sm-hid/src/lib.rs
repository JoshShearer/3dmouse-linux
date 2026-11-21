//! HID/evdev decoding for 6-DOF devices (FR-201, FR-208, §4.4).
//!
//! Two input paths, both producing [`sm_core::RawFrame`] in the device frame:
//! * [`HidrawDecoder`] – descriptor-driven decoding of raw HID reports.
//!   Preferred: every report, including the final all-zero one, is delivered.
//! * [`EvdevAssembler`] – fallback through the kernel input layer. The input
//!   core drops `EV_REL` events whose value is 0, so a report that returns an
//!   axis to neutral may produce *no event at all*. Measured on a SpaceMouse
//!   Compact (tests/fixtures/evdev/256f-c635-usb-capture1.txt): a 136 ms gap
//!   followed a last value of RY=-45. The assembler therefore (a) zero-fills
//!   axes missing from a frame that belongs to the same HID report group and
//!   (b) zeroes a group when no frame for it arrived within a watchdog period.

pub mod descriptor;

use descriptor::{Descriptor, Field, AXIS_USAGES, PAGE_BUTTON, PAGE_GENERIC_DESKTOP, USAGE_MULTI_AXIS};
use sm_core::{RawFrame, AXES, MAX_BUTTONS};

/// Where each axis and button lives in the HID reports.
#[derive(Debug, Clone)]
pub struct MotionLayout {
    pub uses_report_ids: bool,
    pub axes: [Option<Field>; AXES],
    /// (field, 0-based HID button bit = usage - 1)
    pub buttons: Vec<(Field, u16)>,
    /// Largest |logical| extent over the axis fields (normalisation hint).
    pub axis_range: i32,
    pub is_multi_axis: bool,
}

impl MotionLayout {
    pub fn from_descriptor(d: &Descriptor) -> Result<MotionLayout, String> {
        let mut axes: [Option<Field>; AXES] = Default::default();
        let mut buttons = Vec::new();
        for f in &d.fields {
            if f.usage_page == PAGE_GENERIC_DESKTOP {
                if let Some(i) = AXIS_USAGES.iter().position(|u| *u == f.usage) {
                    if axes[i].is_none() {
                        axes[i] = Some(f.clone());
                    }
                }
            } else if f.usage_page == PAGE_BUTTON && f.usage >= 1 && f.bit_size == 1 {
                buttons.push((f.clone(), f.usage - 1));
            }
        }
        if axes.iter().any(|a| a.is_none()) {
            return Err("descriptor does not declare all six Generic Desktop axes (X..Rz)".into());
        }
        let axis_range = axes.iter().flatten().map(|f| f.logical_max.max(-f.logical_min)).max().unwrap_or(0);
        if axis_range <= 0 {
            return Err("axis logical range is empty".into());
        }
        let is_multi_axis = d.applications.contains(&(PAGE_GENERIC_DESKTOP, USAGE_MULTI_AXIS));
        Ok(MotionLayout { uses_report_ids: d.uses_report_ids, axes, buttons, axis_range, is_multi_axis })
    }

    /// Axis groups: axes delivered by the same HID report share a group id.
    pub fn axis_groups(&self) -> [u8; AXES] {
        let mut g = [0u8; AXES];
        for (i, f) in self.axes.iter().enumerate() {
            g[i] = f.as_ref().map(|f| f.report_id).unwrap_or(i as u8 + 100);
        }
        g
    }
}

fn extract(payload: &[u8], f: &Field) -> Option<i32> {
    let end = f.bit_offset + f.bit_size;
    if (end as usize).div_ceil(8) > payload.len() {
        return None;
    }
    let mut v: u64 = 0;
    for b in 0..f.bit_size {
        let bit = f.bit_offset + b;
        let byte = payload[(bit / 8) as usize];
        if byte >> (bit % 8) & 1 == 1 {
            v |= 1 << b;
        }
    }
    let val = if f.signed() && f.bit_size < 64 && (v >> (f.bit_size - 1)) & 1 == 1 { (v as i64 - (1i64 << f.bit_size)) as i32 } else { v as i32 };
    Some(val)
}

/// Maps a HID button bit to a logical button index.
pub type ButtonMap = Vec<(u16, u8)>;

#[derive(Debug, Clone)]
pub struct HidrawDecoder {
    pub layout: MotionLayout,
    /// hid bit -> logical index. Empty = identity for bits < MAX_BUTTONS.
    pub button_map: ButtonMap,
    pub malformed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    UnknownReport(u8),
    Truncated { report_id: u8, len: usize },
    OutOfRange { axis: usize, value: i32 },
}

impl HidrawDecoder {
    pub fn new(layout: MotionLayout, button_map: ButtonMap) -> Self {
        HidrawDecoder { layout, button_map, malformed: 0 }
    }

    fn map_button(&self, bit: u16) -> Option<u8> {
        if self.button_map.is_empty() {
            return if (bit as usize) < MAX_BUTTONS { Some(bit as u8) } else { None };
        }
        self.button_map.iter().find(|(b, _)| *b == bit).map(|(_, i)| *i)
    }

    /// Decode one hidraw read. Returns Ok(None) for reports that carry
    /// nothing we use (e.g. vendor/status reports).
    pub fn decode(&mut self, report: &[u8], t_us: u64) -> Result<Option<RawFrame>, DecodeError> {
        let r = self.decode_inner(report, t_us);
        if r.is_err() {
            self.malformed += 1;
        }
        r
    }

    fn decode_inner(&self, report: &[u8], t_us: u64) -> Result<Option<RawFrame>, DecodeError> {
        if report.is_empty() {
            return Err(DecodeError::Empty);
        }
        let (rid, payload) = if self.layout.uses_report_ids { (report[0], &report[1..]) } else { (0, report) };
        let mut f = RawFrame { t_us, ..Default::default() };
        let mut known = false;
        for (i, af) in self.layout.axes.iter().enumerate() {
            let Some(af) = af else { continue };
            if af.report_id != rid {
                continue;
            }
            known = true;
            let v = extract(payload, af).ok_or(DecodeError::Truncated { report_id: rid, len: report.len() })?;
            if v < af.logical_min || v > af.logical_max {
                return Err(DecodeError::OutOfRange { axis: i, value: v });
            }
            f.axes[i] = v;
            f.axes_present |= 1 << i;
        }
        for (bf, bit) in &self.layout.buttons {
            if bf.report_id != rid {
                continue;
            }
            known = true;
            f.buttons_present = true;
            let v = extract(payload, bf).ok_or(DecodeError::Truncated { report_id: rid, len: report.len() })?;
            if v != 0 {
                if let Some(idx) = self.map_button(*bit) {
                    f.buttons |= 1 << idx;
                }
            }
        }
        if !known {
            return Ok(None);
        }
        Ok(Some(f))
    }
}

// ---------------------------------------------------------------- evdev

pub const EV_SYN: u16 = 0;
pub const EV_KEY: u16 = 1;
pub const EV_REL: u16 = 2;
pub const EV_ABS: u16 = 3;
pub const SYN_REPORT: u16 = 0;
pub const SYN_DROPPED: u16 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    pub t_us: u64,
    pub typ: u16,
    pub code: u16,
    pub value: i32,
}

#[derive(Debug, Clone)]
pub struct EvdevAssembler {
    groups: [u8; AXES],
    /// evdev key code -> logical index
    button_codes: Vec<(u16, u8)>,
    /// For EV_ABS devices: (min, max) per axis to recentre.
    abs_ranges: [Option<(i32, i32)>; AXES],
    watchdog_us: u64,
    cur: [i32; AXES],
    buttons: u64,
    group_seen_us: [u64; AXES],
    pend_axes: [i32; AXES],
    pend_mask: u8,
    pend_buttons: Option<u64>,
    pub dropped_syncs: u64,
    pub watchdog_resets: u64,
}

/// Default watchdog: ~4 report periods of the measured 16 ms per-group cadence.
pub const DEFAULT_EVDEV_WATCHDOG_US: u64 = 64_000;

impl EvdevAssembler {
    pub fn new(groups: [u8; AXES], button_codes: Vec<(u16, u8)>, watchdog_us: u64) -> Self {
        EvdevAssembler {
            groups,
            button_codes,
            abs_ranges: Default::default(),
            watchdog_us,
            cur: [0; AXES],
            buttons: 0,
            group_seen_us: [0; AXES],
            pend_axes: [0; AXES],
            pend_mask: 0,
            pend_buttons: None,
            dropped_syncs: 0,
            watchdog_resets: 0,
        }
    }

    pub fn set_abs_range(&mut self, axis: usize, min: i32, max: i32) {
        if axis < AXES && max > min {
            self.abs_ranges[axis] = Some((min, max));
        }
    }

    fn index_for_code(&self, code: u16) -> Option<u8> {
        if self.button_codes.is_empty() {
            // generic: BTN_0.. contiguous
            return code.checked_sub(0x100).filter(|i| (*i as usize) < MAX_BUTTONS).map(|i| i as u8);
        }
        self.button_codes.iter().find(|(c, _)| *c == code).map(|(_, i)| *i)
    }

    /// Feed one evdev event; returns a frame at SYN_REPORT.
    pub fn push(&mut self, ev: InputEvent) -> Option<RawFrame> {
        match (ev.typ, ev.code) {
            (EV_REL, c) if (c as usize) < AXES => {
                self.pend_axes[c as usize] = ev.value;
                self.pend_mask |= 1 << c;
                None
            }
            (EV_ABS, c) if (c as usize) < AXES => {
                let v = match self.abs_ranges[c as usize] {
                    Some((lo, hi)) => ev.value - (lo + hi) / 2,
                    None => ev.value,
                };
                self.pend_axes[c as usize] = v;
                self.pend_mask |= 1 << c;
                None
            }
            (EV_KEY, code) => {
                if let Some(i) = self.index_for_code(code) {
                    let mut b = self.pend_buttons.unwrap_or(self.buttons);
                    if ev.value != 0 {
                        b |= 1 << i;
                    } else {
                        b &= !(1 << i);
                    }
                    self.pend_buttons = Some(b);
                }
                None
            }
            (EV_SYN, SYN_DROPPED) => {
                // Kernel buffer overrun: state is unknown. Zero motion; keep
                // buttons (a re-sync would need EVIOCGKEY; caller may do it).
                self.dropped_syncs += 1;
                self.pend_mask = 0;
                self.pend_buttons = None;
                let mut f = RawFrame { t_us: ev.t_us, axes_present: 0b111111, ..Default::default() };
                self.cur = [0; AXES];
                f.axes = self.cur;
                Some(f)
            }
            (EV_SYN, SYN_REPORT) => self.flush(ev.t_us),
            _ => None,
        }
    }

    fn flush(&mut self, t_us: u64) -> Option<RawFrame> {
        if self.pend_mask == 0 && self.pend_buttons.is_none() {
            return None;
        }
        let mut f = RawFrame { t_us, ..Default::default() };
        if self.pend_mask != 0 {
            // Every group touched in this frame is fully specified: absent
            // members were zero (dropped by the input core).
            let mut touched_groups = Vec::with_capacity(2);
            for i in 0..AXES {
                if self.pend_mask & (1 << i) != 0 && !touched_groups.contains(&self.groups[i]) {
                    touched_groups.push(self.groups[i]);
                }
            }
            for i in 0..AXES {
                if touched_groups.contains(&self.groups[i]) {
                    let v = if self.pend_mask & (1 << i) != 0 { self.pend_axes[i] } else { 0 };
                    self.cur[i] = v;
                    self.group_seen_us[i] = t_us;
                    f.axes_present |= 1 << i;
                }
            }
            f.axes = self.cur;
        }
        if let Some(b) = self.pend_buttons.take() {
            self.buttons = b;
            f.buttons = b;
            f.buttons_present = true;
        }
        self.pend_mask = 0;
        self.pend_axes = [0; AXES];
        Some(f)
    }

    /// Microseconds until the next watchdog deadline, if any axis is non-zero.
    pub fn next_deadline_us(&self, now: u64) -> Option<u64> {
        (0..AXES).filter(|i| self.cur[*i] != 0).map(|i| (self.group_seen_us[i] + self.watchdog_us).saturating_sub(now)).min()
    }

    /// Zero groups whose last report is older than the watchdog (FR-208).
    pub fn tick(&mut self, now: u64) -> Option<RawFrame> {
        let mut f = RawFrame { t_us: now, ..Default::default() };
        for i in 0..AXES {
            if self.cur[i] != 0 && now.saturating_sub(self.group_seen_us[i]) >= self.watchdog_us {
                self.cur[i] = 0;
                f.axes_present |= 1 << i;
            }
        }
        if f.axes_present == 0 {
            return None;
        }
        self.watchdog_resets += 1;
        f.axes = self.cur;
        Some(f)
    }
}

/// Parse the evdev fixture text format: `<sec.usec> <type> <code> <value>`.
pub fn parse_evdev_fixture(text: &str) -> Vec<InputEvent> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let t: f64 = it.next()?.parse().ok()?;
            Some(InputEvent { t_us: (t * 1e6).round() as u64, typ: it.next()?.parse().ok()?, code: it.next()?.parse().ok()?, value: it.next()?.parse().ok()? })
        })
        .collect()
}

#[cfg(test)]
mod tests;
