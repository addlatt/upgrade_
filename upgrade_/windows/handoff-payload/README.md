# V0 handoff payload — building the stick

The payload is the instrumented EFI program the boot handoff points at. It is
**not** a Linux installer — that is V1. Its only job is to make "did the
firmware run our entry" a machine-readable fact: it boots, writes `fired.txt`
to the stick, and reboots back to Windows.

Two payloads, because V0 tests both a signed and an unsigned path:

| Payload | `BOOTX64.EFI` is | Tests |
|---|---|---|
| **UEFI Shell** (unsigned) | the EDK2 UEFI Shell | Secure Boot **off**; and the `SecureBootUnsigned` fail-mode (must be refused with Secure Boot on) |
| **Fedora shim** (signed) | Fedora's `shimx64.efi` | Secure Boot **on** — the bridge to V1 |

The binaries are redistributable but are **build inputs, not source** — they
are gitignored, not committed. Fetch them:

## UEFI Shell payload

- From EDK2 releases: <https://github.com/tianocore/edk2/releases> — the
  `ShellBinPkg` / `Shell.efi` (X64).
- Or from a Linux box: the `edk2-shell` package ships
  `/usr/share/edk2/x64/Shell.efi` (Fedora) or
  `/usr/share/edk2-shell/x64/Shell.efi` (Debian/Ubuntu).

Build the stick (FAT32, single partition):

```
<stick>\
  EFI\BOOT\BOOTX64.EFI      <- Shell.efi, renamed
  startup.nsh               <- this folder's startup.nsh, copied to the root
```

The shell auto-runs `startup.nsh` from the root of the volume it booted from.

## Fedora shim payload (signed, for Secure Boot on)

From the Fedora netinst image, take the signed chain (**the install-media
build of grub** — the plain `grub2-efi-x64` RPM's binary has prefix
`/EFI/fedora` and never reads `EFI\BOOT\grub.cfg`; `rig/vm/fetch-payload-bits.sh`
extracts the right pair):

```
<stick>\
  EFI\BOOT\BOOTX64.EFI      <- shimx64.efi, renamed
  EFI\BOOT\grubx64.efi      <- Fedora's grubx64.efi (shim loads this next)
  EFI\BOOT\grub.cfg         <- this folder's grub.cfg
  EFI\BOOT\grubenv          <- this folder's grubenv (a clean 1024-byte block)
```

`shim` verifies `grub`, `grub` reads `grub.cfg`, and `grub.cfg` records the
firing then reboots. Since harness 0.2.0 (2026-09-07) **the shim payload
self-records too**: `grub.cfg` does `set upg_fired=1; save_env upg_fired`,
which rewrites `EFI\BOOT\grubenv` in place (the one write GRUB can do on
FAT), and `Test-Handoff.ps1 -Check` reads that block exactly as it reads
`fired.txt` for the Shell payload. `-Arm` resets the block to clean first, so
a stale record cannot fake a pass; if `save_env` is ever refused the reboot
still happens and the row is `ignored` — fail-safe, never a false pass. If you
get to a self-recorded reboot with Secure Boot on, the signed handoff works —
and the same chain is what V1 will boot into Anaconda instead of rebooting.

> `grubenv` must stay exactly 1024 bytes with GRUB's header line. It is
> committed as a build input with `-text` in `.gitattributes` so no checkout
> converts its line ending. The harness's `-SelfTest` pins the block format
> and the marker parse.

## The kit: what actually goes on a stick

Do not assemble a stick by hand. `./make-kit.sh` (repo root) lays out two
complete stick folders under `dist/kit/` — `stick-shell/` and `stick-shim/` —
each carrying the payload, `Test-Handoff.ps1`, the one-click launchers
(`ARM-HANDOFF.cmd`, `CHECK-HANDOFF.cmd`, and the scanner's `RUN-SCANNER.cmd`
with the single-file scanner), `README-STICK.txt` (the run-book for whoever
holds the machine), `SHA256SUMS` and a `KIT-MANIFEST.txt` naming the commit,
the harness and scanner versions and the payload bits' checksums. Before it
writes anything it verifies: tree committed, `dist/` reproduces from source
(R9), all three self-tests green on Windows PowerShell 5.1, shipped scripts
parse under the 5.1 parser, `grubenv` well-formed. `--to <mounted-stick-root>
--variant shell|shim` copies one layout and re-verifies every file at the
destination. It never writes to a device (R16): format the stick in Explorer
(FAT32, label `UPGV0`) and copy the folder's contents to its root.

The launchers use the drive they run from as the payload drive (`%~d0`), so
there is no drive letter to type and no other device to point at. Each
harness row's notes start with a harness-written `[harness: os=…;
bitlocker-via=…; fired-via=…]` prefix so the machine's edition and how the
marker was read are in the row independent of the operator.

## Formatting the stick

Prefer Explorer: right-click the stick → Format → FAT32, label `UPGV0`.
Explorer only offers lettered volumes, which is the safest picker there is
until a real stick writer clears R16. If the stick has a stale partition table
that Explorer will not format, then, Windows elevated and **only after
`Get-Disk` shows the number is the stick and nothing else is removable**:

```powershell
# find the disk number first with: Get-Disk   (BusType USB, the expected size)
Clear-Disk -Number <n> -RemoveData -Confirm:$false
New-Partition -DiskNumber <n> -UseMaximumSize -AssignDriveLetter |
    Format-Volume -FileSystem FAT32 -NewFileSystemLabel UPGV0
```

FAT32 is required — UEFI firmware is only guaranteed to read FAT for boot.

## In a VM (Phase A)

Hyper-V Gen 2 can't easily pass a physical USB through. Instead attach a small
second VHDX, and inside the guest format it FAT32 and lay out the payload
exactly as above. To `bcdedit`, a lettered FAT32 partition is a lettered FAT32
partition — the mechanism under test behaves the same. For QEMU+OVMF, attach
the FAT image as a USB drive (`-drive if=none,format=raw,file=stick.img` +
`-device usb-storage`).
