# ADR-0004: Desktop input delivery and focus on Wayland and X11

- Status: accepted (2026-11-10). Revisit when libei is available on the target desktops.
- Requirements: FR-401, FR-502, FR-503, FR-505, FR-506, FR-507

## Context
Wayland does not let clients inject input or query the focused window. The options are:

| Mechanism | Availability (2026-11) | Notes |
|---|---|---|
| `/dev/uinput` virtual device | every compositor | kernel-level. The compositor treats it as a real keyboard or mouse. Needs device access. |
| libei / RemoteDesktop portal | GNOME ≥ 45, KDE ≥ 6.1. Not Hyprland or Sway | needs a user-approval dialog per session, and an always-running portal session. |
| Compositor IPC (Hyprland `.socket2.sock`, Sway i3-IPC) | Hyprland, Sway | gives focus events, not injection. |
| GNOME Shell extension | GNOME | the only way to learn focus on GNOME Wayland. |
| X11 (`_NET_ACTIVE_WINDOW`, XTest) | X11 sessions | |

## Decision
- **Injection goes through uinput in the broker.** That covers key chords, mouse buttons and
  hi-res scroll. The virtual device advertises only the keys in `sm_actions::keys` plus mouse
  buttons and REL axes. **Alt+SysRq combinations are refused** in two places: chord parsing and the
  uinput writer.
- **Focus providers in the agent** (`sm_desktop::focus`):
  - Hyprland: socket2 events plus `j/activewindow`
  - Sway: i3-IPC subscription
  - X11: `xprop`
  - GNOME: the extension in `integrations/gnome-shell` sends `focus.report`
  - anything else: no focus, so the default profile applies, and the GUI says so
- **Lock detection** combines three signals: a known locker process, logind `LockedHint`, and on
  Hyprland, `LOCK` in `solitaryBlockedBy` from `j/monitors`. The Hyprland signal was added after
  physical testing showed that Omarchy's in-process Quickshell lock was otherwise invisible. Polling
  runs every 1 s. On Hyprland a conclusive `j/monitors` answer is authoritative, since every Wayland
  locker there uses ext-session-lock, and the process and logind checks are skipped. Those run only
  when the answer is undetermined, or on other desktops.
- **OSD** uses desktop notifications (`notify-send`) for now. No layer-shell overlay is drawn.

## Alternatives rejected
- *libei now*: unsupported on the primary test compositor (Hyprland), and it adds a consent dialog
  every session. It stays a future backend behind the same `inject` API.
- *ydotool or wtype*: an extra daemon or a virtual-keyboard protocol that isn't available everywhere.

## Consequences and disclosed limits
- uinput input is indistinguishable from real input. Protection while locked depends on lock
  detection. In the ≤ 1 s before detection, injected events reach the lock surface only.
- KDE focus tracking uses a KWin script that the agent loads over D-Bus
  (`org.kde.kwin.Scripting.loadScript`), which calls `ReportFocus` on the agent's session-bus name.
  It's tested against a headless KWin 6.8 (`tests/integration/kwin-focus-e2e.sh`). COSMIC focus
  tracking uses `ext_foreign_toplevel_list_v1` with `zcosmic_toplevel_info_v1` (cosmic-comp has no
  wlr-foreign-toplevel). It's tested against cosmic-comp 1.11.0 (`tests/integration/cosmic-focus-e2e.sh`).
- Overlays (FR-504) use wlr-layer-shell through LayerShellQt in a separate `3dmouse-osd` process.
  GNOME has no layer-shell, so there the overlay exits and notifications are used. Capability
  reporting (`status` → `osd_overlay`, `diagnostics check`) says which one is active.
