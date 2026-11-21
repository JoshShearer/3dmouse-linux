#!/bin/sh
# Runs inside packaging/rpm/Dockerfile. Source at /src (read-only), output to /out.
set -eu
tar -C /src --exclude=./target --exclude=./gui/build --exclude=./dist --exclude=./.git -cf - . | tar -C /build -xf -
cd /build
. tools/repro-flags.sh
cargo build --release --locked --workspace
cargo test --release --locked --workspace -q
cmake -S gui -B gui/build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build gui/build -j"$(nproc)"
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
rpmbuild -bb packaging/rpm/3dmouse-linux.spec \
    --define "sm_version $VER" --define "sm_tree /build" --define "_topdir /build/rpmbuild"
cp /build/rpmbuild/RPMS/*/*.rpm /out/
ls -l /out/*.rpm
