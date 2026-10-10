#!/usr/bin/env bash
# Build the harvester's Windows-only tests (tests/filesystem.rs) from WSL
# and run them on this Windows machine, from a Windows folder (an .exe run
# from \\wsl.localhost does not start reliably). Prints the test output.
set -euo pipefail
cd "$(dirname "$0")"
ZIG_DIR="$HOME/.local/opt/zig-linux-x86_64-0.13.0"
[ -x "$ZIG_DIR/zig" ] && export PATH="$ZIG_DIR:$PATH"
cargo zigbuild --tests --target x86_64-pc-windows-gnu --quiet
exe="$(ls -t target/x86_64-pc-windows-gnu/debug/deps/filesystem-*.exe | head -1)"
win_tmp="$(powershell.exe -NoProfile -Command '$env:TEMP' | tr -d '\r')"
mkdir -p "$(wslpath -u "$win_tmp")/upgrade-harvest-tests"
cp "$exe" "$(wslpath -u "$win_tmp")/upgrade-harvest-tests/filesystem.exe"
powershell.exe -NoProfile -Command "& '$win_tmp\\upgrade-harvest-tests\\filesystem.exe' --test-threads=1" | tr -d '\r'
