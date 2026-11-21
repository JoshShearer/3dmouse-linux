# FreeCAD

FreeCAD on Linux reads 3D mice through libspnav. See the FreeCAD documentation:
https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/3Dconnexion_input_devices.md

- **Status: not tested** with 3dmouse-linux, because FreeCAD wasn't installed on the test host.
  Because 3dmoused serves the same socket protocol as spacenavd, it is expected to work.
- The `freecad` preset matches `freecad`, `FreeCAD` and `org.freecad.FreeCAD` (Native 6-DOF).
- FreeCAD settings: Tools → Customize → Spaceball Motion and Spaceball Buttons.
- Flatpak FreeCAD: `flatpak override --user --filesystem=/run/spnav.sock org.freecad.FreeCAD`.

If you test it, please follow `docs/compatibility/application-matrix.md` → "Test procedure" and
report the result.
