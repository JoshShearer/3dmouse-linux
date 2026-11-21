# Install on Fedora

**Status:** an `.rpm` for Fedora 41 is built in a container (`tools/build-rpm.sh 41`), installs
cleanly with `dnf` into a clean `fedora:41` container, and its GUI starts. **It hasn't been tested
on a real Fedora desktop with a device yet.** It's unsigned (FR-806).

```sh
tools/build-rpm.sh 41                                   # → dist/3dmouse-linux-0.1.0-1.fc41.x86_64.rpm
sudo dnf install ./dist/3dmouse-linux-0.1.0-1.fc41.x86_64.rpm
systemctl --user enable --now 3dmouse-agent          # or log out and back in
```
The package enables `3dmoused` on first install, reloads udev, and loads `uinput`. If spacenavd
is running, 3dmoused yields to it (ADR-0001). Replug the device after installing.

**Upgrades** restart the broker. Reopen running 3D apps afterwards, because libspnav doesn't
reconnect.

**Remove:** `sudo dnf remove 3dmouse-linux`. Your profiles in `~/.config/3dmouse-linux` are
kept.

## From source
```sh
sudo dnf install rust cargo cmake gcc-c++ qt6-qtbase-devel qt6-qtdeclarative-devel
cargo build --release --locked
cmake -S gui -B gui/build -DCMAKE_BUILD_TYPE=Release && cmake --build gui/build
sudo packaging/install-tree.sh /
sudo udevadm control --reload && sudo udevadm trigger
sudo systemctl daemon-reload && sudo systemctl enable --now 3dmoused
systemctl --user enable --now 3dmouse-agent
```
Read `packaging/install-tree.sh` before running it as root. It copies files into `/usr` and
installs nothing else.

SELinux: the broker needs read access to `hidraw` and `input` nodes and read/write access to
`/dev/uinput`. This has not been verified under Fedora's targeted policy.
