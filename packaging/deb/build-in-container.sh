#!/bin/sh
# Runs inside packaging/deb/Dockerfile. Source at /src (read-only), output to /out.
set -eu
GNOME=${GNOME:?}
. /etc/os-release
tar -C /src --exclude=./target --exclude=./gui/build --exclude=./dist --exclude=./.git -cf - . | tar -C /build -xf -
cd /build
. tools/repro-flags.sh
cargo build --release --locked --workspace
cargo test --release --locked --workspace -q
cmake -S gui -B gui/build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build gui/build -j"$(nproc)"

VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
PKG=/build/pkg
rm -rf "$PKG"
packaging/install-tree.sh "$PKG" "$GNOME"
gzip -9n "$PKG"/usr/share/man/man*/*   # Debian policy: compressed man pages

# shared-library dependencies, computed like debhelper would
mkdir -p /build/shlib/debian
printf 'Source: 3dmouse-linux\n\nPackage: 3dmouse-linux\nArchitecture: any\n' > /build/shlib/debian/control
DEPS=$(cd /build/shlib && dpkg-shlibdeps -O "$PKG"/usr/bin/* 2>/dev/null | sed -n 's/^shlibs:Depends=//p')
# Qt platform plugins (xcb, offscreen) are a separate package on Ubuntu;
# qt6-wayland provides the native Wayland plugin. Both exist on 22.04 and 24.04.
QPA="qt6-qpa-plugins, qt6-wayland"
QML="qml6-module-qtquick, qml6-module-qtquick-controls, qml6-module-qtquick-layouts, qml6-module-qtquick-dialogs, qml6-module-qtquick-window, qml6-module-qtquick-templates, qml6-module-qtqml-workerscript"

mkdir -p "$PKG/DEBIAN"
cat > "$PKG/DEBIAN/control" <<CTRL
Package: 3dmouse-linux
Version: ${VER}-1~ubuntu${VERSION_ID}
Section: utils
Priority: optional
Architecture: $(dpkg --print-architecture)
Maintainer: 3DMouse Linux contributors <noreply@invalid>
Homepage: https://github.com/JoshShearer/3dmouse-linux
Depends: ${DEPS}, ${QPA}, ${QML}, systemd, udev
Recommends: libspnav0, libnotify-bin
Suggests: spacenavd
Conflicts: spacemouse-linux
Replaces: spacemouse-linux
Description: userspace driver and settings for 3Dconnexion 3D mice
 3dmoused owns the device through hidraw/evdev, serves the libspnav
 protocol so Blender, FreeCAD and OpenSCAD work natively, and injects
 desktop shortcuts for button actions. 3dmouse-agent switches profiles
 per focused application; 3dmouse-config is the Qt settings app.
 Coexists with spacenavd: if spacenavd is running it keeps the device.
CTRL
cp packaging/deb/postinst packaging/deb/prerm packaging/deb/postrm "$PKG/DEBIAN/"
chmod 755 "$PKG"/DEBIAN/postinst "$PKG"/DEBIAN/prerm "$PKG"/DEBIAN/postrm
OUT=/out/3dmouse-linux_${VER}-1~ubuntu${VERSION_ID}_$(dpkg --print-architecture).deb
dpkg-deb --root-owner-group --build "$PKG" "$OUT"
dpkg-deb --info "$OUT" | sed -n '/Depends/p'
echo "built $OUT"
