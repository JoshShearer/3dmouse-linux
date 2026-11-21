# Axis pipeline (FR-200)

Implemented in `crates/sm-pipeline`, run by the broker once per device for
every decoded frame (one pipeline, all outputs — FR-210). Applications
connected to libspnav receive the result; the broker never runs a second
pipeline on data that already passed through spacenavd.

```
HID report ─► decoder (hidraw) or assembler (evdev) ─► InputState (device frame)
  ─► canonical transform ─► [1] centre offset ─► [2] normalise + clamp
  ─► [3] axis map ─► [4] invert ─► [5] deadzone ─► [6] curve ─► [7] gain
  ─► [8] smoothing ─► [9] mode constraints ─► [10] output clamp
  ─► spnav integer = round(v × 350)   and   norm (float) for subscribers
```

| Step | Math | Range / units |
|---|---|---|
| 1 centre | `r - center_offset[i]` | raw canonical units |
| 2 normalise | `clamp(r / axis_range, -1, 1)` | axis_range from registry (350) or descriptor |
| 3 map | `out[i] = in[axis_map[i]]` | permutation-free remap allowed (duplicates allowed) |
| 4 invert | `-v` if `per_axis_invert[i]` | |
| 5 deadzone | `0` if `|v| ≤ d`, else `sign(v)·(|v|-d)/(1-d)` | `d ∈ [0, 0.9]`, default 0.02; rescaled so output stays continuous and reaches ±1 |
| 6 curve | linear `v`; power `sign(v)·|v|^e` (e ∈ [0.2, 5]); s-curve `(1-k)v + k·v³` (k ∈ [0, 1]) | monotonic, `f(±1)=±1` |
| 7 gain | `v · per_axis_gain[i] · (translation_gain | rotation_gain) · master_gain` | each gain ∈ [0, 10] |
| 8 smoothing | one-pole low-pass, `α = 1 − exp(−dt/τ)`, τ = `smoothing_time_ms` | τ ∈ [0, 1000] ms; filter resets when `dt > 250 ms` or time goes backwards (suspend/resume); output snaps to 0 below 1e-3 so it never drifts |
| 9 modes | `translation_enabled`, `rotation_enabled`, `per_axis_enabled`, `dominant_axis` (keep only the largest |v|) | |
| 10 clamp | `clamp(v, -8, 8)` | protects consumers from runaway gains |

Timing uses `CLOCK_MONOTONIC` (`sm_core::monotonic_us`). While the
smoothing filter is decaying and the device is silent, the broker ticks the
pipeline every 8 ms so motion still returns to exactly zero.

## Never-stuck guarantees (FR-208)

* **evdev:** the kernel drops `EV_REL` events whose value is 0, so a final
  "return to centre" report may produce nothing (measured: a 136 ms gap after
  RY = −45 with no event). The assembler zero-fills the other axes of the
  same HID report group and a 64 ms watchdog zeroes a group that stopped
  reporting. Replaying all three captures (`3dmousectl diagnostics replay`)
  ends neutral with 14–44 watchdog resets each.
* **hidraw:** the device sends an explicit neutral report; a 500 ms
  watchdog covers a lost final report.
* **disconnect:** held buttons are released and a zero motion event is sent
  before the device is dropped (e2e test `motion_buttons_watchdog_and_unplug`).
