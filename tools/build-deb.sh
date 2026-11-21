#!/bin/sh
# Build Ubuntu .deb packages in containers: tools/build-deb.sh [22.04|24.04 ...]
# Output: dist/3dmouse-linux_<ver>-1~ubuntu<rel>_<arch>.deb
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ENGINE=${CONTAINER_ENGINE:-$(command -v docker || command -v podman)}
mkdir -p "$ROOT/dist"
for REL in ${*:-22.04 24.04}; do
    case $REL in 22.04) G=gnome42 ;; *) G=gnome45 ;; esac
    "$ENGINE" build -q -t "3dmouse-linux-build:$REL" --build-arg UBUNTU="$REL" "$ROOT/packaging/deb"
    "$ENGINE" run --rm -e GNOME=$G -v "$ROOT:/src:ro" -v "$ROOT/dist:/out" "3dmouse-linux-build:$REL" sh /src/packaging/deb/build-in-container.sh
done
