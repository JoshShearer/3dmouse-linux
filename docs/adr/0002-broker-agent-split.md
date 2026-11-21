# ADR-0002: Privileged broker and unprivileged session agent

- Status: accepted (2026-11-10)
- Requirements: FR-804, FR-401, FR-506, SRS §4.1, §4.6, §8

## Context
Two kinds of work have conflicting needs:
- **Device access** needs `/dev/hidraw*`, `/dev/input/event*` and `/dev/uinput`. There is one device
  per machine, it is shared by all users' native apps, and it must survive logouts.
- **User behaviour** covers profiles in `~/.config`, focused-window tracking, lock state, notifications
  and launching applications. All of it is per user and per session, and it must never run as root.

## Decision
- **`3dmoused`** is a system service that owns devices, the libspnav socket, the axis pipeline and
  uinput. It holds no user files. It accepts configuration only from an authorised control client:
  root, the active seat0 user, or the service's own uid when there is no seat.
- **`3dmouse-agent`** is a `systemd --user` service bound to `graphical-session.target`. It
  selects profiles from focus, runs the button engine and macros, and asks the broker to inject input.
- The broker tracks which connection owns the active configuration and which keys each connection
  pressed. When the owner disconnects, or the active seat user changes, the broker releases those
  keys and reverts to `builtin-default`.
- When the agent reports a locked session (`set_session {locked:true}`), the broker refuses `inject`.

## Alternatives rejected
- *One root daemon that reads user configs*: root would parse untrusted user files and run user
  actions.
- *A per-user daemon with device ACLs only*: `uaccess` covers hidraw, but `/dev/uinput` would have
  to be opened to users (an input-injection primitive), and two logged-in users would race for the
  device.

## Consequences
- A crash or kill of the agent cannot leave keys stuck. This is tested in
  `control_socket_config_routing_and_owner_revert`.
- Native 6-DOF apps keep working without the agent, using builtin defaults.
