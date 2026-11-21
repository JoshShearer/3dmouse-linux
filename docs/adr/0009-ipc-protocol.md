# ADR-0009: Control IPC protocol

- Status: accepted (2026-11-10)
- Requirements: SRS §4.3, §8, FR-900

## Decision
- **Framing:** newline-delimited JSON objects over AF_UNIX stream sockets.
  - Requests are `{"v":1,"id":N,"cmd":"…", …args}`. Replies are `{"v":1,"id":N,"ok":true,"result":…}`
    or `{"ok":false,"error":{"code","message"}}`.
  - Events are `{"v":1,"event":"motion"|"button"|"device"|"status"|"profile"|"lock"|…}`.
- **Protocol fields win:** clients set `v`, `id` and `cmd` *after* merging args, so an argument
  named `id` can never corrupt correlation. Profile ids travel as `"profile"`.
- **Limits:**
  - 1 MiB per line (`sm_ipc::MAX_LINE`). A longer line closes the connection.
  - The GUI buffers at most 4 MiB.
  - libspnav clients get a 64 KiB write buffer each.
  - Motion subscriptions are rate-limited (`max_hz`, 1–1000).
  - Slow consumers lose intermediate motion, never buttons. A client that can't take a button event
    is disconnected.
- **Authentication:** `SO_PEERCRED` on every connection (ADR-0005).
- **Versioning:** `v` is checked. An unknown `cmd` returns `unknown_command`. New fields are
  additive.
- The full command reference is in `docs/developer-sdk/README.md`.

## Alternatives rejected
- *D-Bus*: heavier, needs policy files on the system bus, and is awkward for 60 Hz motion streams.
- *gRPC or HTTP*: would create a network-capable listener.
