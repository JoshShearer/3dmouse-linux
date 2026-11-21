# ADR-0003: HID access path

- Status: accepted (2026-11-10)
- Requirements: FR-101..FR-104, FR-108, FR-109, FR-201, FR-805

## Context
The kernel's `hid-generic` binds 3Dconnexion devices and exposes two nodes:
- a hidraw node (raw reports: id 1 translation, id 2 rotation on older units, id 3 buttons)
- an evdev node (`EV_REL`/`EV_ABS` axes plus `BTN_*`)

The evdev mapping differs between models. Some buttons are missing, and on several Pro or Enterprise
units evdev exposes fewer buttons than the HID report carries (see spacenavd's `bnhack_*`). libinput
and compositors may also treat the evdev node as a pointer.

## Decision
- **hidraw is preferred.** The decoder is driven by the parsed report descriptor (`sm-hid`), not by
  hard-coded offsets. Golden fixtures live in `tests/fixtures/descriptors/`.
- **evdev is the fallback**, used when hidraw can't be opened or the descriptor can't be decoded.
  The same canonical frame applies (`docs/architecture/axis-frames.md`).
- **Discovery** scans sysfs for HID devices whose vid/pid is in the registry, and watches `/dev` with
  inotify (150 ms debounce). There is no libudev dependency. The 5 s retry handles nodes whose ACL
  appears late.
- **udev rules** are generated from the registry (`tools/gen-udev-rules.py`):
  - `TAG+="uaccess"` on hidraw, for the seat user's diagnostics
  - `ENV{LIBINPUT_IGNORE_DEVICE}="1"` on the input node, so the desktop doesn't also move the pointer
  - match on `KERNELS=="0003:VID:PID.*"`, for exact devices only
  - no `MODE="0666"` and no group changes
- **No HID output reports** (LEDs, LCD, configuration) are ever sent. There are no verified formats
  (FR-605, FR-109).

## Alternatives rejected
- *libusb or detaching the kernel driver*: breaks every other consumer, and needs more privilege.
- *evdev only*: loses buttons on larger models, and relies on kernel quirks per model.
- *A grab (EVIOCGRAB) without `LIBINPUT_IGNORE_DEVICE`*: compositors start before us, which races at
  boot.

## Consequences
- The physical evidence for `256f:c635` covers both paths
  (`docs/compatibility/evidence/256f-c635-usb.md`).
- Any other model stays `unknown` until a descriptor fixture and a physical test exist.
