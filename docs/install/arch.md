# Install on Arch Linux / Omarchy

Tested on Omarchy (Arch), kernel 7.2.15, Hyprland 0.56.2, with a SpaceMouse Compact (2026-11-10).

## Build and install the package
```sh
sudo pacman -S --needed base-devel rust cmake qt6-base qt6-declarative
git clone <this repository> && cd 3dmouse-linux/packaging/arch
makepkg -f            # add --nodeps if Rust comes from rustup instead of pacman
sudo pacman -U 3dmouse-linux-0.1.0-1-x86_64.pkg.tar.zst
```

## Enable the services
The package prints these commands but doesn't run them, following Arch convention:
```sh
sudo systemctl enable --now 3dmoused          # device broker (system)
systemctl --user enable --now 3dmouse-agent   # profiles, buttons, focus (per user)
```
Unplug and replug the device once, or run `sudo udevadm trigger`, so the new udev rule applies.

## Check
```sh
3dmousectl diagnostics check   # every line should be OK
3dmousectl status
3dmouse-config                 # GUI ("3DMouse Settings" in the launcher)
```

## spacenavd already installed?
Nothing is disabled for you. With spacenavd running, `3dmoused` starts in
**integrated-spacenav** mode:
- spacenavd keeps the device and keeps serving your apps
- 3dmouse-linux adds profiles and button actions on top

To let 3dmouse-linux own the device (which enables per-profile axis settings for native apps):
```sh
sudo systemctl disable --now spacenavd
sudo systemctl restart 3dmoused
```

## Upgrade
Rebuild, then `sudo pacman -U …pkg.tar.zst`. The package restarts `3dmoused`. Restart the agent
yourself with `systemctl --user restart 3dmouse-agent`. **Quit and reopen any open 3D apps**
(Blender, KiCad, OpenSCAD…). libspnav doesn't reconnect to a restarted broker.

Uninstalling is covered in [uninstall.md](uninstall.md).
