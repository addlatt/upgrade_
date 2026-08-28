# VM rig — QEMU/OVMF Windows guest for validation

This rig exists to run validation experiments that need a Windows machine we
can afford to break:

- **V0 (RISKS R15) — the boot handoff, VM leg.** Runs
  `upgrade_/windows/Test-Handoff.ps1` arm → reboot → check against a stick
  image attached as USB, including the fail-safe modes. Evidence rows land in
  `docs/validation-results/v0-handoff.csv` — written by the harness itself,
  never by hand.
- **V5 / R1 level 3 — the VMD spoof.** A patched QEMU presents PCI
  `8086:9a0b` class `0104`; the scanner inside the guest must FAIL through
  the real Windows PnP → WMI pipeline. **DONE 2026-08-26:** source and
  `dist/` both `[FAIL] Intel RST / VMD active`, verdict RED; captured to
  corpus as `vm-qemu-q35-vmd-spoof-9a0b.json` (synthetic). Plumbing only —
  the physical RST machine (V5) is still required.
- **V1b / R21 — the alongside install, VM leg. DONE 2026-08-27** (SB off):
  `v1b.sh` shrinks C:, kickstarts Fedora 42 into the gap reusing the Windows
  ESP unformatted, and power-cycles both systems; row in
  `docs/validation-results/v1b-alongside.csv` (`fallback-loader-replaced` —
  all five checks held, plus five findings; see RISKS R21). Run-book below.
- Later: V1 proper (Secure Boot on — needs an SMM-capable host) and V3
  (BitLocker BITLK reads).

**Two standing rules.**

1. **VM results are synthetic.** A green run here closes plumbing, never a
   real-hardware clause (CLAUDE.md rule #5). The physical vendor matrix and
   the Hyper-V Gen 2 leg of the VM criterion remain open regardless of what
   this rig shows.
2. **One QEMU invocation per arm→reboot→check cycle.** OVMF re-enumerates
   `Boot####` entries when the attached device set changes between boots;
   the harness would misreport that as `reordered`. Change `run-vm.sh` flags
   only between cycles.

## Scripts

| Script | Does |
|---|---|
| `setup.sh` | apt packages, kvm group, `artifacts/` dirs, OVMF vars profiles |
| `fetch-iso.sh` | official Win10 22H2 x64 ISO via Fido (URL expires ~24 h) |
| `fetch-payload-bits.sh` | EDK2 Shell + Fedora shim/grub (from netinst media — see note) |
| `make-stick.sh` | `stick-shell.img` / `stick-shim.img`, MBR + FAT32 |
| `make-unattend.sh` | wraps `autounattend.xml` into `unattend.iso` |
| `install-vm.sh` | one-time guest install (one keypress at "Press any key…") |
| `run-vm.sh` | every later boot; flags `--sb --stick --tpm --smb --vmd --reset-vars` |
| `build-qemu-vmd.sh` | patched QEMU for the VMD spoof (`-device pci-testdev`) |
| `v1b.sh` | the V1b bench: `oemdrv`, `inspect`, `install`, `cycle`, `fwmenu`, `verdict` |
| `v1b-inspect.py` | offline GPT + ESP manifest/hash of `win10.qcow2` (no loop/nbd needed) |
| `v1b-verdict.py` | turns the run's evidence into the CSV row |
| `v1b-ks.cfg` | the alongside kickstart (`--onpart=sda1 --noformat`), auto-loaded from OEMDRV |
| `guest/v1b-shrink.ps1`, `guest/v1b-mark.ps1` | guest-side shrink + per-boot marker (PS 5.1) |

Everything generated lands in `artifacts/` (gitignored). Guest account:
`rig` / `rig`, autologon, ComputerName `UPGRIG`.

Provenance: the Fedora netinst URL + sha256 are recorded by
`fetch-payload-bits.sh` into `artifacts/payload-bits/fedora-netinst.source.txt`.
The shim payload's grub **must** come from install media, not the plain
grub2-efi-x64 RPM — the RPM binary's embedded prefix is `/EFI/fedora` and it
never reads `EFI/BOOT/grub.cfg`.

## First-time sequence

```
./setup.sh              # may ask you to restart WSL for kvm group (optional -
                        # run-vm.sh falls back to `sg kvm`)
./fetch-iso.sh          # ~6 GB
./fetch-payload-bits.sh
./make-stick.sh
./make-unattend.sh
./install-vm.sh         # press a key at the CD prompt, then walk away
```

### Guest one-time prep (at the desktop)

1. Fetch the repo:
   `Invoke-WebRequest https://github.com/addlatt/upgrade_/archive/refs/heads/main.zip -OutFile u.zip; Expand-Archive u.zip C:\; Rename-Item C:\upgrade_-main C:\upgrade_`
2. With `run-vm.sh --smb`: check `\\10.0.2.4\qemu` opens and is writable
   (create + delete a test file). If refused, reboot once (the
   `AllowInsecureGuestAuth` FirstLogonCommand needs a restart on some builds).
3. **fs0: probe** — the stick's `startup.nsh` assumes the stick maps as
   `fs0:`. Once Windows is installed the guest has two FAT volumes (ESP +
   stick). On the first armed run, verify `fired.txt` appeared on the
   **stick**, not the ESP (`mountvol S: /S; dir S:\`). If it went to the
   ESP, the stick copy of `startup.nsh` needs adjusting to the fsN: that
   holds it — flag this upstream, the harness only checks the stick root.

## V0 run-book

Each run, inside the guest (elevated PowerShell), with QEMU left running
through the whole cycle:

```
C:\upgrade_\upgrade_\windows\Test-Handoff.ps1 -Arm -PayloadDrive E: [flags]
shutdown /r /t 0
# watch the boot, then back in Windows:
C:\upgrade_\upgrade_\windows\Test-Handoff.ps1 -Check `
    -ResultsCsv \\10.0.2.4\qemu\docs\validation-results\v0-handoff.csv
```

| # | run-vm.sh flags | Arm flags | Expected `result` | Notes to record |
|---|---|---|---|---|
| 1 | `--sb off --stick shell --smb` | *(none)* | `fired-once` | DONE 2026-08-23: fired-once |
| 2 | `--sb off --stick shell --smb` | `-FailMode NoFile` | `ignored` | DONE 2026-08-23: ignored (fail-safe held) |
| 3 | `--sb on --stick shell --smb` | `-FailMode SecureBootUnsigned` | `ignored` | **BLOCKED on this rig** — see below |
| 4 | `--sb on --stick shim --smb` | *(none)* | `ignored` *by design* | **BLOCKED on this rig** — see below |
| 5 | `--sb off --stick shell --smb --tpm` | `-SuspendBitLocker` | `fired-once` | **BLOCKED on this rig** — no TPM in guest |
| 6 | `--sb off --stick shell --smb --tpm` | `-FailMode NoSuspend` | `fired-once` | **BLOCKED on this rig** — no TPM in guest |

**Rows 5–6 cannot run on this AMD/WSL2 rig either.** BitLocker needs a TPM,
and Windows never detects one here (`Get-Tpm` → `TpmPresent: False`) despite
QEMU wiring the device correctly (`query-tpm` shows `tpm-crb` + emulator
backend) and swtpm running with fresh state. Tried both `tpm-tis` and
`tpm-crb` and a wiped state dir — all `TpmPresent: False`, no security device
at the PnP level. The non-SMM OVMF we must use does not publish the TPM2 ACPI
table to the guest (measured-boot/TPM support in Ubuntu's OVMF travels with
the SMM-requiring `.secboot` build, same as Secure Boot). So the **BitLocker
rows move to the physical matrix / Hyper-V leg** too, alongside 3–4.

**Net for this rig:** it validates the SB-off, no-TPM V0 paths — rows 1–2,
which passed. Rows 3–6 need an SMM-capable OVMF (for Secure Boot and TPM),
which crashes KVM on this AMD/WSL2 host, so they belong on real machines or a
Hyper-V Gen 2 guest.

**Rows 3–4 cannot run on this AMD/WSL2 rig.** Secure Boot *enforcement* in
Ubuntu's OVMF lives only in the `.secboot` CODE build, whose QEMU firmware
descriptor declares `requires-smm` — and SMM crashes KVM on this host (see
below). The non-SMM `OVMF_CODE_4M.fd` we must use does **not** enforce Secure
Boot: confirmed empirically 2026-08-23 — an unsigned EDK2 Shell booted under
the MS-keys varstore instead of being refused. Separately, `vars-ms` has no
Windows boot entry (the installer wrote it into the SB-off varstore), so the
guest can't return to Windows under `--sb on` anyway. **Rows 3–4 move to the
physical vendor matrix and the Hyper-V Gen 2 leg**, where Secure Boot is real.

Before run 5 (BitLocker prep, in the guest, elevated — record the recovery
key somewhere outside the VM):

```
Enable-BitLocker -MountPoint C: -TpmProtector -UsedSpaceOnly -SkipHardwareTest
Add-BitLockerKeyProtector -MountPoint C: -RecoveryPasswordProtector
manage-bde -protectors -get C:
```

Enable BitLocker only **after** runs 1–4 (vars-profile swaps could otherwise
trigger spurious recovery), and keep `--tpm` on every boot afterwards —
`artifacts/tpm/` holds the TPM state BitLocker seals against. Runs 5–6 use
SB **off** deliberately: without SB the TPM protector binds the legacy PCR
profile (0,2,4,11), the one a foreign boot path most plausibly disturbs. If
run 6 produces **no** recovery prompt in OVMF, record that verbatim — it is
a finding about OVMF's PCR behavior, and the physical matrix owns that
clause either way.

Wedged run: state lives in `%ProgramData%\upgrade_\v0`
(`handoff-state.json`, `bcd-backup.bin`); `-Check -RestoreBcd` is the
recovery path.

## VMD spoof session (V5 level 3)

```
./build-qemu-vmd.sh          # once; ~15 min build
./run-vm.sh --vmd --smb
```

In the guest:

1. Device Manager sanity: an unknown **RAID Controller** with hardware ID
   `PCI\VEN_8086&DEV_9A0B…` and compatible IDs including `PCI\CC_010400`.
2. Run the scanner (elevated), source **and** dist:
   `C:\upgrade_\evaluate\windows\upgrade-scan.ps1` — expected:
   `[FAIL] Storage controller mode — Intel RST / VMD active (…)`.
3. Capture: `.\upgrade-scan.ps1 -DumpMachine machine-capture-vm.json`, copy
   to the host over `\\10.0.2.4\qemu`.
4. On the host, curate into
   `evaluate/windows/corpus/vm-qemu-q35-vmd-spoof-9a0b.json`: `Label` =
   `"VM: QEMU q35 + spoofed Intel VMD 9a0b (synthetic)"`, add
   `"Synthetic": true` after `SchemaVersion`, fill `Expected` from the run
   (`Storage controller mode: fail`, others as observed).
5. Verify: scanner `-SelfTest` passes the new corpus line and everything
   already there.
6. Never cite this against R1's real-hardware clause.

**Done 2026-08-26.** Ran the above via `guest/vmd-scan.ps1` (elevated, driven
over QMP). Findings for the record:

- The spoofed device enumerates as an unknown **RAID Controller**, device
  Status `Error` (no INF binds `8086:9a0b`, expected). It still carries
  HardwareID `PCI\VEN_8086&DEV_9A0B` and CompatibleIDs `PCI\CC_010400` /
  `PCI\CC_0104`, which is all the check reads — an unbound/errored device does
  not hide it. Good: real RST controllers with the driver missing look like
  this too.
- Signal 1 (the `9a0b` ID) is what fired; signal 3 (`CC_0104` RAID class) was
  independently present, so the check has a backstop if an ID is ever unknown.
- `-cpu host` passes the host AMD CPU through, so the capture's `Sys.CpuName`
  reads "AMD Ryzen AI 9 HX 370" on a "QEMU Q35" box. Harmless (arch 9 → x64
  `ok`); the corpus `Label` is set explicitly regardless.
- Both source and `dist/` fired identically (parity holds); `dist/` was in
  sync with `data/` already (no rebuild needed).

## V1b run-book (alongside install; done 2026-08-27)

Every step's evidence is produced by the step itself; nothing is typed into
the CSV. **Back up both the disk and the varstore first** — the install is
destructive to the guest, and (learned the hard way) the OVMF varstore is
where the boot entries live:

```
cp artifacts/win10.qcow2 artifacts/win10.pre-v1b.qcow2
cp artifacts/fw/vars-nosb.fd artifacts/fw/vars-nosb.pre-v1b.fd
./v1b.sh inspect 00-baseline                  # GPT + ESP manifest, bootmgfw sha256
./v1b.sh oemdrv                               # FAT volume 'OEMDRV' carrying v1b-ks.cfg
./v1b.sh boot                                 # Windows; elevated PS (Win+X, A, Left, Space):
#   powershell -ExecutionPolicy Bypass -File \\10.0.2.4\qemu\rig\vm\guest\v1b-shrink.ps1
igm\guest1b-shrink.ps1
#   shutdown /s /t 0                          # -> artifacts/v1b/shrink.json (+ registers the marker task)
./v1b.sh inspect 01-post-shrink
./v1b.sh install                              # netinst CD at bootindex=0 + OEMDRV; pick "Install Fedora 42"
                                              # at the ISO menu (up, Enter); Anaconda loads ks.cfg itself,
                                              # runs text-mode, %post ships logs to OEMDRV, then powers off
./v1b.sh inspect 02-post-install
./v1b.sh autoshutdown on
./v1b.sh cycle windows c2 ; ./v1b.sh cycle linux c3 ; ...   # hands-free power cycles; each OS writes
                                              # its own row to OEMDRV:/boots.log and powers off
./v1b.sh inspect 03-post-cycles
./v1b.sh verdict                              # appends the row to docs/validation-results/v1b-alongside.csv
```

`v1b.sh log` prints `boots.log`; `v1b.sh fwmenu TAG [run-vm flags]` boots into
OVMF's Boot Manager, screenshots the `Boot####` list and kills QEMU — the
zero-touch way to see what the firmware currently holds.

What the rig showed (details and consequences in RISKS R21): shrink 32 GiB
(cold shrinkable 46.8 GiB, no immovable-file cap on this guest); Anaconda took
`part /boot/efi --onpart=sda1 --noformat` without complaint; the ESP grew by
6.2 MB and `bootmgfw.efi` never changed; Windows booted through GRUB with
`BootCurrent` = Fedora's entry; both survived every cycle. Findings: shim
overwrites `EFI/Boot/bootx64.efi`; Windows re-took `BootOrder` after a
servicing pass; the firmware dropped the OS entries (see below); os-prober was
already on in stock F42; the ESP's GPT name was case-normalised.

Rig quirks met on the way, all of which a physical run would not see:

- **`bootindex=` deletes every OS-created `Boot####`.** With a fw_cfg boot
  order present (what `--cdrom` sets), OVMF's BDS rewrote NVRAM to
  firmware-auto entries only — Windows *and* Fedora gone — before any OS ran.
  Confirmed with `fwmenu` (CD at bootindex → both absent; extra USB stick,
  no bootindex → both present). Use `--cdrom` only for the install boot,
  never for a run whose point is the NVRAM state, and expect to re-assert
  entries afterwards. Shim's fallback (`EFI/Boot/bootx64.efi` → `fbx64.efi`
  → `BOOTX64.CSV`) re-creates the Fedora entry on the next boot; Windows only
  re-creates its own during servicing.
- **`/usr/local/sbin` does not exist in the F42 chroot at `%post` time** —
  `mkdir -p` it before writing there (the kickstart now does).
- **Guest clocks.** `-rtc base=localtime` + kickstart `timezone --utc` makes
  the Linux rows carry host-local time labelled `Z`; Windows rows are true
  UTC. Order rows by position, not by timestamp.
- **`TOKEN_PRIVILEGES` via Add-Type**: declare the LUID as two 32-bit fields;
  a `long` gets aligned to offset 8 and `AdjustTokenPrivileges` fails with
  1314 (`v1b-mark.ps1` reads the firmware's `BootCurrent` this way).
- **The stale `fired.txt` on the ESP** from the pre-fix V0 run is still
  there; it is in the manifests and harmless.
- State the rig is left in: Windows' firmware entry is absent (dropped by the
  `fwmenu` CD probe); Windows boots from the GRUB menu, and its next
  servicing pass will re-register it.

## Findings & gotchas (from the first bring-up, 2026-08-23)

- **SMM crashes KVM on AMD/WSL2.** The planned `-machine q35,smm=on` +
  secure-boot pflash dies with `KVM: entry failed, hardware error 0xffffffff`
  within seconds. Fix: use the non-SMM `OVMF_CODE_4M.fd` (setup.sh copies it),
  no `smm=on`, no `property=secure`. Secure Boot still *enforces* image
  verification (DXE), we just lose varstore tamper-proofing — irrelevant here.
- **The install "Press any key to boot from CD" window is ~5s** and easy to
  miss; a miss falls through to PXE/HTTP and parks. Drive it from the QMP
  socket instead of by hand: `artifacts/qmp.py` + `send-key`, spammed densely
  across the window. `qmp.py shot` (screendump→png via ffmpeg) is the feedback
  loop — screenshot after every action.
- **`startup.nsh` fs0: misdirection** (now fixed in the repo payload): with an
  installed Windows present, the shell maps the ESP as fs0:, so the marker
  landed on the ESP not the stick. The payload now self-locates the stick.
- **Spurious `reordered`** (now fixed in `Test-Handoff.ps1`): `-Check`
  snapshotted the boot order before removing the harness's own temporary boot
  entry, so every run looked reordered. The compare now excludes the test
  `{guid}`.
- **Guest control** is entirely QMP-driven: Win+X→A→(UAC: Left, Space) opens an
  elevated PowerShell; run guest scripts from the `\\10.0.2.4\qemu` share with
  `-ExecutionPolicy Bypass`. UAC Alt+Y does *not* work on the secure desktop —
  use Left then Space.
