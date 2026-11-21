# ADR-0010: Profile storage and selection

- Status: accepted (2026-11-10)
- Requirements: FR-401..FR-407, FR-809, SRS §5

## Decision
- **Storage:** one TOML file per profile in `$XDG_CONFIG_HOME/3dmouse-linux/profiles/<id>.toml`,
  plus `settings.toml`. Every file has `schema_version`.
- **Writes are atomic:** write a temp file in the same directory, fsync, rename, then fsync the
  directory, all under an advisory `flock`. The previous
  version is kept as `<id>.toml.bak`. Files are mode 0600.
- **Corrupt files** are reported in `status.agent.load_errors`. They are never overwritten, and the
  remaining profiles still load.
- **Presets** (default, desktop, Blender, FreeCAD, KiCad, OpenSCAD, PrusaSlicer, browser-Onshape)
  are seeded on first run. `profiles.reset_presets` restores them without touching custom profiles.
- **Selection precedence** (deterministic, `sm_profile::select`):
  1. manual choice (a session choice from the GUI, CLI or a button, or a persisted pin)
  2. an application profile that matches both the focused app and the device
  3. an application profile that matches the app (app id, then title substring)
  4. a device profile
  5. `default`
- **Import** is untrusted: it is size-limited, schema-validated, and shows a preview of what would
  be overwritten. `confirmed` is cleared on every exec action.
- **Export** is plain TOML or JSON.

## Consequences
- Profiles are human-editable and diffable, and need no account or network (FR-808).
- Window titles are matched in memory only. They are never stored in logs or bundles.
