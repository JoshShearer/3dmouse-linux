# Install on Debian

**Status: not built or tested.** Debian 12 (Qt 6.4) and 13 should work with the Ubuntu recipe.

To build: copy `packaging/deb/Dockerfile`, change `FROM ubuntu:${UBUNTU}` to
`FROM debian:bookworm`, build that image, and run `packaging/deb/build-in-container.sh` in it the
same way `tools/build-deb.sh` does (source mounted read-only at `/src`, output at `/out`). The
package names used (`qt6-qpa-plugins`, `qt6-wayland`, `qml6-module-*`, `libqt6opengl6-dev`) also
exist on Debian 12.

Install with `sudo apt install ./3dmouse-linux_*.deb`, then follow
[ubuntu-popos.md](ubuntu-popos.md) from "Check" onwards.
