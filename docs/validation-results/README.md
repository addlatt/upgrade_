# Validation results

The evidence that closes the gates in [../VALIDATION.md](../VALIDATION.md).
This project closes risks with evidence, not argument — this is where the
evidence lives, in the open, including the runs that failed.

Committing results here is the point. A gate is not closed because someone
remembers it working; it is closed because a row exists.

## `v0-handoff.csv` — the one-time UEFI boot handoff (gate V0, risk R15)

One row per run, appended automatically by
`upgrade_/windows/Test-Handoff.ps1 -Check`. Do not hand-edit; add rows by
running the harness.

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601 |
| `harness` | Test-Handoff.ps1 version |
| `vendor`, `model`, `firmware_version` | the machine under test |
| `secureboot` | on / off / unknown at arm time |
| `bitlocker` | C: protection state at arm time |
| `payload` | the `BOOTX64.EFI` leaf (which payload) |
| `failmode` | blank for a baseline run, else the fail-mode armed |
| `result` | see vocabulary below |
| `keypress_free` | y / n / na — did it reach the payload with no keypress |
| `windows_returned` | y / n — back in Windows normally after |
| `notes` | recovery prompt? logo hang? anything odd. From harness 0.2.0 the row begins with a harness-written `[harness: os=…; bitlocker-via=cmdlet\|manage-bde; fired-via=fired.txt\|grubenv\|none]` prefix, then the operator's words |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `fired-once` | payload ran (marker: `fired.txt` from the Shell payload, or `upg_fired=1` in `EFI/BOOT/grubenv` from the shim payload — harness 0.2.0+), one-shot self-cleared, boot order intact | **pass** |
| `ignored` | booted straight to Windows, order unchanged | fail-**safe** (and the *expected pass* for a fail-mode run) |
| `persisted` | payload ran but the one-shot did not clear — would boot the stick again | fail-**loud** — prologue needs cleanup-on-return |
| `reordered` | firmware permanently changed the boot order | fail-**loud** — design input |
| `error` | the harness could not classify | investigate |

### What "V0 passes" requires

- Both VM firmwares (Hyper-V Gen 2 and QEMU+OVMF): `fired-once` on the
  baseline, `ignored`/refused on the fail-modes.
- BitLocker suspended → `windows_returned=y`, no recovery prompt in `notes`;
  `NoSuspend` fail-mode → the outcome recorded verbatim either way. (Amended
  2026-08-30, per the pre-registration in `rig/vm/README.md`: on the Hyper-V
  Gen 2 leg NoSuspend produced **no** prompt — the one-shot resets before
  Windows boots, so the sealed PCRs are unchanged at unseal time. A prompt
  would prove suspension load-bearing on that firmware; its absence is a
  finding about that firmware's measurement behaviour, not a failed row. The
  prologue suspends regardless — cautious default.)
- Every physical machine (≥3 vendors beyond the G16): `fired-once`, or a
  *detectable* safe failure. Physical rows are written by the harness to the
  stick's own `v0-handoff.csv` (`RUN-TEST.cmd`'s `-Auto` return check, or
  `CHECK-HANDOFF.cmd`) and transported verbatim; the machine's
  `-DumpMachine` capture from the same run is curated into
  `evaluate/windows/corpus/`.

**Progress (2026-09-08): 1 of ≥4 physical machines.** Acer Aspire A515-51G,
Secure Boot on, signed payload, `fired-once` with no keypress, through the
one-click `-Auto` flow — the project's first physical evidence on any gate
(RISKS R15). Owed: three more vendors, the fail-safe rows on real firmware,
and any physical machine with BitLocker on.

**Transporting a physical row — the procedure.** Append the stick's data
rows to this repo's CSV; never retype or edit them, and never copy the
stick's *header* line. A CSV the harness creates fresh on a stick is written
by PowerShell 5.1 `Out-File -Encoding UTF8`, which prefixes the header with a
UTF-8 **byte-order mark**; the repo's file has none, and the data rows never
do. `tail -n +2 <stick.csv> >> v0-handoff.csv` is the whole operation. Check
that the two headers otherwise match, column for column, before appending.

Any `persisted` or `reordered` on real hardware does not fail the project — it
adds a required step to the shipping prologue. Record it and note the machine.

## `v1b-alongside.csv` — installing alongside a shrunk Windows (gate V1b, risk R21)

One row per alongside-install run, appended by `rig/vm/v1b.sh verdict`
(`v1b-verdict.py`) from the run's own evidence — offline disk inspections
(`v1b-inspect.py`), the guest's shrink record, and the boot markers both
OSes write to the OEMDRV volume. Do not hand-edit; add rows by running the
bench (run-book in `rig/vm/README.md`).

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601, when the verdict was computed |
| `harness` | v1b.sh / v1b-verdict.py version |
| `firmware` | the machine or VM firmware under test |
| `secureboot` | on / off during the install **and** the boot cycles |
| `esp_size_mib` | size of the Windows-made ESP that was reused |
| `esp_free_before`, `esp_free_after` | FAT free bytes before the install and after it |
| `esp_added_bytes` | bytes of files the install added to the ESP |
| `fits_100mib_esp` | `computed-yes/NO`: would (Windows' own usage + added) fit a 100 MiB ESP — **arithmetic**, exercised only if `esp_size_mib` is ~100 |
| `bootmgfw_intact` | y/n — `EFI/Microsoft/Boot/bootmgfw.efi` sha256 identical before the install, after it, and after every cycle |
| `preexisting_changed` | every file that existed on the ESP before the install and was modified or removed by it (Windows' own `BCD`/`BCD.LOG*`/`BOOTSTAT.DAT` excluded — Windows rewrites those on boot) |
| `windows_via_grub` | count of Windows boots whose firmware `BootCurrent` was the *Fedora* entry, i.e. Windows was reached through GRUB's chainload, not the firmware's own Windows entry |
| `windows_boots`, `linux_boots` | boot-marker rows after `install-done` |
| `os_prober_stock` | did the stock install list Windows in `grub.cfg`; did it after `GRUB_DISABLE_OS_PROBER=false` + regenerate |
| `result` | see vocabulary below |
| `notes` | what the install added, the shrink numbers, anything odd |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | ESP had room, no pre-existing ESP file changed, Windows booted via GRUB and Linux booted, ≥2 cycles each | **pass for the firmware in the row** — with `secureboot=off` it closes plumbing only |
| `install-failed` | Anaconda never finished (no post-install inspection) | fail — capture the storage/anaconda logs |
| `esp-full` | the shared ESP had no room, or the install grew it | fail — design input (ESP size gate in `evaluate`) |
| `windows-files-changed` | `bootmgfw.efi` or anything else under `EFI/Microsoft/` was modified or removed | **fail-loud** — the safety net is compromised |
| `fallback-loader-replaced` | the five checks held, but the install replaced a Windows-placed file outside `EFI/Microsoft/` (in practice `EFI/Boot/bootx64.efi`, which shim-x64 overwrites) | not a bare pass — design input: the converter must snapshot and restore it; the boot checks in the row still stand |
| `windows-unbootable-via-grub` | no Windows boot arrived through the Fedora entry | fail — os-prober / chainload broken on this firmware |
| `linux-unbootable` | Linux never booted | fail |
| `cycles-incomplete` | everything worked but fewer than 2 boots of each OS were recorded | incomplete, re-run the cycles |

### What "V1b passes" requires

- `pass-plumbing` (or `fallback-loader-replaced` once the converter's own
  install step, which restores the fallback loader, is what runs) on the QEMU
  rig (SB off — the only mode this rig can run; see R15/R21) **and** on a
  Secure-Boot-enforcing firmware (Hyper-V Gen 2 or a physical machine).
- Every physical machine (≥3 vendors): `pass-plumbing` with `secureboot=on`,
  or a *detectable* failure that `evaluate` can steer to clean slate.

A VM row never closes R21's Secure Boot or vendor clauses (CLAUDE.md rule #5).

## `v3-bitlk-read.csv` — reading the kept BitLocker volume from Linux (gate V3, risk R19)

One row per config run, appended by `rig/hyperv/v3.sh verdict`
(`rig/hyperv/v3-verdict.py`) from the run's own evidence: the manifest the
Windows side wrote (`guest/v3-plant.ps1`: sha256 + size of every file in a
planted corpus and under `C:\Users`, written to the OEMDRV volume as the last
thing before a **full** shutdown) and the manifests the installed Fedora wrote
after unlocking the same partition with the recovery password
(`guest/v3-read.sh`: `cryptsetup open --type bitlk --readonly`, `mount -o ro`,
re-hash). Do not hand-edit; add rows by running the bench (run-book in
`rig/hyperv/README.md`). The recovery password never appears in any file
here — the guest scripts remove it from the transport volume before they do
anything else.

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601, when the verdict was computed |
| `harness` | v3.sh / v3-verdict.py version |
| `firmware` | the machine or VM firmware under test |
| `config` | the label the run was given (`xts128-usedspace`, `xts256-usedspace`, `xts128-full`, …) |
| `windows_build` | the Windows build that encrypted the volume |
| `bitlocker_method` | `Get-BitLockerVolume` `EncryptionMethod` (XtsAes128 / XtsAes256 / …) |
| `used_space_only` | yes / no, from `manage-bde -status` |
| `protectors` | key protector types on the volume (`Tpm+RecoveryPassword` is the Windows default) |
| `volume_status` | `FullyEncrypted` — anything else is a mid-encryption volume, a different row |
| `partition_bytes`, `bitlk_volume_bytes` | the partition's size vs the size BitLocker's own metadata records — they differ on every shrunk (keep-Windows) volume |
| `linux_context` | where the read ran: the **installed** Fedora (settle-in's context) or a live environment |
| `kernel`, `cryptsetup` | versions on the reading side |
| `unlock` | ok / failed / not-reached |
| `ntfs_driver` | the filesystem driver that produced the compared manifest — one row each per run: `ntfs-3g` (FUSE), `ntfs3` (kernel), `ntfs3 (readahead 0)` (kernel, `blockdev --setra 0` on the dm device — a diagnostic pass) |
| `corpus_files`, `corpus_identical`, `corpus_mismatch` | the planted corpus: hashed on both sides, byte-identical count, byte-different count |
| `users_files`, `users_identical`, `users_mismatch`, `users_volatile` | the same for `C:\Users`; `volatile` = files Windows itself rewrites between the hash and the shutdown (registry hives, logs, caches — listed in the run's `artifacts/v3/verdict/users-ntfs3.txt`), reported but not held against the read |
| `result` | see vocabulary below |
| `notes` | cryptsetup's warnings verbatim, the ntfs-3g cross-check, anything odd |

Rows are never deleted. When the verdict's volatile-path classification
learns a new Windows-rewritten cache (it did on 2026-09-01: a crypt32
`CryptnetUrlCache\MetaData` entry, same size, new bytes, read identically by
both Linux drivers), the run is re-verdicted and the **later rows for the
same config, kernel and driver supersede the earlier ones** — the earlier
`mismatch` rows stay as the record of what the classifier did not yet know.

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | unlocked with the recovery password, mounted read-only, every corpus file and every non-volatile file under `Users` read back byte-identical (sha256 + size), no file unreadable on Linux that Windows could read | **pass for the config in the row** — a VM row closes plumbing only |
| `unlock-failed` | `cryptsetup open --type bitlk` refused the volume or the key | fail — the config is unsupported: `evaluate` must steer it to decrypt-first or clean slate |
| `mount-failed` | unlocked, but the NTFS inside would not mount read-only | fail — investigate the driver / volume state |
| `mismatch` | at least one non-volatile file read back with different bytes | **fail-loud** — the trust-ending class (R19's "read wrong data"); never softened |
| `read-errors` | no wrong bytes, but Linux could not read a file Windows could, or a file was missing | fail — the copy would be incomplete |
| `guest-crashed` | the reading OS crashed mid-read (kernel oops; the harness's last synced `stage=` and `trace-*.txt` name where) | **fail-loud** for that driver/kernel — a hung machine mid-pull; design input (driver choice / kernel gate) |
| `incomplete` | a manifest is missing, the corpus was too small, or the pass never started (an earlier pass took the guest down) | re-run |

### What "V3 passes" requires

- `pass-plumbing` for all three configs VALIDATION V3 names — XTS-AES-128
  used-space-only (the Windows 10/11 default), XTS-AES-256, and full-disk
  (not used-space-only) — from an **installed** Fedora, for the driver `settle-in` actually uses
  (ntfs-3g, decided 2026-09-01 — RISKS R19); a kernel-driver row is
  informational.
- The same on at least one physical BitLocker machine per Windows version the
  project targets; used-space-only on a fragmented real disk is the named
  residue no VM row closes (CLAUDE.md rule #5).

## `v8-materialize.csv` — OneDrive placeholders are materialized at evaluate (gate V8, risk R8)

One row per run, appended by `evaluate/windows/Test-Materialize.ps1`. Do
not hand-edit; add rows by running the harness. A "free up space" file is
a placeholder — full size in the directory entry, no bytes on disk, fetched
by Windows' cloud files filter on first read; pulled from Linux it arrives
empty. The harness proves the harvester's materialization step
(`Harvest-UpgradeState.ps1 -Materialize`, seam `-MaterializePath`) end to
end against the **real filter** with ground truth: it registers a
temporary sync root through the Cloud Files API (the API OneDrive is built
on), creates dehydrated placeholders whose bytes only the harness knows,
runs the harvester in a **separate process**, and hashes what is on the
NTFS volume afterwards. One placeholder is one the provider refuses to
serve, so the refuse arm is exercised on every run.

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601 (the rig guest's clock was ~7 h off for its first two rows on 2026-09-08 and corrected itself mid-session; rows are transported verbatim) |
| `harness` | Test-Materialize.ps1 version |
| `os_build` | Windows build the filter belongs to |
| `provider` | `cfapi-test-provider` (the harness is the sync provider) or `onedrive` (the signed-in client, `-OneDrive`) |
| `files`, `bytes` | placeholders created and their total logical size |
| `placeholders_confirmed` | how many carried `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` **and** allocated 0 bytes on disk before the run — the setup check |
| `materialized` | files the harvester reported materialized |
| `bytes_verified` | files whose on-disk sha256 equals the ground truth **and** whose placeholder attribute is gone **and** which allocate on disk |
| `refused_expected`, `refused_reported` | files the provider refuses to serve, and how many of those the harvester reported as failed |
| `harvest_exit` | the harvester's exit code (0 = all materialized; 3 = at least one failure, the refusal) |
| `result` | see vocabulary below |
| `notes` | harness-written facts first (OS, provider, hydration policy, sizes), then FETCH_DATA counts and the harvester's summary line |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | every servable placeholder materialized and byte-verified; the refused one reported failed with a non-zero exit | **pass for the filter and the harvester** — the provider was ours, so this closes plumbing only (CLAUDE.md rule #5) |
| `pass` | the same with `provider=onedrive` | **pass** — the residue |
| `setup-failed` | the placeholders did not come out dehydrated — a harness or API problem, nothing learned about materialization | fix the harness |
| `wrong-bytes` | a file materialized but its bytes differ from the ground truth | **fail-loud** — the trust-ending class |
| `not-materialized` | a servable file stayed a placeholder, or was reported materialized without bytes on disk | fail — the harvester's judgment is wrong |
| `refusal-missed` | the unservable file was reported materialized, or a failure came back with exit 0 | **fail-loud** — "materialize, or refuse" would have written a job over an empty file |

### What "V8 passes" requires

- `pass-plumbing` on the rig and on at least one physical machine (done
  2026-09-08: rig guest Windows 10 19045, G16 Windows 11 26200 — two
  `setup-failed` rows precede the rig pass and record the two cfapi facts
  learned that day: parent directories must be placeholders, and a
  placeholder name must be bare, relative to its own directory).
- `pass` with `provider=onedrive` on a signed-in machine with Files
  On-Demand: `Test-Materialize.ps1 -OneDrive` uploads a few MB to the
  account, asks the client to free up space, then materializes — run only
  on a machine and account you own, never unattended.

## `r16-stick-writer.csv` — the stick writer refuses the wrong device (risk R16)

One row per run of `evaluate/windows/Write-UpgradeStick.ps1` — every
`-Plan` (read-only) and every `-Write`. Do not hand-edit. The writer is
the first component that writes to a device; R16 is the risk that it
writes the wrong one, before the commit line, destroying data the whole
architecture exists to protect. The rules (all must hold, each reported):
USB bus and not HDD/SSD media; not the system or boot disk and holding no
volume Windows runs from; exactly one attached disk carries the `-Target`
unique id; size matches what the person was shown; online and writable;
the person types the device's current label (or model) to confirm. The
write path re-resolves the target by unique id from a fresh enumeration
immediately before `Clear-Disk` and hands the cmdlets the object, never a
disk number.

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601 |
| `writer` | Write-UpgradeStick.ps1 version |
| `machine`, `os_build` | where it ran |
| `mode` | `plan` (nothing written) or `write` |
| `disks_attached`, `usb_disks` | how many disks the run saw, and how many on the USB bus |
| `target` | the unique id pointed at |
| `expected_bytes` | the size the person was shown, in bytes (decimal GB × 10⁹ when typed) |
| `decision` | `selected` or `refused` |
| `refusals` | why, when refused — every rule the pointed-at device broke |
| `written`, `verified` | y/n — was a disk erased and written; did every file read back against SHA256SUMS |
| `notes` | harness-written: elevation, then every attached disk with bus/media/size and the rules it broke (`writable` for a candidate), then the write's letters and file counts |

### What "R16 closes" requires

- **The refusal matrix, fabricated** (`-SelfTest`, 23 cases): the system
  disk pointed at, two sticks and a USB hard drive attached with the
  right one selected, a USB SSD enclosure, cloned serials (ambiguous),
  wrong size, off-by-one bytes, SD bus, SAS bus, offline, Windows To Go.
- **The refusal matrix, live, read-only** (done 2026-09-08): the G16 with
  the real 8 GB stick — selected only when pointed at with the right
  size; refused for a 32 GB claim; the NVMe refused on five counts. The
  rig with four SAS disks attached (system, OEMDRV, two blank VHDX
  "sticks" of 8 GB and 32 GB) — every one refused, the pointed-at VHDX
  for its bus.
- **The write, physical** (owed): several sticks **and a USB hard drive
  attached at once**, `-Write` pointed at one stick — that one erased,
  written and verified, the rest untouched (their labels and file counts
  the same before and after, recorded in `notes`). A VM cannot run this
  row: Hyper-V has no USB emulation, so every VHDX is refused for its bus
  before the write path is reached.

## `v1-live-boot.csv` — the stick boots through the handoff and verifies, nothing installed (gate V1, reversible half)

One row per run, appended by `rig/hyperv/v1-verdict.py` from two pieces
of evidence the run itself produced: the V0 harness row the guest wrote
when Windows came back (`Test-Handoff.ps1 -Check -Auto`, `v0-handoff.csv`
on the guest), and the report the `%pre` verifier left on the stick
(`upgrade_/report/verify.json`, written by `upgrade_/linux/verify.sh`
inside Anaconda's stage2). Do not hand-edit; add rows by running the bench
(`rig/hyperv/v1.sh run`). The chain under test: one-time boot entry →
shim → GRUB records `upg_fired` and boots the installer from the stick with
`upg.mode=verify` → `%pre` resolves the job's disk by identity, checks the
hardware, writes the storage `%include` and the report → reboot → Windows.

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601, when the verdict was computed |
| `harness` | v1.sh / v1-verdict.py version |
| `firmware` | the machine or VM firmware under test |
| `secureboot` | on / off at arm time (from the V0 row) |
| `handoff_result` | the V0 row's result — `fired-once` is the only pass |
| `windows_returned` | the V0 row's `windows_returned` |
| `stage2_booted` | y/n — a `verify.json` exists on the stick, i.e. Anaconda's stage2 came up from the stick and ran our `%pre` |
| `identity` | `pass` / `fail` / `not-reached` — the job's disk was found by unique id or serial **and** its size matched exactly |
| `esp` | `pass` / `fail` / `skipped` — for keep-windows, an EFI partition holding `bootmgfw.efi` was found on that disk |
| `display`, `wifi`, `audio_firmware` | `pass` / `fail` / `skipped` — see `verify.sh` for what each means; `skipped` is "nothing to test on this machine", never "not checked" |
| `storage_include` | y/n — the `%include` the install would have used was written |
| `result` | see vocabulary below |
| `notes` | the V0 row's notes prefix, then the verifier's facts (kernel, disk, how it was matched, connector and mode, ESP device, the SecureBoot variable, and from harness 0.2.0 `desktop_image=pass/fail` with the image's size, sha256 verdict against the stick's `SHA256SUMS` and the read speed in MB/s — the cutover's read-back step and RISKS R17's counterfeit-flash test, run in the live session) |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | handoff `fired-once`, stage2 booted from the stick, identity matched, storage include written, no hardware check failed, Windows returned | **pass for the firmware in the row** — a VM row (Secure Boot off on Hyper-V, no USB) closes plumbing only |
| `windows-not-returned` | no V0 row, or `windows_returned=n` | **fail-loud** — the reversible half did not come back |
| `handoff-failed` | the V0 row is not `fired-once` | see `v0-handoff.csv`'s vocabulary |
| `stage2-not-reached` | the entry fired but no report appeared — GRUB, the kernel, dracut or stage2 did not get as far as `%pre` | fail — capture the console |
| `identity-mismatch` | the verifier could not match the job's disk on this machine, or the size differed | on the rig a harness bug; on a real machine **the refusal working as designed** |
| `verify-incomplete` | identity matched but the include was not written, a hardware check failed, or (0.2.0+) the desktop image on the stick did not read back byte-identical | fail — read `verify.log` |

### What "V1 (reversible half) passes" requires

- `pass-plumbing` on the rig, then on an owned physical machine with
  Secure Boot **on** (the same signed chain that fired V0 on the Acer).
- The physical rows are what the vendor matrix's "live boot" and
  "hardware verify" columns are filled from — one half-hour visit per
  machine, read-only.

## `v2-install.csv` — the conversion itself, keep-windows, on the rig (destructive half, step 1)

One row per run, appended by `rig/hyperv/v2-verdict.py` from the run's own
evidence: offline inspections of the guest disk before the install, after
it and after the boot cycles (`rig/vm/v1b-inspect.py`: GPT and ESP file
manifest with sha256), the `outcome.json` the converter's `%post` wrote to
the stick (validated against `schemas/outcome.schema.json`), the boot
markers both OSes left on the stick, and the V0 harness row. Do not
hand-edit; add rows by running the bench (`rig/hyperv/v2.sh run`). What runs
is the product's own kickstart (`New-Kickstart.ps1`), `%pre` verifier and
`%post` checklist (`upgrade_/linux/{verify,outcome}.sh`) against a copy of
the V1b starting disk (C: shrunk, ESP 100 MiB, Windows only).

| Column | Meaning |
|---|---|
| `timestamp`, `harness`, `firmware`, `secureboot` | as the other files |
| `path`, `desktop` | from the job: `keep-windows`, `kde` / `gnome` |
| `handoff_result` | the V0 row's result for the arming reboot |
| `install_done` | y/n — `outcome.json` exists with `status=completed` |
| `outcome_valid` | y/n — it validates against the schema |
| `esp_size_mib`, `esp_free_before`, `esp_free_after`, `esp_added_bytes` | the shared ESP before and after |
| `bootmgfw_intact` | y/n — `EFI/Microsoft/Boot/bootmgfw.efi` sha256 identical before, after, and after the cycles |
| `microsoft_files_changed` | every pre-existing file under `EFI/Microsoft/` modified or removed (Windows' own `BCD*`/`BOOTSTAT.DAT` excluded), or `none` |
| `fallback_loader` | what sits at `EFI/Boot/bootx64.efi` after the install as the checklist named it: `shim` (kept on purpose while Windows is kept), `windows`, `other` |
| `snapshot_files` | files the `%pre` snapshot saved to the stick before the ESP was touched — Windows' fallback loader among them |
| `windows_entry_present`, `linux_first`, `grub_lists_windows` | the checklist's three firmware/GRUB facts from `outcome.json` |
| `windows_boots`, `linux_boots` | boot-marker rows after `install-done` (Windows rows are written by the bench when the return check answers; Linux rows by the marker unit the bench asked `%post` to install) |
| `result` | see vocabulary below |
| `notes` | outcome summary, ESP delta, boot counts, any changed files |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | install completed, outcome valid, no Microsoft file changed, `bootmgfw.efi` intact, Windows entry present and GRUB lists it, shim in the fallback slot **with** the snapshot holding Windows' copy, ≥2 boots of each OS | **pass for the firmware in the row** — SB off on Hyper-V: plumbing only |
| `handoff-failed` | the arming reboot did not fire and no install ran | see `v0-handoff.csv` |
| `install-failed` | no completed `outcome.json` — Anaconda stopped (a `%pre` refusal, a storage error, a bootloader error) | fail — read `report/anaconda.log`, `storage.log` |
| `outcome-invalid` | `outcome.json` does not validate | fail — the contract is wrong or the writer is |
| `windows-files-changed` | a Microsoft boot file changed, or `bootmgfw.efi` differs | **fail-loud** — the safety net is compromised |
| `esp-full` | the ESP had no room | design input |
| `windows-unbootable-via-grub` | no Windows boot arrived, or GRUB does not list Windows | fail |
| `linux-unbootable` | the installed system never booted | fail |
| `fallback-loader-unrecorded` | the fallback slot is not shim, or the snapshot is missing | fail — rollback could not restore Windows' loader |
| `cycles-incomplete` | fewer than 2 boots of each OS | re-run the cycles |
