# ADR-0007: Device model registry

- Status: accepted (2026-11-10)
- Requirements: SRS §2.2, FR-101, FR-301, §4.4, §7.2

## Decision
- There is one data file, `crates/sm-registry/data/devices.toml`, embedded at compile time.
- Each entry is keyed by **exact vid/pid and transport**. It records the name, family, spnav device
  type, axis range, the canonical frame transform, the button table (evdev code, HID bit, logical
  index, label), the **support status**, the evidence path and the data source.
- Status values follow SRS §2.2:
  - `unknown`: listed from upstream data only
  - `detected`, `axis_verified`, `buttons_verified`, `outputs_verified`, `full_verified`
  - `partial`, `blocked`
- A status above `detected` must point to a file in `docs/compatibility/evidence/`.
- The udev rules are generated from the registry, and a unit test (`udev_rules_match_registry`) fails
  if they drift.
- **Unknown 3Dconnexion devices** (vendor `256f`, not in the registry) are driven as a *generic
  candidate*, but only when their report descriptor exposes six axes. Their status is `unknown`, and
  `devices list` and the GUI say plainly that they're unverified. Interfaces without six axes, such as
  the keyboard and mouse interfaces of a receiver, are skipped.
- **Explicitly blocked** ids are ignored, with a reason in the status notes. Other vendors are never
  touched.

## Consequences
- Adding a model is a reviewable data change plus a descriptor fixture.
- Upstream spacenavd data (`usbdb`, `bnhack_*` at `05cc679`) is used for ids and button counts, but
  each entry stays `unknown` until it is physically tested.
