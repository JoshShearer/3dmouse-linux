#!/bin/sh
# Build Fedora .rpm packages in containers: tools/build-rpm.sh [41 ...]
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ENGINE=${CONTAINER_ENGINE:-$(command -v docker || command -v podman)}
mkdir -p "$ROOT/dist"
for REL in ${*:-41}; do
    "$ENGINE" build -q -t "3dmouse-linux-rpm:$REL" --build-arg FEDORA="$REL" "$ROOT/packaging/rpm"
    "$ENGINE" run --rm -v "$ROOT:/src:ro" -v "$ROOT/dist:/out" "3dmouse-linux-rpm:$REL" sh /src/packaging/rpm/build-in-container.sh
done
