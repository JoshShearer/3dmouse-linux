# Landscape: existing Linux 6-DOF software

Researched 2026-11-10. Commit hashes are the upstream `HEAD` on that date, checked with
`git ls-remote`. Licences come from the GitHub API and the LICENSE files.

| Project | URL | Commit / activity | Licence | What it does | Relevance |
|---|---|---|---|---|---|
| **spacenavd** | https://github.com/FreeSpacenav/spacenavd | `05cc679` (last push 2026-10-24) | GPL-3.0 | The de-facto Linux driver. Reads evdev (grabs it), serves the libspnav AF_UNIX socket `/var/run/spnav.sock` and the X11 ClientMessage protocol. Has a per-device database (`usbdb`) and button remap hacks (`bnhack_smpro`, `bnhack_sment`), plus a config file `/etc/spnavrc`. | The incumbent. Its socket protocol is what native apps speak. Source of the device ids and button counts in our registry, which are marked `unknown` until tested. We coexist with it and never disable it (ADR-0001). |
| **libspnav** | https://github.com/FreeSpacenav/libspnav | `476a17b` (2026-09-25) | BSD-style (permissive), © John Tsiombikas | Client library: `spnav_open()` (AF_UNIX), `spnav_x11_open()`, and the v1 protocol with device queries. | Blender, FreeCAD, OpenSCAD and KiCad link it. We implement the **server** side of its wire protocol in `sm-spnav`, written from the protocol description (no code copied), and test it against the real libspnav 1.2 (`tests/integration/c/spnav_probe.c`). |
| **spnavcfg** | https://github.com/FreeSpacenav/spnavcfg | `84543be` (2026-04-11) | GPL-3.0 | Qt GUI for spacenavd settings (sensitivity, invert, deadzone, button map). | The feature baseline for a "simple" config UI. It has no per-application profiles. |
| **SpaceUX** | https://github.com/Maik-0000FF/SpaceUX | last push 2026-11-03 | GPL-3.0 | TypeScript radial command menu for Linux driven by the SpaceMouse. | Shows a demand for radial menus (FR-304/FR-700 view 6). We have not implemented radial menus. |
| **SpaceMouse_3dconnexion** (earlier prototype) | https://github.com/Maik-0000FF/SpaceMouse_3dconnexion | last push 2026-11-03 | GPL-3.0 | Python daemon adding desktop scroll, zoom and workspace switching on top of spacenavd. | Shows that desktop emulation is wanted. We implement scroll and zoom emulation natively in the agent. |
| 3Dconnexion 3DxWare (Windows/macOS) | vendor site | — | proprietary | Reference feature set: per-app profiles, radial menus, QuickView keys, LCD, battery. | Feature target only (`windows-feature-matrix.md`). No vendor code or assets are used. A legacy vendor Linux driver (3DxWare for Linux) existed historically. It was not evaluated, and we don't rely on it. |
| FreeCAD 3Dconnexion docs | https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/3Dconnexion_input_devices.md | — | CC-BY | FreeCAD on Linux uses libspnav through spacenavd. | Confirms that serving the libspnav socket is enough for FreeCAD (not physically tested yet). |

## Gaps in the existing tools that this project fills
1. **Per-application profiles** chosen automatically from the focused window, which spacenavd lacks.
2. **Button actions beyond app buttons:** key chords, mouse buttons, macros, profile switching.
3. **Desktop emulation:** scrolling and zooming for apps without 6-DOF support.
4. **A device registry with honest per-model support status**, and hidraw decoding for buttons
   that evdev drops.
5. **A modern Qt6 GUI** with a live device view, diagnostics and a support bundle.

## Licence implications
- This repository is GPL-3.0 (`LICENSE`, text taken from spacenavd's COPYING).
- Reading spacenavd's device tables as *facts* (vid/pid, button counts) and re-entering them as
  data is compatible with that.
- libspnav's licence is permissive. Apps keep linking the system libspnav, and we don't ship it.
