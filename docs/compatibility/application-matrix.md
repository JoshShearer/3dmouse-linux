# Application compatibility matrix

Every result records the app version, the desktop and the device. "Physically tested" means a
person moved a real 3D mouse and watched the app. Anything else is labelled.

| Application | Version tested | Integration (preset `app_mode`) | Result | Evidence and notes |
|---|---|---|---|---|
| **Blender** | 4.5.15 LTS (official tarball), Arch/Hyprland, Compact | `native6dof`: libspnav NDOF | **Physically tested: works.** All six axes navigate. Per-profile axis inversion is applied and confirmed by the user. | [evidence](evidence/256f-c635-usb.md#native-application-blender-4515) |
| **OpenSCAD** | 2021.01 (Arch `openscad`), Hyprland (XWayland), Compact | `native6dof`: libspnav | **Physically tested: works once OpenSCAD's own SpaceNav driver is enabled.** It is off by default: tick *Edit → Preferences → Axes → SpaceNav*, or add `[input] enableDriverSPNAV=true` to `~/.config/OpenSCAD/OpenSCAD.conf`, then restart OpenSCAD. On the test host the Preferences tick did not persist. | [evidence](evidence/256f-c635-usb.md#native-application-openscad-202101-arch-openscad) |
| **KiCad** (pcbnew, 2D canvas + 3D viewer) | 10.0.7 (Arch), Hyprland, Compact | `native6dof`: libspnav (pcbnew links `libspnav.so.0`) | **Physically tested: works, smooth.** Native navigation on the canvas and in the 3D viewer. Desktop scroll emulation was tried first and felt steppy, because KiCad turns every wheel event into a zoom or pan step, so the preset is now native. KiCad must be restarted after 3dmoused restarts. | evidence file, "KiCad" section |
| **FreeCAD** | — | `native6dof`: libspnav (FreeCAD ≥ 0.19 on Linux) | **Not tested** (not installed on the test host). | `integrations/freecad/README.md` |
| **PrusaSlicer** | — | `none` (PrusaSlicer uses its own 3Dconnexion HIDAPI code) | **Not tested.** PrusaSlicer opens the device's hidraw node itself. The preset therefore sends nothing, to avoid double input. | |
| **Onshape (browser)** | — | `none` | **Not supported.** Browsers have no libspnav path, and the opt-in browser bridge (FR-409) has not been built. | |
| Web browsers (preset `web-browsers`) | Brave on Pop!_OS 24.04 GNOME X11, Compact | `desktop-emulation` | **Physically tested: scroll works** (2026-11-10, with the same mapping through the `desktop` profile). Added as a preset after testers expected browsers to scroll by default. A title containing "Onshape" takes precedence. | evidence file, "Pop!_OS" section |
| Generic desktop (scroll) | Hyprland 0.56.2 | `desktop-emulation` | **Physically tested: works.** Push/pull scrolls vertically, left/right scrolls horizontally, with hi-res wheel events. Buttons map to Back/Forward. | evidence file, "Desktop emulation" section |

## Test procedure per app (SRS §7.3)
1. App discovery: `3dmousectl status` shows the expected profile once the app is focused.
2. All six axes, in both directions.
3. Button mapping.
4. No duplicated motion: with a native preset, the desktop must not also scroll.
5. Lock the screen with the app focused: no input reaches anything.
6. Restart the app after any 3dmoused restart (upgrade). libspnav doesn't reconnect.
7. With spacenavd installed and running instead: the app still works, and 3dmoused reports
   `integrated-spacenav` (ADR-0001).
