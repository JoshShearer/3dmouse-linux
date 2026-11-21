# Blender

Blender talks to `/run/spnav.sock` through libspnav (its "NDOF" support). No plugin is needed.

- **Tested:** Blender 4.5.15 LTS (official tarball), Arch/Hyprland, SpaceMouse Compact,
  2026-11-10. Pan, zoom, orbit and twist work, and per-profile axis inversion applies.
- The `blender` preset matches app ids `blender` and `org.blender.Blender` (Flatpak). It uses Native
  6-DOF and forwards both buttons as NDOF buttons.
- Tune it in two places, whichever you prefer:
  - 3dmouse-linux: Profiles → Blender → Axis directions, or the Navigation page
  - Blender: Edit → Preferences → Input → NDOF (speed, orbit style, invert)

  Settings stack, so set an inversion in one place only.
- **Flatpak Blender** needs access to the socket:
  `flatpak override --user --filesystem=/run/spnav.sock org.blender.Blender`.
