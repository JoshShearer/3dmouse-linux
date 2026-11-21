# Progress and resume notes

Last updated 2026-11-19. Read this first when resuming work.

## Where things stand
- **Version 0.1.0, alpha.** All components are built and installed on the Arch/Omarchy test host,
  and every physical test there passed (`compatibility/evidence/256f-c635-usb.md`).
- Coverage and gaps: `requirements-coverage.md`. Decisions: `adr/0001…0011`.
- Work is on branch `feat/3dmouse-linux-0.1.0`. Commit there; don't push unless asked.
- Second pass (same day), in software only:
  - `.rpm`
  - setup wizard
  - KDE focus provider (`sm-desktop/src/dbus.rs`, KWin script)
  - `3dmouse-osd` overlay
  - radial menus
  The overlay and radial menus still need an on-screen check on the device. The screen was locked
  during the first try.

## Build, test, package
```sh
export PATH=$HOME/.cargo/bin:$PATH
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked                 # also passes on the 1.88.0 MSRV toolchain
python3 tools/gen-udev-rules.py --check
cmake -S gui -B gui/build && cmake --build gui/build
(cd packaging/arch && makepkg -f --nodeps)      # Arch package (cargo from rustup)
tools/build-deb.sh 22.04 24.04                  # Ubuntu packages → dist/
tools/build-rpm.sh 41                           # Fedora package → dist/
tests/integration/kwin-focus-e2e.sh             # KWin provider on a headless KWin (container)
```
GUI screenshots: `QT_QPA_PLATFORM=offscreen SM_GUI_SCREENSHOTS=/tmp/shots gui/build/3dmouse-config`.

## Test host facts
- Omarchy, kernel 7.2.15, Hyprland 0.56.2, Qt 6.12. The user is in the `input` group.
- The Compact is `/dev/hidraw0` (with a uaccess ACL) and `/dev/input/event27`.
- The virtual device is "3DMouse Linux virtual input".
- The installed package is in use, with `3dmoused` and the user's `3dmouse-agent` enabled.
- Reinstalling restarts the broker, and **running libspnav apps must then be reopened**.
- The user's profiles were edited during testing:
  - `kicad` changed to native6dof (this is now also the shipped preset)
  - `desktop` got app scope with `md.obsidian.Obsidian`
  - `blender` has the user's own axis inversions

## Lessons from hardware testing (don't regress)
1. Omarchy locks through Quickshell's in-process ext-session-lock. Only Hyprland's `j/monitors`
   `LOCK` blocker reveals it (ADR-0004).
2. OpenSCAD's SpaceNav driver is off by default (`[input] enableDriverSPNAV=true`).
3. KiCad 10 links libspnav. Native is smooth, wheel emulation is steppy.
4. Clients that stop reading must not accumulate stale motion (`STALE_BACKLOG` in `sm-spnav`).
5. Apps without a profile get Default (native). Ordinary apps need the Desktop profile to scroll.
6. Ubuntu 22.04 needs `qt6-qpa-plugins` and `libqt6opengl6-dev`. proptest needs rustc 1.88.
7. AF_UNIX socket paths over 108 bytes fail: keep test socket paths short (use `$XDG_RUNTIME_DIR`).
8. Containers refuse `kwin_wayland`'s file capabilities: `setcap -r` it, run it as a normal user, and
   `pacman -Syu` (not `-Sy`).
9. When the session is locked, the agent drops every action (by design), and `grim` hangs. Check
   `3dmousectl status` → `agent.locked` before blaming the code.
10. Never `pkill -f` a pattern that also appears in your own shell's command line.

## Next smallest safe milestones
1. Done: the 24.04 `.deb` is installed on a Pop!_OS 24.04 desktop (GNOME 46 on X11) with the
   Compact, and the alpha gate is met there. COSMIC itself is still untested on hardware.
1a. On the device: check that the overlay toast and radial menu appear and select correctly.
2. Test the GNOME extension on real GNOME 46+ and 42.
3. Test with spacenavd actually running (integrated mode), and the switch back.
4. Suspend/resume and 100-cycle replug tests. The soak ran 23.5 h on Pop!_OS with no leak
   (see requirements-coverage.md); a full 72 h run on the current build is a beta gate.
   Upgrading the package restarts the services, so restart a soak after an upgrade.
4a. Release signing: done with key `917C00EB2BF8FB0F` (`RELEASE-KEY.asc`). Re-run `SM_SIGNING_KEY=91C6E16169302F7AED403906917C00EB2BF8FB0F tools/sign-release.sh` for each release.
5. COSMIC focus provider: done (`sm-desktop/src/cosmic.rs`). Confirm it on the Pop!_OS desktop.
6. GUI: button command search, profile search, conflict warnings.
7. Decide (user) whether unmatched apps should default to native or to scroll. Currently native.
