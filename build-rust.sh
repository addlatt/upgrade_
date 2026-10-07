#!/usr/bin/env bash
# Build the Rust Windows binaries the kit carries (RISKS R32, the cut-over):
# upgrade-scan.exe, upgrade-harvest.exe, upgrade-job.exe, upgrade-prologue.exe.
# Each crate's tests run first on this side; then cargo zigbuild makes the
# Windows build (x86_64-pc-windows-gnu, linked by zig; the same toolchain as
# upgrade_/windows/window/build.sh). Prints one line per binary: its path.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
command -v zig >/dev/null || { echo "build-rust.sh: zig not found (see upgrade_/windows/window/build.sh)" >&2; exit 1; }
command -v cargo-zigbuild >/dev/null || { echo "build-rust.sh: cargo-zigbuild not found" >&2; exit 1; }
for crate in evaluate/scan evaluate/harvest evaluate/job upgrade_/prologue; do
    name=$(basename "$crate"); bin="upgrade-$name"
    (cd "$ROOT/$crate" && cargo test --locked --quiet >/dev/null 2>&1) || { echo "build-rust.sh: tests fail in $crate" >&2; exit 1; }
    (cd "$ROOT/$crate" && cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu) || { echo "build-rust.sh: $crate does not build for Windows" >&2; exit 1; }
    echo "$ROOT/$crate/target/x86_64-pc-windows-gnu/release/$bin.exe"
done
