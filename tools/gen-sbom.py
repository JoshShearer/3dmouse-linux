#!/usr/bin/env python3
"""Write a CycloneDX 1.5 SBOM (JSON) for the release from Cargo.lock (FR-806).

usage: tools/gen-sbom.py [--output dist/3dmouse-linux.cdx.json]

Covers every Rust crate compiled into the binaries, with the crates.io SHA-256
from Cargo.lock. Qt 6, glibc and the other shared libraries come from the
distribution's packages; they are listed as external runtime dependencies,
without versions, because the distribution pins those.
"""
import json, sys, tomllib, uuid, datetime, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
out = ROOT / "dist" / "3dmouse-linux.cdx.json"
if "--output" in sys.argv:
    out = pathlib.Path(sys.argv[sys.argv.index("--output") + 1])

lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]

components, deps = [], []
for p in lock["package"]:
    ref = f"pkg:cargo/{p['name']}@{p['version']}"
    c = {"type": "library", "bom-ref": ref, "name": p["name"], "version": p["version"], "purl": ref}
    if p.get("source", "").startswith("registry+"):
        c["hashes"] = [{"alg": "SHA-256", "content": p["checksum"]}]
    else:
        c["type"] = "application" if p["name"] in ("sm-daemon", "sm-agent", "sm-cli") else "library"
        c["description"] = "part of 3dmouse-linux (this repository)"
    components.append(c)
    deps.append({"ref": ref, "dependsOn": [
        next(f"pkg:cargo/{q['name']}@{q['version']}" for q in lock["package"]
             if q["name"] == d.split(" ")[0] and (len(d.split(" ")) == 1 or q["version"] == d.split(" ")[1]))
        for d in p.get("dependencies", [])]})

for name, why in [("qt6-base", "GUI and overlay"), ("qt6-declarative", "QML"), ("layer-shell-qt", "Wayland overlay (optional)"),
                  ("glibc", "C library"), ("systemd", "service manager, udev")]:
    components.append({"type": "library", "bom-ref": f"system:{name}", "name": name, "scope": "required",
                       "description": f"provided by the distribution: {why}"})

bom = {
    "bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
    "serialNumber": f"urn:uuid:{uuid.uuid4()}",
    "metadata": {
        "timestamp": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "component": {"type": "application", "bom-ref": "3dmouse-linux", "name": "3dmouse-linux",
                      "version": version, "licenses": [{"license": {"id": "GPL-3.0-or-later"}}]},
    },
    "components": components,
    "dependencies": deps,
}
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(bom, indent=1) + "\n")
print(f"{out}: {len(components)} components")
