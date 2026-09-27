# Validation results

This folder holds the evidence that closes the gates in
[../VALIDATION.md](../VALIDATION.md). The project closes risks with
evidence, not argument, and this is where that evidence lives, in the open.
The runs that failed are here too.

Committing results here is the point. A gate is not closed because someone
remembers it working. It is closed because a row exists.

A few words used all through this page:

- **The rig** is the set of test virtual machines on the owner's PC
  (Hyper-V, and QEMU with OVMF firmware). A **physical** row comes from a
  real machine.
- **A harness** is the test script that runs an experiment and writes its
  row. Rows are written by harnesses, not by people.
- **`pass-plumbing`** means the machinery worked end to end on that
  firmware, but only as far as a test setup can show. It never closes the
  part of a risk that needs real hardware (CLAUDE.md rule #5).
- **Fail-safe** means it failed in a way that leaves the machine as it was.
  **Fail-loud** means it failed in a way we must design around.

## `v0-handoff.csv`: the one-time UEFI boot handoff (gate V0, risk R15)

The handoff is how Windows tells the firmware (UEFI, the code that runs
before any operating system) to boot the stick exactly once, then go back
to normal.

One row per run, appended automatically by
`upgrade_/windows/Test-Handoff.ps1 -Check`. Do not hand-edit. Add rows by
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
| `keypress_free` | y / n / na: did it reach the payload with no keypress |
| `windows_returned` | y / n: back in Windows normally after |
| `notes` | recovery prompt? logo hang? anything odd. From harness 0.2.0 the row begins with a harness-written `[harness: os=…; bitlocker-via=cmdlet\|manage-bde; fired-via=fired.txt\|grubenv\|none]` prefix, then the operator's words |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `fired-once` | payload ran (marker: `fired.txt` from the Shell payload, or `upg_fired=1` in `EFI/BOOT/grubenv` from the shim payload, harness 0.2.0+), one-shot cleared itself, boot order intact | **pass** |
| `ignored` | booted straight to Windows, order unchanged | fail-**safe** (and the *expected pass* for a fail-mode run) |
| `persisted` | payload ran but the one-shot did not clear, so it would boot the stick again | fail-**loud**: the prologue needs cleanup-on-return |
| `reordered` | firmware permanently changed the boot order | fail-**loud**: design input |
| `error` | the harness could not classify | investigate |

### What "V0 passes" requires

- Both VM firmwares (Hyper-V Gen 2 and QEMU+OVMF): `fired-once` on the
  baseline, `ignored`/refused on the fail-modes.
- BitLocker suspended → `windows_returned=y`, no recovery prompt in `notes`.
  `NoSuspend` fail-mode → the outcome recorded verbatim either way. (Amended
  2026-08-30, per the pre-registration in `rig/vm/README.md`: on the Hyper-V
  Gen 2 leg NoSuspend produced **no** prompt. The one-shot resets before
  Windows boots, so the sealed PCRs (the TPM's boot measurements BitLocker
  checks) are unchanged at unseal time. A prompt would prove suspension
  load-bearing on that firmware. Its absence is a finding about that
  firmware's measurement behaviour, not a failed row. The prologue suspends
  regardless, as the cautious default.)
- Every physical machine (≥3 vendors beyond the G16): `fired-once`, or a
  *detectable* safe failure. Physical rows are written by the harness to the
  stick's own `v0-handoff.csv` (`RUN-TEST.cmd`'s `-Auto` return check, or
  `CHECK-HANDOFF.cmd`) and transported verbatim. The machine's
  `-DumpMachine` capture from the same run is curated into
  `evaluate/windows/corpus/`.

**Progress (2026-09-08): 1 of ≥4 physical machines.** Acer Aspire A515-51G,
Secure Boot on, signed payload, `fired-once` with no keypress, through the
one-click `-Auto` flow. This is the project's first physical evidence on any
gate (RISKS R15). Owed: three more vendors, the fail-safe rows on real
firmware, and any physical machine with BitLocker on.

**Transporting a physical row: the procedure.** Append the stick's data
rows to this repo's CSV. Never retype or edit them, and never copy the
stick's *header* line. A CSV the harness creates fresh on a stick is written
by PowerShell 5.1 `Out-File -Encoding UTF8`, which puts a UTF-8
**byte-order mark** (an invisible marker at the start of the file) before
the header. The repo's file has none, and the data rows never do.
`tail -n +2 <stick.csv> >> v0-handoff.csv` is the whole operation. Before
appending, check that the two headers otherwise match, column for column.

Any `persisted` or `reordered` on real hardware does not fail the project. It
adds a required step to the shipping prologue. Record it and note the machine.

## `v1b-alongside.csv`: installing alongside a shrunk Windows (gate V1b, risk R21)

Here Linux is installed next to a shrunk Windows, and Windows must still
boot afterwards. Both share the **ESP** (EFI System Partition: the small
partition where every operating system keeps its boot loader).

One row per alongside-install run, appended by `rig/vm/v1b.sh verdict`
(`v1b-verdict.py`) from the run's own evidence: offline disk inspections
(`v1b-inspect.py`), the guest's shrink record, and the boot markers both
OSes write to the OEMDRV volume. Do not hand-edit. Add rows by running the
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
| `fits_100mib_esp` | `computed-yes/NO`: would (Windows' own usage + added) fit a 100 MiB ESP. This is **arithmetic**, exercised only if `esp_size_mib` is ~100 |
| `bootmgfw_intact` | y/n: `EFI/Microsoft/Boot/bootmgfw.efi` sha256 identical before the install, after it, and after every cycle |
| `preexisting_changed` | every file that existed on the ESP before the install and was modified or removed by it (Windows' own `BCD`/`BCD.LOG*`/`BOOTSTAT.DAT` excluded, because Windows rewrites those on boot) |
| `windows_via_grub` | count of Windows boots whose firmware `BootCurrent` was the *Fedora* entry, i.e. Windows was reached through GRUB's chainload, not the firmware's own Windows entry |
| `windows_boots`, `linux_boots` | boot-marker rows after `install-done` |
| `os_prober_stock` | did the stock install list Windows in `grub.cfg`; did it after `GRUB_DISABLE_OS_PROBER=false` + regenerate |
| `result` | see vocabulary below |
| `notes` | what the install added, the shrink numbers, anything odd |

GRUB is Linux's boot menu. **Chainload** means GRUB hands over to Windows'
own boot loader (`bootmgfw.efi`). **os-prober** is the tool that finds
other operating systems for GRUB's menu.

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | ESP had room, no pre-existing ESP file changed, Windows booted via GRUB and Linux booted, ≥2 cycles each | **pass for the firmware in the row**. With `secureboot=off` it closes plumbing only |
| `install-failed` | Anaconda never finished (no post-install inspection) | fail: capture the storage/anaconda logs |
| `esp-full` | the shared ESP had no room, or the install grew it | fail: design input (ESP size gate in `evaluate`) |
| `windows-files-changed` | `bootmgfw.efi` or anything else under `EFI/Microsoft/` was modified or removed | **fail-loud**: the safety net is compromised |
| `fallback-loader-replaced` | the five checks held, but the install replaced a Windows-placed file outside `EFI/Microsoft/` (in practice `EFI/Boot/bootx64.efi`, which shim-x64 overwrites) | not a bare pass. Design input: the converter must snapshot and restore it. The boot checks in the row still stand |
| `windows-unbootable-via-grub` | no Windows boot arrived through the Fedora entry | fail: os-prober / chainload broken on this firmware |
| `linux-unbootable` | Linux never booted | fail |
| `cycles-incomplete` | everything worked but fewer than 2 boots of each OS were recorded | incomplete, re-run the cycles |

Anaconda is Fedora's installer. Shim is a small first-stage boot loader,
signed by Microsoft, that lets Linux boot with Secure Boot on.

### What "V1b passes" requires

- `pass-plumbing` (or `fallback-loader-replaced` once the converter's own
  install step, which restores the fallback loader, is what runs) on the QEMU
  rig (SB off, the only mode this rig can run; see R15/R21) **and** on a
  Secure-Boot-enforcing firmware (Hyper-V Gen 2 or a physical machine).
- Every physical machine (≥3 vendors): `pass-plumbing` with `secureboot=on`,
  or a *detectable* failure that `evaluate` can steer to clean slate.

A VM row never closes R21's Secure Boot or vendor clauses (CLAUDE.md rule #5).

## `v3-bitlk-read.csv`: reading the kept BitLocker volume from Linux (gate V3, risk R19)

On the default path, Linux later copies the person's files out of the kept
Windows partition. If that partition is BitLocker-encrypted, Linux must
unlock it and read it back exactly. This file records whether it does.

One row per config run, appended by `rig/hyperv/v3.sh verdict`
(`rig/hyperv/v3-verdict.py`) from the run's own evidence. Two sides are
compared:

- **Windows' side:** the manifest `guest/v3-plant.ps1` wrote (sha256 + size
  of every file in a planted corpus and under `C:\Users`), written to the
  OEMDRV volume as the last thing before a **full** shutdown.
- **Linux's side:** the manifests the installed Fedora wrote after unlocking
  the same partition with the recovery password (`guest/v3-read.sh`:
  `cryptsetup open --type bitlk --readonly`, `mount -o ro`, re-hash).

Do not hand-edit. Add rows by running the bench (run-book in
`rig/hyperv/README.md`). The recovery password never appears in any file
here. The guest scripts remove it from the transport volume before they do
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
| `volume_status` | `FullyEncrypted`. Anything else is a mid-encryption volume, a different row |
| `partition_bytes`, `bitlk_volume_bytes` | the partition's size vs the size BitLocker's own metadata records. They differ on every shrunk (keep-Windows) volume |
| `linux_context` | where the read ran: the **installed** Fedora (settle-in's context) or a live environment |
| `kernel`, `cryptsetup` | versions on the reading side |
| `unlock` | ok / failed / not-reached |
| `ntfs_driver` | the filesystem driver that produced the compared manifest, one row each per run: `ntfs-3g` (FUSE), `ntfs3` (kernel), `ntfs3 (readahead 0)` (kernel, `blockdev --setra 0` on the dm device, a diagnostic pass) |
| `corpus_files`, `corpus_identical`, `corpus_mismatch` | the planted corpus: hashed on both sides, byte-identical count, byte-different count |
| `users_files`, `users_identical`, `users_mismatch`, `users_volatile` | the same for `C:\Users`. `volatile` = files Windows itself rewrites between the hash and the shutdown (registry hives, logs, caches, listed in the run's `artifacts/v3/verdict/users-ntfs3.txt`), reported but not held against the read |
| `result` | see vocabulary below |
| `notes` | cryptsetup's warnings verbatim, the ntfs-3g cross-check, anything odd |

Rows are never deleted. Sometimes the verdict learns about a new cache that
Windows rewrites on its own. It did on 2026-09-01: a crypt32
`CryptnetUrlCache\MetaData` entry, same size, new bytes, read identically by
both Linux drivers. Then the run is re-verdicted, and the **later rows for
the same config, kernel and driver supersede the earlier ones**. The earlier
`mismatch` rows stay as the record of what the classifier did not yet know.

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | unlocked with the recovery password, mounted read-only, every corpus file and every non-volatile file under `Users` read back byte-identical (sha256 + size), no file unreadable on Linux that Windows could read | **pass for the config in the row**. A VM row closes plumbing only |
| `unlock-failed` | `cryptsetup open --type bitlk` refused the volume or the key | fail: the config is unsupported, so `evaluate` must steer it to decrypt-first or clean slate |
| `mount-failed` | unlocked, but the NTFS inside would not mount read-only | fail: investigate the driver / volume state |
| `mismatch` | at least one non-volatile file read back with different bytes | **fail-loud**: the trust-ending class (R19's "read wrong data"). Never softened |
| `read-errors` | no wrong bytes, but Linux could not read a file Windows could, or a file was missing | fail: the copy would be incomplete |
| `guest-crashed` | the reading OS crashed mid-read (kernel oops; the harness's last synced `stage=` and `trace-*.txt` name where) | **fail-loud** for that driver/kernel: a hung machine mid-pull. Design input (driver choice / kernel gate) |
| `incomplete` | a manifest is missing, the corpus was too small, or the pass never started (an earlier pass took the guest down) | re-run |

### What "V3 passes" requires

- `pass-plumbing` for all three configs VALIDATION V3 names: XTS-AES-128
  used-space-only (the Windows 10/11 default), XTS-AES-256, and full-disk
  (not used-space-only). They must come from an **installed** Fedora, for
  the driver `settle-in` actually uses (ntfs-3g, decided 2026-09-01, RISKS
  R19). A kernel-driver row is informational.
- The same on at least one physical BitLocker machine per Windows version the
  project targets. Used-space-only on a fragmented real disk is the named
  residue no VM row closes (CLAUDE.md rule #5).

## `v8-materialize.csv`: OneDrive placeholders are materialized at evaluate (gate V8, risk R8)

One row per run, appended by `evaluate/windows/Test-Materialize.ps1`. Do
not hand-edit. Add rows by running the harness.

The problem: a OneDrive "free up space" file is a **placeholder**. Its
directory entry shows the full size, but no bytes are on the disk. Windows'
cloud files filter fetches the bytes on first read. Pulled from Linux, the
file arrives empty. **Materializing** means forcing the real bytes onto the
disk.

The harness proves the harvester's materialization step
(`Harvest-UpgradeState.ps1 -Materialize`, seam `-MaterializePath`) end to
end against the **real filter**, with ground truth:

1. It registers a temporary sync root through the Cloud Files API (the API
   OneDrive is built on).
2. It creates dehydrated placeholders (no bytes on disk) whose bytes only
   the harness knows.
3. It runs the harvester in a **separate process**.
4. It hashes what is on the NTFS volume afterwards.

One placeholder is one the provider refuses to serve, so the refuse arm is
exercised on every run.

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601 (the rig guest's clock was ~7 h off for its first two rows on 2026-09-08 and corrected itself mid-session; rows are transported verbatim) |
| `harness` | Test-Materialize.ps1 version |
| `os_build` | Windows build the filter belongs to |
| `provider` | `cfapi-test-provider` (the harness is the sync provider) or `onedrive` (the signed-in client, `-OneDrive`) |
| `files`, `bytes` | placeholders created and their total logical size |
| `placeholders_confirmed` | how many carried `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` **and** allocated 0 bytes on disk before the run (the setup check) |
| `materialized` | files the harvester reported materialized |
| `bytes_verified` | files whose on-disk sha256 equals the ground truth **and** whose placeholder attribute is gone **and** which allocate on disk |
| `refused_expected`, `refused_reported` | files the provider refuses to serve, and how many of those the harvester reported as failed |
| `harvest_exit` | the harvester's exit code (0 = all materialized; 3 = at least one failure, the refusal) |
| `result` | see vocabulary below |
| `notes` | harness-written facts first (OS, provider, hydration policy, sizes), then FETCH_DATA counts and the harvester's summary line |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | every servable placeholder materialized and byte-verified; the refused one reported failed with a non-zero exit | **pass for the filter and the harvester**. The provider was ours, so this closes plumbing only (CLAUDE.md rule #5) |
| `pass` | the same with `provider=onedrive` | **pass**: the residue |
| `setup-failed` | the placeholders did not come out dehydrated. A harness or API problem; nothing learned about materialization | fix the harness |
| `wrong-bytes` | a file materialized but its bytes differ from the ground truth | **fail-loud**: the trust-ending class |
| `not-materialized` | a servable file stayed a placeholder, or was reported materialized without bytes on disk | fail: the harvester's judgment is wrong |
| `refusal-missed` | the unservable file was reported materialized, or a failure came back with exit 0 | **fail-loud**: "materialize, or refuse" would have written a job over an empty file |

### What "V8 passes" requires

- `pass-plumbing` on the rig and on at least one physical machine. Done
  2026-09-08: rig guest Windows 10 19045, G16 Windows 11 26200. Two
  `setup-failed` rows come before the rig pass. They record the two cfapi
  facts learned that day: parent directories must be placeholders, and a
  placeholder name must be bare, relative to its own directory.
- `pass` with `provider=onedrive` on a signed-in machine with Files
  On-Demand. `Test-Materialize.ps1 -OneDrive` uploads a few MB to the
  account, asks the client to free up space, then materializes. Run it only
  on a machine and account you own, never unattended. Since 2026-09-26
  (no download, RISKS R8) the launchers don't use the materializer, so this
  row proves a built-but-unused tool; the check that matters moved to
  `settle-in`.

## `r16-stick-writer.csv`: the stick writer refuses the wrong device (risk R16)

One row per run of `evaluate/windows/Write-UpgradeStick.ps1`: every
`-Plan` (read-only) and every `-Write`. Do not hand-edit.

The writer is the first component that writes to a device. R16 is the risk
that it writes the wrong one, before the commit line, destroying the data
the whole design exists to protect. The rules (all must hold, and each is
reported):

- USB bus, and not HDD/SSD media.
- Not the system or boot disk, and holding no volume Windows runs from.
- Exactly one attached disk carries the `-Target` unique id.
- Size matches what the person was shown.
- Online and writable.
- The person types the device's current label (or model) to confirm.

Right before `Clear-Disk`, the write path finds the target again by unique
id from a fresh list of disks, and hands the cmdlets that disk object, never
a disk number (numbers can change between two reads).

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
| `refusals` | why, when refused: every rule the pointed-at device broke |
| `written`, `verified` | y/n: was a disk erased and written; did every file read back against SHA256SUMS |
| `notes` | harness-written: elevation, then every attached disk with bus/media/size and the rules it broke (`writable` for a candidate), then the write's letters and file counts |

**First physical write (2026-09-13).** The General UDisk 8 GB stick, from
the G16 host, elevated. The first attempt stopped after `Clear-Disk`: the
real stick came back MBR-initialized and `Initialize-Disk` refused
("already been initialized"). The rig's VHDX had always come back RAW. The
row was recorded, and writer 0.1.1 initializes only a RAW disk. The second
attempt wrote and verified all 27 kit files (7 GiB FAT32 `UPGV0` + 0.5 GiB
exFAT `UPGDATA`), `written=y verified=y`. That is one device. The
several-devices clause is still owed.

### What "R16 closes" requires

- **The refusal matrix, fabricated** (`-SelfTest`, 23 cases): the system
  disk pointed at, two sticks and a USB hard drive attached with the
  right one selected, a USB SSD enclosure, cloned serials (ambiguous),
  wrong size, off-by-one bytes, SD bus, SAS bus, offline, Windows To Go.
- **The refusal matrix, live, read-only** (done 2026-09-08):
  - The G16 with the real 8 GB stick: selected only when pointed at with
    the right size; refused for a 32 GB claim; the NVMe refused on five
    counts.
  - The rig with four SAS disks attached (system, OEMDRV, two blank VHDX
    "sticks" of 8 GB and 32 GB): every one refused, the pointed-at VHDX for
    its bus.
- **The write, physical** (owed): several sticks **and a USB hard drive
  attached at once**, `-Write` pointed at one stick. That one is erased,
  written and verified; the rest are untouched (their labels and file
  counts the same before and after, recorded in `notes`). A VM cannot run
  this row. Hyper-V has no USB emulation, so every VHDX is refused for its
  bus before the write path is reached.

## `v1-live-boot.csv`: the stick boots through the handoff and verifies, nothing installed (gate V1, reversible half)

One row per run, appended by `rig/hyperv/v1-verdict.py` from two pieces
of evidence the run itself produced:

- the V0 harness row the guest wrote when Windows came back
  (`Test-Handoff.ps1 -Check -Auto`, `v0-handoff.csv` on the guest), and
- the report the `%pre` verifier left on the stick
  (`upgrade_/report/verify.json`, written by `upgrade_/linux/verify.sh`
  inside Anaconda's stage2).

`%pre` is a script the installer runs before it touches any disk. Stage2 is
the main part of the installer, loaded after the kernel starts. Do not
hand-edit. Add rows by running the bench (`rig/hyperv/v1.sh run`).

The chain under test:

```text
one-time boot entry
  -> shim
  -> GRUB records upg_fired and boots the installer from the stick
     with upg.mode=verify
  -> %pre finds the job's disk by identity, checks the hardware,
     writes the storage %include and the report
  -> reboot
  -> Windows
```

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601, when the verdict was computed |
| `harness` | v1.sh / v1-verdict.py version |
| `firmware` | the machine or VM firmware under test |
| `secureboot` | on / off at arm time (from the V0 row) |
| `handoff_result` | the V0 row's result. `fired-once` is the only pass |
| `windows_returned` | the V0 row's `windows_returned` |
| `stage2_booted` | y/n: a `verify.json` exists on the stick, i.e. Anaconda's stage2 came up from the stick and ran our `%pre` |
| `identity` | `pass` / `fail` / `not-reached`: the job's disk was found by unique id or serial **and** its size matched exactly |
| `esp` | `pass` / `fail` / `skipped`: for keep-windows, an EFI partition holding `bootmgfw.efi` was found on that disk |
| `display`, `wifi`, `audio_firmware` | `pass` / `fail` / `skipped`. See `verify.sh` for what each means. `skipped` is "nothing to test on this machine", never "not checked" |
| `storage_include` | y/n: the `%include` the install would have used was written |
| `result` | see vocabulary below |
| `notes` | the V0 row's notes prefix, then the verifier's facts (kernel, disk, how it was matched, connector and mode, ESP device, the SecureBoot variable, and from harness 0.2.0 `desktop_image=pass/fail` with the image's size, sha256 verdict against the stick's `SHA256SUMS` and the read speed in MB/s: the cutover's read-back step and RISKS R17's counterfeit-flash test, run in the live session) |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | handoff `fired-once`, stage2 booted from the stick, identity matched, storage include written, no hardware check failed, Windows returned | **pass for the firmware in the row**. A VM row (Secure Boot off on Hyper-V, no USB) closes plumbing only |
| `windows-not-returned` | no V0 row, or `windows_returned=n` | **fail-loud**: the reversible half did not come back |
| `handoff-failed` | the V0 row is not `fired-once` | see `v0-handoff.csv`'s vocabulary |
| `stage2-not-reached` | the entry fired but no report appeared: GRUB, the kernel, dracut or stage2 did not get as far as `%pre` | fail: capture the console |
| `identity-mismatch` | the verifier could not match the job's disk on this machine, or the size differed | on the rig a harness bug; on a real machine **the refusal working as designed** |
| `verify-incomplete` | identity matched but the include was not written, a hardware check failed, or (0.2.0+) the desktop image on the stick did not read back byte-identical | fail: read `verify.log` |

### What "V1 (reversible half) passes" requires

- `pass-plumbing` on the rig, then on an owned physical machine with
  Secure Boot **on** (the same signed chain that fired V0 on the Acer).
  **Done 2026-09-12:** row 4, Acer Aspire A515-51G, `secureboot=on`, via
  `RUN-VERIFY.cmd`. The stick's own files (`v0-handoff.csv`,
  `upgrade_/report/verify.json`) were fed to `v1-verdict.py` with the
  machine's firmware string. The V0 row was also transported verbatim into
  `v0-handoff.csv`.
- The physical rows fill the vendor matrix's "live boot" and "hardware
  verify" columns: one half-hour visit per machine, read-only.

## `v2-install.csv`: the conversion itself, keep-windows, on the rig (destructive half, step 1)

One row per run, appended by `rig/hyperv/v2-verdict.py` from the run's own
evidence:

- offline inspections of the guest disk before the install, after it and
  after the boot cycles (`rig/vm/v1b-inspect.py`: GPT and ESP file manifest
  with sha256),
- the `outcome.json` the converter's `%post` wrote to the stick (validated
  against `schemas/outcome.schema.json`),
- the boot markers both OSes left on the stick, and
- the V0 harness row.

Do not hand-edit. Add rows by running the bench (`rig/hyperv/v2.sh run`).

What runs is the product's own code: the kickstart (the answer file that
drives the Fedora installer with nobody at the keyboard, made by
`New-Kickstart.ps1`), the `%pre` verifier and the `%post` checklist
(`upgrade_/linux/{verify,outcome}.sh`; `%post` runs after the install). It
runs against a copy of the V1b starting disk (C: shrunk, ESP 100 MiB,
Windows only).

| Column | Meaning |
|---|---|
| `timestamp`, `harness`, `firmware`, `secureboot` | as the other files |
| `path`, `desktop` | from the job: `keep-windows`, `kde` / `gnome` |
| `handoff_result` | the V0 row's result for the arming reboot |
| `install_done` | y/n: `outcome.json` exists with `status=completed` |
| `outcome_valid` | y/n: it validates against the schema |
| `esp_size_mib`, `esp_free_before`, `esp_free_after`, `esp_added_bytes` | the shared ESP before and after |
| `bootmgfw_intact` | y/n: `EFI/Microsoft/Boot/bootmgfw.efi` sha256 identical before, after, and after the cycles |
| `microsoft_files_changed` | every pre-existing file under `EFI/Microsoft/` modified or removed (Windows' own `BCD*`/`BOOTSTAT.DAT` excluded), or `none` |
| `fallback_loader` | what sits at `EFI/Boot/bootx64.efi` after the install as the checklist named it: `shim` (kept on purpose while Windows is kept), `windows`, `other` |
| `snapshot_files` | files the `%pre` snapshot saved to the stick before the ESP was touched, Windows' fallback loader among them |
| `windows_entry_present`, `linux_first`, `grub_lists_windows` | the checklist's three firmware/GRUB facts from `outcome.json` |
| `windows_boots`, `linux_boots` | boot-marker rows after `install-done` (Windows rows are written by the bench when the return check answers; Linux rows by the marker unit the bench asked `%post` to install) |
| `result` | see vocabulary below |
| `notes` | outcome summary, ESP delta, boot counts, any changed files |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | install completed, outcome valid, no Microsoft file changed, `bootmgfw.efi` intact, Windows entry present and GRUB lists it, shim in the fallback slot **with** the snapshot holding Windows' copy, ≥2 boots of each OS | **pass for the firmware in the row**. SB off on Hyper-V: plumbing only |
| `handoff-failed` | the arming reboot did not fire and no install ran. The V0 row for this leg reads `reordered` by design (the converter puts Fedora first in BootOrder), so `reordered` and `fired-once` both count as fired (classifier fixed 2026-09-10; row 1 predates the fix and stays as written) | see `v0-handoff.csv` |
| `install-failed` | no completed `outcome.json`: Anaconda stopped (a `%pre` refusal, a storage error, a bootloader error) | fail: read `report/anaconda.log`, `storage.log` |
| `outcome-invalid` | `outcome.json` does not validate | fail: the contract is wrong or the writer is |
| `windows-files-changed` | a Microsoft boot file changed, or `bootmgfw.efi` differs | **fail-loud**: the safety net is compromised |
| `esp-full` | the ESP had no room | design input |
| `windows-unbootable-via-grub` | no Windows boot arrived, or GRUB does not list Windows | fail |
| `linux-unbootable` | the installed system never booted | fail |
| `fallback-loader-unrecorded` | the fallback slot is not shim, or the snapshot is missing | fail: rollback could not restore Windows' loader |
| `cycles-incomplete` | fewer than 2 boots of each OS | re-run the cycles |

## `r18-prologue.csv`: the prologue as product code: re-validate, the disk check, the shrink, the handoff (risk R18, step 1b)

The **prologue** is the Windows-side part of the conversion. It runs after
the person types CONVERT and before the machine boots the stick: it checks
the machine again, gets the disk healthy enough, shrinks Windows, and arms
the handoff.

**Numbering (stated 2026-09-26):** "row N" everywhere in these docs means
line N of the CSV file, the header being line 1 - so the first data row
is row 2. Rows 5 onward were always numbered this way; the rig rows once
called 1-3 are rows 2-4.

One row per run, appended by `rig/hyperv/prologue-verdict.py` from the
run's own evidence:

- **The prologue's record on the stick** (`upgrade_/prologue.json`).
  `Invoke-Prologue.ps1` writes it at every stage change. Its `prologue`
  block is what `outcome.json` must carry.
- **The prologue's return record** (`upgrade_/prologue-return.json`): the
  handoff, classified by the prologue itself when Windows next boots.
- **`outcome.json`**, validated against the schema, with its `prologue`
  block compared to the record.
- **The bench's fault-injection note** (`fsutil dirty set C:` before the
  flow). This sets NTFS's "dirty" flag, which tells Windows the volume
  needs checking.
- **The offline GPT inspections** before and after: did C: shrink by
  exactly what the prologue says it freed? (The GPT is the disk's partition
  table.)

Do not hand-edit. Add rows by running the bench
(`rig/hyperv/prologue.sh run`).

What runs is the one-click flow a person double-clicks (`RUN-CONVERT.cmd`:
scanner → job writer → kickstart → the typed word →
`Invoke-Prologue.ps1 -Start`). It runs against a copy of the **unshrunk**
install-day disk (`UPGRIGHV.fresh.vhdx`: C: 85.8 GB, 100 MiB ESP, BitLocker
off). Then the install runs as in `v2-install.csv`, whose verdict writes its
own row for the same run.

| Column | Meaning |
|---|---|
| `timestamp`, `harness`, `firmware`, `secureboot` | as the other files (`secureboot` from the return record) |
| `dirty_injected` | y/n: the bench set NTFS's dirty flag on C: before the flow |
| `revalidated` | y/n: job.json matched the live machine on every fact the prologue re-checks |
| `scan` | what `Repair-Volume -Scan` (read-only) answered, verbatim. It chooses the rung |
| `disk_health` | `Get-PhysicalDisk` HealthStatus read immediately before the repair was scheduled. Anything but `Healthy` is a refusal |
| `method` | `spot-fix` / `chkdsk-f` / `none`: the rung the prologue scheduled |
| `restarts` | restarts the check took |
| `wininit_1001` | y/n: the boot-time check left its Wininit event 1001 (the text is in `notes`) |
| `found000` | whether the check left a `found.000` |
| `dirty_after` | `clean` / `dirty` / `unknown`: the flag after the check |
| `remeasured_gb`, `remeasured_by`, `diskpart_gb` | shrinkable space at the moment it mattered: the number branched on, which read-only path gave it, and diskpart's independent figure |
| `fork_taken` | `keep-windows` / `clean-slate` / `stop`: the pre-chosen fork, taken |
| `requested_bytes`, `freed_bytes` | the planned shrink and what `Resize-Partition` freed |
| `partition_shrunk` | y/n: the offline GPT shows C: smaller by exactly `freed_bytes` |
| `hibernation_off`, `pagefile_off` | the mitigations the prologue applied |
| `bitlocker_before`, `bitlocker_suspended` | as the outcome records them |
| `armed`, `handoff_result` | the one-shot entry was armed; the return record's classification (`reordered` is expected here, because the converter puts Fedora first) |
| `install_done`, `outcome_valid` | as `v2-install.csv` |
| `record_in_outcome` | y/n: `outcome.json`'s `prologue` block equals the record the prologue left on the stick |
| `result` | see vocabulary below |
| `notes` | the prologue version and stage, the scan and chkntfs answers, the Wininit text, the shrink plan, the GPT delta, the outcome's stop reason if any; from prologue 0.4.0 the step-1b trigger (`trigger=dirty-flag`, or `repair-queued` when the bit was clean and Windows had a repair queued, R18, 2026-09-17) |

A **rung** is one step on the ladder of disk repairs, gentlest first: a
`spot-fix`, then `chkdsk-f`. The **fork** is the branch the person chose in
advance (`keep-windows`, `clean-slate` or `stop`). The prologue takes it
once the free space has been re-measured.

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | flag confirmed by the scan, health read `Healthy`, a rung scheduled, restart taken, flag clear after, re-measured by a read-only path, fork `keep-windows`, C: shrunk by exactly the request (GPT agrees), handoff armed and fired, install completed, `outcome.json` valid and carrying the prologue's record | **pass for the firmware in the row**. A rig row closes plumbing, never the real-hardware clause (the Aspire's flag is the residue) |
| `stopped-<stage>` | the prologue wrote a stopped `outcome.json` at that stage: an honest refusal. The reason is in `notes` | read it: on the rig a harness or product bug, on a real machine possibly the refusal working |
| `prologue-not-run` | no record on the stick: the flow never reached the prologue | fail: read `convert.log` |
| `flag-not-confirmed` | the flag was injected but the prologue did not find it | fail: the scan or the fsutil read is wrong |
| `check-not-run` | the check was needed but no Wininit 1001 appeared and the flag did not clear | fail: the scheduling did not take |
| `flag-persists` | the check ran and C: is still flagged | fail-loud: the ladder did not clear it. Design input |
| `not-remeasured` | neither read-only path gave a number after the check | fail |
| `fork-<x>` | the fork landed elsewhere than keep-windows (recorded, not a pass for this leg) | design input |
| `shrink-short` | `Resize-Partition` freed less than planned, or the GPT disagrees with the record | **fail-loud** |
| `not-armed`, `handoff-failed`, `install-failed`, `outcome-invalid` | as their names; see `v2-install.csv` | fail |
| `record-mismatch` | `outcome.json` carries a `prologue` block that is not the prologue's record | fail: `outcome.sh` lost the hand-over |
| `resume-attended` | (prologue 0.3.0+) a resume ran with a session present: someone was signed in, or the task ran as the person | fail for the walk-away clause. The row's other columns still stand |
| `resume-with-autologon` | the bench did not switch the guest's autologon off, so the row cannot say whether the resume needed a sign-in | incomplete: re-run with `autologon off` |

From prologue 0.3.0 `notes` also carries one `resume N (<stage>): <account>
session <id> interactive <bool> explorer <bool> uptime <s> s stick after <s> s`
entry per restart, straight from `state.Resumes`, and `bench
AutoAdminLogon=0|1`. A walk-away row is one where every resume reads
`NT AUTHORITY\SYSTEM session 0 interactive False explorer False` and the
bench line reads `=0`. (Session 0 is the one with no desktop, so nobody was
signed in.)

**Rows so far (2026-09-12, rig, Secure Boot off, BitLocker off).**

- **Row 2, `stopped-arm-handoff`.** Everything up to the shrink held: flag
  confirmed `NoErrorsFound`, `Healthy`, spot-fix scheduled, restart, flag
  clear, 57.8 GB by both paths, fork keep-windows, 25 GB freed. Then the
  arm stopped on a prologue bug (the resumed copy copying itself over
  itself). The stop grew C: back, wrote the stopped `outcome.json` and
  scrubbed the credentials. This is the refusal path's first evidence.
- **Row 3** is the same second run **misjudged** by a verdict script that
  read the wrong key of the return record (`handoff-failed` where the
  record says `reordered`).
- **Row 4** is that run judged correctly, `pass-plumbing`. Wininit 1001
  was recorded: autochk ran the **full** three-stage check on the flagged
  volume, 6 s, "found no problems" (the scheduled spot-fix rung became
  Windows' own boot-time check). C: shrunk 79.9 → 54.9 GB with the GPT
  agreeing to the byte. The install completed (`v2-install.csv` row 4,
  whose `handoff_result` reads `no-row` for the same verdict-key reason),
  and `outcome.json`'s `prologue` block equals the record.

Two rig facts: `Repair-Volume -SpotFix` on the boot volume answers
`NoErrorsFound` and leaves the flag for autochk (`chkntfs` says "C: is
dirty"), and Wininit logs event 1001 about 17 s *after* logon.

**Row 5 (2026-09-13, Acer Aspire A515-51G, InsydeH2O V1.21, Secure Boot on,
Windows 11 Home): the first physical step-1b row, `stopped-volume-check`.**
The flag was real. It was confirmed by the scan, `Healthy` was read, the
spot-fix was scheduled and the restart taken. Then no boot-time check was
logged, C: was still flagged, a `found.000` was already present, and the
rescan said `NoErrorsFound`. The prologue refused to escalate. The
read-only diagnostic that followed (RISKS R18, 2026-09-13) showed 18
corruption records queued for offline repair for weeks, `Get-Volume` "Full
Repair Needed", and 30 bad-block events on the SSD holding C:. That is the
dying-drive branch. The refusal stands. The guardrails that let it get that
far were strengthened the same day (scanner 0.2.0 and prologue 0.2.0 read
the drive's error log, SMART and the volume status; RISKS R18).

**Row 7 (2026-09-20, the same machine, prologue 0.4.0, the
acknowledged-data-loss path): `stopped-volume-check`, `trigger=repair-queued`.**
What happened, in order: the launcher's five steps; re-validation; the disk
gate lifted by the typed sentence (465 bad-block events); `chkdsk-f` chosen
and accepted by Windows; the restart; and the resume as SYSTEM in session 0
with nobody signed in (36 s after boot, stick after 3 s). Then no Wininit
1001 came in two minutes of polling, and the prologue refused to measure.
Read it with its sequel: the read-only diagnostic that evening (RISKS R18)
found the check had already run on 09-15, and the trigger had fired on a
week-old event. 0.5.0 corrects that. The row stands as what 0.4.0 did, and
as the first physical evidence of the walk-away resume on the product path.

**Row 8 (2026-09-20 evening, the same machine, prologue 0.5.0, kit
08a852a): `stopped-shrink`.** Both 0.5.0 corrections fired as predicted:
trigger `none`, cold 0 GB naming `\hiberfil.sys`, hibernation and pagefile
off, one restart, the SYSTEM resume (20 s after boot, stick after 2 s).
The re-measure answered 7.2 GB (Storage API) / 3.3 GB (diskpart) against
25 GB needed. The pre-chosen fork was `stop`, so it stopped, before any
shrink or handoff. `secureboot` reads `unknown` and `partition_shrunk`
`unreported` because a run that never arms has no return record, and a
physical run has no offline GPT inspections. Those are the script's words,
not edits. Read it with RISKS R18's fourth-run entry:

- The next unmovable file was System Restore's shadow-copy storage (the
  "about 42 GB" expectation is corrected there).
- The two paths differ because the person signed in and Slack started
  between them.
- The stop left hibernation and the pagefile off while saying "Windows is
  as it was". That was a bug, fixed in prologue 0.5.1 and proven on this
  machine in row 10.

**Row 9 (2026-09-22, the same machine, prologue 0.6.0, job writer 0.7.0,
kit 3dbf910): `stopped-confirm`.** Hibernation and the pagefile had been
put back (with consent, over SSH) before the run.

- The cold measurement this time was 3.2 GB by both paths, pinned by NTFS's
  change journal (`\$Extend\$UsnJrnl:$J`). The job writer did not count the
  journal as something it could clear, so it wrote a clean-slate job
  (`forced-no-room`), although the person had chosen `stop` and the
  launcher had described keep-Windows before CONVERT.
- The prologue staged 0 files (the job carries no folder map yet) and
  stopped at clean slate's unbuilt confirm gate, 52 s after CONVERT, with
  no restart and nothing changed.
- `fork_taken` reads `clean-slate`, and `hibernation_off`/`pagefile_off`
  read `n`, because the mitigation never ran. `before: hibernation=None`
  because nothing was recorded before an act that did not happen.
- Neither 0.5.1's restore nor 0.6.0's restore-point deletion fired.

Read it with RISKS R18's fifth-run entry: two defects (a "stop" turned into
a wipe job; "your files are staged" after staging none). Both were fixed
the same day in job writer 0.8.0 and prologue 0.7.0. The job writer's fix
held on this machine in rows 10 and 11.

**Row 10 (2026-09-23, the same machine, now Windows 11 26200, prologue
0.8.0, job writer 0.9.0, kit 537e093): `stopped-shrink`.** The job was
keep-windows under `stop` (0.8.0's fix, on a real machine).

- Cold: 8.4 GB, pinned by the change journal.
- The journal rung fired twice (once per boot) and bought 1.1 GB.
- Restore-point deletion ran and deleted 0 of 2 (the row's notes say
  "2 -> 2").
- The pagefile rung's restart was followed by two Windows Update restarts
  nobody asked for (the two `resume` entries).
- Re-measured 9.5 GB of 25, fork `stop`.
- The stop put the journal, hibernation and the pagefile back. This was
  read back after two restarts on 2026-09-26 (`post-run-state.txt` in the
  run's gitignored artifacts).

Read it with RISKS R18's sixth-run entry and the new R25.

**Row 11 (2026-09-26, the same machine, prologue 0.9.0, kit 29a1d9f):
`stopped-shrink`.** 7.2 GB, cold and after the pagefile rung, pinned by
`\$Mft::$BITMAP` (NTFS's own metadata; Defrag 259 names it at the same
cluster six times). The update gate read nothing waiting (notes: `windows
update: before-changes pending=False`). No restore-point or journal step
was reached. The stop put hibernation and the pagefile back. Read it with
RISKS R18's seventh-run entry: on this disk keep-Windows is refused.

### What "the prologue passes" requires

- `pass-plumbing` on the rig with the injected flag. That covers the
  plumbing: scan → guardrail → rung → restart → outcome read → re-measure →
  fork → shrink → arm → install → record carried through.
- Then the **Acer Aspire**, whose C: carries a real flag Windows has kept
  through several restarts. That gives the first physical row for step 1b
  and, if the re-measured number fits, the first physical install with
  Windows kept. What a real flag does under the ladder is exactly the
  residue no test setup can fake (rule #5). The rig's `fsutil dirty set` is
  only our model of it.

## `r21-rollback.csv`: rollback: Windows first again, its fallback loader restored from the snapshot (risk R21)

Rollback undoes the boot changes so the machine starts Windows again, the
way it did before the conversion.

One row per run, appended by `rig/hyperv/rollback-verdict.py` from the
run's own evidence:

- the rollback's record on the stick (`upgrade_/rollback.json`, written by
  `Invoke-Rollback.ps1` run from the kept Windows through `ROLLBACK.cmd`),
- the offline ESP manifests before the conversion (Windows' own
  `EFI/Boot/bootx64.efi`), after the boot cycles and after the rollback,
  and
- the boot marker the bench writes when Windows comes up afterwards with
  **no key pressed**.

Do not hand-edit. Add rows by running `rig/hyperv/prologue.sh rollback` on a
converted rig disk.

| Column | Meaning |
|---|---|
| `record` | y/n: `rollback.json` exists |
| `restored` | y/n: the rollback copied Windows' fallback loader back (it had been shim) |
| `loader_matches_snapshot` | y/n: offline, `EFI/Boot/bootx64.efi` after the rollback is byte-identical (sha256) to the pre-conversion inspection's |
| `windows_first` | y/n: `{bootmgr}` leads the firmware display order after `bcdedit ... /addfirst` |
| `windows_direct_boot` | y/n: the next start reached Windows with no key pressed (bench marker `direct-after-rollback`) |
| `linux_partitions_intact` | y/n: the GPT after the rollback equals the GPT after the cycles |
| `efi_fedora_intact` | y/n: every file under `EFI/fedora/` unchanged |
| `result` | `pass-plumbing` when all of the above; else the first failing item by name (`record-missing`, `not-restored`, `loader-mismatch`, `windows-not-first`, `windows-not-direct`, `linux-touched`, `efi-fedora-touched`) |
| `notes` | the shas and orders before/after, the GPT and `EFI/fedora` comparison |

**Row 1 (2026-09-12, rig):** `pass-plumbing` on the disk converted by the
prologue run above. Shim's copy (`4773d74d…`) was replaced by Windows'
(`e721eb27…`, equal to the pre-conversion inspection), `{bootmgr}` first,
a keyless start reached Windows directly, GPT and `EFI/fedora` untouched.

A rig row closes the plumbing of the restore half of the R21 snapshot. The
firmware clause is a physical-matrix column: does *this* vendor's firmware
honour `{fwbootmgr} displayorder` from Windows after a Linux install put its
own entry first?


## `walkaway-probe.csv`: the resume fires with nobody signed in (the walk-away clause of V0/V4; risk R24)

"Walk away" means the person clicks once and leaves. After each restart
the conversion must carry on by itself, before anyone signs in. This probe
tests only that.

Written by **the prologue's own `-Probe` task, from session 0**
(`Invoke-Prologue.ps1 -Probe`, launched by `RUN-PROBE.cmd` on the kit). Not
by a bench script, and never by hand.

- The probe registers the same SYSTEM startup task the conversion uses and
  restarts once.
- The task itself appends the row to `upgrade_/walkaway-probe.csv` on the
  stick (and `upgrade_/probe.json` beside it), queues the sign-in notice
  and removes itself.
- It is read-only: nothing on the disk changes but the locked state
  directory and the one-shot task.

A physical row is **transported verbatim** from the stick's file
(`tail -n +2`, the `harness` column inserted after `timestamp`). Rig rows
come through `rig/hyperv/prologue.sh probe` the same way. Rows 1-2
(2026-09-13): the rig, then the Acer Aspire A515-51G (Secure Boot on,
Windows 11 Home, a real USB stick).

| Column | Meaning |
|---|---|
| `timestamp` | UTC, ISO 8601, when the task wrote the row (after the restart) |
| `harness` | `0.1.0-hv` (rig) or `<prologue>-physical`, inserted at transport |
| `prologue_version` | the version that ran |
| `vendor`, `model`, `bios` | `Win32_ComputerSystem` / `Win32_BIOS` SMBIOSBIOSVersion |
| `os` | Windows caption and build |
| `secure_boot` | on / off / unknown at arm time |
| `stick_bus` | the stick's bus as Windows reports it: `USB` on a real machine, `SAS` on the Hyper-V rig |
| `run_as` | the account the resume ran as. `NT AUTHORITY\SYSTEM` is the design |
| `session_id` | the resume's session. `0` means no desktop |
| `interactive` | `[Environment]::UserInteractive` in the resume |
| `explorer_running` | whether any `explorer.exe` existed (a signed-in desktop) |
| `uptime_s` | seconds from boot to the resume starting |
| `stick_wait_s` | seconds the resume polled before the stick's volume id appeared (real firmware enumerates USB late; the poll allows 120) |
| `notice` | `queued` (RunOnce notice for the next sign-in) or `shown` (a popup, because a session existed) |
| `task_removed` | y/n: the one-shot task unregistered itself |
| `result` | see vocabulary below |
| `notes` | state stage, resume stage, the resume's UTC |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `resumed-unattended` | SYSTEM in session 0, no explorer, stick found, task removed | **pass for the firmware/edition in the row** |
| `resumed-attended` | a session existed when the resume ran: someone signed in early, or the task ran as the person | not a walk-away row; re-run without signing in |
| `stick-not-found` | the task ran but the stick's volume id never appeared within the poll | fail: USB enumeration slower than the poll, or the stick dropped (R16's loose-stick clause) |
| `task-not-removed` | everything else held but the task could not unregister itself | fail-loud: clean-up bug |

### What the probe closes

The **resume mechanism** for that firmware and edition: a SYSTEM startup
task registered by the prologue fires before any sign-in, sees a real
stick, and cleans up. It does **not** close the conversion (no disk check,
no shrink, no handoff; those are `r18-prologue.csv`'s rows). A rig row
closes plumbing only, because its stick is a SCSI disk, present from
power-on. Residue named in R24: managed devices whose policy blocks task
registration, Fast Startup's hybrid shutdown, BitLocker with a PIN.

## `v5-controller-mode.csv`: the "Storage controller mode" check on real Intel RST hardware, both directions (gate V5, risk R1)

Some Intel machines ship with the disk controller in RAID/RST mode (Intel
Rapid Storage Technology), or behind VMD (Volume Management Device, a newer
Intel feature). Linux often cannot see the disk in those modes. The scanner
has a check for it. This file records whether the check fires on real
hardware when it should (**positive direction**) and stays quiet when it
should (**negative direction**).

One row per scanner run in one SATA mode, written by **`rig/v5-verdict.py`**,
never by hand, from two sources:

- **The one-click path** (`RUN-STORAGE-MODE.cmd` → `Test-StorageMode.ps1`).
  The harness:
  1. scans in the mode the machine is in,
  2. restarts straight into the firmware setup for the person to change
     SATA Mode,
  3. boots Safe Mode once through a copied boot entry,
  4. scans again as SYSTEM with nobody signed in,
  5. asks for the mode back, and scans a third time.

  It leaves `upgrade_\storage-mode\leg1..3\` (report + capture per mode) and
  the record `storage-mode.json` on the stick. `rig/v5-verdict.py
  --from-run <that folder>` writes one row per leg. The mode-as-set is what
  the harness **asked** the person to set (`initial` for leg 1). Nobody
  types anything.
- **The two-file path** (a plain `RUN-SCANNER.cmd` run): `rig/v5-verdict.py
  --sata-mode-set raid|ahci|absent <upgrade-report-*.json>
  <machine-capture-*.json>`. The one operator input is the mode the setup was
  set to.

Either way, `result` compares the mode word against the PCI class code the
controller actually declared to Windows. So a setting that did not take, or
a mislabelled pair of files, records as `mode-mismatch`, not as evidence. A
capture marked `Synthetic` is refused (`error`): the VM spoof is plumbing,
never a V5 row. Reports are discarded after the row is written (they hold the
machine's details and are gitignored). Captures are curated into
`evaluate/windows/corpus/` with an `Expected` block, one per mode, so the
machine is replayed on every `-SelfTest` forever.

| Column | Meaning |
|---|---|
| `timestamp` | the report's `ScannedUtc`: when the scanner ran on the machine |
| `harness` | `v5-verdict <version>` |
| `vendor`, `model`, `firmware`, `os` | the machine under test, from the report's `System` block (`BiosVersion`, OS caption + build) |
| `scanner_version` | the scanner that produced the report |
| `leg` | one-click path: 1, 2 or 3; blank for a two-file row |
| `sata_mode_set` | `initial` (leg 1: whatever the machine was in), `raid` (RST Premium / Optane / RAID), `ahci`, or `absent` (two-file path: the setup has no SATA-mode option) |
| `controller` | every Intel PCI mass-storage-class device (or one with an `iaStor*` service) in the capture, by Windows name; `;`-joined |
| `pci_id` | its `vvvv:dddd` |
| `class_code` | the PCI class+subclass Windows published in `CompatibleIDs` (`0104 (RAID)`, `0106 (AHCI)`, `0108 (NVMe)`): the check's third signal |
| `compatible_ids` | the controller's full `CompatibleIDs` list, verbatim |
| `driver_service` | the driver service bound (`storahci`, `stornvme`, `iaStorAC`, `iaStorAVC`, `iaStorVD`, …) |
| `check_status`, `check_detail` | the `Storage controller mode` line of the report |
| `verdict` | the report's overall level |
| `result` | see vocabulary below |
| `flow_result` | one-click path: the harness's own verdict on the run: `restored` (three legs, original mode back), `mode-unchanged` (leg 2 saw the same mode: no option, or not saved), `not-restored`, `no-intel-controller`, `cancelled`, `error`; `single-run` for a two-file row |
| `resume_run_as` | who ran the resume that produced this leg. `SYSTEM unattended` is the design; `launcher` for leg 1 |
| `safe_boot` | the Safe Mode boot before this leg, as its evidence records it: `y (user sign-in, option 1, runonce-signin)` means the person signed in at the Safe Mode screen and the `*`-RunOnce left the marker and restarted (Task Scheduler does not run the task in Safe Mode; rig 2026-09-14); `n` means no marker reached the resume |
| `fw_reboot` | how the restart into setup before this leg was done: `fw` (`shutdown /r /fw`, straight into the firmware setup) or `plain` (the firmware refused `/fw`; the person pressed the setup key) |
| `notes` | harness facts first (scan/capture times, controllers counted, classes seen, whether an RST or VMD service was bound; for the one-click path the stage, the mode seen and the cleanup read-back), then any `operator:` words |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `fail-fired` | set to RAID/RST (or `initial` and RAID seen); a controller declares class `0104` (or `iaStorVD` is bound); the check said `fail` and the report is RED | **the positive direction passes** for that machine |
| `ok-passed` | AHCI seen; no RAID class, no RST service; the check said `ok` | **the negative direction passes** |
| `warn-rst-on-ahci` | AHCI seen but an Intel RST driver (`iaStor*`) is bound; the check said `warn` | the negative direction, as the R7 guard designed it. Linux must be shown to see the disk for it to count (cite the row) |
| `missed` | RAID class or `iaStorVD` present but the check did **not** fail | **the check is wrong**: fix the check, add the capture as a self-test case, never touch the row |
| `false-fail` | no RAID class, no VMD service, yet the check failed | over-refusal: fix the check |
| `mode-mismatch` | the mode asked/set disagrees with the class code seen | not evidence: the setting did not take (setup had no option, or it was not saved), or the wrong pair of files |
| `option-absent` | two-file path: the setup exposes no SATA-mode option; the AHCI-side run recorded for the record | this machine cannot give the positive row |
| `no-intel-controller` | the capture has no Intel storage controller (AMD, or a VM) | not V5 evidence; on the rig it is the flow's plumbing row (read `flow_result`) |
| `error` | files unreadable, from different machines, unelevated, or a `Synthetic` capture | investigate |

### What "V5 passes" requires

At least one physical machine with **both** a `fail-fired` row and an
`ok-passed` or `warn-rst-on-ahci` row. Its RAID-mode capture must be curated
in the corpus with `Expected` `Storage controller mode: fail` and marked real
(not `Synthetic`).

Which of the check's three signals fired is read from the row:

- `class_code` `0104` with a pre-VMD service (`iaStorAC`/`iaStorAVC`) is
  signal 3: the **pre-VMD RST clause** (Skylake-Comet Lake remap
  generation).
- A `pci_id` from the kernel's `vmd.c` table, or `driver_service`
  `iaStorVD`, is signal 1 or 2: **VMD proper** (11th gen+).

A row from a pre-VMD machine closes only the pre-VMD clause. VMD proper
still takes an 11th-gen-or-newer machine with RST on. R1 carries the
residue.

The one-click flow itself has its own plumbing clause. A rig run
(`rig/hyperv/prologue.sh storage-mode`) cannot change a SATA mode, so its
rows are `no-intel-controller` with `flow_result` `mode-unchanged`. What it
proves is the mechanics: Safe Mode boot through the copied entry, the
sign-in RunOnce's marker and restart, the SYSTEM scan on the way back, the
cleanup read-back. `safe_boot` and `resume_run_as` are where to look.

## `harvest-folder-map.csv`: what the folder map finds on real machines (roadmap item 3; risks R5, R6, R8, R26)

The **folder map** is the harvester's list of where the person's files
live (Documents, Pictures and the other known folders), and how big they
are.

One row per read-only measurement with the harvester's `-FolderMapOut`
(the step the job writer runs). It is a measurement, not a pass. It is the
evidence R26 asks for about the folder map's coverage, and the size a clean
slate or the discard offer would have to stage. Hand-written from the
harvester's JSON. Personal file names stay out of the row.

| Column | Meaning |
|---|---|
| `timestamp`, `harvester` | when, and `HarvestVersion` |
| `machine`, `os_build` | the machine measured |
| `folders_found`, `files`, `bytes` | the six known folders that exist, and their contents |
| `online_only` | OneDrive online-only files found (R8; a job refuses while any remain) |
| `unreadable` | sub-folders Windows would not list (R6; a job refuses on any) |
| `files_over_4gib`, `largest_file_bytes` | FAT32 cannot hold a file over 4 GB |
| `onedrive_folders` | how many of the six are redirected into OneDrive |
| `other_profiles` | other accounts' profiles (R5; a clean slate refuses on any) |
| `stick`, `needed_bytes`, `fits` | the stick volume measured against, what the folders need on it, the answer (`n/a` with no stick) |
| `notes` | how it was run, and what lies outside the map |

## `v9-erase.csv`: the one-click erase and install (gate V9, risk R27)

This is the path that erases every internal drive and installs Fedora,
keeping nothing. A 2-minute countdown in the installer is the last chance
to cancel.

Most rows are the rig. Line 11 is the Aspire's physical run 9 (2026-09-26,
`fail`: it erased and installed unattended but came up at a text login,
fixed in kickstart generator 0.3.0). Lines 12-13 are the rig's GNOME and
KDE desktops seen and signed in (2026-09-27).

One row per arm, written by `rig/hyperv/v9-verdict.py` from the arm's own
evidence (`rig/hyperv/v9.sh`; never by hand). The guest is a copy of the
rig's install-day Windows disk plus a blank 64 GiB second disk.

| Column | Meaning |
|---|---|
| `arm` | A refuse (a job naming an absent disk, armed by the V0 harness), B cancel (a key during the countdown), C erase (the countdown left alone) |
| `prologue_version`, `verify_version` | the code under test (`harness` = armed by Test-Handoff, not the prologue) |
| `identity` | verify.json: every drive the job names found by id and exact size |
| `countdown` | countdown.json: `elapsed`, `cancelled`, or `none` (never shown) |
| `outcome_status`, `stopped_at`, `commit_crossed`, `outcome_valid` | outcome.json, and whether it validates against the schema |
| `disks_unchanged` | both disks' GPT and first MiB identical before and after (v9-inspect.py, offline) |
| `system_gpt_after`, `home_gpt_after` | the partition tables afterwards |
| `fedora_booted`, `password_matches`, `home_on_second_disk` | the first Linux boot's marker line (rig-only bench instrumentation): it booted, the account's stored hash is the job's, `/home` is on the second disk |
| `graphical_login` | (from 2026-09-26) the person's choice held at first boot: `desktop` = graphical.target with the display manager running, `console` = multi-user.target with none; `not-recorded` on rows from before the check |
| `result` | `refused-before-countdown` / `cancelled-untouched` / `erased-installed` pass their arm; `fail` is kept, never removed |
