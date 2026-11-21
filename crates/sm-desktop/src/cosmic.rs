//! COSMIC focus provider: a minimal Wayland client (no new dependency).
//!
//! COSMIC (cosmic-comp ≥ 1.0) has no wlr-foreign-toplevel. Window identity
//! comes from `ext_foreign_toplevel_list_v1` (app_id, title), and the
//! activated state from `zcosmic_toplevel_info_v1` v2+, which extends each
//! ext handle via `get_cosmic_toplevel`. Only the requests and events needed
//! here are implemented; everything else is skipped by size.

use crate::focus::FocusInfo;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

// opcodes (order of <request>/<event> in each protocol XML)
const WL_DISPLAY: u32 = 1;
const DISPLAY_SYNC: u16 = 0;
const DISPLAY_GET_REGISTRY: u16 = 1;
const DISPLAY_EV_ERROR: u16 = 0;
const REGISTRY_BIND: u16 = 0;
const REGISTRY_EV_GLOBAL: u16 = 0;
const LIST_EV_TOPLEVEL: u16 = 0;
const EXT_EV_CLOSED: u16 = 0;
const EXT_EV_DONE: u16 = 1;
const EXT_EV_TITLE: u16 = 2;
const EXT_EV_APP_ID: u16 = 3;
const INFO_GET_COSMIC_TOPLEVEL: u16 = 1;
const COSMIC_EV_STATE: u16 = 8;
const STATE_ACTIVATED: u32 = 2;

/// Wayland socket of the session, also when started by systemd without
/// WAYLAND_DISPLAY in the environment.
pub fn wayland_socket() -> Option<PathBuf> {
    let rt = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
    if let Some(d) = std::env::var_os("WAYLAND_DISPLAY") {
        let p = PathBuf::from(&d);
        return Some(if p.is_absolute() { p } else { rt.join(p) });
    }
    let mut socks: Vec<PathBuf> = std::fs::read_dir(&rt)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("wayland-") && !n.ends_with(".lock")).unwrap_or(false))
        .collect();
    socks.sort();
    socks.into_iter().next()
}

// ---------- wire format ----------

pub fn msg(obj: u32, op: u16, args: &[u8]) -> Vec<u8> {
    let size = 8 + args.len();
    let mut m = Vec::with_capacity(size);
    m.extend_from_slice(&obj.to_ne_bytes());
    m.extend_from_slice(&(((size as u32) << 16) | op as u32).to_ne_bytes());
    m.extend_from_slice(args);
    m
}

pub fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_ne_bytes());
}

pub fn put_str(b: &mut Vec<u8>, s: &str) {
    put_u32(b, s.len() as u32 + 1);
    b.extend_from_slice(s.as_bytes());
    b.push(0);
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

pub struct Args<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Args<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Args { b, pos: 0 }
    }
    pub fn u32(&mut self) -> Option<u32> {
        let v = u32::from_ne_bytes(self.b.get(self.pos..self.pos + 4)?.try_into().ok()?);
        self.pos += 4;
        Some(v)
    }
    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n.div_ceil(4) * 4;
        Some(s)
    }
    pub fn string(&mut self) -> Option<String> {
        let s = self.bytes()?;
        Some(String::from_utf8_lossy(s.strip_suffix(&[0]).unwrap_or(s)).into_owned())
    }
}

// ---------- focus tracking state machine (no I/O, unit-tested) ----------

#[derive(Default, Debug)]
struct Window {
    app_id: String,
    title: String,
    active: bool,
}

/// Feeds on decoded events, reports the active window when it changes.
#[derive(Default)]
pub struct Tracker {
    list: u32,
    info: u32,
    /// ext handle id → window
    windows: HashMap<u32, Window>,
    /// cosmic handle id → ext handle id
    cosmic: HashMap<u32, u32>,
    next_id: u32,
    last: Option<(String, String)>,
    /// requests to send (get_cosmic_toplevel)
    pub out: Vec<u8>,
}

impl Tracker {
    pub fn new(list: u32, info: u32, next_id: u32) -> Self {
        Tracker { list, info, next_id, ..Default::default() }
    }

    fn active(&self) -> Option<&Window> {
        self.windows.values().find(|w| w.active)
    }

    /// Handle one event. Returns focus info when the active window changed.
    pub fn event(&mut self, obj: u32, op: u16, args: &[u8]) -> Option<FocusInfo> {
        let mut a = Args::new(args);
        if obj == self.list && op == LIST_EV_TOPLEVEL {
            let h = a.u32()?;
            self.windows.insert(h, Window::default());
            let c = self.next_id;
            self.next_id += 1;
            self.cosmic.insert(c, h);
            let mut b = Vec::new();
            put_u32(&mut b, c);
            put_u32(&mut b, h);
            self.out.extend(msg(self.info, INFO_GET_COSMIC_TOPLEVEL, &b));
            return None;
        }
        if let Some(w) = self.windows.get_mut(&obj) {
            match op {
                EXT_EV_TITLE => w.title = a.string()?,
                EXT_EV_APP_ID => w.app_id = a.string()?,
                EXT_EV_CLOSED => {
                    self.windows.remove(&obj);
                    self.cosmic.retain(|_, h| *h != obj);
                }
                EXT_EV_DONE => return self.report(),
                _ => {}
            }
            return None;
        }
        if let Some(h) = self.cosmic.get(&obj).copied() {
            if op == COSMIC_EV_STATE {
                let st = a.bytes()?;
                let active = st.as_chunks::<4>().0.iter().any(|c| u32::from_ne_bytes(*c) == STATE_ACTIVATED);
                if let Some(w) = self.windows.get_mut(&h) {
                    w.active = active;
                }
                return self.report();
            }
        }
        None
    }

    fn report(&mut self) -> Option<FocusInfo> {
        let cur = self.active().map(|w| (w.app_id.clone(), w.title.clone()))?;
        if cur.0.is_empty() || self.last.as_ref() == Some(&cur) {
            return None;
        }
        self.last = Some(cur.clone());
        Some(FocusInfo { app_id: cur.0, title: cur.1, pid: None, provider: "cosmic".into() })
    }
}

// ---------- connection ----------

fn read_msg(s: &mut UnixStream) -> std::io::Result<(u32, u16, Vec<u8>)> {
    let mut h = [0u8; 8];
    s.read_exact(&mut h)?;
    let obj = u32::from_ne_bytes(h[0..4].try_into().unwrap());
    let w = u32::from_ne_bytes(h[4..8].try_into().unwrap());
    let size = (w >> 16) as usize;
    if size < 8 {
        return Err(std::io::Error::other("bad Wayland message"));
    }
    let mut args = vec![0u8; size - 8];
    s.read_exact(&mut args)?;
    Ok((obj, (w & 0xffff) as u16, args))
}

/// Connect, bind the two globals and stream focus changes to `on_focus`.
pub fn run(mut on_focus: impl FnMut(FocusInfo)) -> std::io::Result<()> {
    let path = wayland_socket().ok_or_else(|| std::io::Error::other("no Wayland socket"))?;
    let mut s = UnixStream::connect(&path)?;
    let registry = 2;
    let sync_cb = 3;
    let mut b = Vec::new();
    put_u32(&mut b, registry);
    s.write_all(&msg(WL_DISPLAY, DISPLAY_GET_REGISTRY, &b))?;
    let mut b = Vec::new();
    put_u32(&mut b, sync_cb);
    s.write_all(&msg(WL_DISPLAY, DISPLAY_SYNC, &b))?;
    // collect globals until the sync callback fires
    let (mut list_g, mut info_g) = (None, None);
    loop {
        let (obj, op, args) = read_msg(&mut s)?;
        if obj == sync_cb {
            break;
        }
        if obj == WL_DISPLAY && op == DISPLAY_EV_ERROR {
            return Err(std::io::Error::other("Wayland protocol error during setup"));
        }
        if obj == registry && op == REGISTRY_EV_GLOBAL {
            let mut a = Args::new(&args);
            let (name, iface, ver) = (a.u32(), a.string(), a.u32());
            match (name, iface.as_deref(), ver) {
                (Some(n), Some("ext_foreign_toplevel_list_v1"), Some(_)) => list_g = Some(n),
                (Some(n), Some("zcosmic_toplevel_info_v1"), Some(v)) if v >= 2 => info_g = Some((n, v.min(3))),
                _ => {}
            }
        }
    }
    let (Some(list_name), Some((info_name, info_ver))) = (list_g, info_g) else {
        return Err(std::io::Error::other("compositor lacks ext_foreign_toplevel_list_v1 or zcosmic_toplevel_info_v1 ≥ 2"));
    };
    let (list, info) = (4, 5);
    for (name, iface, ver, id) in [(list_name, "ext_foreign_toplevel_list_v1", 1, list), (info_name, "zcosmic_toplevel_info_v1", info_ver, info)] {
        let mut b = Vec::new();
        put_u32(&mut b, name);
        put_str(&mut b, iface);
        put_u32(&mut b, ver);
        put_u32(&mut b, id);
        s.write_all(&msg(registry, REGISTRY_BIND, &b))?;
    }
    let mut t = Tracker::new(list, info, 6);
    loop {
        let (obj, op, args) = read_msg(&mut s)?;
        if obj == WL_DISPLAY && op == DISPLAY_EV_ERROR {
            return Err(std::io::Error::other("Wayland protocol error"));
        }
        if let Some(f) = t.event(obj, op, &args) {
            on_focus(f);
        }
        if !t.out.is_empty() {
            s.write_all(&std::mem::take(&mut t.out))?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Vec<u8> {
        let mut b = Vec::new();
        put_str(&mut b, v);
        b
    }
    fn u(v: u32) -> Vec<u8> {
        v.to_ne_bytes().to_vec()
    }
    fn state(v: &[u32]) -> Vec<u8> {
        let mut b = u(v.len() as u32 * 4);
        for x in v {
            b.extend(u(*x));
        }
        b
    }

    #[test]
    fn tracks_the_activated_window() {
        let mut t = Tracker::new(4, 5, 6);
        // two toplevels announced by the server
        assert!(t.event(4, LIST_EV_TOPLEVEL, &u(0xff00_0001)).is_none());
        assert!(t.event(4, LIST_EV_TOPLEVEL, &u(0xff00_0002)).is_none());
        // we asked for cosmic handles 6 and 7
        assert_eq!(t.out.len(), 2 * 16);
        assert_eq!(&t.out[0..4], &5u32.to_ne_bytes());
        t.event(0xff00_0001, EXT_EV_APP_ID, &s("org.blender.Blender"));
        t.event(0xff00_0001, EXT_EV_TITLE, &s("scene.blend"));
        t.event(0xff00_0002, EXT_EV_APP_ID, &s("com.system76.CosmicTerm"));
        assert!(t.event(0xff00_0001, EXT_EV_DONE, &[]).is_none(), "nothing active yet");
        let f = t.event(6, COSMIC_EV_STATE, &state(&[0, STATE_ACTIVATED])).unwrap();
        assert_eq!((f.app_id.as_str(), f.title.as_str(), f.provider.as_str()), ("org.blender.Blender", "scene.blend", "cosmic"));
        // focus moves to the terminal
        t.event(6, COSMIC_EV_STATE, &state(&[]));
        let f = t.event(7, COSMIC_EV_STATE, &state(&[STATE_ACTIVATED])).unwrap();
        assert_eq!(f.app_id, "com.system76.CosmicTerm");
        // repeated state for the same window: no duplicate report
        assert!(t.event(7, COSMIC_EV_STATE, &state(&[STATE_ACTIVATED])).is_none());
        // the terminal closes; blender reactivates
        t.event(0xff00_0002, EXT_EV_CLOSED, &[]);
        assert!(t.event(6, COSMIC_EV_STATE, &state(&[STATE_ACTIVATED])).is_some());
    }

    #[test]
    fn malformed_args_are_ignored() {
        let mut t = Tracker::new(4, 5, 6);
        assert!(t.event(4, LIST_EV_TOPLEVEL, &[1, 2]).is_none());
        t.event(4, LIST_EV_TOPLEVEL, &u(9));
        assert!(t.event(9, EXT_EV_APP_ID, &u(1000)).is_none()); // length beyond the message
        assert!(t.event(6, COSMIC_EV_STATE, &u(400)).is_none());
    }
}
