# 3DxWare (Windows) feature parity matrix

This is the target feature list (SRS §0), compared with 3dmouse-linux 0.1.0 as of 2026-11-10.
The 3DxWare features are described from public product behaviour only. No vendor code, assets or
documents are reproduced.

| 3DxWare feature | 3dmouse-linux 0.1.0 | Notes |
|---|---|---|
| 6-DOF navigation in native apps | **Yes** | libspnav socket. Physically tested with Blender and OpenSCAD. |
| Speed: overall / pan-zoom / rotation | **Yes** | Navigation page, `navigation.master_gain` etc. |
| Per-axis invert, remap, enable, speed, dead zone | **Yes** | Per profile. Physically confirmed in Blender. |
| Response curves | **Yes** | linear, power, S-curve |
| Dominant axis, pan-zoom-only / rotation-only | **Yes** | |
| Smoothing | **Yes** | time constant 0–300 ms |
| Calibrate / re-centre | **Yes** | software offset only (`3dmousectl calibrate`). Nothing is written to the device. |
| Per-application profiles, auto-switch | **Yes** on Hyprland, Sway and X11. GNOME needs the extension. **No** on KDE and COSMIC. | `../compatibility/desktop-matrix.md` |
| Button → key chord / mouse / scroll | **Yes** | uinput. Alt+SysRq is refused. |
| Tap / hold / double press | **Yes** | button engine with explicit timings |
| Macros (sequences with delays) | **Yes** | engine and profile schema. The GUI editor is basic. |
| QuickView keys (top/front/…, fit) | **Yes, per app** | `View` actions map to each app's own shortcuts in the preset |
| Launch app / run command | **Yes** (opt-in) | Exec needs `allow_exec` plus per-action confirmation, and runs without a shell |
| On-screen button help | **Partial** | desktop notification, not an overlay |
| Radial menus | **No** | not implemented |
| First-run wizard | **No** | not implemented. The Overview page shows status and next steps. |
| Import / export / backup profiles | **Yes** | TOML/JSON, with import preview |
| Battery level, wireless pairing help | **No** | no tested wireless hardware |
| Enterprise LCD, LED control | **No** | no verified output formats (blocked by FR-605) |
| Multiple devices at once | **Partial** | each device has its own decoder, watchdog and pipeline instance, but all of them share the active profile. Profiles can target a device (`device_selector`), but two devices can't run different profiles at the same time (FR-105). |
| Browser CAD (Onshape) | **No** | the bridge is not built (FR-409) |
| Firmware update | **Out of scope** (FR-607) | |
