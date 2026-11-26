# The handoff payload: what the stick starts with

This folder holds the stick's own boot files and its one starter:

| File | What it is |
|---|---|
| `UPGRADE.cmd` | the starter a person double-clicks; it starts `UPGRADE.exe`, the window |
| `README-STICK.txt` | the step-by-step for whoever has the machine |
| `grub.cfg`, `grubenv` | the menu Fedora's signed GRUB reads from the stick, and its 1024-byte record block |
| `grub-ubuntu.cfg` | the same menu for a stick that carries Ubuntu (Ubuntu's GRUB reads it from `boot/grub/`) |
| `ks-reclaim.cfg` | the installer recipe reclaim starts (RISKS R41); it installs nothing |
| `startup.nsh` | the UEFI Shell's script, for the unsigned test payload |

The payload is the small EFI program the boot handoff points at. (An EFI
program is what UEFI firmware runs at power-on, before any operating
system.) Its first job is to turn "did the firmware run our entry?" into a
fact a machine can read: it records that it ran, on the stick. That was all
it did for gate V0, like a tripwire. Since V1 the signed payload then goes
on, by a marker file on the stick: `upgrade_/boot-verify` starts the
installer to check and change nothing, `upgrade_/boot-install` starts the
conversion, `upgrade_/boot-reclaim` (on a stick that has converted the
computer) starts reclaim, and with none of them it restarts back into
Windows. `grub.cfg`'s own header says exactly which.

There are two payloads, because V0 tests both a signed and an unsigned path:

| Payload | `BOOTX64.EFI` is | Tests |
|---|---|---|
| **UEFI Shell** (unsigned) | the EDK2 UEFI Shell | Secure Boot **off**; and the `SecureBootUnsigned` fail-mode (must be refused with Secure Boot on). It only writes `fired.txt` and restarts |
| **Signed shim** | the release's own `shimx64.efi` | Secure Boot **on**: the product's path |

The binaries may be redistributed, but they are **build inputs, not
source**. They are gitignored, not committed. Fetch them as below.

## UEFI Shell payload

- From EDK2 releases: <https://github.com/tianocore/edk2/releases>, the
  `ShellBinPkg` / `Shell.efi` (X64).
- Or from a Linux box: the `edk2-shell` package ships
  `/usr/share/edk2/x64/Shell.efi` (Fedora) or
  `/usr/share/edk2-shell/x64/Shell.efi` (Debian/Ubuntu).

Where it sits on the kit stick:

```
<stick>\
  EFI\SHELL\SHELLX64.EFI    <- Shell.efi
  startup.nsh               <- this folder's startup.nsh, copied to the root
```

The shell runs `startup.nsh` by itself. `upgrade-prologue verify-arm
--payload shell` points the one-time entry at it.

## The signed shim payload (for Secure Boot on)

The signed chain comes from the release the kit carries
(`data/releases.json`), fetched and checked against that table by
`rig/vm/fetch-release.sh`, or by `upgrade-fetch.exe` on the person's own
stick. For Fedora it must be **the install-media build of grub**. The
binary in the plain `grub2-efi-x64` RPM has the prefix `/EFI/fedora` and
never reads `EFI\BOOT\grub.cfg`.

```
<stick>\
  EFI\BOOT\BOOTX64.EFI      <- shimx64.efi, renamed
  EFI\BOOT\grubx64.efi      <- the release's grubx64.efi (shim loads this next)
  EFI\BOOT\grub.cfg         <- this folder's grub.cfg
  EFI\BOOT\grubenv          <- this folder's grubenv (a clean 1024-byte block)
```

The chain: `shim` checks `grub`'s signature, `grub` reads `grub.cfg`, and
`grub.cfg` records the firing first. Since 2026-09-07 **the shim payload
records itself too**: `grub.cfg` does `set upg_fired=1; save_env
upg_fired`, which rewrites `EFI\BOOT\grubenv` in place (the one write GRUB
can do on FAT). `upgrade-prologue verify-check` reads that block the same
way it reads `fired.txt` for the Shell payload. `verify-arm` resets the
block to clean first, so an old record cannot fake a pass. If `save_env` is
ever refused, the restart still happens and the row is `ignored`: it fails
safe, never a false pass. If you reach a self-recorded restart with Secure
Boot on, the signed handoff works.

> `grubenv` must stay exactly 1024 bytes with GRUB's header line. It is
> committed as a build input with `-text` in `.gitattributes`, so no
> checkout changes its line endings. `make-kit.sh` refuses a block that is
> not 1024 bytes with the header, and the prologue's tests pin the marker
> parse.

## The kit: what actually goes on a stick

Do not put a stick together by hand. `./make-kit.sh` (repo root) lays out
one complete stick folder, `dist/kit/stick/`. It carries:

- both payloads, as above;
- `UPGRADE.exe`, `UPGRADE.cmd` and the Windows programs the window runs;
- the release's installer and desktop images (`--release none` leaves them
  out: the window's picker fetches the Linux the person chooses);
- `upgrade_/`: the installer's program, `ks-reclaim.cfg`, and settle-in
  with its service, window and desktop entries;
- the diagnostics' starters (`evaluate/windows/usb-kit/`);
- `README-STICK.txt`, `SHA256SUMS` and a `KIT-MANIFEST.txt` naming the
  commit, the versions and each program's sha256.

Before it writes anything, it checks: the tree is committed, the Rust
programs' tests pass, the one PowerShell holdover's self-test passes (the
old stick writer, which is not shipped), the starters' `if` blocks carry no
unescaped parenthesis, the payload bits are the ones fetched, the menus
carry every branch, and `grubenv` is well-formed. `--to <directory>` copies
the layout and re-checks every file where it lands. `make-kit.sh` never
writes to a device.

## Writing the stick

`upgrade-stick.exe` (`evaluate/stick`; `make-kit.sh` puts it at
`dist/kit/upgrade-stick.exe`, beside the stick's folder and never on it)
partitions, formats, copies and reads back, after refusing every disk that
is not the one USB stick pointed at (RISKS R16). The boot volume is FAT32
with the label `UPGV0`: UEFI firmware is only guaranteed to read FAT when
it boots, and the menus find the stick by that label.

The starter runs the window from the folder it sits in (`%~dp0`), so there
is no drive letter to type and no other device to point at. Each handoff row's
notes start with a prefix the program writes itself, `[harness: os=…;
bitlocker-via=…; fired-via=…]`, so the machine's edition and how the marker
was read are in the row whoever ran it.

## In a VM (Phase A)

Hyper-V Gen 2 can't easily pass a physical USB through. Instead, attach a
small second VHDX, and inside the guest format it FAT32 and lay out the
payload exactly as above. To `bcdedit`, a FAT32 partition with a drive
letter is a FAT32 partition with a drive letter, so the mechanism under
test behaves the same. For QEMU+OVMF, attach the FAT image as a USB drive
(`-drive if=none,format=raw,file=stick.img` + `-device usb-storage`).
