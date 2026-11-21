# Axis frames (FR-209)

Two coordinate frames exist. Every number shown to users, stored in a
profile, or sent to applications is in the **canonical frame**.

## Device (HID) frame

What the HID reports and the evdev `REL_*` codes carry. Measured on a
SpaceMouse Compact (256f:c635, bcdDevice 0437) on 2026-11-10 with guided
captures (`tests/fixtures/evdev/256f-c635-usb-guided.txt`,
`…-retake.txt`), viewed from the user's seat, cable away from the user:

| Physical action | HID field | Sign |
|---|---|---|
| Push cap right | X (`REL_X`) | + |
| Pull cap toward user | Y (`REL_Y`) | + |
| Press cap down | Z (`REL_Z`) | + |
| Tilt cap back (top toward user) | RX | + |
| Tilt cap right (roll) | RY | − |
| Twist clockwise (seen from above) | RZ | + |

Range: exactly ±350 on all six axes (matches the descriptor's logical
min/max of −350..350).

## Canonical frame (libspnav / spacenavd)

Right-handed, **X right, Y up, Z away from the user** — the frame libspnav
applications (Blender, FreeCAD, OpenSCAD) expect from spacenavd.
spacenavd converts every 3Dconnexion (vid 256f) device with
`DF_SWAPYZ | DF_INVYZ` (spacenavd `src/dev.c`, commit 05cc679); we apply the
identical transform so applications behave exactly as with spacenavd:

```
canonical = [ x, -z, -y, rx, -rz, -ry ]      (registry: canonical = "swap_yz_inv_yz")
```

| Physical action | Canonical axis | Sign | Verified |
|---|---|---|---|
| Push right | TX | + | 2026-11-10, libspnav 1.2 client, peak +164 |
| Pull up (lift) | TY | + | 2026-11-10, peak +343 |
| Push forward (away) | TZ | + | derived from HID −Y (guided capture) |
| Twist clockwise | RY | − | 2026-11-10, peak −246 (right-hand rule about +Y up) |
| Tilt forward | RX | − | derived from HID RX |
| Roll right | RZ | + | derived from HID −RY |

Rows marked "derived" follow from the measured HID signs and the transform;
the libspnav end-to-end run (`tests/fixtures/spnav/256f-c635-libspnav-e2e.jsonl`)
checked right, up and twist directly.

Legacy Logitech-era devices (vid 046d) already report in the canonical
frame (`canonical = "identity"` in the registry, as in spacenavd).

## Where each frame appears

| Place | Frame |
|---|---|
| `devices.toml` button/axis data, HID decoder, evdev assembler, fixtures | device |
| `sm_core::InputState` | device |
| Broker → pipeline input, `motion` events (`raw`), GUI raw bars, `calibrate` | canonical |
| `NavConfig` (axis_map, invert, deadzone, center_offset) | canonical |
| libspnav motion events | canonical |
