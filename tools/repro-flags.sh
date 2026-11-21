# Sourced by the package builds: strip build-machine paths from the Rust binaries so that
# builds from different directories or users are byte-identical (FR-806). Usage:
#   . tools/repro-flags.sh   (from the source root)
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$PWD=/build/3dmouse-linux --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo"
export RUSTFLAGS
