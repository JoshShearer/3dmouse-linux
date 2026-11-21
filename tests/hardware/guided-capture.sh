#!/bin/sh
# Guided physical capture for one device (see tests/hardware/README.md).
# usage: tests/hardware/guided-capture.sh <name, e.g. 256f-c635-usb> [seconds-per-step]
set -eu
NAME=${1:?usage: $0 <vid-pid-transport> [seconds]}
SECS=${2:-5}
OUT="captures/$NAME"
mkdir -p "$OUT"

3dmousectl version > "$OUT/version.txt" 2>&1 || true
uname -a >> "$OUT/version.txt"
3dmousectl --json diagnostics check > "$OUT/diagnostics.json" 2>&1 || true
3dmousectl --json devices inspect > "$OUT/inspect.json" 2>&1 || true

step() {
    n=$1; shift
    printf '\n[%s] %s\n    Press Enter, then do it within %ss…' "$n" "$*" "$SECS"
    read -r _
    3dmousectl diagnostics record --duration "${SECS}s" --redact --output "$OUT/$n.jsonl"
    echo "    saved $OUT/$n.jsonl"
}

step 1-right     "push the cap RIGHT and release"
step 2-up        "pull the cap UP and release"
step 3-forward   "push the cap FORWARD (away from you) and release"
step 4-tilt-fwd  "TILT the cap forward and release"
step 5-twist-cw  "TWIST the cap clockwise (seen from above) and release"
step 6-roll-right "ROLL the cap to the right and release"
step 7-combined  "move freely in all directions, then let go and keep your hands off"
step 8-buttons   "press each button once, in order (left to right, top to bottom)"

echo
echo "Done. Replay each with: 3dmousectl diagnostics replay $OUT/<step>.jsonl"
echo "Then write docs/compatibility/evidence/$NAME.md (model: 256f-c635-usb.md)."
