# Hyper-V Gen 2 rig — the Secure-Boot-and-TPM leg

The QEMU rig (`rig/vm/`) cannot enforce Secure Boot or present a TPM: both
live in the SMM-requiring OVMF build, and SMM crashes KVM on this AMD/WSL2
host (RISKS R15, R21). Hyper-V Generation 2 on the same Windows host has both
natively — real Secure Boot with a chosen certificate db, and a vTPM — so this
is where the rows the QEMU rig had to leave blocked get their VM leg:

| Gate | What this rig can fire | What it still cannot |
|---|---|---|
| **V0 / R15** rows 3–4 (Secure Boot refusal of an unsigned payload; signed shim accepted) and rows 5–6 (BitLocker suspended / `NoSuspend` → recovery prompt) | SB enforcement is real; vTPM lets BitLocker seal to PCRs | **The USB clause.** Gen 2 has no USB device emulation: the "stick" is a VHDX on the SCSI bus. Whether firmware honours a one-shot `bootsequence` for a *removable USB* entry stays with the physical matrix. |
| **V1b / R21** the Secure-Boot-on chainload (shim → GRUB → `bootmgfw.efi`), and the **~100 MiB ESP row** (this guest's `autounattend.xml` asks for Windows Setup's default 100 MB, not the QEMU guest's 260) | both | vendor firmware behaviour (entry deletion, order pinning) — Hyper-V's UEFI is one more firmware, not the population |
| **V1** unattended install to a login screen, Secure Boot on | yes | same |
| **V3 / R19** BITLK reads against TPM-sealed BitLocker volumes | vTPM makes the default protector shape testable | real-disk variants (used-space-only on a fragmented disk etc.) |

**Two standing rules carry over unchanged.** A green run here closes plumbing,
never a real-hardware clause (CLAUDE.md rule #5); and the evidence rows are
written by the harnesses, never by hand.

**The Secure Boot templates are mutually exclusive — measured 2026-08-30.**
Hyper-V offers two dbs, `MicrosoftWindows` and
`MicrosoftUEFICertificateAuthority`, and there is no third and no custom
mechanism (`Set-VMFirmware` accepts only those two names; no template key in
the registry). A/B on a throwaway diskless Gen 2 VM, Secure Boot on, one ISO
in the DVD, boot-first DVD, screenshots over the first 8 s:

| template | Windows 10 install ISO | Fedora 42 netinst ISO |
|---|---|---|
| `MicrosoftUEFICertificateAuthority` | **refused** (no "Press any key", straight to PXE) | boots (GRUB menu) |
| `MicrosoftWindows` | boots ("Press any key" → Setup) | **refused** (straight to PXE) |

Neither db trusts both CAs. Real machines ship both, so **a Secure-Boot-on
dual boot as it exists on real hardware cannot be reproduced on Hyper-V with
the built-in templates**: under the UEFI-CA template the firmware will not
start `bootmgfw.efi` from its own Windows entry, and GRUB's `chainloader`
goes through shim's verification, which consults the same db — plus
**MokList**. That leaves one honest route for the V1b SB-on *chainload*
clause: enrol the Microsoft Windows Production PCA (extracted from
`bootmgfw.efi`'s signature) into MokList, so shim → GRUB → `bootmgfw.efi`
verifies under Secure Boot. That exercises the chainload verification path;
it does **not** exercise a firmware db holding both CAs, and it leaves Windows
unbootable from its own firmware entry — both to be stated in the row's
`notes`. The db-composition clause stays with real hardware.

Two more Hyper-V facts learned the same day:

- **The template locks once the vTPM is initialised** — `Set-VMFirmware
  -SecureBootTemplate` fails with *"Cannot modify the secure boot template ID
  property after the virtual TPM is initialized"*, and `Disable-VMTPM` does
  not unlock it. Choose the template at `new-vm.ps1` time; changing it means
  a new VM.
- `GetVirtualSystemThumbnailImage` returns RGB565 rows of exactly `width*2`
  bytes; copy row by row into the bitmap's padded stride (a whole-buffer
  `Marshal.Copy` was an AccessViolation).

## Layout

| File | Does |
|---|---|
| `setup.ps1` | **elevated, once**: adds you to *Hyper-V Administrators*, creates `C:\upgrade-rig\hv\{iso,vm,shots}` |
| `autounattend.xml` | the QEMU rig's unattend with a **100 MB ESP** and ComputerName `UPGRIGHV` |
| `make-unattend.sh` | wraps it into `C:\upgrade-rig\hv\iso\unattend-hv.iso` |
| `new-vm.ps1` | creates the Gen 2 guest: Secure Boot on (template selectable), vTPM, two DVDs, DVD-first |
| `vm.ps1` | the QMP stand-in: start/stop, WMI keyboard (`type`, `key`, `press-any-key`), thumbnail `shot`, `fw`, `sb`, `dvd`, `disk`, `boot-first`, PowerShell Direct `ps`, `copy` |

Hyper-V cannot open files under `\\wsl.localhost`, so ISOs, VHDXs and
screenshots live on the Windows side in `C:\upgrade-rig\hv\` (gitignored by
location — nothing there is in the repo). The Windows and Fedora ISOs are the
same files the QEMU rig fetched (`rig/vm/fetch-iso.sh`,
`fetch-payload-bits.sh`), copied over; the netinst sha256 is in
`rig/vm/artifacts/payload-bits/fedora-netinst.source.txt`.

## First-time sequence

```
# 1. once, in an ELEVATED Windows PowerShell on the host:
powershell -ExecutionPolicy Bypass -File \\wsl.localhost\Ubuntu\home\addlatt\upgrade_\rig\hyperv\setup.ps1
#    then sign out and back in, and from an admin prompt: wsl --shutdown
#    (the WSL shell's token must pick up the new group)

# 2. from WSL, everything below runs through powershell.exe unelevated:
cp rig/vm/artifacts/win10.iso rig/vm/artifacts/payload-bits/fedora-netinst.iso /mnt/c/upgrade-rig/hv/iso/
./rig/hyperv/make-unattend.sh
powershell.exe -ExecutionPolicy Bypass -File rig/hyperv/new-vm.ps1        # UEFI-CA template, TPM on
powershell.exe -ExecutionPolicy Bypass -File rig/hyperv/vm.ps1 start
powershell.exe -ExecutionPolicy Bypass -File rig/hyperv/vm.ps1 press-any-key
powershell.exe -ExecutionPolicy Bypass -File rig/hyperv/vm.ps1 shot       # ~20-30 min hands-off to the desktop
```

Guest account `rig` / `rig`, autologon, ComputerName `UPGRIGHV`. Guest control
is PowerShell Direct (`vm.ps1 ps "<command>"`) plus `Copy-VMFile`; there is no
SMB share, so harness results come back with `vm.ps1 ps` and `Copy-VMFile`
(guest→host is not supported by `Copy-VMFile`; read files back through `ps`).

## State (2026-08-30)

`UPGRIGHV` exists and is installed: Windows 10 Pro 22H2, Secure Boot **on**
under the `MicrosoftWindows` template (`Confirm-SecureBootUEFI` → True), vTPM
present (`Get-Tpm` → TpmPresent True), **ESP 100 MiB exactly**, C: 85.8 GB
with 65 GB free, `rig`/`rig` autologon, PowerShell Direct answering. Nothing
had been tested on it as of install day; the V0 rows below fired the same
day (rows 3, 5, 6 — see `docs/validation-results/v0-handoff.csv` and RISKS
R15). BitLocker is now ON (TPM + RecoveryPassword protectors; recovery key in
`C:\upgrade-rig\hv\UPGRIGHV-bitlocker-recovery.txt`, host side only) and
Secure Boot is currently OFF (the BitLocker rows' run-book state); the
pristine pre-BitLocker disk is `UPGRIGHV.fresh.vhdx`. First guest prep step is the same as the QEMU rig's: fetch the repo zip
into `C:\upgrade_` (or `vm.ps1 copy` the pieces).

## Planned run-books (not yet run — nothing below is evidence)

- **V0 rows 3, 5, 6 — DONE 2026-08-30** (row 4 not meaningful here, see
  RISKS R15). Ran as: stick image → VHDX (`qemu-img convert -O vhdx`),
  `vm.ps1 disk add`, delete the stale `fired.txt` the image carried, then
  `Test-Handoff.ps1 -Arm … / -Check -ResultsCsv <guest path>` per row, the
  row transported verbatim into `docs/validation-results/v0-handoff.csv`
  with the three Read-Host observer fields filled from screenshots (a PS
  Direct session cannot answer Read-Host — run `-Check` with stdin closed,
  `< /dev/null`, or the pipeline can hang at the prompt; one run's CSV
  append was lost that way and the row re-run). BitLocker prep: eject BOTH
  install DVDs first (`Enable-BitLocker` refuses while bootable media is
  attached), and note `Enable-BitLocker` re-runs can drop an existing
  RecoveryPassword protector — verify protectors and re-save the key after.
- **V1b, 100 MiB ESP — two rows.** First **SB off** (both templates refuse
  one OS, so the plain dual boot needs SB off here), which gives the ~100 MiB
  ESP row R21 is owed plus Hyper-V's own NVRAM behaviour; then **SB on under
  the UEFI-CA template with the Windows PCA enrolled in MokList** for the
  chainload clause, notes stating what that does and does not show.
  `rig/vm/v1b.sh`'s pieces: the guest-side
  shrink (`rig/vm/guest/v1b-shrink.ps1`, via `ps`), the kickstart
  (`rig/vm/v1b-ks.cfg`) on an OEMDRV VHDX (`qemu-img convert -O vhdx
  rig/vm/artifacts/v1b/oemdrv.img`), Fedora netinst on the DVD with
  `boot-first dvd`, and `v1b-inspect.py` pointed at the VHDX (`qemu-img dd`
  reads VHDX). The verdict script is unchanged.

## Known limits of this leg

- No USB emulation (above). No QMP: keyboard is WMI `Msvm_Keyboard`,
  screenshots are `GetVirtualSystemThumbnailImage` (RGB565, 1024×768 here).
- Hyper-V's UEFI has no setup menu to press Esc into; boot order is set with
  `Set-VMFirmware` and inspected with `vm.ps1 fw`. Whether *its* BDS rewrites
  OS-created `Boot####` entries the way OVMF's `bootindex` path did is a data
  point to record, not assume.
- `Copy-VMFile` is host→guest only.
- **WSL's /mnt/c 9p page cache serves stale pages of files Windows rewrites**
  (proven 2026-08-30: an offline inspection read a pre-servicing
  `bootmgfw.efi` out of `UPGRIGHV.vhdx` hours after Windows had updated it,
  and a `cp` baked the stale pages into a backup — two independent readers
  agreed on the wrong bytes because both read the same poisoned cache).
  Evict before EVERY WSL read of a Windows-written file
  (`posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)`); `v1b-inspect.py` and
  `v1b.sh` (`evict`/`oem_pull`) now do it themselves. Verify any doubtful
  read against the guest's own view of the same bytes.
