//! Control-plane IPC (SRS §4.1, §4.3, §8).
//!
//! Transport: AF_UNIX stream sockets, newline-delimited JSON, one object per
//! line, max [`MAX_LINE`] bytes. Every request carries `"v": 1` and an
//! optional numeric `id` echoed in the reply. Peers are authenticated with
//! `SO_PEERCRED`; the broker only accepts state-changing commands from root
//! or the uid that owns the *active* logind session on seat0.
//!
//! Sockets:
//! * broker (system):  `/run/3dmouse/broker.sock`
//! * agent  (session): `$XDG_RUNTIME_DIR/3dmouse/agent.sock` (0600 dir)

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sm_core::{AppMode, DeviceState, OwnershipMode};
use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

pub const PROTO_VERSION: u32 = 1;
pub const MAX_LINE: usize = 1 << 20;
pub const DEFAULT_BROKER_SOCKET: &str = "/run/3dmouse/broker.sock";

pub fn broker_socket() -> PathBuf {
    std::env::var_os("THREEDMOUSE_BROKER_SOCKET").map(PathBuf::from).unwrap_or_else(|| DEFAULT_BROKER_SOCKET.into())
}

pub fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })))
}

pub fn agent_socket() -> PathBuf {
    std::env::var_os("THREEDMOUSE_AGENT_SOCKET").map(PathBuf::from).unwrap_or_else(|| runtime_dir().join("3dmouse").join("agent.sock"))
}

// ------------------------------------------------------------ peer creds

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PeerCred {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

pub fn peer_cred(fd: &impl AsRawFd) -> std::io::Result<PeerCred> {
    let mut c = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: valid fd, out-pointer sized correctly.
    let r = unsafe { libc::getsockopt(fd.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, &mut c as *mut _ as *mut libc::c_void, &mut len) };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(PeerCred { pid: c.pid, uid: c.uid, gid: c.gid })
}

/// uid of the active session on `seat`, from logind's public runtime
/// state files (`/run/systemd/seats/<seat>`: `ACTIVE_UID=`).
pub fn active_seat_uid(seat: &str) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/run/systemd/seats/{seat}")).ok()?;
    s.lines().find_map(|l| l.strip_prefix("ACTIVE_UID=")).and_then(|v| v.trim().parse().ok())
}

pub fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

// ------------------------------------------------------------ framing

/// Bounded line reader for non-blocking sockets.
#[derive(Default)]
pub struct LineBuf {
    buf: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Open,
    Closed,
    Overflow,
}

impl LineBuf {
    /// Read whatever is available; push complete lines into `out`.
    pub fn read_from(&mut self, s: &mut impl Read, out: &mut Vec<String>) -> ReadOutcome {
        let mut tmp = [0u8; 8192];
        loop {
            match s.read(&mut tmp) {
                Ok(0) => {
                    self.split(out);
                    return ReadOutcome::Closed;
                }
                Ok(n) => {
                    self.buf.extend_from_slice(&tmp[..n]);
                    self.split(out);
                    if self.buf.len() > MAX_LINE {
                        return ReadOutcome::Overflow;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return ReadOutcome::Open,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return ReadOutcome::Closed,
            }
        }
    }

    fn split(&mut self, out: &mut Vec<String>) {
        while let Some(p) = self.buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=p).collect();
            if let Ok(s) = String::from_utf8(line) {
                let s = s.trim();
                if !s.is_empty() {
                    out.push(s.to_string());
                }
            }
        }
    }
}

/// Non-blocking writer with a bounded queue. Returns false if the peer is
/// too slow (caller should drop droppable messages or disconnect).
#[derive(Default)]
pub struct WriteBuf {
    pub buf: Vec<u8>,
}

pub const MAX_WBUF: usize = 4 << 20;

impl WriteBuf {
    pub fn push_line(&mut self, v: &Value) -> bool {
        let mut s = serde_json::to_vec(v).unwrap_or_default();
        s.push(b'\n');
        if self.buf.len() + s.len() > MAX_WBUF {
            return false;
        }
        self.buf.extend_from_slice(&s);
        true
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Returns Err if the peer is gone.
    pub fn flush(&mut self, s: &mut impl Write) -> std::io::Result<()> {
        while !self.buf.is_empty() {
            match s.write(&self.buf) {
                Ok(0) => return Err(ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.buf.drain(..n);
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------ messages

pub fn ok(id: &Value, result: Value) -> Value {
    serde_json::json!({"v": PROTO_VERSION, "id": id, "ok": true, "result": result})
}

pub fn err(id: &Value, code: &str, message: impl Into<String>) -> Value {
    serde_json::json!({"v": PROTO_VERSION, "id": id, "ok": false, "error": {"code": code, "message": message.into()}})
}

pub fn event(name: &str, body: Value) -> Value {
    let mut v = serde_json::json!({"v": PROTO_VERSION, "event": name});
    if let (Some(o), Value::Object(b)) = (v.as_object_mut(), body) {
        o.extend(b);
    }
    v
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceStatus {
    pub id: u32,
    pub name: String,
    pub usb_id: String,
    pub transport: String,
    pub path: String,
    /// "hidraw" or "evdev"
    pub input_path: String,
    pub state: DeviceState,
    pub support_status: String,
    pub generic: bool,
    pub buttons: Vec<ButtonLabel>,
    pub axis_range: i32,
    pub serial: String,
    pub frames: u64,
    pub malformed: u64,
    pub watchdog_resets: u64,
    pub event_rate_hz: f32,
    pub battery: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ButtonLabel {
    pub index: u8,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrokerStatus {
    pub version: String,
    pub mode: OwnershipMode,
    pub mode_reason: String,
    pub devices: Vec<DeviceStatus>,
    pub spnav_socket: Option<String>,
    pub spnav_clients: Vec<String>,
    pub spnav_motion_events: u64,
    pub injection_available: bool,
    pub injection_reason: String,
    /// a virtual game controller exists (virtual-joystick profile active, FR-507)
    #[serde(default)]
    pub virtual_joystick: bool,
    pub locked: bool,
    pub app_mode: AppMode,
    pub profile: String,
    pub config_owner_uid: Option<u32>,
    pub active_uid: Option<u32>,
    pub uptime_s: u64,
}

// ------------------------------------------------------------ client

/// Blocking request/response client for CLI and agent.
pub struct Client {
    s: UnixStream,
    lb: LineBuf,
    pending: Vec<String>,
    next_id: u64,
}

impl Client {
    pub fn connect(path: &std::path::Path) -> std::io::Result<Client> {
        let s = UnixStream::connect(path)?;
        s.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        Ok(Client { s, lb: LineBuf::default(), pending: Vec::new(), next_id: 1 })
    }

    pub fn stream(&self) -> &UnixStream {
        &self.s
    }

    pub fn send(&mut self, cmd: &str, args: Value) -> std::io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        // protocol fields always win over call arguments
        let mut v = match args {
            Value::Object(a) => Value::Object(a),
            _ => serde_json::json!({}),
        };
        v["v"] = PROTO_VERSION.into();
        v["id"] = id.into();
        v["cmd"] = cmd.into();
        let mut line = serde_json::to_vec(&v)?;
        line.push(b'\n');
        self.s.write_all(&line)?;
        Ok(id)
    }

    /// Next message (reply or event), blocking up to the read timeout.
    pub fn recv(&mut self) -> std::io::Result<Value> {
        loop {
            if !self.pending.is_empty() {
                let l = self.pending.remove(0);
                return serde_json::from_str(&l).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e));
            }
            let mut tmp = [0u8; 8192];
            let n = self.s.read(&mut tmp)?;
            if n == 0 {
                return Err(ErrorKind::UnexpectedEof.into());
            }
            let mut cur = std::io::Cursor::new(&tmp[..n]);
            let mut lines = Vec::new();
            self.lb.read_from(&mut cur, &mut lines);
            self.pending.extend(lines);
        }
    }

    /// Send and wait for the matching reply, skipping events.
    pub fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        let id = self.send(cmd, args).map_err(|e| e.to_string())?;
        loop {
            let m = self.recv().map_err(|e| e.to_string())?;
            if m.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if m.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                    return Ok(m.get("result").cloned().unwrap_or(Value::Null));
                }
                let e = m.get("error").cloned().unwrap_or(Value::Null);
                return Err(format!(
                    "{}: {}",
                    e.get("code").and_then(|c| c.as_str()).unwrap_or("error"),
                    e.get("message").and_then(|c| c.as_str()).unwrap_or("")
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_buffer_splits_and_bounds() {
        let mut lb = LineBuf::default();
        let mut out = Vec::new();
        let mut c = std::io::Cursor::new(b"{\"a\":1}\n{\"b\"".to_vec());
        assert_eq!(lb.read_from(&mut c, &mut out), ReadOutcome::Closed);
        assert_eq!(out, vec!["{\"a\":1}"]);
        let mut big = std::io::Cursor::new(vec![b'x'; MAX_LINE + 10]);
        let mut lb = LineBuf::default();
        assert_eq!(lb.read_from(&mut big, &mut out), ReadOutcome::Overflow);
    }

    #[test]
    fn envelopes() {
        let id = serde_json::json!(3);
        assert_eq!(ok(&id, Value::Null)["ok"], true);
        assert_eq!(err(&id, "denied", "no")["error"]["code"], "denied");
        let e = event("motion", serde_json::json!({"dev": 1}));
        assert_eq!(e["event"], "motion");
        assert_eq!(e["dev"], 1);
    }

    #[test]
    fn peer_cred_on_socketpair() {
        let (a, _b) = UnixStream::pair().unwrap();
        let c = peer_cred(&a).unwrap();
        assert_eq!(c.uid, current_uid());
        assert_eq!(c.pid as u32, std::process::id());
    }

    #[test]
    fn client_call_roundtrip() {
        let (a, mut b) = UnixStream::pair().unwrap();
        let h = std::thread::spawn(move || {
            b.set_nonblocking(true).unwrap();
            let mut lb = LineBuf::default();
            let mut lines = Vec::new();
            while lines.is_empty() {
                lb.read_from(&mut b, &mut lines);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            b.set_nonblocking(false).unwrap();
            let req: Value = serde_json::from_str(&lines[0]).unwrap();
            let mut w = WriteBuf::default();
            w.push_line(&event("noise", serde_json::json!({})));
            w.push_line(&ok(&req["id"], serde_json::json!({"echo": req["cmd"]})));
            w.flush(&mut b).unwrap();
        });
        let mut c = Client { s: a, lb: LineBuf::default(), pending: vec![], next_id: 1 };
        let r = c.call("status", serde_json::json!({})).unwrap();
        assert_eq!(r["echo"], "status");
        h.join().unwrap();
    }
}
