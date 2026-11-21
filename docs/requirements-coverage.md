# Requirement coverage, SRS-001 (as of 2026-11-10, version 0.1.0)

Legend:
- **Done**: implemented, with automated tests and/or physical verification (named).
- **Partial**: implemented with listed gaps.
- **Not done**: not implemented.
- **Blocked**: cannot be done honestly without hardware or information we don't have.

Physical verification used one SpaceMouse Compact (`256f:c635`, USB) on Arch/Omarchy with Hyprland
0.56.2 (see [evidence](compatibility/evidence/256f-c635-usb.md)).

## FR-100 Discovery and ownership
| ID | Status | Evidence / gap |
|---|---|---|
| FR-101 | Done | sysfs scan plus inotify on `/dev`, registry match (ADR-0003, ADR-0007). e2e `motion_buttons_watchdog_and_unplug`. Physical. |
| FR-102 | Done | read-only hidraw/evdev, no grab, other HID devices untouched |
| FR-103 | Partial | interfaces without six axes are skipped. Combining multiple collections of one device (receivers) is untested, for lack of hardware. |
| FR-104 | Done | unplug mid-motion or mid-hold sends neutral plus releases (e2e). Physical replug ready in 186 ms. |
| FR-105 | Partial | multiple devices each get their own decoder and pipeline, but share one active profile. Untested with two physical devices. |
| FR-106 | Done for the Compact | status shows connection, path, rate and registry status. Serials are redacted in bundles. The Compact exposes no firmware or battery data. |
| FR-107 | Done | auto, native-owner, integrated-spacenav, passive-monitor modes and inode takeover (ADR-0001). e2e `auto_mode_yields_to_existing_spnav_server`. **Not tested with a real spacenavd running.** |
| FR-108 | Done | generated udev rules: `uaccess` + `LIBINPUT_IGNORE_DEVICE`, no 0666 nodes. Unit `udev_rules_match_registry`. `udevadm verify` clean. |
| FR-109 | Done | no output reports exist in the code at all |

## FR-200 Six-axis pipeline
| ID | Status | Evidence / gap |
|---|---|---|
| FR-201 | Done | descriptor-driven decoder with fixtures. Physical signs verified on all six axes (hidraw and evdev). |
| FR-202 | Done | normalized [-1, 1] plus raw values in monitor and GUI |
| FR-203 | Done | gain, invert, remap, deadzone, enable per axis. Inversion physically confirmed in Blender. Asymmetric deadzone not implemented. |
| FR-204 | Done | linear, power, S-curve, documented in `architecture/axis-pipeline.md` |
| FR-205 | Done | time-constant smoothing, with tests for timestamp jumps |
| FR-206 | Partial | dominant axis, translation-only, rotation-only. No "intelligent view mode". |
| FR-207 | Done | software centre offset only (`calibrate`). Nothing written to the device. |
| FR-208 | Done | watchdog neutral after silence, plus neutral on disconnect (e2e and replay fixtures) |
| FR-209 | Done | `architecture/axis-frames.md`, physically verified frame table |
| FR-210 | Done | one pipeline feeds spnav and desktop emulation |

## FR-300 Buttons and commands
| ID | Status | Evidence / gap |
|---|---|---|
| FR-301 | Partial | the registry button table drives the GUI grid. Only the Compact is verified. No device illustration. |
| FR-302 | Done | app button, key chord, mouse, scroll, view, profile, sensitivity, toggle, launch, exec, macro, help, **radial menu** (hold, push the cap towards a slice, release; 2–8 slices, no nesting; unit-tested slice geometry and validation). Physically: Back/Forward and scroll. Radial overlay rendering verified on screen. Selecting with the cap on the Compact: verified by the tester on Arch, 2026-11-10 (evidence file). |
| FR-303 | Done | tap, hold, double with explicit timings (unit tests) |
| FR-304 | Done | the macro engine has steps and delays. The GUI step editor adds, removes, retypes and reorders steps (keys, key down/up, mouse button, scroll, wait) and shows the step count and total wait against the limits (64 steps, 5 s per wait, 30 s total). Reordering uses up/down buttons rather than drag-and-drop, which keeps it keyboard-accessible. Cancellation: a lock or profile switch clears queued steps and releases held keys. Recursion: a step cannot contain a macro. Conditional predicates (optional in the SRS) are not implemented. |
| FR-305 | Done | exec is off by default, needs per-action confirmation, runs without a shell, and import clears confirmation |
| FR-306 | Done | key-up on agent crash, unplug, profile switch, lock, owner change (e2e owner revert) |
| FR-307 | Partial | View actions map to each app's shortcuts. No user-defined saved views. |
| FR-308 | Done | ShowHelp shows the current bindings in the overlay (`3dmouse-osd`), or as a notification where no overlay is possible |

## FR-400 Application-aware behaviour
| ID | Status | Evidence / gap |
|---|---|---|
| FR-401 | Partial | Hyprland (physical), KDE Plasma (KWin script over D-Bus, tested on headless KWin 6.8.2), COSMIC (toplevel-info protocol, tested on cosmic-comp 1.11.0), Sway, X11, GNOME extension (untested). |
| FR-402 | Done | app id or class (Wayland app id, X11 WM_CLASS, Flatpak and desktop-file ids), **executable** (file name or absolute path, from `/proc/<pid>/exe` of the focused window: Hyprland, Sway, X11, KDE), title substring and **title regex** (opt-in, linear-time engine, length and size limited, validated on save and import, never logged). Document and workspace overrides: the most specific title condition wins (unit-tested, regex property-fuzzed). |
| FR-403 | Done | deterministic precedence (ADR-0010) |
| FR-404 | Done | config is pushed atomically. Keys are released on switch. |
| FR-405 | Partial | presets for all six apps. Blender, OpenSCAD and KiCad physically verified. FreeCAD, PrusaSlicer and desktop verified as listed in `compatibility/application-matrix.md`. |
| FR-406 | Done | TOML/JSON, schema validation, import preview, schema_version |
| FR-407 | Done | duplicate (New from selected), **search** (name, id, mode), reset presets, backup, import and export (sharing as TOML), all local. |
| FR-408 | Done | native-6dof, application-plugin, desktop-emulation, none |
| FR-409 | Done (evaluated) | ADR-0011: options compared. 0.1 keeps emulation. A future bridge must be a native-messaging host (no localhost port), with a per-origin opt-in by user gesture, a fixed two-message schema, and no commands. Implementing it needs the maintainer's authorisation. |

## FR-500 Desktop backends
| ID | Status | Evidence / gap |
|---|---|---|
| FR-501 | Done | libspnav v1 server, tested against real libspnav 1.2 (Arch) and 1.1 (Ubuntu 24.04 container), Blender, OpenSCAD and KiCad |
| FR-502 | Done (X11 session) | X11 focus through xprop, and injection through uinput (works under X11 without XTest). **Physically tested** on Pop!_OS 24.04 GNOME/Xorg with the Compact: per-app profile switching, browser scroll, lock and overlay. The legacy X11 ClientMessage spnav protocol is not served (documented; modern libspnav uses the socket). |
| FR-503 | Partial | uinput, not libei or portals (ADR-0004) |
| FR-504 | Partial | `3dmouse-osd`: LayerShellQt overlay layer, input-transparent, never focused; X11 fallback window; on GNOME Wayland it exits and notifications are used, reported as `osd_overlay: false`. Verified on Hyprland 0.56.2 (2026-11-10): the toast maps on the overlay layer at the bottom centre (220×48) and hides after its timeout. The radial ring maps in the centre (420×420) with labelled slices, and closes on release (screenshot in the evidence file). Ubuntu packages have no Qt6 LayerShellQt. Also verified on Pop!_OS 24.04 GNOME X11 (toast and radial, X11 fallback window). |
| FR-505 | Partial | documented in `compatibility/desktop-matrix.md`. Physically tested: Hyprland (Arch) and GNOME 46 on Xorg (Pop!_OS 24.04). Headless: KWin 6.8.2 and cosmic-comp 1.11. GNOME Wayland (extension) is untested. |
| FR-506 | Done | lock detection plus broker refusal. Physically verified on Omarchy, **after fixing a real bug** (see evidence). |
| FR-507 | Done | *Motion does*: scroll and zoom, **pointer** (pan/accessibility navigation: the cap moves the pointer, lift scrolls) or **keys** (axis gestures with threshold, hysteresis and auto-repeat: media control, window and workspace switching). **Virtual joystick** output mode: a broker-owned uinput controller (6 ABS axes, 16 buttons) that exists only while such a profile is active, is centred on lock, and is only requested when *Allow the virtual joystick* is on. Presets: Media control, Accessibility: pointer control, Game controller, none auto-selected. Tests: `crates/sm-agent/tests/desktop_modes.rs` (gestures incl. repeat without frames, pointer, permission gate) the kernel check `gamepad_is_a_kernel_joystick` (host gives `js0` and `ID_INPUT_JOYSTICK=1`) and `virtual_joystick_follows_the_mode_without_input` (broker e2e, `SM_UINPUT_TEST=1` as root in a container). On the Pop!_OS 24.04 test host with the Compact (2026-11-12, remotely, without touching the cap): all three presets load in the expected mode, the joystick gate switches on and off, and the controller turned out to be created only on the first touch. Fixed: it now appears when the mode starts (games list controllers at launch). Confirmed on the installed fix build on Pop!_OS the same day: with no input the controller appears as `js1` ("3DMouse Linux virtual joystick") on activation and is removed on leaving the mode; the same on the Arch install (`js0`). Physical motion through each mode is pending. |
| FR-508 | Done | spnav buttons and agent actions are routed per button per profile. Physical: Blender plus desktop buttons. |

## FR-600 Wireless and outputs
| ID | Status | Evidence / gap |
|---|---|---|
| FR-601 | Done for the Compact; N/A otherwise | USB wired verified on the target device (Compact). Receiver and Bluetooth ids are in the registry. Out of scope for 0.1.0: the Compact is wired-only (maintainer decision, 2026-11-10). |
| FR-602 | N/A (0.1.0) | the Compact has no wireless link or battery (maintainer decision, 2026-11-10) |
| FR-603 | N/A (0.1.0) | the Compact has no Bluetooth. If added later: kernel BT HID and BlueZ pairing, untested. |
| FR-604 | Partial | USB replug verified. Suspend/resume and power-cycle not tested. |
| FR-605 | N/A (0.1.0) | the Compact has no LEDs or outputs. No output reports are sent to unverified devices (by design). |
| FR-606 | N/A (0.1.0) | the Compact has no LCD. Needs an Enterprise unit, and the protocol is unknown. |
| FR-607 | Out of scope | |

## FR-700 GUI
| View / item | Status |
|---|---|
| 1 Home dashboard | Done (no one-click self-test button. Diagnostics page has "Run checks".) |
| 2 Device page | Done (axes, cube, buttons, info). Raw event inspector: live values only. |
| 3 Navigation | Done (profile picker, per-axis invert and remap, calibration). No undo/redo, only Save/Revert. |
| 4 Buttons | Done (bind, edit, clear, tap/hold/double. **Conflict warnings**: radial with hold or double, duplicate chords, missing profiles, undefined views, unconfirmed or disabled commands and invalid chords, shown live on each button and via `3dmousectl profile lint`. The action-type list is the command picker; there is no free-text palette.) |
| 5 Profiles | Done (list, focused app, import/export, Axis directions). No search. |
| 6 Macro/radial editor | Done (radial: add/remove/label slices, a full action editor per slice. Macro step editor with up/down reordering. Conflict warnings are shown live on the Buttons page.) |
| 7 OSD customisation | Partial (on/off, position top/centre/bottom, size 75–200 %, per-profile timeout. No themes.) |
| 8 Wireless/device features | Not done |
| 9 Diagnostics | Done (checks, support bundle, raw status) |
| 10 Settings | Partial (auto-switch, notifications, pinned profile, allow command actions, about. No logging level or update channel. Telemetry is deliberately absent.) |
| First-run wizard | Done (device, desktop and focus, input test of all six axes and buttons, 3D app connection, default for unmatched apps (3D only or scroll), save. Opens once; Settings → Run setup. No sudo.) |
| Accessibility | Done (evaluated 2026-11-10): `tests/a11y/run-audit.sh` walks the live AT-SPI tree (Xvfb + at-spi2-core): **189/189** interactive controls have accessible names (24 were missing before the audit and were fixed). Keyboard: arrow keys switch sections, Tab moves into the page with a visible focus frame. Not yet tried by a screen-reader user with Orca. |

## FR-800 Installer and lifecycle
| ID | Status | Evidence / gap |
|---|---|---|
| FR-801 | Done (AppImage N/A) | `.deb` (22.04 and 24.04, container-installed), Arch (physical) and **`.rpm` (Fedora 41, container-installed)**. AppImage is out of scope (maintainer decision, 2026-11-10): the broker needs a root-installed system service, udev rules and uinput access, which a self-contained user-run image can't provide. Distro packages are the delivery path. |
| FR-802 | Done | install, upgrade, downgrade and uninstall documented. Tested on Ubuntu 24.04 (container, 2026-11-10): install 0.1.0, add a user profile, downgrade to a repacked 0.0.9 with `--allow-downgrades`, upgrade back, then remove. The package state is right at each step and the user profile is byte-identical throughout. Arch: reinstall/upgrade done physically several times. |
| FR-803 | Done | spacenavd detection, messages and docs. Never disabled automatically. |
| FR-804 | Done | ADR-0002 and ADR-0005. `systemd-analyze security` 1.1 OK. |
| FR-805 | Done | udev reload and trigger in package scripts. Replug verified. |
| FR-806 | Done | CI computes SHA256SUMS and a CycloneDX 1.5 SBOM (`tools/gen-sbom.py`, every Rust crate with its crates.io SHA-256, plus the distro libraries). `tools/sign-release.sh` makes detached OpenPGP signatures for the checksums, SBOM and every package, and verifies them; Release key: ed25519 `91C6 E161 6930 2F7A ED40  3906 917C 00EB 2BF8 FB0F` (public key in `RELEASE-KEY.asc`, expires 2028-11-10). The 0.1.0 packages (both debs, the rpm and the Arch package) and the SBOM were signed and verified on 2026-11-10. Reproducible: `tools/check-reproducible.sh` builds the committed tree from two different directories with the package flags (`tools/repro-flags.sh`, path remapping). The Rust binaries are byte-identical and contain no build paths (verified 2026-11-10). The C++ GUI binaries are not checked. |
| FR-807 | Done | desktop file, icon, CLI, man pages, overlay service, and an optional status icon (`3dmouse-tray`, a StatusNotifierItem). It is off until enabled in Settings, shows device and profile state, switches profiles, and exits quietly when no tray host exists, so nothing prompts at boot. Checked on Hyprland/waybar: registers with the watcher and exports its menu with the live profile list. |
| FR-808 | Done | offline, no accounts, no network (`PrivateNetwork=yes`) |
| FR-809 | Done | XDG, atomic writes plus flock, `.bak`, 0600 |
| FR-810 | Done | starts without a device. The GUI never blocks on hardware. |

## FR-900 CLI
All commands listed in the SRS exist, with `--json`. `diagnostics check` sets its exit code from the
results. The bundle is always redacted. **Done.**

## Non-functional targets (SRS §6), measured 2026-11-10 on the test host
| Property | Target | Result | Status |
|---|---|---|---|
| Discovery | ≤ 2 s | 186 ms (1 replug) | Partial (100-cycle test not run) |
| Latency | p95 ≤ 15 ms | button: device evdev → virtual uinput ≈ 0.2 ms (4 samples). Motion → spnav not timestamped. | Partial |
| Idle CPU | < 1 % | broker 0.017 %, agent 0.067 % (60 s samples, Arch/Hyprland). The Pop!_OS soak found the agent at 1.19 % on a GNOME X11 host with 923 processes: lock detection read every process's `comm` once a second. Fixed: GNOME and KDE use logind's LockedHint alone, and elsewhere only the user's own processes are read (GNOME 1,175 → 13 reads/s). The 23.5 h Pop!_OS soak (old build) averaged 1.33 %. Re-measured on Pop!_OS on 2026-11-11 over 120 s, side by side, with 937 processes on GNOME X11: the fixed agent (24.04 deb build, run privately without a broker) used 0.116 % and the installed old agent 1.233 %. Installed fix build, 2026-11-12: Pop!_OS agent 0.125 % (broker 0.025 %, 903 processes), Arch/Hyprland agent 0.033 % (broker 0.017 %). | Met |
| Memory | < 100 MB | broker 3.6 MB, agent 4.1 MB RSS | Done |
| Recovery | ≤ 5 s | 0.19 s after hidraw appears | Partial (1 cycle) |
| Stability | 72 h soak | Stopped early at 23.5 h on 2026-11-11 (the host was also under memory pressure from unrelated workloads). Pop!_OS 24.04, 0.1.0 build, Compact attached for all 1,408 samples: broker RSS 2.6–3.1 MB, agent 2.8–4.5 MB, OSD peak 31 MB, all flat, 0 restarts, 0 disconnects. No leak. The full 72 h run is deferred to the beta gate, after the reinstall with the CPU fix (`tools/soak-monitor.sh`, CSV in `~/.local/state/3dmouse-linux/soak.csv` on the test host). | Partial (23.5 h clean); 72 h deferred to beta |
| Installation | no manual permission edits | Arch bare-metal: none needed. Pop!_OS 24.04 bare metal: `apt install` then log out and in, none needed (evidence file). Ubuntu 22.04: containers only. | Done (Arch, Pop!_OS 24.04) |
| Profiles | atomic | fsync + rename + flock. Kill-fault injection (`crates/sm-profile/tests/kill_fault.rs`): 60 SIGKILLs at random points during saves; the profile always parses as a complete version and the lock is never left stuck. The same test fails against a deliberately non-atomic writer. | Done |
| Security | no 0666 device nodes, no network | verified. Property-based fuzzing of every untrusted-input parser: spnav client requests, agent/broker IPC lines, D-Bus, Hyprland/xprop lines, COSMIC Wayland events, HID descriptors and reports, profile import, registry TOML, key chords, imported actions. Campaign on 2026-11-10: 200,000 cases per target, no findings. Coverage-guided fuzzing (cargo-fuzz) and an independent security review are not done. | Done (fuzzing); review pending |
| Accessibility | evaluated | AT-SPI audit: all 189 controls named; keyboard navigation checked (see GUI table) | Done (Orca session by a user pending) |

## Release gate assessment (SRS §13)
**Alpha:**
- *met:* a physically tested device with a complete motion and button path, automated tests green,
  unsupported matrix visible, calibration and diagnostics in the GUI.
- *met (2026-11-10):* installer tested on a real Pop!_OS 24.04 machine (GNOME 46, X11). Device ready,
  libspnav client connected, overlay shown, all diagnostics OK.
  Hand-run checks there with the Compact all passed: six axes, both buttons, Blender, browser
  scroll, and lock (GNOME logind lock hint).
- *outstanding:* none for alpha.

**Beta and 1.0:** not met. Roadmap after 0.1.0 (maintainer decision, 2026-11-10):
- **More device models:** at least one wireless model (SpaceMouse Wireless or Pro Wireless, with
  receiver and Bluetooth) and an Enterprise (LCD, LEDs). This unblocks FR-103/105/601–606.
- **Independent security review** of the broker (root service, uinput, spnav socket) and the agent
  IPC, beyond the fuzzing and threat model already done.
- **72 h soak**: stopped at 23.5 h with no leak or restart; the full run is deferred to the beta gate.
- **Hardware checks waiting on the soak:** suspend/resume (FR-604) and a GNOME Wayland session with
  the Shell extension (FR-401/504) on Pop!_OS (both restart the session, so they need someone at the machine).
