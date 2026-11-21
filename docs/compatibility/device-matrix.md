# Device compatibility matrix

Generated from `crates/sm-registry/data/devices.toml` (the source of truth), 2026-11-10.

**How to read this:**
- Each status applies to the exact vid:pid and transport only (SRS §2.2).
- `unknown` means the id comes from upstream spacenavd data (`usbdb` at `05cc679`) and nobody has
  tested it with 3dmouse-linux. It may work, because the decoder follows the descriptor, but it
  is **not claimed**.
- "Buttons" is the count in the registry's button table. `?` means not yet mapped: the device works
  for motion if its descriptor decodes, and buttons appear by raw index.

| USB id | Model | Transport | Buttons | Status | Evidence |
|---|---|---|---|---|---|
| `256f:c635` | SpaceMouse Compact | usb | 2 | **buttons_verified** | [record](evidence/256f-c635-usb.md) |
| `046d:c626` | SpaceNavigator | usb | 2 | **unknown** | — |
| `046d:c628` | SpaceNavigator for Notebooks | usb | 2 | **unknown** | — |
| `046d:c62b` | SpaceMouse Pro | usb | 15 | **unknown** | — |
| `256f:c631` | SpaceMouse Pro Wireless (cable) | usb | ? | **unknown** | — |
| `256f:c632` | SpaceMouse Pro Wireless (receiver) | usb-receiver | ? | **unknown** | — |
| `256f:c638` | SpaceMouse Pro Wireless BT (cable) | usb | ? | **unknown** | — |
| `256f:c633` | SpaceMouse Enterprise | usb | 31 | **unknown** | — |
| `256f:c62e` | SpaceMouse Wireless (cable) | usb | ? | **unknown** | — |
| `256f:c62f` | SpaceMouse Wireless (receiver) | usb-receiver | ? | **unknown** | — |
| `256f:c63a` | SpaceMouse Wireless (Bluetooth) | bluetooth | ? | **unknown** | — |
| `256f:c636` | SpaceMouse Module | usb | ? | **unknown** | — |
| `046d:c627` | SpaceExplorer | usb | ? | **unknown** | — |
| `046d:c625` | SpacePilot | usb | ? | **unknown** | — |
| `046d:c629` | SpacePilot Pro | usb | ? | **unknown** | — |
| `046d:c623` | SpaceTraveler | usb | ? | **unknown** | — |
| `046d:c621` | SpaceBall 5000 (USB) | usb | ? | **unknown** | — |
| `046d:c605` | CadMan | usb | ? | **unknown** | — |
| `046d:c603` | SpaceMouse Plus XT (USB) | usb | ? | **unknown** | — |
| `046d:c606` | SpaceMouse Classic (USB) | usb | ? | **unknown** | — |

## Status levels
| Status | Meaning |
|---|---|
| `unknown` | listed, not tested |
| `detected` | enumerates and is recognised, with a descriptor fixture captured |
| `axis_verified` | all six directions and combined motion physically verified, with neutral and no drift |
| `buttons_verified` | plus every physical button |
| `outputs_verified` | plus any declared device outputs (LED/LCD), on verified formats |
| `full_verified` | plus reconnect, power cycle, suspend/resume, 2 h use and native apps (SRS §7.2) |
| `partial` / `blocked` | works with listed gaps / known not to work |

## Not supported, by design or for lack of evidence
- **Bluetooth:** the ids are listed, and the code accepts HID bus 0x05, but nothing has been
  tested. There is no battery reporting.
- **LED, LCD and display output (Enterprise):** no output report is ever sent (FR-605, ADR-0003).
- **Serial-port devices** (Spaceball, Magellan serial): out of scope. spacenavd supports them.

## Contributing a device test
Run `3dmousectl diagnostics record --duration 30s --redact -o capture.jsonl`, follow
`tests/hardware/README.md`, and open a PR with the capture and an evidence file modelled on
`evidence/256f-c635-usb.md`.
