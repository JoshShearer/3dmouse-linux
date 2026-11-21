# ADR-0011: Web CAD (Onshape) browser bridge

- Status: accepted (2026-11-10). Evaluation complete; implementation **deferred**, pending the
  maintainer's authorisation.
- Requirements: FR-409, SRS §3 (threat model), FR-305

## Context
FR-409 asks us to *evaluate* an opt-in browser extension plus a localhost bridge for web CAD, with
a user gesture or permission, origin validation, CSRF protection, message schemas, and no unbounded
localhost command interface.

Today, Onshape in the browser:
- can get **desktop emulation** from us (scroll and zoom through uinput, once its preset is switched to it), which works but isn't 6-DOF;
- on Windows and macOS talks to the vendor's own driver through a vendor-specific local web
  service. That protocol is undocumented, and we won't reverse-engineer or impersonate it (SRS §1:
  no copying of proprietary interfaces).

## Options
| Option | 6-DOF in the page | Risk | Notes |
|---|---|---|---|
| A. Keep desktop emulation (status quo) | no | none added | the `browser-onshape` preset (browser with "Onshape" in the title) ships with mode *none*; switch it to desktop emulation to scroll and zoom |
| B. WebHID in the page | yes, if the site supports it | none on our side | the browser asks the user per device. Needs the site to implement it, and the device can't also be owned by 3dmoused at the same time (hidraw is shared, but the cap would then drive both). Out of our control. |
| C. Browser extension + localhost WebSocket bridge | yes, through a content-script API the site must adopt | **new network-facing surface** | needs a stable API that sites would have to call; nobody calls it today |
| D. Extension + native messaging host (no socket) | as C | low: no listening port, the browser launches the host and pins the extension id | best shape if C-style integration is ever wanted |

## Decision
- **0.1 ships option A.** No listening port, no extension.
- If a web CAD vendor or a user community wants native 6-DOF, implement **option D**, not C:
  - a native-messaging host (`3dmouse-web-host`) declared for one extension id;
  - the extension injects nothing until the user enables it per origin (an allowlist, starting
    with `https://cad.onshape.com`), through a toolbar click (user gesture);
  - host → page messages are motion and buttons only, rate-limited to 60 Hz. Page → host messages
    have one fixed JSON schema (`subscribe`, `unsubscribe`) and no command execution, profile
    writes or action triggers. That makes the interface bounded by construction;
  - the host connects to the agent socket read-only, which reuses the agent's lock gating: nothing
    is sent while locked.
- Option B needs nothing from us, beyond documenting how to set Onshape to *None* mode so the
  device isn't driven twice.

## Consequences
- FR-409 is **evaluated**. The bridge isn't implemented. The coverage matrix says so.
- No localhost server exists, so the network threat surface stays at zero (`PrivateNetwork=yes`
  for the broker, and the agent has no sockets beyond its 0700 AF_UNIX one).
