# ADR-0006: GUI architecture (Qt6/QML, separate process)

- Status: accepted (2026-11-10)
- Requirements: FR-700, SRS §4.1, §4.2

## Decision
- `3dmouse-config` is a C++17 + Qt 6 (≥ 6.2) Quick/QML application. It is built with CMake and
  uses the Fusion style by default.
- It talks **only** to the session agent, over its JSON-lines socket (`AgentClient`). It never opens
  devices or the broker socket.
- Long-running diagnostics are delegated to `3dmousectl` through `QProcess` (`runCtl`).
- `SM_GUI_SCREENSHOTS=<dir>` together with `QT_QPA_PLATFORM=offscreen` renders every page. This is
  used for docs and for smoke tests in package containers.

## Alternatives rejected
- *Rust bindings to Qt (cxx-qt)*: adds a large build dependency, and Ubuntu 22.04 ships Qt 6.2,
  which cxx-qt's current releases don't target.
- *GTK4/libadwaita*: the SRS names Qt6/QML.
- *A web UI*: the SRS forbids a browser framework as a hard dependency.

## Consequences
- The GUI can crash or be absent without affecting input.
- Qt 6.2 (Ubuntu 22.04) compatibility limits the QML features available. Pages are
  smoke-tested on Qt 6.2.4 (22.04 container) and Qt 6.12 (Arch).
