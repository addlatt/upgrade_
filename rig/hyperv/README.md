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

**The Secure Boot template is itself a finding to record.** Hyper-V lets the
db be `MicrosoftWindows` (Windows Production CA only) or
`MicrosoftUEFICertificateAuthority` (the third-party UEFI CA that signs
Fedora's shim). Real machines ship both; a firmware with only the Windows CA
would refuse shim outright, which is exactly the V1/V1b refusal path. Run the
alongside install under the UEFI-CA template and record whether Windows still
boots under it (its own CA is normally included), and run once under the
Windows-only template to see the refusal shape.

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

## Planned run-books (not yet run — nothing below is evidence)

- **V0 rows 3–6.** Build the stick images as VHDX (`qemu-img convert -O vhdx
  rig/vm/artifacts/stick-shell.img …`), `vm.ps1 disk add`, then the same
  `Test-Handoff.ps1 -Arm … / -Check -ResultsCsv` cycle as `rig/vm/README.md`,
  with `-ResultsCsv` pointing at a guest path and the row copied back. Record
  the SCSI-not-USB caveat in `notes`.
- **V1b, SB on, 100 MiB ESP.** `rig/vm/v1b.sh`'s pieces: the guest-side
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
