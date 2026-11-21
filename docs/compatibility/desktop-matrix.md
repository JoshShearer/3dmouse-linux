# Desktop and session compatibility matrix

The capabilities that depend on the desktop are **focus tracking** (automatic per-app profiles),
**input injection** (key, mouse and scroll actions, desktop emulation) and **lock detection**
(FR-506). Native 6-DOF apps over libspnav work on every desktop, because they don't depend on any
of these.

| Desktop / session | Focus tracking | Injection (uinput) | Lock detection | Tested? |
|---|---|---|---|---|
| **Hyprland** (Wayland) | yes: `.socket2.sock` events + `j/activewindow` | yes | yes: `j/monitors` `LOCK` blocker (any ext-session-lock client, including Omarchy's Quickshell lock) + locker process | **Physically tested**, Omarchy, Hyprland 0.56.2 (2026-11-10) |
| **Sway** (Wayland) | yes: i3-IPC `window` events | yes | locker process (swaylock, gtklock…) or logind `LockedHint` | unit-tested parser only |
| **GNOME** ≥ 45 (Wayland) | only with the bundled Shell extension (`integrations/gnome-shell/gnome45`) | yes | logind `LockedHint` (GNOME sets it) | **not tested** on a real GNOME session |
| **GNOME 42** (Ubuntu 22.04) | extension variant `gnome42` | yes | logind `LockedHint` | **not tested** |
| **X11** (any WM) | yes: `xprop` `_NET_ACTIVE_WINDOW` | yes | locker process or `LockedHint` | **Physically tested** on Pop!_OS 24.04, GNOME Shell 46 Xorg session, with the Compact (2026-11-10). Focus switches per app (Blender, Brave, Remmina, 3dmouse-config), uinput scroll and Back/Forward reach Brave, the lock is seen via `LockedHint` and input is dropped, and the overlay toast and radial show as X11 windows. |
| **KDE Plasma** (Wayland, X11) | yes: KWin script loaded by the agent, reports over D-Bus (`io.github.joshshearer._3dmouse_linux.Agent.ReportFocus`) | yes | logind `LockedHint` (KDE sets it) | **headless KWin 6.8.2** (virtual backend, container): focus reports and reload after a KWin restart. Not on a real Plasma desktop. |
| **COSMIC** (Pop!_OS 24.04) | yes: `ext_foreign_toplevel_list_v1` + `zcosmic_toplevel_info_v1` v2+ (activated state), own minimal Wayland client | yes | `LockedHint` if set | **real cosmic-comp 1.11.0** (winit backend in Xvfb, container): focus reports, also without `WAYLAND_DISPLAY`. Not on a real COSMIC desktop. |
| Other wlroots compositors | no | yes | locker process / `LockedHint` | not tested |

## Known limitations (disclosed per SRS §0 item 5)
- **On-screen display:** `3dmouse-osd` draws messages, button help and radial menus on the
  wlr-layer-shell overlay layer (Hyprland, Sway, KDE, COSMIC) or as an always-on-top window on X11.
  It never takes focus and input passes through it. GNOME Wayland has no layer-shell, and the
  Ubuntu packages are built without it (Ubuntu ships only the Qt5 LayerShellQt), so those use
  `notify-send`. On Hyprland 0.56.2 it was checked on screen: the toast and the radial ring map on the
  overlay layer and hide again. While the session was locked, the agent sent nothing, so nothing
  was drawn.
- **Injection** is a uinput virtual device. Everything that has keyboard focus receives it,
  including password fields, which is why injection stops while the screen is locked. Lock
  detection is polled every 1 s, so a key action during the first second of a lock may reach the
  lock screen.
- **GNOME** without the extension: no per-app switching. The GUI and `diagnostics check` tell the
  user.
- **No libei or RemoteDesktop portal backend yet** (ADR-0004).
- **X11 ClientMessage libspnav protocol:** not served. Apps that only use `spnav_x11_open` (very old
  builds) need spacenavd. All current apps tested use the AF_UNIX socket.
