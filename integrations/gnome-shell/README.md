# GNOME Shell focus bridge

GNOME on Wayland does not let ordinary programs see which window is focused,
so per-application profiles need this small extension. It sends the focused
window's application id (and title, which the agent only *matches* against
`title_contains` selectors and never stores or logs) to the agent socket
`$XDG_RUNTIME_DIR/3dmouse/agent.sock`, which only your user can open.

| GNOME | Ubuntu | Directory |
|-------|--------|-----------|
| 42–44 | 22.04  | `gnome42/` |
| 45+   | 24.04+ | `gnome45/` |

Packages install the right variant to
`/usr/share/gnome-shell/extensions/3dmouse-linux@joshshearer.github.io`.
Enable it once per user (then log out and back in on Wayland):

    gnome-extensions enable 3dmouse-linux@joshshearer.github.io

`3dmousectl diagnostics check` reports "focus tracking" as failed on GNOME
until the extension has sent its first report.

Status: **untested on real GNOME** — the API calls are standard
(`global.display` `notify::focus-window`, `Gio.SocketClient`), but no GNOME
session was available while writing it. See
docs/compatibility/desktop-matrix.md.
