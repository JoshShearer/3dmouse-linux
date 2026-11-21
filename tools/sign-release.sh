#!/bin/sh
# Checksums, SBOM and detached OpenPGP signatures for release artifacts (FR-806).
#
# usage: SM_SIGNING_KEY=<key id or fingerprint> tools/sign-release.sh [dist]
#
# Writes into the directory:
#   SHA256SUMS, SHA256SUMS.asc, 3dmouse-linux.cdx.json(.asc), <artifact>.asc
# The release key is the maintainer's decision and never lives in this
# repository or in CI logs. Verify with:
#   gpg --verify SHA256SUMS.asc SHA256SUMS && sha256sum -c SHA256SUMS
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIST=${1:-$ROOT/dist}
KEY=${SM_SIGNING_KEY:?set SM_SIGNING_KEY to the release key id (gpg --list-secret-keys)}
cd "$DIST"
ls ./*.deb ./*.rpm ./*.pkg.tar.zst >/dev/null 2>&1 || { echo "no artifacts in $DIST" >&2; exit 1; }
python3 "$ROOT/tools/gen-sbom.py" --output "$DIST/3dmouse-linux.cdx.json"
rm -f SHA256SUMS ./*.asc
sha256sum -- $(ls ./*.deb ./*.rpm ./*.pkg.tar.zst 3dmouse-linux.cdx.json 2>/dev/null | sed 's|^\./||') > SHA256SUMS
for f in SHA256SUMS 3dmouse-linux.cdx.json $(ls ./*.deb ./*.rpm ./*.pkg.tar.zst 2>/dev/null); do
    gpg --batch --yes --local-user "$KEY" --armor --detach-sign --output "$f.asc" "$f"
done
gpg --batch --verify SHA256SUMS.asc SHA256SUMS
sha256sum -c --quiet SHA256SUMS
echo "signed $(wc -l < SHA256SUMS) files with $KEY"
