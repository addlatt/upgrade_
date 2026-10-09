#!/usr/bin/env bash
# Build UPGRADE.exe (the window in front of the kit's scripts) from WSL/Linux.
#
# Windows Rust target: x86_64-pc-windows-gnu, linked by zig through
# cargo-zigbuild, all in the user's home, no system packages:
#   rustup target add x86_64-pc-windows-gnu
#   zig 0.13.0 at ~/.local/opt/zig-linux-x86_64-0.13.0 (ziglang.org,
#     sha256 d45312e61ebcc48032b77bc4cf7fd6915c11fa16e4aad116b66c9468211230ea)
#   cargo install --locked cargo-zigbuild
# The tests (the flow logic, rule #5's logic level) run on Linux first.
# Prints the path of the built .exe on success.
set -euo pipefail
cd "$(dirname "$0")"
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
command -v zig >/dev/null || { echo "build.sh: zig not found (see the header of this file)" >&2; exit 1; }
command -v cargo-zigbuild >/dev/null || { echo "build.sh: cargo-zigbuild not found (see the header of this file)" >&2; exit 1; }
ROOT="$(cd ../../.. && pwd)"
# the same bytes from any checkout (RISKS R14): see build-rust.sh
export RUSTFLAGS="--remap-path-prefix=$ROOT=/upgrade_ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo ${RUSTFLAGS:-}"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || echo 0)}"   # the PE timestamp (see build-rust.sh)
cargo test --locked --quiet >/dev/null 2>&1 || { echo "build.sh: the window's tests fail (cargo test)" >&2; exit 1; }
cargo zigbuild --locked --release --quiet --target x86_64-pc-windows-gnu
echo "$PWD/target/x86_64-pc-windows-gnu/release/UPGRADE.exe"
