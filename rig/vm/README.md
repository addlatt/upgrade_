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
  the real Windows PnP → WMI pipeline.
- Later: the bench for V1/V1b (unattended install alongside shrunk Windows)
  and V3 (BitLocker BITLK reads).

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
