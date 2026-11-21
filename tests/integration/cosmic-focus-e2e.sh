#!/bin/sh
# End-to-end check of the COSMIC focus provider against a real cosmic-comp
# (winit backend inside Xvfb) in an Arch container. Needs a release build:
#   cargo build --release -p sm-agent && tests/integration/cosmic-focus-e2e.sh
# The agent is started without WAYLAND_DISPLAY, as systemd would.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ENGINE=${CONTAINER_ENGINE:-$(command -v docker || command -v podman)}
"$ENGINE" run --rm -v "$ROOT:/src:ro" archlinux:latest sh -c '
pacman -Syu --noconfirm --needed cosmic-comp xorg-server-xvfb mesa weston dbus libxkbcommon-x11 >/dev/null 2>&1
useradd -m u; mkdir -p /tmp/ru; chown u /tmp/ru; chmod 700 /tmp/ru
su u -c "
export XDG_RUNTIME_DIR=/tmp/ru LANG=C.UTF-8
Xvfb :1 -screen 0 1280x800x24 >/dev/null 2>&1 & sleep 1
DISPLAY=:1 timeout 30 dbus-run-session -- sh -c \"
  cosmic-comp >/dev/null 2>&1 & sleep 5; unset DISPLAY
  XDG_CURRENT_DESKTOP=COSMIC /src/target/release/3dmouse-agent --debug >/tmp/ru/agent.log 2>&1 &
  sleep 2; WAYLAND_DISPLAY=wayland-1 weston-terminal >/dev/null 2>&1 & sleep 3
  WAYLAND_DISPLAY=wayland-1 weston-simple-shm >/dev/null 2>&1 & sleep 3
  grep -a \\\"(cosmic)\\\" /tmp/ru/agent.log
  kill 0
\" || true"
' | tee /dev/stderr | grep -c "(cosmic)" | grep -qx 2 && echo "PASS: COSMIC focus provider" || { echo FAIL; exit 1; }
