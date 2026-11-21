# ADR-0008: Packaging and lifecycle

- Status: accepted (2026-11-10)
- Requirements: FR-801..FR-807, SRS §9

## Decision
- There is one install layout, `packaging/install-tree.sh DESTDIR [gnome45|gnome42]`, shared by
  every package format.
- **Arch:** `packaging/arch/PKGBUILD` with a `.install` hook that *prints* enable instructions. It
  does not enable anything, following Arch convention.
- **Ubuntu:** `.deb` packages are built per release in a container (`tools/build-deb.sh 22.04
  24.04`), against that release's glibc and Qt.
  - Rust is pinned to 1.88.0 through rustup inside the build image, and the Cargo.lock is committed.
  - `Depends` comes from `dpkg-shlibdeps`, plus the QML modules and the Qt platform plugins
    (`qt6-qpa-plugins`, `qt6-wayland`).
  - The maintainer scripts follow Debian practice: `postinst` enables `3dmoused` and globally
    enables the user agent unit, reloads udev and loads `uinput`. `prerm` stops
    `3dmoused`, and `postrm` (on remove or purge) disables both units and reloads udev.
  - User profiles are never deleted by the package. Removing them is a documented manual step
    (`docs/install/uninstall.md`).
- spacenavd is never touched (ADR-0001).
- **Not done yet:** `.rpm`, signing, SBOM, CI-built artefacts with checksums. These are tracked in
  `docs/requirements-coverage.md`.

## Consequences
- Pop!_OS 22.04 uses the 22.04 package. Its Qt 6.2 is the floor for the GUI.
