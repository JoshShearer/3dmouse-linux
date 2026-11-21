# Troubleshooting

Start with:
```sh
3dmousectl diagnostics check        # exit code 0 = all OK
3dmousectl status
journalctl -u 3dmoused -b
journalctl --user -u 3dmouse-agent -b
```
A redacted bundle for bug reports: `3dmousectl support-bundle --output bundle.json`. Read it
before sharing. Serial numbers, window titles and key activity are never included.

| Symptom | Likely cause | Fix |
|---|---|---|
| `devices list` is empty | the udev rule isn't applied yet, or the device is unknown | replug the device. Run `3dmousectl devices inspect`. Check `lsusb` for a `256f:` or `046d:c6xx` id. |
| broker says `integrated-spacenav` | spacenavd is running and owns the device (by design, ADR-0001) | keep it, or `sudo systemctl disable --now spacenavd && sudo systemctl restart 3dmoused` |
| An app doesn't react, but `3dmousectl monitor` shows motion | the app isn't connected to the socket. Check `status` → `spnav_clients`. | **OpenSCAD:** enable *Preferences → Axes → SpaceNav*, or add `[input] enableDriverSPNAV=true` to `~/.config/OpenSCAD/OpenSCAD.conf`, then restart it. **PrusaSlicer** uses its own driver. **Flatpak apps** need `--filesystem=/run/spnav.sock`. |
| Stopped working in every app after an upgrade or `systemctl restart 3dmoused` | libspnav apps don't reconnect to a restarted server (spacenavd behaves the same way) | quit and reopen the apps (for KiCad, make sure no `pcbnew` process remains) |
| A normal app (browser, Obsidian…) doesn't scroll | it has no profile, so Default (native 6-DOF) applies | Profiles → *Desktop navigation (scroll)* → **Add focused app** → Save |
| An app reacts twice (scroll plus 3D) | the profile is `desktop-emulation` for a native app | set the profile's output mode to *Native 6-DOF* |
| A direction feels reversed | preference | GUI → Profiles → *Axis directions*, per profile |
| No per-app switching | the desktop has no focus provider (KDE, COSMIC, GNOME without the extension) | see [wayland.md](wayland.md). Pick a profile manually: `3dmousectl profile activate blender` |
| Buttons do nothing in the desktop | the agent isn't running, or the session is locked | `systemctl --user status 3dmouse-agent`. `status` → `agent.locked` |
| `inject` refused: locked | the screen is locked (by design) | unlock |
| The cursor jumps when touching the cap | libinput uses the device as a pointer | see [wayland.md](wayland.md) |
| Drift when hands are off | the neutral offset of an older unit | `3dmousectl calibrate --apply` (hands off!), or raise the dead zone |

## Migrating from spacenavd
Your `/etc/spnavrc` is not read. Recreate the sensitivity, inversion and button settings in the
GUI's *Default* profile, then disable spacenavd as shown above. To go back:
`sudo systemctl enable --now spacenavd`. 3dmoused then yields automatically.
