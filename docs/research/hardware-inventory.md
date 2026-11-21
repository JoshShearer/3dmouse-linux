# Hardware inventory

## Devices physically available to this project
| Device | USB id | bcdDevice | Transport | Acquired / tested | Result |
|---|---|---|---|---|---|
| SpaceMouse Compact (wired) | `256f:c635` | 4.37 | USB 2.0 low-speed | 2026-11-10, Omarchy (Arch) laptop | `buttons_verified`, see `../compatibility/evidence/256f-c635-usb.md` |

There is no other hardware, so every other model is `unknown` (SRS §7.2 hard gate).

## Observed facts for `256f:c635`
- The kernel binds `hid-generic` with one HID interface ("Multi-Axis Controller") and exposes
  `hidraw` plus one `input` node with `EV_REL` axes and `BTN_0`/`BTN_1`.
- Report descriptor: `tests/fixtures/descriptors/256f-c635-usb.hex`. Translation and rotation
  arrive in a single report on this unit. Buttons are in report id 3, bits 0–1.
- Axis range is ±350. The device frame is converted with `swap_yz_inv_yz` (spacenavd
  `DF_SWAPYZ|DF_INVYZ`) to the canonical libspnav frame (`../architecture/axis-frames.md`).
- It sends an explicit all-zero report on release. The broker watchdog also forces neutral after
  silence.
- No output reports are documented, and none are sent.

## Registry entries without hardware (ids from spacenavd usbdb @05cc679)
SpaceNavigator (`046d:c626`), SpaceNavigator for Notebooks (`046d:c628`), SpaceMouse Pro
(`046d:c62b`), Pro Wireless cable and receiver (`256f:c631`, `256f:c632`), Pro Wireless BT cable
(`256f:c638`), Enterprise (`256f:c633`), Wireless cable, receiver and BT (`256f:c62e`, `256f:c62f`,
`256f:c63a`), Module (`256f:c636`), SpaceExplorer (`046d:c627`), SpacePilot (`046d:c625`),
SpacePilot Pro (`046d:c629`), SpaceTraveler (`046d:c623`), SpaceBall 5000 USB (`046d:c621`),
CadMan (`046d:c605`), SpaceMouse Plus XT USB (`046d:c603`), SpaceMouse Classic USB (`046d:c606`).

## Wanted for testing (in priority order)
1. SpaceNavigator (`046d:c626`): the most common legacy device. It sends separate translation and
   rotation reports.
2. SpaceMouse Wireless with receiver (`256f:c62f`): receiver handling and multiple interfaces.
3. SpaceMouse Pro (`046d:c62b`): 15 buttons, needs the evdev button hack.
4. SpaceMouse Enterprise (`256f:c633`): 31 buttons, LCD (output stays disabled).
5. Any Bluetooth unit (`256f:c63a`): the BlueZ path.
