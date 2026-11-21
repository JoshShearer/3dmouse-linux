//! Minimal, bounded HID report-descriptor parser (HID 1.11 §6.2.2).
//!
//! Only what is needed to locate 6-DOF axes and buttons: Input main items,
//! report ids, usage pages/usages, logical ranges, sizes and counts.
//! Every loop is bounded by the input length; malformed input yields `Err`.

use serde::Serialize;

pub const PAGE_GENERIC_DESKTOP: u16 = 0x01;
pub const PAGE_BUTTON: u16 = 0x09;
pub const USAGE_MULTI_AXIS: u16 = 0x08;
/// Generic Desktop X..Rz (0x30..0x35).
pub const AXIS_USAGES: [u16; 6] = [0x30, 0x31, 0x32, 0x33, 0x34, 0x35];

const MAX_DESCRIPTOR: usize = 4096;
const MAX_FIELDS: usize = 1024;
const MAX_REPORT_BITS: u32 = 8 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    pub report_id: u8,
    /// Bit offset in the report payload (after the report-id byte).
    pub bit_offset: u32,
    pub bit_size: u32,
    pub usage_page: u16,
    pub usage: u16,
    pub logical_min: i32,
    pub logical_max: i32,
    pub relative: bool,
}

impl Field {
    pub fn signed(&self) -> bool {
        self.logical_min < 0
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Descriptor {
    pub uses_report_ids: bool,
    pub fields: Vec<Field>,
    /// (usage_page, usage) of top-level application collections.
    pub applications: Vec<(u16, u16)>,
    /// Payload bit length per report id.
    pub report_bits: Vec<(u8, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HID descriptor: {}", self.0)
    }
}

#[derive(Clone, Default)]
struct Globals {
    usage_page: u16,
    logical_min: i32,
    logical_max: i32,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

fn sdata(d: &[u8]) -> i32 {
    match d.len() {
        0 => 0,
        1 => d[0] as i8 as i32,
        2 => i16::from_le_bytes([d[0], d[1]]) as i32,
        _ => i32::from_le_bytes([d[0], d[1], d[2], d[3]]),
    }
}

fn udata(d: &[u8]) -> u32 {
    match d.len() {
        0 => 0,
        1 => d[0] as u32,
        2 => u16::from_le_bytes([d[0], d[1]]) as u32,
        _ => u32::from_le_bytes([d[0], d[1], d[2], d[3]]),
    }
}

pub fn parse(desc: &[u8]) -> Result<Descriptor, ParseError> {
    if desc.is_empty() || desc.len() > MAX_DESCRIPTOR {
        return Err(ParseError(format!("bad length {}", desc.len())));
    }
    let mut out = Descriptor::default();
    let mut g = Globals::default();
    let mut stack: Vec<Globals> = Vec::new();
    let mut usages: Vec<u32> = Vec::new();
    let mut umin: Option<u32> = None;
    let mut umax: Option<u32> = None;
    let mut depth: u32 = 0;
    let mut bit_pos: std::collections::BTreeMap<u8, u32> = Default::default();
    let mut lmax_raw_unsigned: u32 = 0;
    let mut lmax_len = 0usize;

    let mut i = 0usize;
    while i < desc.len() {
        let p = desc[i];
        if p == 0xFE {
            // long item: 0xFE, size, tag, data
            let sz = *desc.get(i + 1).ok_or_else(|| ParseError("truncated long item".into()))? as usize;
            i += 3 + sz;
            continue;
        }
        let size = match p & 3 {
            3 => 4,
            n => n as usize,
        };
        let typ = (p >> 2) & 3;
        let tag = p >> 4;
        let data = desc.get(i + 1..i + 1 + size).ok_or_else(|| ParseError(format!("truncated item at {i}")))?;
        i += 1 + size;

        match typ {
            0 => {
                // main
                match tag {
                    0x8 => {
                        // Input
                        let flags = udata(data);
                        let constant = flags & 1 != 0;
                        let variable = flags & 2 != 0;
                        let relative = flags & 4 != 0;
                        let pos = bit_pos.entry(g.report_id).or_insert(0);
                        let mut lmax = g.logical_max;
                        if lmax < g.logical_min && lmax_len > 0 {
                            // common descriptor bug: unsigned max encoded in a signed byte
                            lmax = lmax_raw_unsigned as i32;
                        }
                        for n in 0..g.report_count {
                            let usage = if let (Some(a), Some(b)) = (umin, umax) {
                                let u = a + n;
                                if u > b {
                                    b
                                } else {
                                    u
                                }
                            } else if !usages.is_empty() {
                                usages[(n as usize).min(usages.len() - 1)]
                            } else {
                                0
                            };
                            if !constant && variable && g.report_size > 0 && g.report_size <= 32 {
                                let page = if usage > 0xffff { (usage >> 16) as u16 } else { g.usage_page };
                                if out.fields.len() >= MAX_FIELDS {
                                    return Err(ParseError("too many fields".into()));
                                }
                                out.fields.push(Field {
                                    report_id: g.report_id,
                                    bit_offset: *pos,
                                    bit_size: g.report_size,
                                    usage_page: page,
                                    usage: (usage & 0xffff) as u16,
                                    logical_min: g.logical_min,
                                    logical_max: lmax,
                                    relative,
                                });
                            }
                            *pos = pos.saturating_add(g.report_size);
                            if *pos > MAX_REPORT_BITS {
                                return Err(ParseError("report too long".into()));
                            }
                        }
                    }
                    0x9 | 0xB => { /* Output / Feature: not consumed (FR-605) */ }
                    0xA => {
                        if depth == 0 {
                            let u = usages.first().copied().unwrap_or(0);
                            let page = if u > 0xffff { (u >> 16) as u16 } else { g.usage_page };
                            out.applications.push((page, (u & 0xffff) as u16));
                        }
                        depth += 1;
                        if depth > 32 {
                            return Err(ParseError("collections nested too deep".into()));
                        }
                    }
                    0xC => {
                        depth = depth.checked_sub(1).ok_or_else(|| ParseError("unbalanced End Collection".into()))?;
                    }
                    _ => {}
                }
                usages.clear();
                umin = None;
                umax = None;
            }
            1 => match tag {
                0x0 => g.usage_page = udata(data) as u16,
                0x1 => g.logical_min = sdata(data),
                0x2 => {
                    g.logical_max = sdata(data);
                    lmax_raw_unsigned = udata(data);
                    lmax_len = data.len();
                }
                0x7 => g.report_size = udata(data),
                0x8 => {
                    let id = udata(data);
                    if id == 0 || id > 255 {
                        return Err(ParseError("invalid report id".into()));
                    }
                    g.report_id = id as u8;
                    out.uses_report_ids = true;
                }
                0x9 => g.report_count = udata(data).min(4096),
                0xA => {
                    if stack.len() > 16 {
                        return Err(ParseError("push stack overflow".into()));
                    }
                    stack.push(g.clone());
                }
                0xB => g = stack.pop().ok_or_else(|| ParseError("pop without push".into()))?,
                _ => {}
            },
            2 => match tag {
                0x0 => {
                    if usages.len() < 256 {
                        let u = udata(data);
                        usages.push(if data.len() == 4 { u } else { u & 0xffff });
                    }
                }
                0x1 => umin = Some(udata(data)),
                0x2 => umax = Some(udata(data)),
                _ => {}
            },
            _ => return Err(ParseError(format!("reserved item type at {i}"))),
        }
    }
    out.report_bits = bit_pos.into_iter().collect();
    Ok(out)
}

/// Parse hex text like "05 01 09 08 ..." (fixture format).
pub fn from_hex(s: &str) -> Result<Vec<u8>, ParseError> {
    s.split_whitespace().map(|t| u8::from_str_radix(t, 16).map_err(|e| ParseError(e.to_string()))).collect()
}
