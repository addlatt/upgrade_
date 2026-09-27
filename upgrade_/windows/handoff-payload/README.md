# V0 handoff payload: building the stick

The payload is the small, instrumented EFI program the boot handoff points
at. (An EFI program is what UEFI firmware runs at power-on, before any
operating system.) It is **not** a Linux installer; that is V1. Its only
job is to turn "did the firmware run our entry?" into a fact a machine can
read. It boots, writes `fired.txt` to the stick, and restarts back into
Windows. Think of it as a tripwire: it proves someone walked through the
door, and nothing more.

There are two payloads, because V0 tests both a signed and an unsigned path:

| Payload | `BOOTX64.EFI` is | Tests |
|---|---|---|
| **UEFI Shell** (unsigned) | the EDK2 UEFI Shell | Secure Boot **off**; and the `SecureBootUnsigned` fail-mode (must be refused with Secure Boot on) |
| **Fedora shim** (signed) | Fedora's `shimx64.efi` | Secure Boot **on**: the bridge to V1 |

The binaries may be redistributed, but they are **build inputs, not
source**. They are gitignored, not committed. Fetch them as below.

## UEFI Shell payload

- From EDK2 releases: <https://github.com/tianocore/edk2/releases>, the
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

The shell runs `startup.nsh` by itself from the root of the volume it booted
from.

## Fedora shim payload (signed, for Secure Boot on)

From the Fedora netinst image, take the signed chain. It must be **the
install-media build of grub**. The binary in the plain `grub2-efi-x64` RPM
has the prefix `/EFI/fedora` and never reads `EFI\BOOT\grub.cfg`.
`rig/vm/fetch-payload-bits.sh` extracts the right pair.

```
<stick>\
  EFI\BOOT\BOOTX64.EFI      <- shimx64.efi, renamed
  EFI\BOOT\grubx64.efi      <- Fedora's grubx64.efi (shim loads this next)
  EFI\BOOT\grub.cfg         <- this folder's grub.cfg
  EFI\BOOT\grubenv          <- this folder's grubenv (a clean 1024-byte block)
```

The chain: `shim` checks `grub`'s signature, `grub` reads `grub.cfg`, and
`grub.cfg` records the firing, then restarts. Since harness 0.2.0
(2026-09-07) **the shim payload records itself too**: `grub.cfg` does
`set upg_fired=1; save_env upg_fired`, which rewrites `EFI\BOOT\grubenv` in
place (the one write GRUB can do on FAT). `Test-Handoff.ps1 -Check` reads
that block the same way it reads `fired.txt` for the Shell payload. `-Arm`
resets the block to clean first, so an old record cannot fake a pass. If
`save_env` is ever refused, the restart still happens and the row is
`ignored`: it fails safe, never a false pass. If you reach a self-recorded
restart with Secure Boot on, the signed handoff works. The same chain is
what V1 boots into Anaconda (Fedora's installer) instead of restarting.

> `grubenv` must stay exactly 1024 bytes with GRUB's header line. It is
> committed as a build input with `-text` in `.gitattributes`, so no
> checkout changes its line endings. The harness's `-SelfTest` pins the
> block format and the marker parse.

## The kit: what actually goes on a stick

Do not put a stick together by hand. `./make-kit.sh` (repo root) lays out
two complete stick folders under `dist/kit/`: `stick-shell/` and
`stick-shim/`. Each one carries:

- the payload and `Test-Handoff.ps1`;
- the one-click launchers (`ARM-HANDOFF.cmd`, `CHECK-HANDOFF.cmd`, and the
  scanner's `RUN-SCANNER.cmd` with the single-file scanner);
- `README-STICK.txt` (the step-by-step for whoever has the machine);
- `SHA256SUMS` and a `KIT-MANIFEST.txt` naming the commit, the harness and
  scanner versions and the payload bits' checksums.

Before it writes anything, it checks: the tree is committed, `dist/`
rebuilds the same from source (R9), all three self-tests pass on Windows
PowerShell 5.1, the shipped scripts parse under the 5.1 parser, and
`grubenv` is well-formed. `--to <mounted-stick-root> --variant shell|shim`
copies one layout and re-checks every file where it lands. It never writes
to a device (R16): format the stick in Explorer (FAT32, label `UPGV0`) and
copy the folder's contents to its root.

The launchers use the drive they run from as the payload drive (`%~d0`), so
there is no drive letter to type and no other device to point at. Each
harness row's notes start with a prefix the harness writes itself,
`[harness: os=…; bitlocker-via=…; fired-via=…]`, so the machine's edition
and how the marker was read are in the row whoever ran it.

## Formatting the stick

Use Explorer if you can: right-click the stick → Format → FAT32, label
`UPGV0`. Explorer only offers volumes with a drive letter, which makes it
the safest picker there is until a real stick writer clears R16. If the
stick has an old partition table that Explorer will not format, then do
this in an elevated Windows PowerShell, and **only after `Get-Disk` shows
that the number is the stick and nothing else is removable**:

```powershell
# find the disk number first with: Get-Disk   (BusType USB, the expected size)
Clear-Disk -Number <n> -RemoveData -Confirm:$false
New-Partition -DiskNumber <n> -UseMaximumSize -AssignDriveLetter |
    Format-Volume -FileSystem FAT32 -NewFileSystemLabel UPGV0
```

FAT32 is required: UEFI firmware is only guaranteed to read FAT when it
boots.

## In a VM (Phase A)

Hyper-V Gen 2 can't easily pass a physical USB through. Instead, attach a
small second VHDX, and inside the guest format it FAT32 and lay out the
payload exactly as above. To `bcdedit`, a FAT32 partition with a drive
letter is a FAT32 partition with a drive letter, so the mechanism under
test behaves the same. For QEMU+OVMF, attach the FAT image as a USB drive
(`-drive if=none,format=raw,file=stick.img` + `-device usb-storage`).
