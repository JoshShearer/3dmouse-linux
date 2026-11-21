#!/bin/sh
# Install built artifacts into a staging tree (or / for a source install).
# Shared by the Arch PKGBUILD, the Debian/Ubuntu package build and
# tools/install-from-source.sh so every channel ships the same layout.
#
# usage: packaging/install-tree.sh DESTDIR [GNOME_VARIANT]
#   expects target/release/{3dmoused,3dmouse-agent,3dmousectl}
#   and gui/build/3dmouse-config to exist.
#   GNOME_VARIANT: gnome45 (default, GNOME 45+) or gnome42 (GNOME 42-44)
set -eu
DEST=${1:?usage: install-tree.sh DESTDIR [gnome45|gnome42]}
GNOME=${2:-gnome45}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
UUID=3dmouse-linux@joshshearer.github.io

i() { install -D -m "$1" "$2" "$DEST$3"; }

for b in 3dmoused 3dmouse-agent 3dmousectl; do
    i 755 "$ROOT/target/release/$b" "/usr/bin/$b"
done
i 755 "$ROOT/gui/build/3dmouse-config" /usr/bin/3dmouse-config
i 755 "$ROOT/gui/build/3dmouse-osd" /usr/bin/3dmouse-osd
i 644 "$ROOT/packaging/systemd/3dmouse-osd.service" /usr/lib/systemd/user/3dmouse-osd.service
# optional status icon (opt-in, FR-807); absent when Qt Widgets was not available at build time
if [ -x "$ROOT/gui/build/3dmouse-tray" ]; then
    i 755 "$ROOT/gui/build/3dmouse-tray" /usr/bin/3dmouse-tray
    i 644 "$ROOT/packaging/systemd/3dmouse-tray.service" /usr/lib/systemd/user/3dmouse-tray.service
fi
i 644 "$ROOT/packaging/systemd/3dmoused.service" /usr/lib/systemd/system/3dmoused.service
i 644 "$ROOT/packaging/systemd/3dmouse-agent.service" /usr/lib/systemd/user/3dmouse-agent.service
i 644 "$ROOT/packaging/udev/70-3dmouse-linux.rules" /usr/lib/udev/rules.d/70-3dmouse-linux.rules
i 644 "$ROOT/packaging/modules-load/3dmouse-linux.conf" /usr/lib/modules-load.d/3dmouse-linux.conf
i 644 "$ROOT/gui/assets/io.github.joshshearer._3dmouse_linux.config.desktop" /usr/share/applications/io.github.joshshearer._3dmouse_linux.config.desktop
i 644 "$ROOT/gui/assets/3dmouse-linux.svg" /usr/share/icons/hicolor/scalable/apps/3dmouse-linux.svg
for f in metadata.json extension.js; do
    i 644 "$ROOT/integrations/gnome-shell/$GNOME/$UUID/$f" "/usr/share/gnome-shell/extensions/$UUID/$f"
done
for f in README.md LICENSE CHANGELOG.md SECURITY.md; do
    [ -f "$ROOT/$f" ] && i 644 "$ROOT/$f" "/usr/share/doc/3dmouse-linux/$f"
done
for f in "$ROOT"/docs/install/*.md; do
    [ -f "$f" ] && i 644 "$f" "/usr/share/doc/3dmouse-linux/$(basename "$f")"
done
# prefix user-guide pages so docs/user-guide/README.md can't clobber the top-level README
for f in "$ROOT"/docs/user-guide/*.md; do
    [ -f "$f" ] && i 644 "$f" "/usr/share/doc/3dmouse-linux/user-guide-$(basename "$f")"
done
i 644 "$ROOT/integrations/blender/README.md" /usr/share/doc/3dmouse-linux/blender.md 2>/dev/null || true
for m in "$ROOT"/packaging/man/*.[18]; do
    sec=${m##*.}
    i 644 "$m" "/usr/share/man/man$sec/$(basename "$m")"
done
echo "installed into ${DEST:-/}"
