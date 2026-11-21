# Architecture overview

```
 3dmouse-config (Qt6/QML)     3dmousectl (CLI)      GNOME Shell extension
            │  JSON-lines, $XDG_RUNTIME_DIR/3dmouse/agent.sock (0600, same uid)
            ▼
 3dmouse-agent  (systemd --user, unprivileged)
   focus provider (Hyprland / Sway / X11 / GNOME ext) → profile selection
   button engine (tap/hold/double, macros) · desktop emulation (scroll)
   lock detection · OSD (notify-send) · launch/exec (opt-in)
            │  JSON-lines, /run/3dmouse/broker.sock (0666, SO_PEERCRED auth)
            ▼
 3dmoused  (system service, root, zero capabilities, sandboxed)
   sysfs discovery · hidraw decoder / evdev assembler · watchdogs
   axis pipeline (one per device) · libspnav server /run/spnav.sock
   uinput virtual keyboard/mouse · ownership negotiation
            │
   /dev/hidraw*  /dev/input/event*  /dev/uinput
```

| Process | Crate | Why separate |
|---|---|---|
| `3dmoused` | `sm-daemon` | The only component that touches device nodes and uinput. Runs once per machine; serves every user's native apps. |
| `3dmouse-agent` | `sm-agent` | Needs the user's session (focus, lock state, desktop launching) and owns the user's profiles in `~/.config`. Never privileged. |
| `3dmouse-config` | `gui/` | Optional; talks only to the agent. Process boundary instead of Rust↔Qt bindings (ADR-0006). |
| `3dmousectl` | `sm-cli` | Scriptable access; works without the agent for read-only diagnostics. |

Library crates: `sm-core` (shared types, input state), `sm-registry`
(device data, `data/devices.toml`), `sm-hid` (descriptor parser, decoders),
`sm-pipeline` (axis math), `sm-actions` (actions, key table, button engine),
`sm-profile` (schema, presets, storage, selection), `sm-spnav` (libspnav wire
protocol server and client), `sm-desktop` (uinput, focus providers, lock
detection), `sm-ipc` (control protocol, peer credentials, status types).

## Event flow

1. A device report arrives (hidraw preferred, evdev fallback).
2. The broker updates `InputState`, converts to the canonical frame and runs the pipeline.
3. Motion → libspnav clients (unless the active profile's `app_mode` is
   `desktop-emulation` or `none`) and → subscribed control clients (agent, rate-limited).
4. Button edges → libspnav clients for buttons the active profile maps to
   "application button"; every edge → the agent, whose button engine runs the
   other actions and asks the broker to `inject` keys/mouse/scroll.
5. The agent selects a profile from the focused application and pushes
   `set_config` (navigation, app mode, spnav button map) to the broker.

## Failure behaviour

| Event | Result |
|---|---|
| Device unplugged mid-motion / button held | broker sends zero motion and button releases before dropping the device |
| Agent crashes | broker releases keys that connection pressed, reverts to built-in defaults |
| Broker restarts | agent reconnects every 2 s and re-pushes its config; engine state is cancelled |
| Session locks | agent cancels pending actions and tells the broker; broker refuses `inject` |
| Active seat user changes | broker reverts config owned by the previous user, releases their keys |
| spacenavd starts later and takes `/run/spnav.sock` | broker detects the inode change and switches to integrated-spacenav (it releases the devices) |
