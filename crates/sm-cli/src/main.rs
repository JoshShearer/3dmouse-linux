//! `3dmousectl` – command-line interface (SRS FR-900).
//!
//! Every command supports `--json` for scripting. Commands that only read
//! state work without the agent (falling back to the broker or to sysfs).

use serde_json::{json, Value};
use sm_daemon::devices;
use sm_ipc::Client;
use sm_registry::{Match, Registry};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "3dmousectl – 3DMouse Linux control

USAGE: 3dmousectl [--json] <command> [args]

  status                          broker + agent + active profile
  version
  devices list                    detected 6-DOF devices
  devices inspect [USB_ID]        descriptor, nodes, permissions, registry entry
  monitor [--duration 10s]        live axes and buttons
  profile list
  profile show ID
  profile lint ID                 binding conflicts and bindings that can't work
  profile activate ID [--persist] manual override (session, or saved)
  profile auto                    return to automatic selection
  profile export ID [--output FILE]
  profile import FILE [--dry-run | --apply] [--overwrite]   dry run unless --apply
  profile delete ID
  profile backup [--output FILE]
  profile reset-presets
  settings get
  settings set KEY VALUE          e.g. settings set allow_exec false
  calibrate [--duration 3s] [--apply]   measure neutral offset (hands off!)
  test-action JSON [--phase press|release]  e.g. '{\"type\":\"key\",\"keys\":\"ctrl+z\"}'
  diagnostics check               permissions, conflicts, services, desktop
  diagnostics record [--duration 10s] [--redact] [--output FILE]   capture raw evdev events
  diagnostics replay FILE         run a capture through the input pipeline
  support-bundle [--redact] [--output FILE]   redacted diagnostic bundle (always redacted)
  service health                  systemd units and sockets

Environment: THREEDMOUSE_BROKER_SOCKET, THREEDMOUSE_AGENT_SOCKET, THREEDMOUSE_CONFIG_DIR";

struct Ctx {
    json: bool,
}

type R = Result<Value, String>;

fn agent() -> Result<Client, String> {
    Client::connect(&sm_ipc::agent_socket()).map_err(|e| {
        format!("session agent not reachable at {} ({e}); is 3dmouse-agent running? (systemctl --user status 3dmouse-agent)", sm_ipc::agent_socket().display())
    })
}

fn broker() -> Result<Client, String> {
    Client::connect(&sm_ipc::broker_socket())
        .map_err(|e| format!("broker not reachable at {} ({e}); is 3dmoused running? (systemctl status 3dmoused)", sm_ipc::broker_socket().display()))
}

fn agent_call(cmd: &str, args: Value) -> R {
    agent()?.call(cmd, args)
}

fn flag(args: &mut Vec<String>, f: &str) -> bool {
    if let Some(i) = args.iter().position(|a| a == f) {
        args.remove(i);
        true
    } else {
        false
    }
}

fn opt(args: &mut Vec<String>, f: &str) -> Option<String> {
    let i = args.iter().position(|a| a == f)?;
    args.remove(i);
    if i < args.len() {
        Some(args.remove(i))
    } else {
        None
    }
}

/// `--duration 10s` / `--seconds 10` (s, ms, m suffixes accepted).
fn duration_secs(args: &mut Vec<String>, default: u64) -> u64 {
    if let Some(d) = opt(args, "--duration") {
        let d = d.trim();
        let (num, mul) = if let Some(n) = d.strip_suffix("ms") {
            (n, 0.001)
        } else if let Some(n) = d.strip_suffix('s') {
            (n, 1.0)
        } else if let Some(n) = d.strip_suffix('m') {
            (n, 60.0)
        } else {
            (d, 1.0)
        };
        return num.parse::<f64>().map(|v| (v * mul).ceil().max(1.0) as u64).unwrap_or(default);
    }
    opt(args, "--seconds").and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// `--output FILE` (or the short `--out` / `-o`).
fn output_opt(args: &mut Vec<String>) -> Option<String> {
    opt(args, "--output").or_else(|| opt(args, "--out")).or_else(|| opt(args, "-o"))
}

fn pos(args: &[String], i: usize, what: &str) -> Result<String, String> {
    args.get(i).cloned().ok_or_else(|| format!("missing {what}; see 3dmousectl --help"))
}

fn write_or_print(out: Option<String>, text: &str) -> R {
    match out {
        Some(f) => {
            std::fs::write(&f, text).map_err(|e| format!("{f}: {e}"))?;
            Ok(json!({"written": f}))
        }
        None => {
            print!("{text}");
            Ok(Value::Null)
        }
    }
}

// ------------------------------------------------------------- status

fn status(ctx: &Ctx) -> R {
    match agent_call("status", json!({})) {
        Ok(v) => {
            if !ctx.json {
                let a = &v["agent"];
                let p = &v["profile"];
                let b = &v["broker"];
                println!(
                    "agent       {} (broker {})",
                    a["version"].as_str().unwrap_or("?"),
                    if a["broker_connected"] == true { "connected" } else { "NOT connected" }
                );
                if a["broker_error"].as_str().map(|s| !s.is_empty()).unwrap_or(false) {
                    println!("            {}", a["broker_error"].as_str().unwrap());
                }
                println!(
                    "profile     {} [{}] via {}  mode {}",
                    p["display_name"].as_str().unwrap_or("?"),
                    p["id"].as_str().unwrap_or("?"),
                    p["selected_by"].as_str().unwrap_or("?"),
                    p["app_mode"].as_str().unwrap_or("?")
                );
                println!("focus       {} ({})", a["focus"]["app_id"].as_str().unwrap_or(""), a["desktop"]["provider"].as_str().unwrap_or("?"));
                println!("locked      {}", a["locked"]);
                if !b.is_null() {
                    print_broker(b);
                }
                return Ok(Value::Null);
            }
            Ok(v)
        }
        Err(ae) => {
            let b = broker()?.call("status", json!({}))?;
            if !ctx.json {
                println!("agent       not running ({ae})");
                print_broker(&b);
                return Ok(Value::Null);
            }
            Ok(json!({"agent": null, "broker": b}))
        }
    }
}

fn print_broker(b: &Value) {
    println!("broker      {} mode {} ({})", b["version"].as_str().unwrap_or("?"), b["mode"].as_str().unwrap_or("?"), b["mode_reason"].as_str().unwrap_or(""));
    println!(
        "spnav       {} – clients: {}",
        b["spnav_socket"].as_str().unwrap_or("not served"),
        b["spnav_clients"].as_array().map(|a| a.iter().filter_map(|c| c.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
    );
    println!("injection   {} ({})", if b["injection_available"] == true { "available" } else { "unavailable" }, b["injection_reason"].as_str().unwrap_or(""));
    if b["virtual_joystick"] == true {
        println!("joystick    virtual game controller active");
    }
    for d in b["devices"].as_array().into_iter().flatten() {
        println!(
            "device      [{}] {} {} via {} – {} – support: {}{}",
            d["id"],
            d["usb_id"].as_str().unwrap_or(""),
            d["name"].as_str().unwrap_or(""),
            d["input_path"].as_str().unwrap_or(""),
            d["state"].as_str().unwrap_or(""),
            d["support_status"].as_str().unwrap_or(""),
            if d["generic"] == true { " (generic fallback)" } else { "" }
        );
    }
    if b["devices"].as_array().map(|a| a.is_empty()).unwrap_or(true) {
        println!("device      none detected");
    }
}

// ------------------------------------------------------------- devices

fn cand_json(c: &devices::Candidate) -> Value {
    let node_info = |p: &Option<PathBuf>| {
        p.as_ref().map(|p| {
            let m = std::fs::metadata(p);
            let readable = std::fs::File::open(p).is_ok();
            json!({"path": p.display().to_string(), "mode": m.as_ref().map(|m| format!("{:o}", m.permissions().mode() & 0o777)).unwrap_or_default(), "gid": m.as_ref().map(|m| m.gid()).ok(), "readable_by_you": readable})
        })
    };
    json!({
        "key": c.key,
        "usb_id": format!("{:04x}:{:04x}", c.vid, c.pid),
        "bus": if c.bus == 5 { "bluetooth" } else { "usb" },
        "name": c.name,
        "phys": c.phys,
        "serial": sm_core::redact(&c.uniq),
        "hidraw": node_info(&c.hidraw),
        "evdev": node_info(&c.evdev),
        "descriptor_bytes": c.descriptor.len(),
        "registry": c.model.as_ref().map(|m| json!({"name": m.name, "status": m.status.as_str(), "evidence": m.evidence, "source": m.source, "buttons": m.buttons.len(), "spnav_type": format!("0x{:x}", m.spnav_type)})),
        "generic": c.generic,
    })
}

fn devices_cmd(ctx: &Ctx, args: &[String]) -> R {
    let sub = args.first().map(String::as_str).unwrap_or("list");
    let (cands, notes) = devices::scan(Registry::embedded(), Path::new("/sys"), Path::new("/dev"));
    match sub {
        "list" => {
            let live = broker().and_then(|mut b| b.call("status", json!({}))).ok();
            if !ctx.json {
                if cands.is_empty() {
                    println!("no 6-DOF devices found");
                }
                for c in &cands {
                    let st = c.model.as_ref().map(|m| m.status.as_str()).unwrap_or("generic");
                    let owned = live
                        .as_ref()
                        .and_then(|l| l["devices"].as_array().and_then(|d| d.iter().find(|d| d["usb_id"] == format!("{:04x}:{:04x}", c.vid, c.pid)).cloned()));
                    println!(
                        "{:04x}:{:04x}  {:<36} support: {:<16} {}",
                        c.vid,
                        c.pid,
                        c.name,
                        st,
                        owned
                            .map(|o| format!("broker: {} via {}", o["state"].as_str().unwrap_or(""), o["input_path"].as_str().unwrap_or("")))
                            .unwrap_or_else(|| "broker: -".into())
                    );
                }
                for n in &notes {
                    println!("note: {n}");
                }
                return Ok(Value::Null);
            }
            Ok(json!({"devices": cands.iter().map(cand_json).collect::<Vec<_>>(), "notes": notes, "broker": live}))
        }
        "inspect" => {
            let want = args.get(1).map(|s| s.to_lowercase());
            let c = cands.iter().find(|c| want.as_ref().map(|w| *w == format!("{:04x}:{:04x}", c.vid, c.pid)).unwrap_or(true)).ok_or("no matching device")?;
            let mut v = cand_json(c);
            match sm_hid::descriptor::parse(&c.descriptor) {
                Ok(d) => {
                    let lay = sm_hid::MotionLayout::from_descriptor(&d);
                    v["descriptor"] = json!({
                        "hex": c.descriptor.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "),
                        "applications": d.applications.iter().map(|(p, u)| format!("{p:04x}:{u:04x}")).collect::<Vec<_>>(),
                        "report_bits": d.report_bits,
                        "layout": lay.as_ref().map(|l| json!({"axis_range": l.axis_range, "axis_groups": l.axis_groups(), "buttons": l.buttons.len(), "multi_axis": l.is_multi_axis})).map_err(|e| e.clone()).unwrap_or_else(|e| json!({"error": e})),
                    });
                }
                Err(e) => v["descriptor"] = json!({"error": format!("{e:?}")}),
            }
            if !ctx.json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
                return Ok(Value::Null);
            }
            Ok(v)
        }
        _ => Err(format!("unknown devices subcommand {sub}")),
    }
}

// ------------------------------------------------------------- monitor

fn monitor(ctx: &Ctx, args: &mut Vec<String>) -> R {
    let secs = duration_secs(args, 0);
    let (mut c, via) = match agent() {
        Ok(mut c) => {
            c.call("subscribe", json!({"live": true, "events": false}))?;
            (c, "agent")
        }
        Err(_) => {
            let mut c = broker()?;
            c.call("subscribe", json!({"motion": true, "max_hz": 30, "buttons": true}))?;
            (c, "broker")
        }
    };
    c.stream().set_read_timeout(Some(Duration::from_millis(500))).ok();
    if !ctx.json {
        eprintln!("live input via {via}; Ctrl-C to stop{}", if secs > 0 { format!(" ({secs}s)") } else { String::new() });
        println!("{:>7} {:>7} {:>7} {:>7} {:>7} {:>7}   spnav", "TX", "TY", "TZ", "RX", "RY", "RZ");
    }
    let start = Instant::now();
    loop {
        if secs > 0 && start.elapsed().as_secs() >= secs {
            return Ok(Value::Null);
        }
        let m = match c.recv() {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(e.to_string()),
        };
        if ctx.json {
            println!("{m}");
            continue;
        }
        match m["event"].as_str() {
            Some("motion") => {
                let r: Vec<i64> = m["raw"].as_array().map(|a| a.iter().map(|v| v.as_i64().unwrap_or(0)).collect()).unwrap_or_default();
                println!(
                    "{:>7} {:>7} {:>7} {:>7} {:>7} {:>7}   {}",
                    r.first().unwrap_or(&0),
                    r.get(1).unwrap_or(&0),
                    r.get(2).unwrap_or(&0),
                    r.get(3).unwrap_or(&0),
                    r.get(4).unwrap_or(&0),
                    r.get(5).unwrap_or(&0),
                    m["spnav"]
                );
            }
            Some("button") => println!("button {} {}", m["button"], if m["pressed"] == true { "pressed" } else { "released" }),
            _ => {}
        }
    }
}

// ------------------------------------------------------------- profiles

fn profile_cmd(ctx: &Ctx, args: &mut Vec<String>) -> R {
    let persist = flag(args, "--persist");
    let dry_run = flag(args, "--dry-run");
    let apply = flag(args, "--apply") && !dry_run;
    let output = output_opt(args);
    let overwrite = flag(args, "--overwrite");
    let sub = pos(args, 0, "profile subcommand")?;
    match sub.as_str() {
        "list" => {
            let v = agent_call("profiles.list", json!({}))?;
            if !ctx.json {
                for p in v.as_array().into_iter().flatten() {
                    println!(
                        "{} {:<18} {:<28} {:<18} {}",
                        if p["active"] == true { "*" } else { " " },
                        p["id"].as_str().unwrap_or(""),
                        p["display_name"].as_str().unwrap_or(""),
                        p["app_mode"].as_str().unwrap_or(""),
                        p["notes"].as_str().unwrap_or("")
                    );
                }
                return Ok(Value::Null);
            }
            Ok(v)
        }
        "lint" => {
            let v = agent_call("profiles.lint", json!({"profile": pos(args, 1, "profile id")?}))?;
            if !ctx.json {
                let items = v.as_array().cloned().unwrap_or_default();
                if items.is_empty() {
                    println!("no problems found");
                }
                for l in &items {
                    let b = l["button"].as_u64().map(|b| format!("button {} {}", b + 1, l["slot"].as_str().unwrap_or(""))).unwrap_or_else(|| "profile".into());
                    println!("{:7} {b}: {}", l["severity"].as_str().unwrap_or("?"), l["message"].as_str().unwrap_or(""));
                }
                return Ok(Value::Null);
            }
            Ok(v)
        }
        "show" => {
            let v = agent_call("profiles.get", json!({"profile": pos(args, 1, "profile id")?}))?;
            if !ctx.json {
                let p: sm_profile::Profile = serde_json::from_value(v).map_err(|e| e.to_string())?;
                print!("{}", p.to_toml()?);
                return Ok(Value::Null);
            }
            Ok(v)
        }
        "activate" => agent_call("profiles.activate", json!({"profile": pos(args, 1, "profile id")?, "persist": persist})),
        "auto" => agent_call("profiles.activate", json!({"profile": null, "persist": true})),
        "export" => {
            let v = agent_call("profiles.export", json!({"profile": pos(args, 1, "profile id")?}))?;
            write_or_print(output.or_else(|| args.get(2).cloned()), v["toml"].as_str().unwrap_or(""))
        }
        "import" => {
            let f = pos(args, 1, "file")?;
            let mut text = String::new();
            std::fs::File::open(&f)
                .and_then(|h| h.take(sm_profile::MAX_PROFILE_BYTES as u64 + 1).read_to_string(&mut text))
                .map_err(|e| format!("{f}: {e}"))?;
            let v = agent_call("profiles.import", json!({"text": text, "apply": apply, "overwrite": overwrite}))?;
            if !ctx.json {
                let p = &v["preview"];
                println!("profile      {} ({})", p["display_name"].as_str().unwrap_or(""), p["id"].as_str().unwrap_or(""));
                println!("bindings     {}", p["bindings"]);
                println!("overwrites   {}", p["overwrites_existing"]);
                for w in p["warnings"].as_array().into_iter().flatten() {
                    println!("warning      {}", w.as_str().unwrap_or(""));
                }
                println!("{}", if v["applied"] == true { "imported." } else { "dry run – re-run with --apply to import." });
                return Ok(Value::Null);
            }
            Ok(v)
        }
        "delete" => agent_call("profiles.delete", json!({"profile": pos(args, 1, "profile id")?})),
        "backup" => {
            let v = agent_call("profiles.backup", json!({}))?;
            write_or_print(output.or_else(|| args.get(1).cloned()), v["toml"].as_str().unwrap_or(""))
        }
        "reset-presets" => agent_call("profiles.reset_presets", json!({})),
        _ => Err(format!("unknown profile subcommand {sub}")),
    }
}

fn settings_cmd(args: &[String]) -> R {
    match args.first().map(String::as_str) {
        Some("get") | None => agent_call("settings.get", json!({})),
        Some("set") => {
            let k = pos(args, 1, "key")?;
            let raw = pos(args, 2, "value")?;
            let v: Value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
            agent_call("settings.set", json!({"settings": {k: v}}))
        }
        Some(s) => Err(format!("unknown settings subcommand {s}")),
    }
}

// ------------------------------------------------------------- calibrate

fn calibrate(ctx: &Ctx, args: &mut Vec<String>) -> R {
    let secs = duration_secs(args, 3);
    let apply = flag(args, "--apply");
    let mut b = broker()?;
    b.call("subscribe", json!({"motion": true, "max_hz": 1000}))?;
    b.stream().set_read_timeout(Some(Duration::from_millis(200))).ok();
    if !ctx.json {
        eprintln!("hands off the device: measuring resting offset for {secs}s…");
    }
    let start = Instant::now();
    let mut sum = [0i64; 6];
    let mut n = 0i64;
    let mut peak = [0i64; 6];
    while start.elapsed().as_secs() < secs {
        let Ok(m) = b.recv() else { continue };
        if m["event"] != "motion" {
            continue;
        }
        if let Some(r) = m["raw"].as_array() {
            for i in 0..6 {
                let v = r.get(i).and_then(|v| v.as_i64()).unwrap_or(0);
                sum[i] += v;
                peak[i] = peak[i].max(v.abs());
            }
            n += 1;
        }
    }
    // the broker only reports changes; an idle device that sends nothing is centred
    let offset: Vec<i64> = sum.iter().map(|s| if n > 0 { (*s as f64 / n as f64).round() as i64 } else { 0 }).collect();
    let noisy = peak.iter().any(|p| *p > 60);
    let res = json!({"samples": n, "offset_canonical": offset, "peak": peak, "touched": noisy});
    if noisy {
        return Err(format!("device moved during calibration (peak {peak:?}); keep hands off and retry"));
    }
    if apply {
        let mut a = agent()?;
        let st = a.call("status", json!({}))?;
        let id = st["profile"]["id"].as_str().ok_or("no active profile")?.to_string();
        let mut p: sm_profile::Profile = serde_json::from_value(a.call("profiles.get", json!({"profile": id}))?).map_err(|e| e.to_string())?;
        // center_offset is in raw canonical units, the same frame the
        // broker reports and feeds to the pipeline.
        let o = &offset;
        p.navigation.center_offset = [o[0] as i32, o[1] as i32, o[2] as i32, o[3] as i32, o[4] as i32, o[5] as i32];
        a.call("profiles.save", json!({"profile": p}))?;
        if !ctx.json {
            println!("saved centre offset {:?} to profile {id}", p.navigation.center_offset);
        }
    } else if !ctx.json {
        println!("{n} samples; resting offset (canonical) {offset:?}; re-run with --apply to store it in the active profile");
    }
    Ok(if ctx.json { res } else { Value::Null })
}

// ------------------------------------------------------------- diagnostics

fn os_release() -> Value {
    let t = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let get = |k: &str| t.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim_matches('"').to_string());
    json!({"id": get("ID"), "version_id": get("VERSION_ID"), "pretty": get("PRETTY_NAME"), "kernel": std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim()})
}

fn process_running(name: &str) -> bool {
    std::fs::read_dir("/proc")
        .map(|rd| rd.flatten().any(|e| std::fs::read_to_string(e.path().join("comm")).map(|c| c.trim() == name).unwrap_or(false)))
        .unwrap_or(false)
}

fn systemctl(user: bool, unit: &str) -> String {
    let mut c = std::process::Command::new("systemctl");
    if user {
        c.arg("--user");
    }
    c.args(["is-active", unit]);
    c.output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_else(|_| "unknown (no systemctl)".into())
}

const UDEV_RULE_PATHS: &[&str] =
    &["/usr/lib/udev/rules.d/70-3dmouse-linux.rules", "/lib/udev/rules.d/70-3dmouse-linux.rules", "/etc/udev/rules.d/70-3dmouse-linux.rules"];

struct Check {
    name: &'static str,
    ok: Option<bool>, // None = informational
    detail: String,
    fix: String,
}

fn checks() -> Vec<Check> {
    let mut out = Vec::new();
    let reg = Registry::embedded();
    let (cands, notes) = devices::scan(reg, Path::new("/sys"), Path::new("/dev"));
    out.push(Check {
        name: "device detected",
        ok: Some(!cands.is_empty()),
        detail: cands.iter().map(|c| format!("{:04x}:{:04x} {}", c.vid, c.pid, c.name)).collect::<Vec<_>>().join("; "),
        fix: "connect the device (USB cable or receiver); check `dmesg | tail`".into(),
    });
    for n in notes {
        out.push(Check { name: "device note", ok: None, detail: n, fix: String::new() });
    }
    for c in &cands {
        if let Match::Known(m) = reg.lookup(c.vid, c.pid) {
            out.push(Check {
                name: "support status",
                ok: None,
                detail: format!(
                    "{}: {} {}",
                    m.name,
                    m.status.as_str(),
                    if m.evidence.is_empty() { "(no hardware evidence recorded)".to_string() } else { m.evidence.clone() }
                ),
                fix: String::new(),
            });
        } else if c.generic {
            out.push(Check {
                name: "support status",
                ok: None,
                detail: format!("{:04x}:{:04x} is not in the registry; using descriptor-driven generic support", c.vid, c.pid),
                fix: "please report this device (3dmousectl support-bundle)".into(),
            });
        }
        if let Some(h) = &c.hidraw {
            let r = std::fs::File::open(h).is_ok();
            out.push(Check {
                name: "hidraw access (for apps using hidapi)",
                ok: None,
                detail: format!("{}: {}", h.display(), if r { "readable by you" } else { "not readable by you (broker reads it as a system service)" }),
                fix: String::new(),
            });
        }
    }
    let rule = UDEV_RULE_PATHS.iter().find(|p| Path::new(p).exists());
    out.push(Check { name: "udev rules installed", ok: Some(rule.is_some()), detail: rule.map(|s| s.to_string()).unwrap_or_default(), fix: "install the package, or: sudo install -m644 packaging/udev/70-3dmouse-linux.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger".into() });
    let spnavd = process_running("spacenavd");
    out.push(Check {
        name: "spacenavd",
        ok: None,
        detail: if spnavd { "running – 3dmoused will use integrated-spacenav mode (spacenavd keeps the device)".into() } else { "not running".into() },
        fix: if spnavd { "to let 3dmoused own the device: sudo systemctl disable --now spacenavd".into() } else { String::new() },
    });
    let b = broker().and_then(|mut c| c.call("status", json!({})));
    match &b {
        Ok(s) => {
            out.push(Check {
                name: "broker",
                ok: Some(true),
                detail: format!(
                    "{} mode {} ({})",
                    s["version"].as_str().unwrap_or(""),
                    s["mode"].as_str().unwrap_or(""),
                    s["mode_reason"].as_str().unwrap_or("")
                ),
                fix: String::new(),
            });
            out.push(Check {
                name: "libspnav socket",
                ok: Some(s["spnav_socket"].is_string() || s["mode"] == "integrated-spacenav"),
                detail: s["spnav_socket"].as_str().unwrap_or("not served").to_string(),
                fix: "another process holds /run/spnav.sock? see `3dmousectl status`".into(),
            });
            out.push(Check {
                name: "desktop input injection",
                ok: Some(s["injection_available"] == true),
                detail: s["injection_reason"].as_str().unwrap_or("").to_string(),
                fix: "the broker needs /dev/uinput (modprobe uinput; the systemd unit loads it)".into(),
            });
            for d in s["devices"].as_array().into_iter().flatten() {
                out.push(Check {
                    name: "device stream",
                    ok: Some(d["state"] == "ready"),
                    detail: format!(
                        "{} {} via {} – {} frames, {} malformed, {} watchdog resets",
                        d["usb_id"].as_str().unwrap_or(""),
                        d["state"].as_str().unwrap_or(""),
                        d["input_path"].as_str().unwrap_or(""),
                        d["frames"],
                        d["malformed"],
                        d["watchdog_resets"]
                    ),
                    fix: "replug the device; see journalctl -u 3dmoused".into(),
                });
            }
        }
        Err(e) => out.push(Check { name: "broker", ok: Some(false), detail: e.clone(), fix: "sudo systemctl enable --now 3dmoused".into() }),
    }
    match agent_call("status", json!({})) {
        Ok(s) => {
            let a = &s["agent"];
            out.push(Check {
                name: "session agent",
                ok: Some(true),
                detail: format!("profile {} via {}", s["profile"]["id"].as_str().unwrap_or(""), s["profile"]["selected_by"].as_str().unwrap_or("")),
                fix: String::new(),
            });
            out.push(Check {
                name: "agent may control broker",
                ok: Some(a["can_control"] == true),
                detail: String::new(),
                fix: "only the user of the active seat0 session may change settings".into(),
            });
            out.push(Check {
                name: "focus tracking",
                ok: Some(a["capabilities"]["focus_tracking"] == true),
                detail: format!("{} – {}", a["desktop"]["provider"].as_str().unwrap_or(""), a["desktop"]["provider_note"].as_str().unwrap_or("")),
                fix: "per-application switching needs a supported compositor (see docs/compatibility/desktop-matrix.md)".into(),
            });
            out.push(Check {
                name: "notifications (OSD)",
                ok: None,
                detail: if a["capabilities"]["osd_overlay"] == true {
                    "on-screen overlay (3dmouse-osd) connected".into()
                } else if a["capabilities"]["osd"] == true {
                    "desktop notifications (notify-send); no overlay connected".into()
                } else {
                    "neither overlay nor notify-send – OSD disabled".into()
                },
                fix: if a["capabilities"]["osd_overlay"] == true {
                    String::new()
                } else {
                    "systemctl --user enable --now 3dmouse-osd (not available on GNOME Wayland)".into()
                },
            });
        }
        Err(e) => out.push(Check { name: "session agent", ok: Some(false), detail: e, fix: "systemctl --user enable --now 3dmouse-agent".into() }),
    }
    let sess = std::env::var("XDG_SESSION_TYPE").unwrap_or_default();
    out.push(Check {
        name: "session",
        ok: None,
        detail: format!("{} {} ", sess, std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()),
        fix: String::new(),
    });
    out
}

fn diagnostics(ctx: &Ctx, args: &mut Vec<String>) -> R {
    let sub = args.first().cloned().unwrap_or("check".into());
    match sub.as_str() {
        "check" => {
            let cs = checks();
            let failed = cs.iter().filter(|c| c.ok == Some(false)).count();
            if ctx.json {
                return Ok(
                    json!({"failed": failed, "checks": cs.iter().map(|c| json!({"name": c.name, "ok": c.ok, "detail": c.detail, "fix": c.fix})).collect::<Vec<_>>()}),
                );
            }
            for c in &cs {
                let mark = match c.ok {
                    Some(true) => "OK  ",
                    Some(false) => "FAIL",
                    None => "info",
                };
                println!("[{mark}] {:<34} {}", c.name, c.detail);
                if c.ok == Some(false) && !c.fix.is_empty() {
                    println!("       fix: {}", c.fix);
                } else if c.ok.is_none() && !c.fix.is_empty() {
                    println!("       hint: {}", c.fix);
                }
            }
            if failed > 0 {
                std::process::exit(1);
            }
            Ok(Value::Null)
        }
        "record" => {
            args.remove(0);
            flag(args, "--redact"); // captures never contain serials or text
            let secs = duration_secs(args, 15);
            let out = output_opt(args);
            record(secs, out, ctx)
        }
        "replay" => replay(ctx, &pos(args, 1, "capture file")?),
        _ => Err(format!("unknown diagnostics subcommand {sub}")),
    }
}

fn record(secs: u64, out: Option<String>, ctx: &Ctx) -> R {
    let (cands, _) = devices::scan(Registry::embedded(), Path::new("/sys"), Path::new("/dev"));
    let c = cands.first().ok_or("no 6-DOF device found")?;
    let ev = c.evdev.as_ref().ok_or("device has no evdev node")?;
    let mut f = std::fs::File::open(ev).map_err(|e| format!("{}: {e} (you need read access: member of group `input`, or run with sudo)", ev.display()))?;
    let out = out.unwrap_or_else(|| format!("{:04x}-{:04x}-capture.txt", c.vid, c.pid));
    use std::os::fd::AsRawFd;
    let fd = f.as_raw_fd();
    let mut lines = vec![
        format!("# 3dmousectl {VERSION} evdev capture"),
        format!("# device {:04x}:{:04x} {} via {}", c.vid, c.pid, c.name, ev.display()),
        format!("# kernel {}", os_release()["kernel"].as_str().unwrap_or("")),
        "# format: <sec.usec since first event> <type> <code> <value>".into(),
    ];
    if !ctx.json {
        eprintln!("recording {} for {secs}s – move the device and press buttons…", ev.display());
    }
    let start = Instant::now();
    let mut t0: Option<f64> = None;
    let mut n = 0;
    let mut buf = [0u8; 24 * 64];
    while start.elapsed().as_secs() < secs {
        let mut p = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut p, 1, 200) } <= 0 {
            continue;
        }
        let k = f.read(&mut buf).map_err(|e| e.to_string())?;
        for ch in buf[..k].as_chunks::<24>().0 {
            let sec = i64::from_ne_bytes(ch[0..8].try_into().unwrap()) as f64;
            let usec = i64::from_ne_bytes(ch[8..16].try_into().unwrap()) as f64;
            let typ = u16::from_ne_bytes(ch[16..18].try_into().unwrap());
            let code = u16::from_ne_bytes(ch[18..20].try_into().unwrap());
            let val = i32::from_ne_bytes(ch[20..24].try_into().unwrap());
            let t = sec + usec / 1e6;
            let base = *t0.get_or_insert(t);
            lines.push(format!("{:.6} {typ} {code} {val}", t - base));
            n += 1;
        }
    }
    std::fs::write(&out, lines.join("\n") + "\n").map_err(|e| format!("{out}: {e}"))?;
    if !ctx.json {
        println!("wrote {n} events to {out} (contains no serial numbers)");
    }
    Ok(json!({"events": n, "file": out}))
}

fn replay(ctx: &Ctx, file: &str) -> R {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let evs = sm_hid::parse_evdev_fixture(&text);
    // header "# device VID:PID" selects the registry entry; default Compact
    let usb = text.lines().find_map(|l| l.strip_prefix("# device ")).and_then(|r| r.split_whitespace().next()).unwrap_or("256f:c635").to_string();
    let (vid, pid) =
        usb.split_once(':').and_then(|(a, b)| Some((u16::from_str_radix(a, 16).ok()?, u16::from_str_radix(b, 16).ok()?))).unwrap_or((0x256f, 0xc635));
    let reg = Registry::embedded();
    let (buttons, canon, range) = match reg.lookup(vid, pid) {
        Match::Known(m) => (m.buttons.iter().map(|b| (b.code, b.index)).collect(), m.canonical, m.axis_range.unwrap_or(350)),
        _ => (vec![(256, 0), (257, 1)], sm_registry::Canonical::SwapYzInvYz, 350),
    };
    // 3Dconnexion USB layout: report 1 = translation, report 2 = rotation
    let mut asm = sm_hid::EvdevAssembler::new([1, 1, 1, 2, 2, 2], buttons, sm_hid::DEFAULT_EVDEV_WATCHDOG_US);
    let mut st = sm_core::InputState::default();
    let mut pipe = sm_pipeline::Pipeline::new(sm_pipeline::NavConfig::default(), range);
    let mut frames = 0;
    let mut presses = 0;
    let mut peak = [0i32; 6];
    let mut last_t = 0;
    let mut handle = |f: sm_core::RawFrame, st: &mut sm_core::InputState, pipe: &mut sm_pipeline::Pipeline| {
        let (_, edges) = st.apply(&f);
        presses += (edges & st.buttons).count_ones();
        let c = canon.apply(st.axes);
        for i in 0..6 {
            if c[i].abs() > peak[i].abs() {
                peak[i] = c[i];
            }
        }
        pipe.process(c, f.t_us);
    };
    for e in evs {
        while let Some(d) = asm.next_deadline_us(last_t) {
            if last_t + d > e.t_us {
                break;
            }
            match asm.tick(last_t + d) {
                Some(f) => handle(f, &mut st, &mut pipe),
                None => break,
            }
        }
        last_t = e.t_us;
        if let Some(f) = asm.push(e) {
            frames += 1;
            handle(f, &mut st, &mut pipe);
        }
    }
    let end = last_t + 1_000_000;
    while let Some(d) = asm.next_deadline_us(last_t) {
        last_t += d.max(1);
        if last_t > end {
            break;
        }
        if let Some(f) = asm.tick(last_t) {
            handle(f, &mut st, &mut pipe);
        }
    }
    let neutral = st.is_neutral();
    let v = json!({"file": file, "device": usb, "frames": frames, "button_presses": presses, "peak_canonical": peak, "watchdog_resets": asm.watchdog_resets, "dropped_syncs": asm.dropped_syncs, "ends_neutral": neutral});
    if !ctx.json {
        println!("{file}: {frames} frames, {presses} button presses, peak (canonical) {peak:?}");
        println!(
            "watchdog resets {}, SYN_DROPPED {}, ends neutral: {}",
            asm.watchdog_resets,
            asm.dropped_syncs,
            if neutral { "yes" } else { "NO – stuck axis!" }
        );
        return Ok(Value::Null);
    }
    Ok(v)
}

fn support_bundle(args: &mut Vec<String>) -> R {
    flag(args, "--redact"); // always redacted
    let out = output_opt(args).unwrap_or_else(|| "3dmouse-support.json".into());
    let (cands, notes) = devices::scan(Registry::embedded(), Path::new("/sys"), Path::new("/dev"));
    let journal = |user: bool, unit: &str| {
        let mut c = std::process::Command::new("journalctl");
        if user {
            c.arg("--user");
        }
        c.args(["-u", unit, "-n", "200", "--no-pager", "-o", "short-monotonic"]);
        c.output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    let scrub = |s: String| {
        let mut s = s;
        if !home.is_empty() {
            s = s.replace(&home, "~");
        }
        if user.len() > 2 {
            s = s.replace(&user, "<user>");
        }
        s
    };
    let b = json!({
        "generated_by": format!("3dmousectl {VERSION}"),
        "redacted": "serial numbers hashed; window titles never collected; home path and user name replaced",
        "os": os_release(),
        "session": {"type": std::env::var("XDG_SESSION_TYPE").ok(), "desktop": std::env::var("XDG_CURRENT_DESKTOP").ok()},
        "devices": cands.iter().map(cand_json).collect::<Vec<_>>(),
        "device_descriptors": cands.iter().map(|c| json!({"usb_id": format!("{:04x}:{:04x}", c.vid, c.pid), "hex": c.descriptor.iter().map(|b| format!("{b:02x}")).collect::<String>()})).collect::<Vec<_>>(),
        "scan_notes": notes,
        "checks": checks().iter().map(|c| json!({"name": c.name, "ok": c.ok, "detail": c.detail})).collect::<Vec<_>>(),
        "broker_status": broker().and_then(|mut c| c.call("status", json!({}))).ok(),
        "agent_status": agent_call("status", json!({})).ok(),
        "udev_rule": UDEV_RULE_PATHS.iter().find(|p| Path::new(p).exists()),
        "spacenavd_running": process_running("spacenavd"),
        "spnavrc_present": Path::new("/etc/spnavrc").exists(),
        "journal_broker": journal(false, "3dmoused"),
        "journal_agent": journal(true, "3dmouse-agent"),
    });
    let text = scrub(serde_json::to_string_pretty(&b).unwrap());
    std::fs::write(&out, text).map_err(|e| format!("{out}: {e}"))?;
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o600)).ok();
    Ok(json!({"written": out, "note": "review the file before sharing it"}))
}

fn service_health(ctx: &Ctx) -> R {
    let v = json!({
        "3dmoused.service": systemctl(false, "3dmoused.service"),
        "3dmouse-agent.service (user)": systemctl(true, "3dmouse-agent.service"),
        "broker_socket": {"path": sm_ipc::broker_socket().display().to_string(), "reachable": broker().is_ok()},
        "agent_socket": {"path": sm_ipc::agent_socket().display().to_string(), "reachable": agent().is_ok()},
        "spnav_socket": {"path": "/run/spnav.sock", "present": Path::new("/run/spnav.sock").exists()},
        "spacenavd_running": process_running("spacenavd"),
    });
    if !ctx.json {
        for (k, val) in v.as_object().unwrap() {
            println!("{k:<34} {}", if val.is_string() { val.as_str().unwrap().to_string() } else { val.to_string() });
        }
        return Ok(Value::Null);
    }
    Ok(v)
}

fn run(ctx: &Ctx, mut args: Vec<String>) -> R {
    if args.is_empty() {
        return status(ctx);
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "-h" | "--help" | "help" => {
            println!("{HELP}");
            Ok(Value::Null)
        }
        "version" | "--version" => {
            let b = broker().and_then(|mut c| c.call("hello", json!({}))).ok();
            let a = agent_call("hello", json!({})).ok();
            if !ctx.json {
                println!("3dmousectl {VERSION}");
                println!("3dmoused   {}", b.as_ref().and_then(|b| b["version"].as_str()).unwrap_or("not running"));
                println!("agent         {}", a.as_ref().and_then(|a| a["version"].as_str()).unwrap_or("not running"));
                return Ok(Value::Null);
            }
            Ok(json!({"cli": VERSION, "broker": b, "agent": a}))
        }
        "status" => status(ctx),
        "devices" => devices_cmd(ctx, &args),
        "monitor" => monitor(ctx, &mut args),
        "profile" | "profiles" => profile_cmd(ctx, &mut args),
        "settings" => settings_cmd(&args),
        "calibrate" => calibrate(ctx, &mut args),
        "test-action" => {
            let a: Value = serde_json::from_str(&pos(&args, 0, "action JSON")?).map_err(|e| e.to_string())?;
            let mut req = json!({"action": a});
            if let Some(i) = args.iter().position(|x| x == "--phase") {
                req["phase"] = json!(args.get(i + 1).ok_or("--phase needs press, release or pulse")?);
            }
            agent_call("test_action", req)
        }
        "diagnostics" | "diag" => diagnostics(ctx, &mut args),
        "support-bundle" => support_bundle(&mut args),
        "service" => match args.first().map(String::as_str) {
            Some("health") | None => service_health(ctx),
            Some(s) => Err(format!("unknown service subcommand {s}")),
        },
        _ => Err(format!("unknown command `{cmd}`; see 3dmousectl --help")),
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let ctx = Ctx { json: flag(&mut args, "--json") };
    match run(&ctx, args) {
        Ok(Value::Null) => {}
        Ok(v) => {
            if ctx.json {
                println!("{v}");
            } else if v.is_object() || v.is_array() {
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
            } else {
                println!("{v}");
            }
        }
        Err(e) => {
            if ctx.json {
                println!("{}", json!({"error": e}));
            } else {
                eprintln!("3dmousectl: {e}");
            }
            std::process::exit(1);
        }
    }
}
