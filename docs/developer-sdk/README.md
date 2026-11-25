# Developer reference: IPC protocol (v1)

Both sockets speak newline-delimited JSON (max 1 MiB per line). Requests:

```json
{"v": 1, "id": 7, "cmd": "status", ...arguments}
```

Replies echo `id`: `{"v":1,"id":7,"ok":true,"result":{…}}` or
`{"v":1,"id":7,"ok":false,"error":{"code":"denied","message":"…"}}`.
Events have no `id`: `{"v":1,"event":"motion",…}`. A request without
`"v": 1` is rejected with code `version`. Arguments never override `v`,
`id` or `cmd`.

## Broker — `/run/3dmouse/broker.sock`

Any local user may connect. Commands marked **control** require the peer
(`SO_PEERCRED`) to be root or the uid of the active seat0 session
(`/run/systemd/seats/seat0`, `ACTIVE_UID`); without a logind seat only the
broker's own uid is accepted.

| Command | Args | Notes |
|---|---|---|
| `hello` | – | `{daemon, version, proto, uid, can_control}` |
| `status` | – | `BrokerStatus` (`crates/sm-ipc/src/lib.rs`) |
| `registry` | – | the embedded device registry |
| `subscribe` | `motion` bool, `max_hz` 1–1000 (default 60), `buttons` bool, `devices` bool | events: `motion {dev, raw[6], norm[6], spnav[6], t}`, `button {dev, button, pressed, t}`, `device {op, …}`, `status` |
| `set_config` **control** | `nav` NavConfig, `app_mode`, `spnav_buttons` {"devbtn": spnavbtn}, `profile` | every field is optional; a supplied `nav`, `app_mode` or `spnav_buttons` that is not understood (including `null`) is rejected with `invalid` and the active profile is kept; the sender becomes the config owner; owner disconnect reverts |
| `clear_config` **control** | – | built-in defaults; only the current config owner may clear, anyone else gets `not_owner` |
| `set_session` **control** | `locked` bool | while locked `inject` is refused and keys are released |
| `inject` **control** | one of `keys` [codes] (+ `down` bool, omitted = tap), `scroll {v, h}` (1/120 notch units), `rel {code, value}` | key codes limited to the shipped key table + mouse buttons; 2000 events/s/connection |
| `release_all` **control** | – | release every key this connection holds |
| `set_mode` **control** | `mode`: `auto` (any controller) or a forced mode (root only) | re-negotiate ownership |

## Agent — `$XDG_RUNTIME_DIR/3dmouse/agent.sock`

Directory 0700, socket 0600, peer uid must equal the agent's uid.

| Command | Args |
|---|---|
| `hello`, `status`, `diagnostics` | – |
| `subscribe` | `live` (motion/button events ≤ 30 Hz), `events` (profile/device/lock/focus/osd) |
| `profiles.list` / `profiles.get` / `profiles.delete` / `profiles.export` | `profile` id |
| `profiles.save` | `profile` (full Profile object, validated) |
| `profiles.activate` | `profile` id or null (automatic), `persist` bool |
| `profiles.import` | `text` (TOML/JSON), `apply` bool (false = dry run), `overwrite` bool → `{preview, applied}`; exec actions always arrive unconfirmed |
| `profiles.backup`, `profiles.reload`, `profiles.reset_presets` | – |
| `settings.get` / `settings.set` | `settings` partial object |
| `preview_nav` | `nav` – apply without saving (GUI live tuning) |
| `test_action` | `action` – run once |
| `focus.report` | `app_id`, `title`, `pid` – used by the GNOME Shell extension |
| `broker` | `broker_cmd`: `status` or `registry` (read-only proxy) |

## libspnav compatibility

`/run/spnav.sock` implements the spacenavd AF_UNIX protocol: v0 (bare
8×int32 events) and v1 (protocol upgrade, client name, event mask, device
queries, config getters). Config **setters** from clients are refused (−1):
settings belong to profiles, not to whichever application wrote last.
Verified with the real libspnav 1.2 client (`tests/integration/c/spnav_probe.c`).
X11 ClientMessage events (the pre-socket magellan path) are not provided.

## Profiles

TOML in `~/.config/3dmouse-linux/profiles/<id>.toml` (schema
`crates/sm-profile/src/lib.rs`, `schema_version = 1`). Unknown fields
survive load/save. Writes are atomic (temp file + rename, `flock`, `.bak`).
