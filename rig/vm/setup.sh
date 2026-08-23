#!/usr/bin/env bash
#
# One-shot host prep for the VM rig. Idempotent - safe to re-run.
# Needs sudo for the package install and the kvm group add.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
    qemu-system-x86 ovmf swtpm swtpm-tools \
    mtools dosfstools parted genisoimage efi-shell-x64 samba libarchive-tools curl

mkdir -p artifacts/fw artifacts/tpm artifacts/payload-bits

# Per-profile UEFI vars copies. One Secure-Boot-capable CODE image serves all
# profiles; Secure Boot on/off is purely which VARS copy is attached.
#   vars-nosb.fd - no Platform Key enrolled -> Secure Boot off
#   vars-ms.fd   - Microsoft keys enrolled  -> Secure Boot on
#
# We use the NON-SMM CODE build (OVMF_CODE_4M.fd), not the .ms/.secboot one:
# the .secboot build is compiled SMM_REQUIRE, and SMM entry crashes KVM on
# this AMD-under-WSL2 host ("KVM: entry failed, hardware error 0xffffffff").
# The non-SMM build still ENFORCES Secure Boot image verification (done in
# DXE, not SMM); SMM only makes the authenticated varstore tamper-proof
# against a hostile guest - irrelevant in a throwaway test VM, and the
# physical matrix is where tamper-realistic Secure Boot is validated anyway.
cp -n /usr/share/OVMF/OVMF_CODE_4M.fd    artifacts/fw/OVMF_CODE_4M.fd
cp -n /usr/share/OVMF/OVMF_VARS_4M.fd    artifacts/fw/vars-nosb.fd
cp -n /usr/share/OVMF/OVMF_VARS_4M.ms.fd artifacts/fw/vars-ms.fd

# kvm access: usermod lands in /etc/group immediately, but this login shell
# only picks it up after a WSL restart. run-vm.sh falls back to `sg kvm`
# automatically, so a restart is optional.
if ! id -nG "$USER" | grep -qw kvm; then
    sudo usermod -aG kvm "$USER"
    echo "setup: added $USER to the kvm group."
fi
if [ -r /dev/kvm ] && [ -w /dev/kvm ]; then
    echo "setup: /dev/kvm accessible from this shell."
else
    echo "setup: /dev/kvm not accessible from this shell yet."
    echo "       Either restart WSL (from Windows: wsl.exe --shutdown) or let"
    echo "       run-vm.sh wrap qemu in 'sg kvm' automatically."
fi
echo "setup: done."
