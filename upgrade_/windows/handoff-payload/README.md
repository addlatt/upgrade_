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

From a Fedora system or the netinst image, take the signed chain:

```
<stick>\
  EFI\BOOT\BOOTX64.EFI      <- shimx64.efi, renamed
  EFI\BOOT\grubx64.efi      <- Fedora's grubx64.efi (shim loads this next)
  EFI\BOOT\grub.cfg         <- one line:  reboot
```

`shim` verifies `grub`, `grub` reads `grub.cfg`, `grub.cfg` says `reboot`. If
you get to a reboot with Secure Boot on, the signed handoff works — and the
same chain is what V1 will boot into Anaconda instead of rebooting.

> The shim payload does not write `fired.txt` (GRUB rebooting immediately has
> no easy way to). For the shim runs, "fired" = you observed the reboot;
> record it in the `-Check` prompts. The Shell payload is the one that
> self-records, so use it for the unattended matrix runs.

## Formatting the stick

Windows, elevated:

```powershell
# find the disk number first with: Get-Disk
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
