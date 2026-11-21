//! spacenavd-compatible AF_UNIX protocol (FR-501).
//!
//! Wire format re-implemented from the public protocol definition in
//! spacenavd `src/proto.h` / `src/proto_unix.c` and libspnav `src/spnav.c`
//! (FreeSpacenav, commits recorded in docs/research/landscape.md). No code
//! was copied; this is an interoperable implementation of the interface.
//!
//! * Events: 8 × int32 native-endian: `[type, d1..d6, d7]`.
//!   MOTION: `[0, x, y, z, rx, ry, rz, period_ms]`;
//!   PRESS/RELEASE: `[1|2, bnum, press, 0...]`; DEV: `[3, op, id, type, vid, pid, 0, 0]`.
//! * Protocol v0: clients may send a 4-byte float sensitivity.
//! * Upgrade: client sends `REQ_TAG | REQ_CHANGE_PROTO | ver` (4 bytes);
//!   the server answers with the negotiated value and switches to v1.
//! * v1 requests/responses: `{int32 type; int32 data[7]}` (32 bytes),
//!   `data[6]` = status; the response echoes the request type.
//!
//! Multi-device: the legacy protocol has no device identifiers in events
//! (FR-105). Policy: motion from all owned devices is aggregated into the
//! one stream; device queries describe the *primary* (first connected) device.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

pub const REQ_TAG: i32 = 0x7faa0000;
pub const REQ_CHANGE_PROTO: i32 = 0x5500;
pub const MAX_PROTO_VER: i32 = 1;

pub const UEV_MOTION: i32 = 0;
pub const UEV_PRESS: i32 = 1;
pub const UEV_RELEASE: i32 = 2;
pub const UEV_DEV: i32 = 3;
pub const UEV_CFG: i32 = 4;
pub const UEV_RAWAXIS: i32 = 5;
pub const UEV_RAWBUTTON: i32 = 6;

pub const EVMASK_MOTION: u32 = 0x01;
pub const EVMASK_BUTTON: u32 = 0x02;
pub const EVMASK_DEV: u32 = 0x04;
pub const EVMASK_CFG: u32 = 0x08;
pub const EVMASK_RAWAXIS: u32 = 0x10;
pub const EVMASK_RAWBUTTON: u32 = 0x20;

pub const REQ_SET_NAME: i32 = 0x1000;
pub const REQ_SET_SENS: i32 = 0x1001;
pub const REQ_GET_SENS: i32 = 0x1002;
pub const REQ_SET_EVMASK: i32 = 0x1003;
pub const REQ_GET_EVMASK: i32 = 0x1004;
pub const REQ_DEV_NAME: i32 = 0x2000;
pub const REQ_DEV_PATH: i32 = 0x2001;
pub const REQ_DEV_NAXES: i32 = 0x2002;
pub const REQ_DEV_NBUTTONS: i32 = 0x2003;
pub const REQ_DEV_USBID: i32 = 0x2004;
pub const REQ_DEV_TYPE: i32 = 0x2005;
pub const REQ_GCFG_SENS: i32 = 0x3001;
pub const REQ_GCFG_SENS_AXIS: i32 = 0x3003;
pub const REQ_GCFG_DEADZONE: i32 = 0x3005;
pub const REQ_GCFG_INVERT: i32 = 0x3007;
pub const REQ_GCFG_AXISMAP: i32 = 0x3009;
pub const REQ_GCFG_BNMAP: i32 = 0x300b;
pub const REQ_GCFG_BNACTION: i32 = 0x300d;
pub const REQ_GCFG_KBMAP: i32 = 0x300f;
pub const REQ_GCFG_SWAPYZ: i32 = 0x3011;
pub const REQ_GCFG_LED: i32 = 0x3013;
pub const REQ_GCFG_GRAB: i32 = 0x3015;
pub const REQ_GCFG_REPEAT: i32 = 0x3019;

pub const REQSTR_CHUNK: usize = 24;
pub const REQSTR_CONT_BIT: i32 = 0x10000;

pub const DEFAULT_SOCKET: &str = "/run/spnav.sock";
/// Per-client output buffer cap. Motion is a state, not a delta, so when a
/// client falls behind we drop intermediate motion; buttons are never
/// dropped – a client that cannot take a button event is disconnected.
const MAX_WBUF: usize = 64 * 1024;
/// Unread data (kernel send queue, as reported by SIOCOUTQ, plus our buffer)
/// above which non-neutral motion is withheld from a client. Apps such as
/// KiCad only read the socket while their canvas is active; without this
/// bound they would later replay seconds of stale motion as a view jump.
/// SIOCOUTQ on AF_UNIX counts skb truesize (~1 KiB per 32-byte event), so
/// this is roughly 16 queued events. Neutral frames and buttons always go out.
const STALE_BACKLOG: usize = 16 * 1024;
const MAX_CLIENTS: usize = 64;
const MAX_NAME: usize = 256;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: i32,
    pub name: String,
    pub path: String,
    pub naxes: i32,
    pub nbuttons: i32,
    pub vid: u16,
    pub pid: u16,
    pub devtype: i32,
}

pub fn encode8(d: [i32; 8]) -> [u8; 32] {
    let mut b = [0u8; 32];
    for (i, v) in d.iter().enumerate() {
        b[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
    }
    b
}

pub fn decode8(b: &[u8]) -> [i32; 8] {
    let mut d = [0i32; 8];
    for (i, v) in d.iter_mut().enumerate() {
        *v = i32::from_ne_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]]);
    }
    d
}

struct Client {
    s: UnixStream,
    proto: i32,
    evmask: u32,
    sens: f32,
    name: String,
    rbuf: Vec<u8>,
    strbuf: Option<(usize, Vec<u8>)>,
    wbuf: VecDeque<u8>,
    dead: bool,
    dropped_motion: u64,
}

impl Client {
    fn queue(&mut self, bytes: &[u8], droppable: bool) {
        if self.wbuf.len() + bytes.len() > MAX_WBUF {
            if droppable {
                self.dropped_motion += 1;
                return;
            }
            self.dead = true;
            return;
        }
        self.wbuf.extend(bytes);
    }

    /// Bytes written but not yet read by the peer, plus our own buffer.
    fn backlog(&self) -> usize {
        let mut outq: libc::c_int = 0;
        // SIOCOUTQ == TIOCOUTQ for sockets
        let r = unsafe { libc::ioctl(self.s.as_raw_fd(), libc::TIOCOUTQ, &mut outq) };
        self.wbuf.len() + if r == 0 && outq > 0 { outq as usize } else { 0 }
    }

    fn flush(&mut self) {
        while !self.wbuf.is_empty() && !self.dead {
            let (a, _) = self.wbuf.as_slices();
            match self.s.write(a) {
                Ok(0) => self.dead = true,
                Ok(n) => {
                    self.wbuf.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => self.dead = true,
            }
        }
    }

    fn resp(&mut self, typ: i32, mut data: [i32; 7], status: i32) {
        data[6] = status;
        let mut d = [0; 8];
        d[0] = typ;
        d[1..].copy_from_slice(&data);
        self.queue(&encode8(d), false);
    }

    fn send_str(&mut self, typ: i32, s: &str) {
        let bytes = s.as_bytes();
        let mut rem = bytes.len();
        let mut off = 0;
        let mut first = true;
        loop {
            let mut chunk = [0u8; 28];
            let n = rem.min(REQSTR_CHUNK);
            chunk[..n].copy_from_slice(&bytes[off..off + n]);
            let mut d = [0i32; 7];
            for (i, v) in d.iter_mut().take(6).enumerate() {
                *v = i32::from_ne_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
            }
            d[6] = rem as i32 | if first { 0 } else { REQSTR_CONT_BIT };
            let mut out = [0; 8];
            out[0] = typ;
            out[1..].copy_from_slice(&d);
            self.queue(&encode8(out), false);
            first = false;
            off += n;
            rem -= n;
            if rem == 0 {
                break;
            }
        }
    }
}

pub struct SpnavServer {
    listener: UnixListener,
    path: PathBuf,
    clients: Vec<Client>,
    pub devices: Vec<DeviceInfo>,
    pub motion_events: u64,
    pub button_events: u64,
    /// Display names of connected clients (for diagnostics).
    pub accepted_total: u64,
}

impl SpnavServer {
    /// Bind the socket. Refuses to replace a *live* socket owned by someone
    /// else (e.g. a running spacenavd) – FR-107.
    pub fn bind(path: &Path) -> std::io::Result<SpnavServer> {
        if path.exists() {
            if UnixStream::connect(path).is_ok() {
                return Err(std::io::Error::new(ErrorKind::AddrInUse, format!("{} is served by another process", path.display())));
            }
            std::fs::remove_file(path)?;
        }
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        // spacenavd convention: any local user may connect.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
        Ok(SpnavServer { listener, path: path.to_path_buf(), clients: Vec::new(), devices: Vec::new(), motion_events: 0, button_events: 0, accepted_total: 0 })
    }

    pub fn listen_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }

    pub fn client_fds(&self) -> Vec<(RawFd, bool)> {
        self.clients.iter().map(|c| (c.s.as_raw_fd(), !c.wbuf.is_empty())).collect()
    }

    pub fn client_names(&self) -> Vec<String> {
        self.clients.iter().map(|c| if c.name.is_empty() { "(unnamed)".into() } else { c.name.clone() }).collect()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn dropped_motion(&self) -> u64 {
        self.clients.iter().map(|c| c.dropped_motion).sum()
    }

    pub fn accept(&mut self) {
        loop {
            match self.listener.accept() {
                Ok((s, _)) => {
                    if self.clients.len() >= MAX_CLIENTS || s.set_nonblocking(true).is_err() {
                        continue;
                    }
                    self.accepted_total += 1;
                    self.clients.push(Client {
                        s,
                        proto: 0,
                        evmask: EVMASK_MOTION | EVMASK_BUTTON,
                        sens: 1.0,
                        name: String::new(),
                        rbuf: Vec::new(),
                        strbuf: None,
                        wbuf: VecDeque::new(),
                        dead: false,
                        dropped_motion: 0,
                    });
                }
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    }

    /// Handle readable/writable client sockets.
    pub fn service(&mut self, fd: RawFd) {
        let devs = self.devices.first().cloned();
        if let Some(c) = self.clients.iter_mut().find(|c| c.s.as_raw_fd() == fd) {
            let mut buf = [0u8; 512];
            loop {
                match c.s.read(&mut buf) {
                    Ok(0) => {
                        c.dead = true;
                        break;
                    }
                    Ok(n) => {
                        c.rbuf.extend_from_slice(&buf[..n]);
                        if c.rbuf.len() > 4096 {
                            c.dead = true;
                            break;
                        }
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => {
                        c.dead = true;
                        break;
                    }
                }
            }
            process_requests(c, devs.as_ref());
            c.flush();
        }
        self.reap();
    }

    fn reap(&mut self) {
        self.clients.retain(|c| !c.dead);
    }

    fn broadcast(&mut self, mask: u32, d: [i32; 8], droppable: bool, scale_motion: bool) {
        for c in &mut self.clients {
            if c.evmask & mask == 0 {
                continue;
            }
            let mut d = d;
            if scale_motion {
                for v in &mut d[1..7] {
                    *v = (*v as f32 * c.sens) as i32;
                }
            }
            // Stale-motion guard: a client that has stopped reading only gets
            // neutral frames (so it can never be left moving), never a backlog.
            let neutral = mask == EVMASK_MOTION && d[1..7].iter().all(|v| *v == 0);
            if droppable && !neutral && c.backlog() > STALE_BACKLOG {
                c.dropped_motion += 1;
                continue;
            }
            c.queue(&encode8(d), droppable);
            c.flush();
        }
        self.reap();
    }

    pub fn send_motion(&mut self, m: [i32; 6], period_ms: i32) {
        self.motion_events += 1;
        self.broadcast(EVMASK_MOTION, [UEV_MOTION, m[0], m[1], m[2], m[3], m[4], m[5], period_ms], true, true);
    }

    pub fn send_button(&mut self, bnum: i32, pressed: bool) {
        self.button_events += 1;
        let t = if pressed { UEV_PRESS } else { UEV_RELEASE };
        self.broadcast(EVMASK_BUTTON, [t, bnum, pressed as i32, 0, 0, 0, 0, 0], false, false);
    }

    pub fn send_raw_axis(&mut self, idx: i32, value: i32) {
        self.broadcast(EVMASK_RAWAXIS, [UEV_RAWAXIS, idx, value, 0, 0, 0, 0, 0], true, false);
    }

    pub fn send_raw_button(&mut self, bnum: i32, pressed: bool) {
        self.broadcast(EVMASK_RAWBUTTON, [UEV_RAWBUTTON, bnum, pressed as i32, 0, 0, 0, 0, 0], false, false);
    }

    /// op: 0 = add, 1 = remove (SPNAV_DEV_ADD / SPNAV_DEV_RM)
    pub fn send_dev(&mut self, op: i32, d: &DeviceInfo) {
        self.broadcast(EVMASK_DEV, [UEV_DEV, op, d.id, d.devtype, d.vid as i32, d.pid as i32, 0, 0], false, false);
    }

    pub fn flush_all(&mut self) {
        for c in &mut self.clients {
            c.flush();
        }
        self.reap();
    }
}

impl Drop for SpnavServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn process_requests(c: &mut Client, dev: Option<&DeviceInfo>) {
    loop {
        if c.dead {
            return;
        }
        if c.proto == 0 {
            if c.rbuf.len() < 4 {
                return;
            }
            let msg = i32::from_ne_bytes([c.rbuf[0], c.rbuf[1], c.rbuf[2], c.rbuf[3]]);
            c.rbuf.drain(..4);
            if (msg as u32 & 0xffff_ff00) == (REQ_TAG | REQ_CHANGE_PROTO) as u32 {
                let mut v = msg & 0xff;
                if v > MAX_PROTO_VER {
                    v = MAX_PROTO_VER;
                }
                let reply = REQ_TAG | REQ_CHANGE_PROTO | v;
                c.queue(&reply.to_ne_bytes(), false);
                c.proto = v;
                if v > 0 {
                    c.evmask = EVMASK_MOTION | EVMASK_BUTTON | EVMASK_DEV;
                }
            } else {
                let f = f32::from_bits(msg as u32);
                if f.is_finite() && f.abs() <= 1000.0 {
                    c.sens = f;
                }
            }
        } else {
            if c.rbuf.len() < 32 {
                return;
            }
            let d = decode8(&c.rbuf[..32]);
            c.rbuf.drain(..32);
            handle_v1(c, d, dev);
        }
    }
}

fn handle_v1(c: &mut Client, d: [i32; 8], dev: Option<&DeviceInfo>) {
    let typ = d[0];
    let mut data = [0i32; 7];
    data.copy_from_slice(&d[1..]);
    let ok = 0;
    let fail = -1;
    match typ & 0xffff {
        REQ_SET_NAME => {
            // string transfer: data[6] = remaining length (+cont bit)
            if data[6] < 0 {
                c.strbuf = None;
                return;
            }
            let rem = (data[6] & 0xffff) as usize;
            let first = data[6] & REQSTR_CONT_BIT == 0;
            if first {
                if rem > MAX_NAME {
                    c.strbuf = None;
                    return;
                }
                c.strbuf = Some((rem, Vec::with_capacity(rem)));
            }
            let Some((expect, buf)) = c.strbuf.as_mut() else { return };
            if rem != *expect {
                c.strbuf = None;
                return;
            }
            let n = rem.min(REQSTR_CHUNK);
            let raw: Vec<u8> = data[..6].iter().flat_map(|v| v.to_ne_bytes()).collect();
            buf.extend_from_slice(&raw[..n]);
            *expect -= n;
            if *expect == 0 {
                c.name = String::from_utf8_lossy(buf).chars().filter(|ch| !ch.is_control()).collect();
                c.strbuf = None;
            }
            // spacenavd sends no response to SET_NAME
        }
        REQ_SET_SENS => {
            let f = f32::from_bits(data[0] as u32);
            if f.is_finite() && f.abs() <= 1000.0 {
                c.sens = f;
                c.resp(typ, data, ok)
            } else {
                c.resp(typ, data, fail)
            }
        }
        REQ_GET_SENS => {
            data[0] = c.sens.to_bits() as i32;
            c.resp(typ, data, ok)
        }
        REQ_SET_EVMASK => {
            c.evmask = data[0] as u32;
            c.resp(typ, data, ok)
        }
        REQ_GET_EVMASK => {
            data[0] = c.evmask as i32;
            c.resp(typ, data, ok)
        }
        REQ_DEV_NAME | REQ_DEV_PATH => match dev {
            Some(d) => {
                let s = if typ & 0xffff == REQ_DEV_NAME { d.name.clone() } else { d.path.clone() };
                c.send_str(typ, &s)
            }
            None => c.resp(typ, data, fail),
        },
        REQ_DEV_NAXES | REQ_DEV_NBUTTONS | REQ_DEV_USBID | REQ_DEV_TYPE => match dev {
            Some(d) => {
                match typ & 0xffff {
                    REQ_DEV_NAXES => data[0] = d.naxes,
                    REQ_DEV_NBUTTONS => data[0] = d.nbuttons,
                    REQ_DEV_USBID => {
                        data[0] = d.vid as i32;
                        data[1] = d.pid as i32;
                    }
                    _ => data[0] = d.devtype,
                }
                c.resp(typ, data, ok)
            }
            None => c.resp(typ, data, fail),
        },
        // Read-only views of configuration: the pipeline is configured through
        // 3DMouse Linux profiles, so we report neutral values (the motion
        // clients receive is already processed).
        REQ_GCFG_SENS => {
            data[0] = 1.0f32.to_bits() as i32;
            c.resp(typ, data, ok)
        }
        REQ_GCFG_SENS_AXIS => {
            for v in data.iter_mut().take(6) {
                *v = 1.0f32.to_bits() as i32;
            }
            c.resp(typ, data, ok)
        }
        REQ_GCFG_INVERT => {
            data[..6].fill(0);
            c.resp(typ, data, ok)
        }
        REQ_GCFG_DEADZONE => {
            data[1] = 0;
            c.resp(typ, data, ok)
        }
        REQ_GCFG_AXISMAP | REQ_GCFG_BNMAP => {
            data[1] = data[0];
            c.resp(typ, data, ok)
        }
        REQ_GCFG_BNACTION | REQ_GCFG_KBMAP | REQ_GCFG_SWAPYZ | REQ_GCFG_GRAB | REQ_GCFG_REPEAT => {
            data[1] = 0;
            data[0] = if typ & 0xffff == REQ_GCFG_BNACTION || typ & 0xffff == REQ_GCFG_KBMAP { data[0] } else { 0 };
            c.resp(typ, data, ok)
        }
        REQ_GCFG_LED => {
            data[0] = 2; // auto
            c.resp(typ, data, ok)
        }
        // Everything else (SCFG_*, CFG_SAVE/RESTORE/RESET, unknown):
        // refused. Configuration belongs to the user's profile (FR-210:
        // prevents a second, conflicting sensitivity path).
        _ => c.resp(typ, data, fail),
    }
}

// ------------------------------------------------------------- client side

/// Minimal non-blocking client, used in `integrated-spacenav` mode to
/// observe an existing spacenavd without touching the hardware (FR-107).
pub struct SpnavClient {
    s: UnixStream,
    rbuf: Vec<u8>,
    pub proto: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientEvent {
    Motion([i32; 6], i32),
    Button(i32, bool),
    Dev { op: i32, devtype: i32, vid: i32, pid: i32 },
    Other,
}

impl SpnavClient {
    pub fn connect(path: &Path, name: &str, evmask: u32) -> std::io::Result<SpnavClient> {
        let mut s = UnixStream::connect(path)?;
        s.set_read_timeout(Some(std::time::Duration::from_millis(500)))?;
        s.write_all(&(REQ_TAG | REQ_CHANGE_PROTO | MAX_PROTO_VER).to_ne_bytes())?;
        let mut r = [0u8; 4];
        let proto = match s.read_exact(&mut r) {
            Ok(()) => i32::from_ne_bytes(r) & 0xff,
            Err(_) => 0,
        };
        let mut c = SpnavClient { s, rbuf: Vec::new(), proto };
        if proto >= 1 {
            c.send_name(name)?;
            let mut d = [0i32; 8];
            d[0] = REQ_TAG | REQ_SET_EVMASK;
            d[1] = evmask as i32;
            c.s.write_all(&encode8(d))?;
        }
        c.s.set_nonblocking(true)?;
        Ok(c)
    }

    fn send_name(&mut self, name: &str) -> std::io::Result<()> {
        let b = name.as_bytes();
        let mut rem = b.len().min(MAX_NAME);
        let mut off = 0;
        let mut first = true;
        while rem > 0 {
            let n = rem.min(REQSTR_CHUNK);
            let mut chunk = [0u8; 24];
            chunk[..n].copy_from_slice(&b[off..off + n]);
            let mut d = [0i32; 8];
            d[0] = REQ_TAG | REQ_SET_NAME;
            for i in 0..6 {
                d[1 + i] = i32::from_ne_bytes([chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3]]);
            }
            d[7] = rem as i32 | if first { 0 } else { REQSTR_CONT_BIT };
            self.s.write_all(&encode8(d))?;
            first = false;
            off += n;
            rem -= n;
        }
        Ok(())
    }

    pub fn fd(&self) -> RawFd {
        self.s.as_raw_fd()
    }

    /// Read available events. Returns Err on disconnect.
    pub fn read_events(&mut self) -> std::io::Result<Vec<ClientEvent>> {
        let mut buf = [0u8; 1024];
        loop {
            match self.s.read(&mut buf) {
                Ok(0) => return Err(std::io::Error::new(ErrorKind::UnexpectedEof, "spacenavd closed")),
                Ok(n) => self.rbuf.extend_from_slice(&buf[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let mut out = Vec::new();
        while self.rbuf.len() >= 32 {
            let d = decode8(&self.rbuf[..32]);
            self.rbuf.drain(..32);
            out.push(match d[0] {
                UEV_MOTION => ClientEvent::Motion([d[1], d[2], d[3], d[4], d[5], d[6]], d[7]),
                UEV_PRESS | UEV_RELEASE => ClientEvent::Button(d[1], d[0] == UEV_PRESS),
                UEV_DEV => ClientEvent::Dev { op: d[1], devtype: d[3], vid: d[4], pid: d[5] },
                _ => ClientEvent::Other,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
