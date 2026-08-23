#!/usr/bin/env bash
#
# Build a patched QEMU 8.2.2 whose pci-testdev impersonates an Intel VMD
# controller: vendor 8086, device 9a0b (Tiger Lake - the ID Windows names
# "Intel RST VMD Controller 9A0B"), class 0104 (mass storage / RAID).
# Windows then builds CompatibleIDs PCI\CC_010400 / PCI\CC_0104 itself, so a
# guest boot with `-device pci-testdev` drives the scanner's storage check
# through the real PnP -> WMI -> parsing pipeline.
#
# SPOOF ONLY: this exercises the scanner's plumbing (CLAUDE.md rule #5,
# level 3). It is never evidence that real RST hardware presents these IDs.
#
# The edit is applied with verified sed (each pattern must match exactly
# once) rather than a context diff pinned to a guessed upstream layout; the
# resulting diff is saved to patches/vmd-spoof.patch for the record.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

sudo apt-get install -y build-essential ninja-build pkg-config python3-venv \
    libglib2.0-dev libpixman-1-dev libslirp-dev libgtk-3-dev zlib1g-dev

cd artifacts
if [ ! -d qemu-8.2.2 ]; then
    [ -f qemu-8.2.2.tar.xz ] || curl -LO https://download.qemu.org/qemu-8.2.2.tar.xz
    tar xf qemu-8.2.2.tar.xz
fi
cd qemu-8.2.2

F=hw/misc/pci-testdev.c
if ! grep -q '0x9a0b' "$F"; then
    cp "$F" "$F.orig"
    require_once() {
        local n
        n=$(grep -c "$1" "$F")
        [ "$n" = 1 ] || { echo "build-qemu-vmd: pattern '$1' matched $n times in $F - refusing to patch blind" >&2; exit 1; }
    }
    require_once 'k->vendor_id = PCI_VENDOR_ID_REDHAT;'
    require_once 'k->device_id = PCI_DEVICE_ID_REDHAT_TEST;'
    require_once 'k->class_id = PCI_CLASS_OTHERS;'
    sed -i \
        -e 's/k->vendor_id = PCI_VENDOR_ID_REDHAT;/k->vendor_id = 0x8086; \/* upgrade_ rig: spoof Intel VMD *\//' \
        -e 's/k->device_id = PCI_DEVICE_ID_REDHAT_TEST;/k->device_id = 0x9a0b;\n    k->subsystem_vendor_id = 0x8086;\n    k->subsystem_id = 0x0000;/' \
        -e 's/k->class_id = PCI_CLASS_OTHERS;/k->class_id = 0x0104; \/* mass storage \/ RAID -> CC_010400 *\//' \
        "$F"
    diff -u "$F.orig" "$F" > ../../patches/vmd-spoof.patch || true
    echo "build-qemu-vmd: patched $F (diff saved to patches/vmd-spoof.patch)"
fi

if [ ! -f build/build.ninja ]; then
    ./configure --target-list=x86_64-softmmu --enable-kvm --enable-gtk \
                --enable-slirp --disable-docs --disable-werror
fi
make -j"$(nproc)"

echo
echo "build-qemu-vmd: smoke check"
./build/qemu-system-x86_64 -device help 2>/dev/null | grep -q pci-testdev \
    && echo "  pci-testdev present in patched binary" \
    || { echo "  pci-testdev MISSING" >&2; exit 1; }
echo "build-qemu-vmd: done -> artifacts/qemu-8.2.2/build/qemu-system-x86_64"
