#!/usr/bin/env bash
# Build upgrade-gate.exe, the gate on a "Go back to Windows" stick (RISKS R33),
# the same way as UPGRADE.exe (upgrade_/windows/window/build.sh: zig 0.13.0 +
# cargo-zigbuild, x86_64-pc-windows-gnu). Its tests (the gate's decisions,
# rule #5's logic level) run on Linux first. Prints the .exe's path.
set -euo pipefail
cd "$(dirname "$0")"
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
command -v zig >/dev/null || { echo "build.sh: zig not found (see upgrade_/windows/window/build.sh)" >&2; exit 1; }
command -v cargo-zigbuild >/dev/null || { echo "build.sh: cargo-zigbuild not found" >&2; exit 1; }
cargo test --quiet >/dev/null 2>&1 || { echo "build.sh: the gate's tests fail (cargo test)" >&2; exit 1; }
cargo zigbuild --release --quiet --target x86_64-pc-windows-gnu
echo "$PWD/target/x86_64-pc-windows-gnu/release/upgrade-gate.exe"
