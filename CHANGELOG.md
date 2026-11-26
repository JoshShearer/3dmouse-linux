# Changelog

## 0.1.0 (unreleased, 2026-11-10)

First alpha, and the first public release.

### Changed
- Renamed from "SpaceMouse Linux" to **3DMouse Linux** before the first public release, to keep the
  3Dconnexion trademark out of the project name. Hardware model names are unchanged.

  | Was | Now |
  |---|---|
  | `spacemoused`, `spacemousectl`, `spacemouse-agent`, `-config`, `-osd`, `-tray` | `3dmoused`, `3dmousectl`, `3dmouse-agent`, `-config`, `-osd`, `-tray` |
  | package `spacemouse-linux` | `3dmouse-linux`, which replaces the old package |
  | `~/.config/spacemouse-linux` | `~/.config/3dmouse-linux`, moved automatically on first start |
  | `/run/spacemouse`, `$XDG_RUNTIME_DIR/spacemouse` | `/run/3dmouse`, `$XDG_RUNTIME_DIR/3dmouse` |
  | `SPACEMOUSE_*` environment variables | `THREEDMOUSE_*` |
  | GNOME extension `spacemouse-linux@…` | `3dmouse-linux@joshshearer.github.io` |
  | app and D-Bus ID `io.github.spacemouse_linux…` | `io.github.joshshearer._3dmouse_linux…` |

  If you ran a pre-release build: re-enable the services under their new names
  (`sudo systemctl enable --now 3dmoused`, `systemctl --user enable --now 3dmouse-agent 3dmouse-osd`).
  On GNOME Wayland, also enable the extension under its new ID.

### Fixed
- Agent idle CPU on busy hosts: lock detection no longer reads every process once a second
  (GNOME and KDE use logind's LockedHint; elsewhere only the user's processes are checked).
- The virtual joystick appeared only after the cap was first touched, so a game started
  before that saw no controller. It now appears as soon as a game-controller profile is active.
- Broker: `set_config` rejects an unknown `app_mode` or malformed `spnav_buttons` instead of
  applying a different mode or mapping and reporting success (#1).
- Broker: `clear_config` from a client that is not the config owner is refused (`not_owner`), so it
  can't clear a profile another client set after it (#2).
- Agent: a profile the broker rejects is logged as a warning instead of only at debug level.
- Broker: `set_session` without a boolean `locked` (missing, `null`, a string or a number) is
  refused instead of unlocking the session.
- Broker: a control client sending more than 256 requests at once had the rest silently dropped.
  Every request is now answered, in order; a client that sends faster than the broker handles
  requests waits on the socket instead.
- Virtual input: a key chord with a refused key (outside the allowlist, or Alt+SysRq) pressed the
  keys before it and left them held. Such a chord now presses nothing.

### Added
- Opt-in desktop modes: pointer control (accessibility), key gestures on axes (media control,
  window and workspace switching) and a virtual joystick for games. Presets: *Media control*,
  *Accessibility: pointer control*, *Game controller*.
- Optional status icon (`3dmouse-tray`): device and profile state, profile switching, open settings.
  Off by default; enable it in Settings.
- Macro step editor: add, remove, retype and reorder steps, with a live step count and wait total against the
  limits. Replaces the JSON box.
- Profile matching by executable and by window-title regular expression (opt-in). The most specific match
  wins. Profile search box.
- Accessibility: every control has an accessible name (AT-SPI audit: `tests/a11y/run-audit.sh`).
- Fuzzing for all untrusted-input parsers, a profile kill-fault test, and a reproducible-build check.
- `3dmoused` broker:
  - hidraw (descriptor-driven) and evdev input
  - sysfs and inotify hotplug
  - per-device axis pipeline
  - libspnav v1 socket server
  - uinput virtual input
  - spacenavd coexistence (auto, native-owner, integrated-spacenav, passive-monitor)
  - hardened systemd unit (`Type=notify`, watchdog, no capabilities)
- `3dmouse-agent`:
  - per-application profiles from focus (Hyprland, Sway, X11, GNOME extension)
  - button engine (tap, hold, double), macros
  - desktop scroll and zoom emulation
  - lock detection, OSD notifications
- `3dmousectl`: status, devices, monitor, profiles, settings, calibrate, test-action,
  diagnostics check/record/replay, support bundle, service health.
- `3dmouse-config` Qt6/QML GUI: overview, device, navigation, buttons, profiles, diagnostics and
  settings pages.
- Presets: default, desktop, Blender, FreeCAD, KiCad, OpenSCAD, PrusaSlicer, Onshape (no-op).
- Device registry (20 entries) and generated udev rules (`uaccess`, `LIBINPUT_IGNORE_DEVICE`).
- Packages: Arch PKGBUILD, plus Ubuntu 22.04 and 24.04 `.deb` (built in containers).
- Man pages, install guides, ADRs 0001–0010, compatibility matrices.

### Fixed during hardware validation
- Browsers got no input, because the Default profile is native 6-DOF. A **Web browsers (scroll)** preset now
  covers common browsers. Profile selection prefers a title-specific match (Onshape) over an app-only one.
- Profiles page: on wide windows the list took the spare width and pushed the editor off-screen.
- SIGHUP killed the session agent instead of reloading profiles, and systemd didn't restart it.
  The focus thread started before the signals were blocked. They're now blocked first.
- `3dmouse-config` and `3dmouse-osd` segfaulted on exit when QML modules were missing
  (seen on Pop!_OS). They now exit with status 1.
- Screen lock under Omarchy's Quickshell locker went undetected, and scroll was injected while
  locked. It is now detected through Hyprland's session-lock state.
- Ubuntu 22.04 package: the GUI couldn't start, because the Qt platform plugins weren't a
  dependency.
- The Profiles page editor overflowed the window and hid its action buttons.
- KiCad preset switched from desktop emulation (steppy) to native libspnav (smooth).
- A libspnav client that stops reading (KiCad while unfocused) no longer accumulates stale motion
  that would replay as a view jump.
- The agent's lock polling costs 6× less CPU on Hyprland (0.40 % → 0.067 % idle).

### Added (after hardware testing)
- Per-profile *Axis directions* card, and a profile picker on the Navigation page.

### Added (second pass)
- `.rpm` for Fedora 41 (`tools/build-rpm.sh`), container-installed in CI.
- First-run setup wizard in `3dmouse-config`.
- KDE Plasma per-app profiles: the agent loads a KWin script that reports focus over D-Bus (own
  minimal D-Bus client, no new dependency). Tested against a headless KWin 6.8.2.
- COSMIC per-app profiles (Pop!_OS 24.04): Wayland toplevel-info client, tested on cosmic-comp 1.11.
- `3dmouse-osd` on-screen overlay (wlr-layer-shell or X11) for messages, button help and radial
  menus. Overlay position and size in Settings.
- Radial menu action (hold, push the cap towards a slice, release), with an editor.
- `3dmousectl test-action --phase press|release`.
- Release tooling: CycloneDX SBOM (`tools/gen-sbom.py`), signed checksums and packages
  (`tools/sign-release.sh`), and a soak monitor (`tools/soak-monitor.sh`).

### Verified hardware
- SpaceMouse Compact `256f:c635` (USB): `buttons_verified`.
