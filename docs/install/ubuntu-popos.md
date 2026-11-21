# Install on Ubuntu 22.04 / 24.04 and Pop!_OS 22.04

> **Status:** the packages are built, and they install and run in clean Ubuntu 22.04 and 24.04
> containers. They have **not yet been tested on a real Ubuntu desktop with hardware.** Please
> report results.

## Build the packages (any Linux host with Docker or Podman)
```sh
tools/build-deb.sh 22.04 24.04
ls dist/   # 3dmouse-linux_0.1.0-1~ubuntu22.04_amd64.deb, …~ubuntu24.04_amd64.deb
```
Use the 22.04 package on Pop!_OS 22.04. It is built against that release's Qt 6.2 and glibc. Base
system Qt is never replaced.

## Install
```sh
sudo apt install ./3dmouse-linux_0.1.0-1~ubuntu24.04_amd64.deb
```
Following Debian practice, the package:
- loads `uinput`, reloads udev and enables and starts `3dmoused`
- globally enables the user unit `3dmouse-agent`

Log out and back in, or run `systemctl --user start 3dmouse-agent`.

### GNOME (Ubuntu's default desktop): enable per-app profiles
GNOME on Wayland exposes the focused window only to Shell extensions. Enable the bundled one:
```sh
gnome-extensions enable 3dmouse-linux@joshshearer.github.io
```
Then log out and in. Without it everything works except automatic per-application switching, and
the Default profile is used. The extension is **untested on real GNOME** so far.

## Check
```sh
3dmousectl diagnostics check
3dmouse-config
```

## spacenavd
If the `spacenavd` package is installed and running, 3dmoused detects it and runs in
integrated-spacenav mode. Nothing is removed or disabled. See [arch.md](arch.md#spacenavd-already-installed)
to switch ownership; the commands are the same.

## Upgrade, downgrade and removal
- Upgrade: `sudo apt install ./newer.deb`. The broker restarts. Restart the agent yourself with
  `systemctl --user restart 3dmouse-agent`, and reopen any open 3D apps, because libspnav doesn't
  reconnect.
- Downgrade: `sudo apt install ./older.deb --allow-downgrades`. Profiles carry a `schema_version`,
  and newer schemas are refused rather than corrupted.
- Removal: see [uninstall.md](uninstall.md).
