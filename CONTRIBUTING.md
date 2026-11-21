# Contributing

## Reporting bugs and devices
Use the issue templates. **Device reports are the most useful contribution right now**: only the
SpaceMouse Compact is verified, so a report for any other model helps, even if it just works.
Attach `3dmousectl diagnostics check` and, for devices, `3dmousectl devices inspect`.
Security problems go through the private advisory form (see [SECURITY.md](SECURITY.md)).

## Ground rules (from the SRS)
- **No support claim without evidence.** A registry status above `detected` needs a file in
  `docs/compatibility/evidence/` with the date, versions and the captures.
- **No speculative HID output reports** (LEDs, LCD, configuration), ever.
- **Never disable or remove spacenavd** or any other user software from code or package scripts.
- Don't suppress failing tests. Don't claim a test ran if it didn't.

## Development
```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 tools/gen-udev-rules.py --check   # rules must match the registry
cmake -S gui -B gui/build && cmake --build gui/build
QT_QPA_PLATFORM=offscreen SM_GUI_SCREENSHOTS=/tmp/shots gui/build/3dmouse-config
```
To run the stack unprivileged against a fake device, see `crates/sm-daemon/tests/broker_e2e.rs`. It
builds a fake sysfs with a FIFO as the device node.

## Adding a device
1. Capture the descriptor: `3dmousectl devices inspect <vid:pid> --json`, then save the hex to
   `tests/fixtures/descriptors/<vid>-<pid>-<transport>.hex`.
2. Add or adjust the entry in `crates/sm-registry/data/devices.toml`.
3. Run `python3 tools/gen-udev-rules.py` to regenerate the udev rules.
4. Run the guided test in `tests/hardware/README.md` and write the evidence file.
5. Only then raise the status.

## Changes to central types
IPC types (`sm-ipc`), the registry schema and the profile schema are shared contracts. Change them
in a dedicated PR, keep the change additive, and update `docs/developer-sdk/README.md`.

## Commit and PR
Small, reviewable commits. Each PR describes what was tested, and on which hardware and desktop.
