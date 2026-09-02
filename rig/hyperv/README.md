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
| `v1b.sh` | the V1b alongside-install bench (run-book below) |
| `v3.sh`, `v3-verdict.py` | the V3 BITLK-read bench: builds the OEMDRV-v3 transport, drives the Windows plant and the Fedora read, writes the evidence rows |
| `guest/oemdrv-run.sh`, `guest/v3-bootstrap.sh` | the Fedora-side **run hook**: a unit that runs `OEMDRV:/run.sh` as root on every boot and leaves `run.log` on the volume — installed once from the console by the bootstrap |
| `guest/v3-plant.ps1`, `guest/v3-read.sh`, `guest/v3-encrypt.ps1` | V3 guest halves: Windows plants + hashes the corpus and `C:\Users` and does a full shutdown; Fedora unlocks, mounts read-only and re-hashes; the encrypt script builds the other configs in the product's order (encrypt, then shrink) |

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

**State after the V1b run (2026-08-31):** the guest dual-boots — C: shrunk by
32 GiB, Fedora 42 installed alongside reusing the 100 MiB ESP, firmware boots
Fedora's shim first with Windows chainloadable from the GRUB menu (two
Downs), BitLocker protection back On and re-sealed to the GRUB path. The V0
stick VHDX is detached. Backups host-side in `C:\upgrade-rig\hv\vm\`:
`UPGRIGHV.pre-install.vhdx` (post-shrink, pre-install, BitLocker suspended —
verified against the guest's own hashes) and `UPGRIGHV.fresh.vhdx`
(install-day, pre-BitLocker). The original `UPGRIGHV.pre-v1b.vhdx` was
deleted: the 9p stale-cache hazard had poisoned it.

**State after the V3 run (2026-09-01):** `UPGRIGHV`'s Fedora was
`dnf`-upgraded (kernel 6.19.14-108 is now the GRUB default; the install
kernel 6.14.0-63 is the second entry, still there for the ntfs3 rows), the
OEMDRV run hook is installed, and the guest carries `C:\v3corpus` (~1.5 GB).
A third guest `UPGRIGV3` (SB off, vTPM, no disk of its own) exists to build
configs; the config disks `UPGRIGHV.xts256.vhdx` (XtsAes256, used-space) and
`UPGRIGHV.full.vhdx` (XtsAes128, "full"), both planted, sit host-side with
their recovery keys in `C:\upgrade-rig\hv\UPGRIGV3.<disk>-bitlocker-recovery.txt`.
OEMDRV volumes: `oemdrv-v3.vhdx` (config 1, attached to UPGRIGHV),
`oemdrv-v3-x256.vhdx`, `oemdrv-v3-full.vhdx`.

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
- **V1b, 100 MiB ESP, SB off — DONE 2026-08-31** (row 2 of
  `docs/validation-results/v1b-alongside.csv`, result
  `fallback-loader-replaced`; findings in RISKS R21). Ran via `v1b.sh`
  (this directory): fresh OEMDRV built and attached, offline inspections
  through the cache-evicting `v1b-inspect.py`, the share-free shrink
  (`guest/v1b-shrink-hv.ps1` + `rig/vm/guest/v1b-mark.ps1`, both
  `Copy-VMFile`d in), netinst DVD `boot-first dvd` with the ISO menu driven
  by WMI keys, cycles driven by `v1b.sh cycle` (GRUB: two Downs = Windows).
  BitLocker: suspended `-RebootCount 1` before the installer boot; the first
  chainloaded Windows boot auto-resumed and re-sealed, the second unsealed
  silently — no recovery prompt. Hyper-V's UEFI **kept** the Windows
  `Boot####` entry across the install (unlike OVMF's `bootindex` path).
  The disk backup taken before the run had to be re-taken after the 9p
  stale-cache hazard (below) was caught poisoning it.
- **V1b, SB on — the MOK chainload experiment — DONE 2026-08-31** (record:
  `docs/validation-results/v1b-mok-chainload-2026-08-31.md`). Second guest
  `UPGRIGMOK` on the `MicrosoftUEFICertificateAuthority` template (disk copied
  from `UPGRIGHV.pre-install.vhdx`), Fedora alongside-installed under SB
  enforcing via `v1b.sh` with `VMNAME=UPGRIGMOK LEG=mok SB=on
  KS=v1b-mok-ks.cfg OEM_EXTRA=artifacts/v1b-mok/win-pca.der`. The Windows
  Production PCA 2011 (extracted from `bootmgfw.efi`'s Authenticode signature,
  `rig/hyperv/artifacts/v1b-mok/win-pca.der`) was enrolled into shim's
  MokList. Negative (PCA not enrolled): GRUB → `bad shim signature`, Windows
  refused. Positive (enrolled): Windows boots to the desktop, SB enforcing
  confirmed both sides. Proves the chainload verification only — NOT a both-CA
  db (Hyper-V can't express one), NOT the vendor matrix.

**Driving MokManager and the console — learned 2026-08-31 (obey):**
- **`vm.ps1 type` / WMI `TypeText` is unreliable here** — it drops or garbles
  characters. Drive ALL text as per-character Windows virtual-key codes via
  `vm.ps1 key` (letters `A`..`Z` = VK 65..90 → lowercase in a Linux console;
  digits = 48..57; space=32, `/`=191, `-`=189, `.`=190). A `str2vk` helper is
  the reliable path; `|` etc. need Shift and are best avoided (use
  `mokutil --test-key` instead of `... | grep`).
- **GRUB `$root` is polluted by a failed chainload** — after a
  `bad shim signature`, the Fedora BLS entry fails with `vmlinuz not found`.
  Boot Fedora from a **fresh** GRUB (hard power-cycle) and select Windows
  **deterministically** with `sudo grub2-reboot 2; sudo reboot`, never by
  racing the menu countdown.
- **MokManager's "press any key" window is short and jittery on Hyper-V.**
  Set `sudo mokutil --timeout -1` from Fedora first so MokManager waits
  indefinitely, then drive it calmly (Down/Enter for Enroll MOK → Continue →
  Yes → password → Reboot). A MokManager prompt that times out **deletes** the
  pending `MokNew` without enrolling — re-`mokutil --import` if that happens.

- **V3 / R19, config XTS-AES-128 used-space-only — DONE 2026-09-01**
  (rows in `docs/validation-results/v3-bitlk-read.csv`, findings in RISKS
  R19). The bench is `v3.sh`; per config: `v3.sh oemdrv guest/v3-read.sh`
  (fresh OEMDRV-v3 carrying the reader, the hook, and the CURRENT recovery
  password from the host-side key file — the guest deletes it first thing),
  `v3.sh windows` (GRUB two Downs, waits for PS Direct), `v3.sh plant
  <config>` (`guest/v3-plant.ps1`: corpus + `C:\Users` hashed onto OEMDRV,
  OneDrive & co. stopped first, then `shutdown /s` — a FULL shutdown, never
  hybrid), `v3.sh read` (Fedora boots, the run hook executes the reader:
  ntfs-3g pass, ntfs3 pass, ntfs3 with readahead 0; one manifest per
  corpus subtree with a `sync` after each so a kernel crash leaves the last
  stage on the volume; the previous boot's kernel journal is saved first),
  `v3.sh verdict` (rows). `v3.sh run <config>` chains them. `v3.sh rearm`
  puts the reader + key back without wiping the Windows manifests; `v3.sh
  read old` boots the second GRUB entry (the previous kernel after a
  `rearm-upgrade` run, which `dnf`-upgrades the kernel and reboots into the
  reader). Other configs: `VMNAME=UPGRIGV3 v3.sh mkvm <disk>` wraps a copy
  of `UPGRIGHV.fresh.vhdx` in a throwaway SB-off/vTPM guest, `v3.sh encrypt
  <disk> XtsAes256 usedspace|full 32` runs `guest/v3-encrypt.ps1` (encrypt
  THEN shrink — the product's order) and captures the new recovery password
  to `C:\upgrade-rig\hv\UPGRIGV3.<disk>-bitlocker-recovery.txt` (host side
  only, redacted from every kept output); then the disk goes on UPGRIGHV as a
  data disk (`DEV=/dev/sdb3` when building that OEMDRV) for the read.

**Learned running V3 — obey:**
- **The Fedora-side run hook** (`guest/oemdrv-run.sh`, installed once by
  `v3.sh login-bootstrap` → `guest/v3-bootstrap.sh`) is the general
  transport now: put a script at `OEMDRV:/run.sh`, boot, read
  `OEMDRV:/run.log`; the script writes `poweroff` / `reboot` /
  `reboot-windows` to `OEMDRV:/next`. Scripts that must run once remove
  `run.sh` themselves.
- **`vm.ps1 key` now types with Shift** (`s<vk>` tokens) and `v3.sh type`
  maps upper-case and most punctuation — the WMI `TypeText` garble stands,
  and `str2vk` is still the reliable path.
- **A `sudo` timestamp expires mid-session and swallows the next typed
  lines as password attempts** (three tries locks you out for a while). Type
  `rig` again before the next `sudo` after a few minutes' gap.
- **ntfs-3g presents Windows' 0-byte SYSTEM files as FIFOs**; any reader
  that `open()`s without `lstat` hangs forever, in S state (load average 0,
  so it looks idle). `find … -not -type f -not -type d -not -type l` lists
  them.
- **The F42 install kernel's `ntfs3` oopses on this volume** and may wedge
  the guest silently (console still shows a login prompt, no poweroff):
  `wait-off` timing out after the ntfs3 stage is that. Kill, pull, look at
  `facts-linux.txt`'s last `stage=` and `trace-*.txt`.
- **`Enable-BitLocker` prints the numeric recovery password in its own
  text**, not only where a script echoes it — capture raw output to a
  gitignored file, extract once, redact the pattern before anything is kept
  or shown.
- A hard `kill` of the guest loses whatever the reader had not `sync`ed:
  every fact write and every manifest part syncs for that reason.

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
