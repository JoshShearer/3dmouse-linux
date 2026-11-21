# Uninstall

Your profiles (`~/.config/3dmouse-linux/`) are **never** deleted by the package manager.

## Arch
```sh
systemctl --user disable --now 3dmouse-agent
sudo pacman -R 3dmouse-linux          # stops and disables 3dmoused
```

## Ubuntu / Debian
```sh
systemctl --user disable --now 3dmouse-agent
sudo apt remove 3dmouse-linux         # or: apt purge (same; no system config is kept)
```
To return to spacenavd afterwards: `sudo systemctl enable --now spacenavd`.

## Also remove your settings (optional and irreversible)
```sh
3dmousectl profile backup --output ~/3dmouse-profiles-backup.toml   # keep a copy first
rm -rf ~/.config/3dmouse-linux
gnome-extensions disable 3dmouse-linux@joshshearer.github.io 2>/dev/null
```

## From source
`packaging/install-tree.sh` installed these paths. Remove them as root:
```
/usr/bin/{3dmoused,3dmouse-agent,3dmousectl,3dmouse-config}
/usr/lib/systemd/system/3dmoused.service
/usr/lib/systemd/user/3dmouse-agent.service
/usr/lib/udev/rules.d/70-3dmouse-linux.rules
/usr/lib/modules-load.d/3dmouse-linux.conf
/usr/share/applications/io.github.joshshearer._3dmouse_linux.config.desktop
/usr/share/icons/hicolor/scalable/apps/3dmouse-linux.svg
/usr/share/gnome-shell/extensions/3dmouse-linux@joshshearer.github.io/
/usr/share/doc/3dmouse-linux/
```
then run `sudo systemctl daemon-reload && sudo udevadm control --reload`.
