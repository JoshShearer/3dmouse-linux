# 3DMouse Linux

Linux driver, per-application profiles and a configuration app for 3Dconnexion 6-DOF devices.
It is compatible with existing **spacenavd/libspnav** applications (Blender, FreeCAD, OpenSCAD…)
and adds automatic per-app profiles, button actions and desktop scrolling.

**Status: 0.1.0, alpha.** Exactly **one** device has been physically verified: the SpaceMouse
Compact `256f:c635` (USB), on two desktops: Arch/Omarchy with Hyprland, and Pop!_OS 24.04 with
GNOME 46 on X11. Everything else is listed honestly as untested in the
[device matrix](docs/compatibility/device-matrix.md). Bug reports and device reports are very
welcome: see [CONTRIBUTING.md](CONTRIBUTING.md).

## What works (verified on hardware, 2026-11-10 to 11-12)
- All six axes and both buttons of the SpaceMouse Compact, over hidraw and evdev.
- Native apps through the libspnav socket: **Blender 4.5**, and **OpenSCAD 2021.01** once its
  SpaceNav driver is enabled.
- Per-profile speed, curves, dead zones, per-axis invert and remap, live in the GUI.
- Automatic profile switching from the focused window (verified on Hyprland and on GNOME X11.
  Also implemented for Sway, KDE, COSMIC and GNOME Wayland through an extension, but untested
  there).
- Button actions: keys, mouse buttons, scroll, macros, profile switching, launch.
- Desktop scroll emulation for apps without 6-DOF support.
- Hotplug: ready about 0.2 s after the device appears.
- No injection while the screen is locked.
- Coexistence with spacenavd: if it's running, we integrate and never disable it.

## Not yet
Bluetooth and battery, LCD and LEDs, devices other than the Compact, a Wayland
overlay on GNOME and in the Ubuntu packages (no Qt6 layer-shell there), and the full 72 h soak
(a 23.5 h run showed no leak). Opt-in modes (pointer control, media gestures, virtual joystick)
are tested in software and their devices are created on real hardware, but cap motion through
them is not yet hand-tested. See
[requirements coverage](docs/requirements-coverage.md) for the full list of gaps.

## Install
Signed packages (`.deb` for Ubuntu 22.04 and 24.04, `.rpm` for Fedora, and the Arch package) are on the
[releases page](https://github.com/JoshShearer/3dmouse-linux/releases).

- Arch / Omarchy: [docs/install/arch.md](docs/install/arch.md)
- Ubuntu 22.04 / 24.04, Pop!_OS: [docs/install/ubuntu-popos.md](docs/install/ubuntu-popos.md)
- Debian, Fedora: [debian.md](docs/install/debian.md), [fedora.md](docs/install/fedora.md)
- Problems: [troubleshooting.md](docs/install/troubleshooting.md). Removal: [uninstall.md](docs/install/uninstall.md)

```sh
3dmousectl diagnostics check   # verify the setup
3dmouse-config                 # settings GUI
```

### Verifying downloads
Releases are signed with the key in `RELEASE-KEY.asc`
(fingerprint `91C6 E161 6930 2F7A ED40  3906 917C 00EB 2BF8 FB0F`):
```sh
gpg --import RELEASE-KEY.asc
gpg --verify SHA256SUMS.asc SHA256SUMS && sha256sum -c --ignore-missing SHA256SUMS
```

## How it fits together
`3dmoused` (system broker: devices, libspnav socket, uinput) ← `3dmouse-agent` (your
session: profiles, focus, buttons) ← `3dmouse-config` (GUI) and `3dmousectl` (CLI).
See [docs/architecture/overview.md](docs/architecture/overview.md) and the [ADRs](docs/adr/).

## Build from source
```sh
cargo build --release --locked && cargo test --workspace
cmake -S gui -B gui/build -DCMAKE_BUILD_TYPE=Release && cmake --build gui/build
```
Requires Rust ≥ 1.88 and Qt ≥ 6.2 (Quick, QuickControls2).

## Licence
GPL-3.0-or-later ([LICENSE](LICENSE)). Not affiliated with or endorsed by 3Dconnexion.
"SpaceMouse" and "3Dconnexion" are trademarks of their owner and are used here only to identify
compatible hardware.
