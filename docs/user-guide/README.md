# User guide

## First steps
1. Install the package and enable the services ([install guides](../install/)).
2. Plug in the device and run `3dmousectl diagnostics check`.
3. Open **3DMouse Settings** (`3dmouse-config`). The **Device** page shows live axes, a 3D
   cube and the buttons. Push the cap and check that the bars move the way you expect.

## Profiles
A profile holds the navigation tuning, the output mode and the button bindings. The active profile
is chosen in this order:
1. your manual choice
2. the focused application
3. the device
4. **Default**

The title bar of the settings window shows the active profile. To check from the terminal:
`3dmousectl status`.

| Output mode | Use for |
|---|---|
| Native 6-DOF | apps that support 3D mice (Blender, FreeCAD, OpenSCAD). Motion goes to the app. |
| Desktop emulation | everything else. *Motion does* picks scrolling (push/pull scrolls up and down, left/right sideways, optional zoom axis), moving the pointer, or pressing keys. |
| None | apps that drive the device themselves (PrusaSlicer), or to pause. |
| Game controller | games: the axes and buttons appear as a joystick. Opt-in (see below). |

**Opt-in modes.** These presets never switch on by themselves. Pick them in the profile list (or
from the status icon), or copy their settings into an app's profile.
- **Media control**: twist for volume (clockwise is louder), push right or left for the next or
  previous track, press down to play or pause. Left button plays or pauses, right button mutes.
  The same *Press keys* style switches windows or workspaces: add a gesture such as push right →
  `super+ctrl+right`.
- **Accessibility: pointer control**: the cap moves the pointer (push away moves it up), lift or
  press scrolls, the buttons click. Set the pointer speed in the profile.
- **Game controller (virtual joystick)**: turn on *Settings → Allow the virtual joystick* first.
  While the profile is active, games see a six-axis controller called "3DMouse Linux virtual
  joystick" with the device's buttons. It is removed when you switch to another profile and
  centred while the screen is locked.

**Web browsers** (Firefox, Chromium, Chrome, Brave, Vivaldi, Edge, LibreWolf, Zen) scroll out of the box: the
**Web browsers (scroll)** preset applies. Onshape tabs (window title contains "Onshape") use their own profile instead.

**Scrolling in other ordinary apps (Obsidian, editors).** An app without its own profile gets
**Default**, which is *Native 6-DOF* and feeds only 3D apps. Default stays native deliberately, so
an unrecognised 3D app is never starved of 6-DOF input. To scroll in an app:
1. Focus the app.
2. In *Profiles*, open **Desktop navigation (scroll)** and click **Add focused app**.
3. Click **Save**.

**Making a profile for a new app:**
1. Focus the app.
2. In *Profiles*, choose **New…**, set *Applies to → Application*, then click **Add focused app**.

**Matching options** (all optional, and all must hold when set):
- *Application ids*: Wayland app id, X11 class, Flatpak id or desktop-file id (exact, case-insensitive).
- *Executables*: a file name (`blender`) matches that program from any directory. An absolute path
  (`/opt/cad/bin/cad`) matches only that file. Works where the desktop reports the window's process:
  Hyprland, Sway, X11 and KDE.
- *Window title contains* / *Window title matches*: a substring or a regular expression
  (case-insensitive) for document- or workspace-specific profiles, such as Onshape assemblies.
  Titles are only matched in memory, never stored or logged.
- When several profiles match, the one with the most specific title condition wins
  (regex and substring, then regex, then substring, then app only).

Use the search box above the list to filter profiles by name, id or output mode.

## Navigation tuning
On the *Navigation* page, choose the profile to edit at the top. A profile that isn't active is
saved but not previewed.
- **Overall / pan-zoom / rotation speed**
- **Curve:** *Power* gives fine control near the centre.
- **Smoothing** (ms): more smoothing means more lag.
- **Dominant axis only:** only the strongest axis moves.
- **Axes:** for each output axis, its source, speed, dead zone, **Invert** and **On**.
- **Axis directions** (Profiles page): quick per-profile inversion of any axis, plus *Invert all*,
  *Invert rotations* and *Invert pan/zoom*.
- **Calibration:** keep your hands off and measure the neutral offset. Use it only if the cursor or
  view drifts.

Changes preview live. **Save** keeps them, and **Revert** discards them.

## Buttons
For each button and profile you set:
- **Tap**, **Hold** and **Double** actions
- action types: Application button (sent to native apps), Key chord, Mouse button, Scroll, View,
  Profile switch or cycle, Sensitivity, Toggle, Launch app, Run command (disabled unless enabled in
  Settings, and asks for confirmation), Macro, Show help, Radial menu

## Radial menus
A radial menu puts up to 8 actions on one button:
1. In *Buttons*, set the button's tap action to **Radial menu**.
2. Edit the slices. Slice 1 is at the top, and the rest follow clockwise.
3. Hold the button, push or tilt the cap towards a slice, and release the button to run it.
   Releasing in the centre does nothing.

While the menu is open, 3D apps don't move. Use a button that has no hold or double-press action.
The menu is drawn by the overlay. Without an overlay, a notification names the selected slice.

## On-screen overlay
`3dmouse-osd` shows profile changes, toggles, button help and radial menus on top of every
window, without taking focus. It works on Hyprland, Sway, KDE and COSMIC with the Arch or Fedora
package, and on X11. On GNOME, and with the Ubuntu packages on Wayland, notifications are used
instead. Set the position and size in *Settings*. Enable it with
`systemctl --user enable --now 3dmouse-osd`.

## Status icon (optional)
Turn on *Settings → Show a status icon in the panel* to get a tray icon. It shows the device and
the active profile, and is greyed out when no device is connected. Its menu switches profiles (or
goes back to automatic selection) and opens the settings app. It needs a panel with a tray:
waybar (Omarchy keeps tray icons behind the `<` button), KDE, COSMIC, XFCE, or GNOME with the
AppIndicator extension (Ubuntu ships it). Turn it off with *Hide status icon* in its menu. From a
terminal: `systemctl --user enable --now 3dmouse-tray`.

## Setup wizard
The first time you open the settings app, a wizard checks the device, desktop and controls, and
whether 3D apps can connect. It also asks what ordinary apps should get: 3D navigation only, or
scrolling. Run it again from *Settings → Run setup…*.

## Safety
- Nothing is injected while the screen is locked.
- Run-command actions are off by default. Imported profiles never run commands until you confirm
  them again.
- The kernel's Alt+SysRq combination can't be bound.

## Per-application notes
- **Blender:** works out of the box (NDOF). Blender's own NDOF preferences (Edit → Preferences →
  Input → NDOF) also apply.
- **OpenSCAD:** enable *Edit → Preferences → Axes → SpaceNav* and restart OpenSCAD. If the tick
  doesn't stick, add `[input]` and `enableDriverSPNAV=true` to `~/.config/OpenSCAD/OpenSCAD.conf`.
- **FreeCAD:** see `integrations/freecad/README.md`.
- **KiCad:** native (smooth canvas and 3D viewer navigation). KiCad 10 links libspnav. If an older
  build doesn't react, set the profile's output mode to *Desktop emulation*.

## Getting help
`3dmousectl support-bundle --output bundle.json` creates a redacted report. Look at it, then
attach it to an issue.
