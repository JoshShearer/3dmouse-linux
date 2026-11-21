# Linux distribution compatibility matrix

| Distribution | Package | Toolchain / Qt | Status (2026-11-10) |
|---|---|---|---|
| **Arch / Omarchy** | `packaging/arch/PKGBUILD` → `3dmouse-linux-0.1.0-1-x86_64.pkg.tar.zst` | system Rust (or rustup), Qt 6.12 | **Built, installed and physically tested** with a SpaceMouse Compact on kernel 7.2.15 and Hyprland 0.56.2. `systemd-analyze security`: 1.1 OK. |
| **Ubuntu 24.04** | `.deb` from `tools/build-deb.sh 24.04` | Rust 1.88.0 (rustup in the build image), Qt 6.4.2 | **Built.** Installed in a clean `ubuntu:24.04` container: `apt install ./…deb` resolves all dependencies, the binaries run, the udev rule and units are installed, and the GUI renders all 7 pages offscreen. **Not tested on hardware or a real desktop.** |
| **Ubuntu 22.04 / Pop!_OS 22.04** | `.deb` from `tools/build-deb.sh 22.04` | Rust 1.88.0, Qt 6.2.4 | **Built.** Installed in a clean `ubuntu:22.04` container: dependencies resolve, binaries run, the GUI loads and navigates every page with no QML errors. Qt 6.2's offscreen backend could not capture screenshots, and it prints one benign "polish loop" warning on the Profiles page. **Not tested on hardware or a real desktop.** |
| Debian 12/13 | not built | Qt 6.4 / 6.8 | expected to work with the 24.04 recipe. **Untested.** |
| Fedora | no `.rpm` yet | | **Not packaged** (FR-801 partial) |

## Runtime requirements
- Linux kernel with `hid-generic`, `hidraw`, `uinput` (`packaging/modules-load/3dmouse-linux.conf`
  loads `uinput`).
- systemd ≥ 247, the oldest release with every hardening directive the unit uses (Ubuntu 22.04
  ships 249). systemd-logind provides seat `uaccess`.
- The GUI needs Qt ≥ 6.2 with QtQuick Controls 2, Layouts and Dialogs.
- libspnav is *not* needed by us. Apps bring their own.
- Optional: `notify-send` (libnotify) for the OSD, `gtk-launch` or `gio` for Launch actions.
