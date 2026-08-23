#!/usr/bin/env bash
# Wrap autounattend.xml into a small ISO attached as a second cdrom - Windows
# Setup scans the root of every removable/CD volume for autounattend.xml.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
genisoimage -quiet -o artifacts/unattend.iso -V UNATTEND -J -r autounattend.xml
echo "make-unattend: wrote artifacts/unattend.iso"
