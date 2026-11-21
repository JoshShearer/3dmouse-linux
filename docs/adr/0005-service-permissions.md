# ADR-0005: Service permissions and hardening

- Status: accepted (2026-11-10)
- Requirements: FR-804, FR-805, SRS §8, §9, NFR Security

## Decision
`3dmoused.service` runs as `User=root` but with **no capabilities**. Root is needed only so the
service can open `/dev/hidraw*`, `/dev/input/event*` and `/dev/uinput` without making those nodes
group-writable for everyone. The kernel's own checks are DAC (discretionary access control) checks,
which pass for uid 0.

Hardening settings:

| Directive | Value |
|---|---|
| `CapabilityBoundingSet=` / `AmbientCapabilities=` | empty |
| `NoNewPrivileges` | yes |
| `DevicePolicy=closed` + `DeviceAllow` | `char-hidraw r`, `char-input r`, `/dev/uinput rw` |
| `ProtectSystem=strict`, `ReadWritePaths=/run`, `RuntimeDirectory=3dmouse` | |
| `ProtectHome`, `PrivateTmp`, `PrivateNetwork`, `RestrictAddressFamilies=AF_UNIX` | no network at all |
| `SystemCallFilter=@system-service` | |
| `MemoryDenyWriteExecute`, `LockPersonality`, `RestrictRealtime`, `RestrictNamespaces` | |
| `Type=notify`, `WatchdogSec=20`, `Restart=on-failure` | |

- `systemd-analyze security 3dmoused` gave exposure **1.1 ("OK")** on the Arch test host.
- **Socket permissions:**
  - The broker control socket `/run/3dmouse/broker.sock` is mode 0666, but every request is
    authorised by `SO_PEERCRED`. Read-only commands (`status`, `subscribe`) are open to everyone.
    Control commands need root, the active seat user, or the service's own uid.
  - `/run/spnav.sock` is 0666, the same as spacenavd, so every user's apps can connect.
  - The agent socket is `$XDG_RUNTIME_DIR/3dmouse/agent.sock`, mode 0600, same uid only.
- **Exec actions** in profiles run only when settings say `allow_exec = true` and the action is
  `confirmed`. They are spawned with `setsid` and **no shell**. Imported profiles are validated and
  have `confirmed` cleared.

## Alternatives rejected
- *`User=3dmouse` + `SupplementaryGroups=input`*: the `input` group can read every keyboard on
  the system (a keylogger primitive). We would also need `/dev/uinput` group access.
- *`DynamicUser`*: same device-access problem.

## Consequences
- Device ACLs are not widened. The only change udev makes is a `uaccess` ACL on SpaceMouse hidraw
  nodes for the seat user.
