#!/bin/sh
# Accessibility audit (SRS §11): starts 3dmouse-config on a virtual X server with the AT-SPI
# bridge on, and checks that every interactive control has an accessible name.
#   tests/a11y/run-audit.sh [gui-build-dir]   (needs docker; default gui/build)
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GUI=$(cd "${1:-$ROOT/gui/build}" && pwd)
docker run --rm -v "$GUI":/g:ro -v "$ROOT/tests/a11y":/s:ro archlinux:latest sh -c '
pacman -Syu --noconfirm -q xorg-server-xvfb at-spi2-core python-gobject qt6-declarative qt6-svg layer-shell-qt dbus ttf-dejavu >/dev/null 2>&1
useradd -m t
su t -c "cd /tmp; dbus-run-session -- sh -c \"
  Xvfb :9 -screen 0 1280x900x24 >/dev/null 2>&1 & sleep 1
  export DISPLAY=:9 QT_QPA_PLATFORM=xcb QT_LINUX_ACCESSIBILITY_ALWAYS_ON=1
  /usr/lib/at-spi-bus-launcher --launch-immediately >/dev/null 2>&1 & sleep 1
  /g/3dmouse-config >/tmp/gui.log 2>&1 & sleep 4
  python3 -I /s/atspi-audit.py
\""' | tee /dev/stderr | grep -q "unnamed: 0$"
