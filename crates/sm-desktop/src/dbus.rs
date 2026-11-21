//! Minimal D-Bus client (session bus) for the KDE Plasma focus provider.
//!
//! KWin scripts can only talk to the outside world through D-Bus
//! (`callDBus`), so the agent owns a well-known name and receives
//! `ReportFocus` calls. Only what that needs is implemented: EXTERNAL auth,
//! method calls and returns, errors, and the basic types `y b i u d s o g`.
//! No new dependency, no async runtime (ADR-0009 keeps the agent small).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub const METHOD_CALL: u8 = 1;
pub const METHOD_RETURN: u8 = 2;
pub const ERROR: u8 = 3;
pub const SIGNAL: u8 = 4;
const NO_REPLY_EXPECTED: u8 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Byte(u8),
    Bool(bool),
    I32(i32),
    U32(u32),
    F64(f64),
    Str(String),
    Path(String),
    Sig(String),
}

impl Arg {
    fn code(&self) -> char {
        match self {
            Arg::Byte(_) => 'y',
            Arg::Bool(_) => 'b',
            Arg::I32(_) => 'i',
            Arg::U32(_) => 'u',
            Arg::F64(_) => 'd',
            Arg::Str(_) => 's',
            Arg::Path(_) => 'o',
            Arg::Sig(_) => 'g',
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Arg::Str(s) | Arg::Path(s) | Arg::Sig(s) => Some(s),
            _ => None,
        }
    }
    /// Numbers and numeric strings (KWin's JS bridge sends numbers as `d`).
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Arg::Byte(v) => Some(*v as u64),
            Arg::I32(v) => u64::try_from(*v).ok(),
            Arg::U32(v) => Some(*v as u64),
            Arg::F64(v) if *v >= 0.0 && v.is_finite() => Some(*v as u64),
            Arg::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Message {
    pub typ: u8,
    pub flags: u8,
    pub serial: u32,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error_name: Option<String>,
    pub reply_serial: Option<u32>,
    pub destination: Option<String>,
    pub sender: Option<String>,
    pub body: Vec<Arg>,
}

// ---------- marshalling (little-endian) ----------

struct W(Vec<u8>);
impl W {
    fn align(&mut self, n: usize) {
        while !self.0.len().is_multiple_of(n) {
            self.0.push(0);
        }
    }
    fn u32(&mut self, v: u32) {
        self.align(4);
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
    }
    fn sig(&mut self, s: &str) {
        self.0.push(s.len() as u8);
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
    }
    fn arg(&mut self, a: &Arg) {
        match a {
            Arg::Byte(v) => self.0.push(*v),
            Arg::Bool(v) => self.u32(*v as u32),
            Arg::I32(v) => self.u32(*v as u32),
            Arg::U32(v) => self.u32(*v),
            Arg::F64(v) => {
                self.align(8);
                self.0.extend_from_slice(&v.to_le_bytes());
            }
            Arg::Str(s) | Arg::Path(s) => self.str(s),
            Arg::Sig(s) => self.sig(s),
        }
    }
}

pub fn encode(m: &Message) -> Vec<u8> {
    let sig: String = m.body.iter().map(Arg::code).collect();
    let mut body = W(Vec::new());
    for a in &m.body {
        body.arg(a);
    }
    let mut h = W(vec![b'l', m.typ, m.flags, 1]);
    h.u32(body.0.len() as u32);
    h.u32(m.serial);
    // header field array: placeholder length, filled in below
    h.u32(0);
    let start = h.0.len();
    let field = |h: &mut W, code: u8, a: Arg| {
        h.align(8);
        h.0.push(code);
        h.sig(&a.code().to_string());
        h.arg(&a);
    };
    if let Some(p) = &m.path {
        field(&mut h, 1, Arg::Path(p.clone()));
    }
    if let Some(v) = &m.interface {
        field(&mut h, 2, Arg::Str(v.clone()));
    }
    if let Some(v) = &m.member {
        field(&mut h, 3, Arg::Str(v.clone()));
    }
    if let Some(v) = &m.error_name {
        field(&mut h, 4, Arg::Str(v.clone()));
    }
    if let Some(v) = m.reply_serial {
        field(&mut h, 5, Arg::U32(v));
    }
    if let Some(v) = &m.destination {
        field(&mut h, 6, Arg::Str(v.clone()));
    }
    if !sig.is_empty() {
        field(&mut h, 8, Arg::Sig(sig));
    }
    let len = (h.0.len() - start) as u32;
    h.0[12..16].copy_from_slice(&len.to_le_bytes());
    h.align(8);
    h.0.extend_from_slice(&body.0);
    h.0
}

struct R<'a> {
    b: &'a [u8],
    pos: usize,
    le: bool,
}
impl R<'_> {
    fn align(&mut self, n: usize) {
        self.pos = self.pos.div_ceil(n) * n;
    }
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        self.align(4);
        let s: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(if self.le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let s = String::from_utf8(self.take(n)?.to_vec()).ok()?;
        self.take(1)?;
        Some(s)
    }
    fn sig(&mut self) -> Option<String> {
        let n = *self.take(1)?.first()? as usize;
        let s = String::from_utf8(self.take(n)?.to_vec()).ok()?;
        self.take(1)?;
        Some(s)
    }
    fn arg(&mut self, c: char) -> Option<Arg> {
        Some(match c {
            'y' => Arg::Byte(*self.take(1)?.first()?),
            'b' => Arg::Bool(self.u32()? != 0),
            'i' => Arg::I32(self.u32()? as i32),
            'u' => Arg::U32(self.u32()?),
            'd' => {
                self.align(8);
                let s: [u8; 8] = self.take(8)?.try_into().ok()?;
                Arg::F64(if self.le { f64::from_le_bytes(s) } else { f64::from_be_bytes(s) })
            }
            's' => Arg::Str(self.str()?),
            'o' => Arg::Path(self.str()?),
            'g' => Arg::Sig(self.sig()?),
            _ => return None,
        })
    }
}

/// Total length of the message starting with this 16-byte fixed header.
pub fn frame_len(fixed: &[u8; 16]) -> Option<usize> {
    let le = match fixed[0] {
        b'l' => true,
        b'B' => false,
        _ => return None,
    };
    let rd = |o: usize| {
        let s: [u8; 4] = fixed[o..o + 4].try_into().unwrap();
        (if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) }) as usize
    };
    let (body, fields) = (rd(4), rd(12));
    if body > 1 << 24 || fields > 1 << 16 {
        return None; // far beyond anything this provider expects
    }
    Some((16 + fields).div_ceil(8) * 8 + body)
}

pub fn decode(b: &[u8]) -> Option<Message> {
    let le = match *b.first()? {
        b'l' => true,
        b'B' => false,
        _ => return None,
    };
    let mut r = R { b, pos: 1, le };
    let typ = *r.take(1)?.first()?;
    let flags = *r.take(1)?.first()?;
    r.take(1)?;
    let body_len = r.u32()? as usize;
    let serial = r.u32()?;
    let fields_len = r.u32()? as usize;
    let fields_end = r.pos + fields_len;
    let mut m = Message { typ, flags, serial, ..Default::default() };
    let mut sig = String::new();
    while r.pos < fields_end {
        r.align(8);
        let code = *r.take(1)?.first()?;
        let vsig = r.sig()?;
        let v = r.arg(vsig.chars().next()?)?;
        match (code, v) {
            (1, Arg::Path(s)) => m.path = Some(s),
            (2, Arg::Str(s)) => m.interface = Some(s),
            (3, Arg::Str(s)) => m.member = Some(s),
            (4, Arg::Str(s)) => m.error_name = Some(s),
            (5, Arg::U32(v)) => m.reply_serial = Some(v),
            (6, Arg::Str(s)) => m.destination = Some(s),
            (7, Arg::Str(s)) => m.sender = Some(s),
            (8, Arg::Sig(s)) => sig = s,
            _ => {}
        }
    }
    r.align(8);
    let body_start = r.pos;
    let body = b.get(body_start..body_start + body_len)?;
    // body alignment is relative to the body start, which is 8-aligned
    let mut br = R { b: body, pos: 0, le };
    for c in sig.chars() {
        match br.arg(c) {
            Some(a) => m.body.push(a),
            None => break, // container types: not needed here, ignore the rest
        }
    }
    Some(m)
}

// ---------- connection ----------

pub fn session_bus_path() -> PathBuf {
    if let Ok(a) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        for part in a.split(';') {
            if let Some(rest) = part.strip_prefix("unix:") {
                for kv in rest.split(',') {
                    if let Some(p) = kv.strip_prefix("path=") {
                        return PathBuf::from(p);
                    }
                }
            }
        }
    }
    let rt = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    rt.join("bus")
}

pub struct Conn {
    rd: BufReader<UnixStream>,
    wr: UnixStream,
    serial: u32,
    pub unique_name: String,
    /// messages that arrived while waiting for a reply
    pending: std::collections::VecDeque<Message>,
}

impl Conn {
    pub fn session() -> std::io::Result<Conn> {
        Conn::connect(&session_bus_path())
    }

    pub fn connect(path: &std::path::Path) -> std::io::Result<Conn> {
        let s = UnixStream::connect(path)?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut wr = s.try_clone()?;
        let mut rd = BufReader::new(s);
        let uid = unsafe { libc::getuid() }.to_string();
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        wr.write_all(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let mut line = String::new();
        rd.read_line(&mut line)?;
        if !line.starts_with("OK ") {
            return Err(std::io::Error::other(format!("D-Bus auth rejected: {}", line.trim())));
        }
        wr.write_all(b"BEGIN\r\n")?;
        let mut c = Conn { rd, wr, serial: 0, unique_name: String::new(), pending: Default::default() };
        let r = c.call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "Hello", vec![])?;
        c.unique_name = r.body.first().and_then(Arg::as_str).unwrap_or_default().to_string();
        Ok(c)
    }

    pub fn set_read_timeout(&self, t: Option<Duration>) -> std::io::Result<()> {
        self.rd.get_ref().set_read_timeout(t)
    }

    fn next_serial(&mut self) -> u32 {
        self.serial = self.serial.wrapping_add(1).max(1);
        self.serial
    }

    pub fn send(&mut self, mut m: Message) -> std::io::Result<u32> {
        m.serial = self.next_serial();
        self.wr.write_all(&encode(&m))?;
        Ok(m.serial)
    }

    pub fn read(&mut self) -> std::io::Result<Message> {
        if let Some(m) = self.pending.pop_front() {
            return Ok(m);
        }
        let mut fixed = [0u8; 16];
        self.rd.read_exact(&mut fixed)?;
        let n = frame_len(&fixed).ok_or_else(|| std::io::Error::other("bad D-Bus frame"))?;
        let mut buf = fixed.to_vec();
        buf.resize(n, 0);
        self.rd.read_exact(&mut buf[16..])?;
        decode(&buf).ok_or_else(|| std::io::Error::other("undecodable D-Bus message"))
    }

    /// Call a method and wait for its reply; an ERROR reply becomes `Err`.
    pub fn call(&mut self, dest: &str, path: &str, iface: &str, member: &str, body: Vec<Arg>) -> std::io::Result<Message> {
        let serial = self.send(Message {
            typ: METHOD_CALL,
            destination: Some(dest.into()),
            path: Some(path.into()),
            interface: Some(iface.into()),
            member: Some(member.into()),
            body,
            ..Default::default()
        })?;
        loop {
            let mut fixed = [0u8; 16];
            self.rd.read_exact(&mut fixed)?;
            let n = frame_len(&fixed).ok_or_else(|| std::io::Error::other("bad D-Bus frame"))?;
            let mut buf = fixed.to_vec();
            buf.resize(n, 0);
            self.rd.read_exact(&mut buf[16..])?;
            let m = decode(&buf).ok_or_else(|| std::io::Error::other("undecodable D-Bus message"))?;
            if m.reply_serial == Some(serial) && (m.typ == METHOD_RETURN || m.typ == ERROR) {
                if m.typ == ERROR {
                    let detail = m.body.first().and_then(Arg::as_str).unwrap_or("");
                    return Err(std::io::Error::other(format!("{}: {detail}", m.error_name.unwrap_or_default())));
                }
                return Ok(m);
            }
            self.pending.push_back(m);
        }
    }

    pub fn reply(&mut self, to: &Message, body: Vec<Arg>) -> std::io::Result<()> {
        if to.flags & NO_REPLY_EXPECTED != 0 {
            return Ok(());
        }
        self.send(Message { typ: METHOD_RETURN, reply_serial: Some(to.serial), destination: to.sender.clone(), body, ..Default::default() })?;
        Ok(())
    }

    pub fn reply_error(&mut self, to: &Message, name: &str, text: &str) -> std::io::Result<()> {
        if to.flags & NO_REPLY_EXPECTED != 0 {
            return Ok(());
        }
        self.send(Message {
            typ: ERROR,
            reply_serial: Some(to.serial),
            destination: to.sender.clone(),
            error_name: Some(name.into()),
            body: vec![Arg::Str(text.into())],
            ..Default::default()
        })?;
        Ok(())
    }

    /// RequestName with DO_NOT_QUEUE; true when we became the primary owner.
    pub fn request_name(&mut self, name: &str) -> std::io::Result<bool> {
        let r = self.call("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName", vec![Arg::Str(name.into()), Arg::U32(4)])?;
        Ok(matches!(r.body.first(), Some(Arg::U32(1)) | Some(Arg::U32(4))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_method_call() {
        let m = Message {
            typ: METHOD_CALL,
            serial: 7,
            path: Some("/a/b".into()),
            interface: Some("x.y".into()),
            member: Some("M".into()),
            destination: Some("x.y".into()),
            body: vec![Arg::Str("blender".into()), Arg::U32(5), Arg::F64(1234.0), Arg::Bool(true), Arg::Byte(3), Arg::Str("".into())],
            ..Default::default()
        };
        let b = encode(&m);
        let fixed: [u8; 16] = b[..16].try_into().unwrap();
        assert_eq!(frame_len(&fixed), Some(b.len()));
        assert_eq!(decode(&b).unwrap(), m);
    }

    #[test]
    fn truncated_or_garbage_is_rejected() {
        let m = Message { typ: METHOD_CALL, serial: 1, member: Some("Hello".into()), body: vec![Arg::Str("x".into())], ..Default::default() };
        let b = encode(&m);
        for n in 0..b.len() {
            assert!(decode(&b[..n]).is_none() || n == b.len(), "decoded truncated message of {n} bytes");
        }
        assert!(decode(b"Xgarbage-garbage").is_none());
        let mut huge = [0u8; 16];
        huge[0] = b'l';
        huge[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(frame_len(&huge).is_none());
    }

    #[test]
    fn numbers_from_kwin_js() {
        assert_eq!(Arg::F64(4242.0).as_u64(), Some(4242));
        assert_eq!(Arg::Str("17".into()).as_u64(), Some(17));
        assert_eq!(Arg::F64(-1.0).as_u64(), None);
    }
}
