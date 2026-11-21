# Physical device validation (SRS §7.2)

These tests need a real device and a person, and they can't run in CI. Record the results in
`docs/compatibility/evidence/<vid>-<pid>-<transport>.md`, using `256f-c635-usb.md` as the model.

## Guided run
```sh
tests/hardware/guided-capture.sh 256f-c635-usb   # prompts for each motion; writes captures/
```
The script records every step with `3dmousectl diagnostics record`. It also saves
`diagnostics check`, `devices inspect` and version info. Check each capture by replaying it:
`3dmousectl diagnostics replay captures/<name>/<step>.jsonl`. It should end neutral.

## Checklist (all required for `full_verified`)
| # | Test | How | Level it supports |
|---|---|---|---|
| 1 | Enumerates, no spurious matches | `devices list`. Other HID devices are untouched. | `detected` |
| 2 | Six directions, signs correct | guided steps 1–6, compare with `docs/architecture/axis-frames.md` | `axis_verified` |
| 3 | Combined motion, neutral, no drift | guided step 7, then 30 s hands off | `axis_verified` |
| 4 | Every physical button | guided step 8 (press each in order) | `buttons_verified` |
| 5 | Declared outputs (LED/LCD) | only with a verified output format | `outputs_verified` |
| 6 | Replug ready ≤ 2 s | unplug and replug, `journalctl -u 3dmoused` | `full_verified` |
| 7 | Power cycle and suspend/resume | `systemctl suspend`, wake, move the cap | `full_verified` |
| 8 | 2 h sustained use | normal work in a native app | `full_verified` |
| 9 | Native application | Blender at minimum, per `docs/compatibility/application-matrix.md` | `full_verified` |
| 10 | Lock screen | lock, move the cap and press buttons: nothing reaches the desktop | required for any desktop-emulation claim |

Track each item as `tested`, `partially tested`, `not available` or `blocked`.
