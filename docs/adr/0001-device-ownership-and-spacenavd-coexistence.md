# ADR-0001: Device ownership and spacenavd coexistence

- Status: accepted (2026-11-10)
- Requirements: FR-107, FR-501, FR-803, SRS §0 ("never silently disable spacenavd")

## Context
- spacenavd (FreeSpacenav, checked at commit `05cc679`, 2026-11-10) is the incumbent Linux driver.
- Native applications such as Blender, FreeCAD and OpenSCAD use libspnav (commit `476a17b`).
  libspnav connects to the AF_UNIX socket `/var/run/spnav.sock` (= `/run/spnav.sock`), or falls
  back to the X11 ClientMessage protocol.
- spacenavd opens the evdev node and grabs it (EVIOCGRAB). Two drivers reading or grabbing the same
  device would produce duplicated or missing motion.
- Users may already have a tuned spacenavd setup. The SRS forbids removing or disabling it silently.

## Decision
`3dmoused` has four ownership modes, chosen at start-up (`--mode`, default `auto`):

| Mode | When | Behaviour |
|---|---|---|
| `native-owner` | no other server owns `/run/spnav.sock` | 3dmoused opens the devices (hidraw preferred) and serves `/run/spnav.sock` itself, speaking the libspnav v1 protocol. |
| `integrated-spacenav` | spacenavd is running, or takes the socket later | 3dmoused does **not** open the devices. It connects to spacenavd as a libspnav client, forwards motion and buttons to the agent (profiles, button actions, desktop emulation), and leaves native apps on spacenavd. |
| `passive-monitor` | explicit only (`--mode passive-monitor`) | Diagnostics: serves the control socket for status and monitoring, but serves no libspnav socket and opens no uinput device, so it can't affect apps or the desktop. |
| `auto` | default | Probes the socket. A live peer selects integrated mode. A stale socket file (nobody listening) is reclaimed. |

Further rules:
- The socket's inode is checked every second. If another server replaces it, 3dmoused releases
  its devices and switches to integrated mode. It never fights for the socket.
- The unit has `After=spacenavd.service` but no `Conflicts=`. Package scripts never stop, mask or
  remove spacenavd. `diagnostics check` reports which mode is active and why.
- Moving from spacenavd to 3dmoused is a documented manual choice
  (`docs/install/troubleshooting.md`, "Migrating from spacenavd").

## Alternatives rejected
- *`Conflicts=spacenavd.service`*: silently stops the user's driver on install, which violates the SRS.
- *Both drivers reading the device without grabbing*: native apps would receive duplicate motion.
- *A libspnav fork or LD_PRELOAD*: invasive, and it breaks Flatpak and statically built apps.

## Consequences
- Integrated mode works only as well as spacenavd's configuration. The pipeline and axis inversion
  of 3dmouse-linux apply only to desktop emulation there, not to native apps.
- The e2e test `auto_mode_yields_to_existing_spnav_server` covers the yield path. The
  `stale socket reclaimed` test covers crash recovery.
