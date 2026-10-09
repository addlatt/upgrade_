#!/usr/bin/env bash
# Build the Rust Windows binaries the kit carries (RISKS R32, the cut-over):
# upgrade-scan.exe, upgrade-harvest.exe, upgrade-job.exe, upgrade-prologue.exe.
# Each crate's tests run first on this side; then cargo zigbuild makes the
# Windows build (x86_64-pc-windows-gnu, linked by zig; the same toolchain as
# upgrade_/windows/window/build.sh). Prints one line per binary: its path.
# `build-rust.sh --toolchain` prints the toolchain line the kit manifest carries.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
command -v zig >/dev/null || { echo "build-rust.sh: zig not found (see upgrade_/windows/window/build.sh)" >&2; exit 1; }
command -v cargo-zigbuild >/dev/null || { echo "build-rust.sh: cargo-zigbuild not found" >&2; exit 1; }
# the same bytes from any checkout (RISKS R14): the source and registry paths that
# rustc would write into the binary are remapped to fixed names; the toolchain is
# pinned by rust-toolchain.toml at the root; Cargo.lock is honoured (--locked)
export RUSTFLAGS="--remap-path-prefix=$ROOT=/upgrade_ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo ${RUSTFLAGS:-}"
# the one byte that still differed between two clean builds was the PE header's link
# timestamp (rebuild-check, 2026-10-09); zig's linker takes it from SOURCE_DATE_EPOCH, so
# it is the commit's own time (or 0 outside git)
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || echo 0)}"
if [ "${1:-}" = --toolchain ]; then
    echo "rustc $(rustc --version | cut -d' ' -f2-) ($(cat "$ROOT/rust-toolchain.toml" | grep channel | cut -d'"' -f2) pinned); $(cargo-zigbuild --version); zig $(zig version); target x86_64-pc-windows-gnu; cargo --locked; paths remapped"
    exit 0
fi
for crate in evaluate/scan evaluate/harvest evaluate/job upgrade_/prologue; do
    name=$(basename "$crate"); bin="upgrade-$name"
    (cd "$ROOT/$crate" && cargo test --locked --quiet >/dev/null 2>&1) || { echo "build-rust.sh: tests fail in $crate" >&2; exit 1; }
    (cd "$ROOT/$crate" && cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu) || { echo "build-rust.sh: $crate does not build for Windows" >&2; exit 1; }
    echo "$ROOT/$crate/target/x86_64-pc-windows-gnu/release/$bin.exe"
done
