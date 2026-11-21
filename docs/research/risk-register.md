# Risk register

Scales: L = likelihood, I = impact, each H/M/L. Updated 2026-11-10.

| # | Risk | L | I | Mitigation | Residual / owner action |
|---|---|---|---|---|---|
| R1 | Conflict with spacenavd: duplicate motion or a lost device | M | H | auto mode yields to a live socket, inode watch, no `Conflicts=` (ADR-0001), e2e tested | test on a host with spacenavd actually running |
| R2 | Synthetic input reaches a password or lock dialog | M | H | lock detection (locker process, `LockedHint`, Hyprland `LOCK`), broker refuses `inject` while locked, engine cancelled on lock | ≤ 1 s detection window. Desktops without any lock signal aren't protected. **Found and fixed on Omarchy during testing.** |
| R3 | Stuck modifier keys | M | M | the broker tracks keys per connection and releases them on disconnect, owner change, lock or unplug. Macros release on cancel. | none known |
| R4 | Hostile imported profile (exec, SysRq, huge files) | M | H | size and schema limits, `confirmed` cleared on import, exec off by default and run without a shell, Alt+SysRq refused in two layers | |
| R5 | Forged IPC client | L | H | `SO_PEERCRED` auth, control commands only from root, the active seat user, or our own uid; agent socket 0600 | |
| R6 | Malformed HID reports from a faulty or hostile device | L | M | the descriptor-driven decoder validates lengths, malformed reports are counted and dropped, values clamped | fuzzing not yet in CI |
| R7 | Claiming untested models work | M | H | registry statuses, `unknown` by default, evidence files, generated matrices | |
| R8 | Wayland focus is unavailable (KDE, COSMIC, GNOME without the extension) | H | M | fall back to the default profile, shown in the GUI and diagnostics | implement KDE (KWin script) and COSMIC providers |
| R9 | The compositor treats the SpaceMouse as a pointer | M | M | udev `LIBINPUT_IGNORE_DEVICE=1` | |
| R10 | Qt version spread (6.2 on 22.04) | M | M | the QML stays inside 6.2 features, and the 22.04 package is smoke-tested | real 22.04 desktop test pending |
| R11 | Apps whose own 6-DOF driver is off by default (OpenSCAD) look like "driver broken" | H | L | preset note and app matrix explain how to enable it | |
| R12 | Supply chain (crates, rustup) | L | H | Cargo.lock committed, pinned toolchain | SBOM, `cargo audit` and package signing not yet in place |
| R13 | A client that stops reading the spnav socket (KiCad when its 3D viewer is closed) | M | L | bounded 64 KiB buffer, motion dropped, disconnect only if a button can't be delivered | |
