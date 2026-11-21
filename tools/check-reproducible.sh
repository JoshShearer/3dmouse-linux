#!/bin/sh
# Reproducibility check (FR-806): build the committed tree twice, from two different
# directories, with the packaging flags, and compare the Rust binaries byte for byte.
#   tools/check-reproducible.sh [git-rev]   (default HEAD)
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
REV=${1:-HEAD}
WORK=$(mktemp -d "${TMPDIR:-/tmp}/sm-repro.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
BINS="3dmoused 3dmouse-agent 3dmousectl"
for side in a long-different-path-b; do
    mkdir -p "$WORK/$side/src"
    git -C "$ROOT" archive "$REV" | tar -C "$WORK/$side/src" -xf -
    ( cd "$WORK/$side/src" && . tools/repro-flags.sh && CARGO_TARGET_DIR="$WORK/$side/target" cargo build --release --locked -q --bins )
done
fail=0
for b in $BINS; do
    ha=$(sha256sum < "$WORK/a/target/release/$b" | cut -d' ' -f1)
    hb=$(sha256sum < "$WORK/long-different-path-b/target/release/$b" | cut -d' ' -f1)
    if [ "$ha" = "$hb" ]; then echo "identical  $b  $ha"; else echo "DIFFERENT  $b  $ha $hb"; fail=1; fi
done
# no build path may leak into the binaries
for b in $BINS; do
    if grep -a -q -e "$WORK" -e "$HOME" "$WORK/a/target/release/$b"; then echo "LEAKS PATH $b"; fail=1; fi
done
exit $fail
