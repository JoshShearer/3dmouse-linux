#!/bin/sh
# End-to-end check of the KWin focus provider against a real, headless KWin
# (virtual backend) in an Arch container. Needs a release build of the agent:
#   cargo build --release -p sm-agent && tests/integration/kwin-focus-e2e.sh
# Expected: two "focus: org.freedesktop.weston.* (kwin)" lines, the second one
# after KWin was restarted (the agent reloads its script on NameOwnerChanged).
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ENGINE=${CONTAINER_ENGINE:-$(command -v docker || command -v podman)}
"$ENGINE" run --rm -v "$ROOT:/src:ro" archlinux:latest sh -c '
set -e
pacman -Syu --noconfirm --needed kwin dbus xorg-xwayland weston libcap >/dev/null 2>&1
setcap -r /usr/bin/kwin_wayland          # containers refuse its file capabilities
useradd -m u; mkdir -p /tmp/ru; chown u /tmp/ru; chmod 700 /tmp/ru
su u -c "
export XDG_RUNTIME_DIR=/tmp/ru XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=wayland WAYLAND_DISPLAY=wayland-9
timeout 40 dbus-run-session -- sh -c \"
  kwin_wayland --virtual --no-lockscreen --socket wayland-9 >/dev/null 2>&1 & K=\\\$!
  sleep 2; /src/target/release/3dmouse-agent --debug >/tmp/ru/agent.log 2>&1 &
  sleep 3; weston-terminal >/dev/null 2>&1 & sleep 3
  kill \\\$K; sleep 2; rm -f /tmp/ru/wayland-9*
  kwin_wayland --virtual --no-lockscreen --socket wayland-9 >/dev/null 2>&1 &
  sleep 4; weston-simple-shm >/dev/null 2>&1 & sleep 4
  grep -a \\\"(kwin)\\\" /tmp/ru/agent.log
  kill 0
\" || true"
' | tee /dev/stderr | grep -c "(kwin)" | grep -qx 2 && echo "PASS: KWin focus provider" || { echo "FAIL"; exit 1; }
