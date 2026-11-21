# Wayland notes

Native 6-DOF apps (Blender, FreeCAD, OpenSCAD) work on every Wayland compositor, because they talk
to the libspnav socket and not to the compositor.

What depends on the compositor:

| Feature | How it works | Where it works |
|---|---|---|
| Automatic per-app profiles | compositor IPC (Hyprland, Sway), a KWin script over D-Bus (KDE), or the GNOME Shell extension | Hyprland, Sway, KDE Plasma, COSMIC, GNOME with the extension. |
| Key, mouse and scroll actions, desktop emulation | `/dev/uinput` virtual device "3DMouse Linux virtual input" | every compositor |
| No input while locked | locker process, logind `LockedHint`, Hyprland session-lock state | see the matrix |
| On-screen overlay (messages, button help, radial menus) | `3dmouse-osd` on the wlr-layer-shell overlay layer, never focused, input passes through | Hyprland, Sway, KDE, COSMIC (needs a LayerShellQt build: Arch, Fedora). X11 as an always-on-top window. |
| Notifications (fallback) | `notify-send` when no overlay is connected | GNOME, Ubuntu packages, any desktop with a notification daemon |

Full table: [../compatibility/desktop-matrix.md](../compatibility/desktop-matrix.md).

## XWayland apps
OpenSCAD 2021 and other Qt5 apps may run under XWayland. That doesn't matter for libspnav, which
uses the socket, not X11. Focus tracking still sees them: Hyprland reports their class, e.g.
`OpenSCAD`.

## The cursor moves when I touch the 3D mouse
Your compositor is treating the device as a pointer. The udev rule sets `LIBINPUT_IGNORE_DEVICE=1`.
Replug the device after installing, then check with:
```sh
udevadm info /dev/input/by-id/*3Dconnexion*event* | grep LIBINPUT_IGNORE
```
