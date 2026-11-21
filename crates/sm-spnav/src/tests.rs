use super::*;
use std::time::Duration;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sm-spnav-test-{}-{}", std::process::id(), name));
    let _ = std::fs::create_dir_all(&d);
    d.join("spnav.sock")
}

fn pump(srv: &mut SpnavServer) {
    for _ in 0..20 {
        srv.accept();
        for (fd, _) in srv.client_fds() {
            srv.service(fd);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn dev() -> DeviceInfo {
    DeviceInfo {
        id: 0,
        name: "3Dconnexion SpaceMouse Compact".into(),
        path: "/dev/hidraw0".into(),
        naxes: 6,
        nbuttons: 2,
        vid: 0x256f,
        pid: 0xc635,
        devtype: 0x20f,
    }
}

#[test]
fn v1_client_roundtrip() {
    let p = tmp("v1");
    let mut srv = SpnavServer::bind(&p).unwrap();
    srv.devices.push(dev());
    let h = std::thread::spawn({
        let p = p.clone();
        move || SpnavClient::connect(&p, "unit-test", EVMASK_MOTION | EVMASK_BUTTON | EVMASK_DEV)
    });
    // the client blocks on the protocol reply; serve until it returns
    let mut cl = loop {
        pump(&mut srv);
        if h.is_finished() {
            break h.join().unwrap().unwrap();
        }
    };
    assert_eq!(cl.proto, 1);
    pump(&mut srv);
    assert_eq!(srv.client_names(), vec!["unit-test".to_string()]);
    // drain SET_EVMASK response
    std::thread::sleep(Duration::from_millis(20));
    let _ = cl.read_events();

    srv.send_motion([1, -2, 3, -4, 5, -6], 16);
    srv.send_button(1, true);
    srv.send_button(1, false);
    std::thread::sleep(Duration::from_millis(20));
    let ev = cl.read_events().unwrap();
    assert_eq!(ev, vec![ClientEvent::Motion([1, -2, 3, -4, 5, -6], 16), ClientEvent::Button(1, true), ClientEvent::Button(1, false)]);
    drop(srv);
    assert!(!p.exists(), "socket removed on drop");
}

#[test]
fn v1_device_queries_and_refusals() {
    let p = tmp("q");
    let mut srv = SpnavServer::bind(&p).unwrap();
    srv.devices.push(dev());
    let mut s = UnixStream::connect(&p).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s.write_all(&(REQ_TAG | REQ_CHANGE_PROTO | 7).to_ne_bytes()).unwrap();
    pump(&mut srv);
    let mut r = [0u8; 4];
    s.read_exact(&mut r).unwrap();
    assert_eq!(i32::from_ne_bytes(r), REQ_TAG | REQ_CHANGE_PROTO | 1, "clamped to max version");

    let ask = |s: &mut UnixStream, srv: &mut SpnavServer, req: i32| -> [i32; 8] {
        let mut d = [0; 8];
        d[0] = REQ_TAG | req;
        s.write_all(&encode8(d)).unwrap();
        pump(srv);
        let mut b = [0u8; 32];
        s.read_exact(&mut b).unwrap();
        decode8(&b)
    };
    let r = ask(&mut s, &mut srv, REQ_DEV_USBID);
    assert_eq!((r[0], r[1], r[2], r[7]), (REQ_TAG | REQ_DEV_USBID, 0x256f, 0xc635, 0));
    let r = ask(&mut s, &mut srv, REQ_DEV_TYPE);
    assert_eq!(r[1], 0x20f);
    let r = ask(&mut s, &mut srv, REQ_DEV_NBUTTONS);
    assert_eq!(r[1], 2);
    // device name: multi-chunk string, 30 chars -> 2 packets
    let r = ask(&mut s, &mut srv, REQ_DEV_NAME);
    assert_eq!(r[7], 30);
    let mut b = [0u8; 32];
    s.read_exact(&mut b).unwrap();
    let r2 = decode8(&b);
    assert_eq!(r2[7], 6 | REQSTR_CONT_BIT);
    // config writes are refused (profile owns configuration)
    let r = ask(&mut s, &mut srv, 0x3000);
    assert_eq!(r[7], -1);
    let r = ask(&mut s, &mut srv, 0x7fff);
    assert_eq!(r[7], -1);
}

#[test]
fn v0_client_gets_events_and_sensitivity() {
    let p = tmp("v0");
    let mut srv = SpnavServer::bind(&p).unwrap();
    let mut s = UnixStream::connect(&p).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    s.write_all(&2.0f32.to_bits().to_ne_bytes()).unwrap();
    pump(&mut srv);
    srv.send_motion([10, 0, 0, 0, 0, -10], 8);
    let mut b = [0u8; 32];
    s.read_exact(&mut b).unwrap();
    assert_eq!(decode8(&b), [UEV_MOTION, 20, 0, 0, 0, 0, -20, 8]);
}

#[test]
fn refuses_live_foreign_socket() {
    let p = tmp("live");
    let _other = UnixListener::bind(&p).unwrap();
    let e = SpnavServer::bind(&p).err().unwrap();
    assert_eq!(e.kind(), ErrorKind::AddrInUse);
    assert!(p.exists(), "never delete another daemon's socket");
}

#[test]
fn stale_socket_replaced() {
    let p = tmp("stale");
    drop(UnixListener::bind(&p).unwrap()); // leaves a dead socket file
    assert!(p.exists());
    let srv = SpnavServer::bind(&p).unwrap();
    assert_eq!(std::fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o666);
    drop(srv);
}

#[test]
fn garbage_input_disconnects_without_panic() {
    let p = tmp("garbage");
    let mut srv = SpnavServer::bind(&p).unwrap();
    let mut s = UnixStream::connect(&p).unwrap();
    s.write_all(&(REQ_TAG | REQ_CHANGE_PROTO | 1).to_ne_bytes()).unwrap();
    s.write_all(&[0xffu8; 8000]).unwrap();
    pump(&mut srv);
    assert_eq!(srv.client_count(), 0);
}

#[test]
fn idle_client_backlog_is_bounded_and_ends_neutral() {
    let p = tmp("backlog");
    let mut srv = SpnavServer::bind(&p).unwrap();
    let h = std::thread::spawn({
        let p = p.clone();
        move || SpnavClient::connect(&p, "sleepy", EVMASK_MOTION | EVMASK_BUTTON)
    });
    let mut cl = loop {
        pump(&mut srv);
        if h.is_finished() {
            break h.join().unwrap().unwrap();
        }
    };
    std::thread::sleep(Duration::from_millis(20));
    let _ = cl.read_events();

    // the client stops reading while the device keeps moving (~16 s at 125 Hz)
    for i in 0..2000 {
        srv.send_motion([i % 300 + 1, 0, 0, 0, 0, 0], 8);
    }
    srv.send_button(0, true);
    srv.send_button(0, false);
    srv.send_motion([0; 6], 8);
    assert!(srv.dropped_motion() > 1900, "stale motion withheld, dropped {}", srv.dropped_motion());
    assert_eq!(srv.client_names().len(), 1, "a sleepy client is not disconnected");

    // when it wakes up it sees a short backlog, the buttons and a final neutral
    std::thread::sleep(Duration::from_millis(20));
    let mut ev = Vec::new();
    for _ in 0..50 {
        match cl.read_events() {
            Ok(e) if !e.is_empty() => ev.extend(e),
            _ => break,
        }
    }
    let motions = ev.iter().filter(|e| matches!(e, ClientEvent::Motion(..))).count();
    assert!(motions < 40, "bounded stale motion, got {motions}");
    assert!(ev.contains(&ClientEvent::Button(0, true)) && ev.contains(&ClientEvent::Button(0, false)));
    assert_eq!(ev.last(), Some(&ClientEvent::Motion([0; 6], 8)), "ends neutral");
}

// ---------------------------------------------------------------- fuzzing (SRS §12)
// Any local process can connect to the spnav socket, so request parsing must never panic,
// never grow buffers without bound, and keep the client name within MAX_NAME.

fn fuzz_client() -> (Client, UnixStream) {
    let (a, b) = UnixStream::pair().unwrap();
    a.set_nonblocking(true).unwrap();
    let c = Client {
        s: a,
        proto: 0,
        evmask: 0,
        sens: 1.0,
        name: String::new(),
        rbuf: Vec::new(),
        strbuf: None,
        wbuf: VecDeque::new(),
        dead: false,
        dropped_motion: 0,
    };
    (c, b)
}

fn arb_request() -> impl proptest::strategy::Strategy<Value = Vec<u8>> {
    use proptest::prelude::*;
    let tagged = (0x1000i32..0x1020, proptest::array::uniform7(any::<i32>())).prop_map(|(req, rest)| {
        let mut d = [0i32; 8];
        d[0] = REQ_TAG | req;
        d[1..].copy_from_slice(&rest);
        encode8(d).to_vec()
    });
    prop_oneof![
        3 => tagged,
        1 => Just((REQ_TAG | REQ_CHANGE_PROTO | 1).to_ne_bytes().to_vec()),
        1 => any::<u8>().prop_map(|v| (REQ_TAG | REQ_CHANGE_PROTO | v as i32).to_ne_bytes().to_vec()),
        2 => proptest::collection::vec(any::<u8>(), 0..64),
    ]
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(
        std::env::var("SM_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(512)))]
    #[test]
    fn fuzz_requests_never_panic(reqs in proptest::collection::vec(arb_request(), 0..40), chunk in 1usize..64) {
        let (mut c, _peer) = fuzz_client();
        let bytes: Vec<u8> = reqs.concat();
        let d = dev();
        for part in bytes.chunks(chunk) {
            c.rbuf.extend_from_slice(part);
            process_requests(&mut c, Some(&d));
            proptest::prop_assert!(c.rbuf.len() < 32 + chunk || c.dead);
            proptest::prop_assert!(c.wbuf.len() <= MAX_WBUF);
            proptest::prop_assert!(c.name.len() <= MAX_NAME);
            proptest::prop_assert!(c.strbuf.as_ref().map(|(_, b)| b.len() <= MAX_NAME + 24).unwrap_or(true));
            proptest::prop_assert!(c.sens.is_finite());
            if c.dead { break; }
        }
        // a device-less server must cope too
        process_requests(&mut c, None);
    }
}
