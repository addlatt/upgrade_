# Validation results

This folder holds the evidence that closes the gates in
[../VALIDATION.md](../VALIDATION.md) and the risks in [../RISKS.md](../RISKS.md).
Risks close on evidence, not argument, and this is where it lives, in the
open, failed runs included: a gate is closed because a row exists. This page
says what each CSV's rows **mean**: columns, result words, and what a pass
requires. What happened in a run is in the log ([../log/](../log/)); where
everything stands is in [../STATUS.md](../STATUS.md), and nowhere else.

## How to read a row

- **Line N** is the CSV's line number as an editor shows it. Line 1 is the
  header, so the first data row is line 2. "Row N" in older writing means the
  same (stated 2026-09-26; the rig rows of `r18-prologue.csv` once called 1-3
  are lines 2-4).
- Rows are **appended, never edited**. A `fail` stays. When a verdict script
  itself was wrong, the same evidence is judged again and the new row goes
  after the old one; both stay.
- Rows are written by **harnesses** (the script that runs an experiment and
  writes its row), never by hand, except where an entry says so.
- `docs/STATUS.md` cites a row as `` `file.csv` line N, `result` ``, and the
  checker reads that line's `result` column.
- **The rig** is the test virtual machines on the owner's PC (Hyper-V, and
  QEMU with OVMF); a **physical** row comes from a real machine.
  **`pass-plumbing`**: the machinery worked end to end on that firmware, as
  far as a test setup can show; it never closes the part of a risk that needs
  real hardware (CLAUDE.md rule #5). **Fail-safe**: the machine was left as
  it was. **Fail-loud**: a failure we must design around. `timestamp` is
  UTC, ISO 8601, unless the entry says otherwise.

## Transporting a physical row

A harness on the kit writes its row to the stick's own copy of the CSV.
Append the stick's data rows to the repo's file, never retyped or edited, and
never its *header* line: PowerShell 5.1 `Out-File -Encoding UTF8` puts a
UTF-8 byte-order mark (an invisible marker) before it; the repo's file has
none. `tail -n +2 <stick.csv> >> <repo.csv>` is the whole operation, once the
two headers are seen to match. A product stick has no bench marker: the boot
lines and each drive's partition table (its first MiB) are gathered over the
carried SSH key with the marker's own commands (kept under
`rig/hyperv/artifacts/`), so `disks_unchanged` compares partition tables
only. What only a person at the machine can see is passed to the verdict as
observations, and the row says so. The verdict writes `<name>` for a real
account name.

## `v0-handoff.csv`: the one-time UEFI boot handoff (gate V0, risk R15)

The handoff is how Windows tells the firmware (UEFI, the code that runs
before any operating system) to boot the stick exactly once, then go back to
normal. One row per run, appended by `upgrade_/windows/Test-Handoff.ps1
-Check`; physical rows come from the stick's own file (`RUN-TEST.cmd`'s
`-Auto` return check, or `CHECK-HANDOFF.cmd`), and the machine's
`-DumpMachine` capture from the same run is curated into
`evaluate/windows/corpus/`.

| Column | Meaning |
|---|---|
| `harness` | Test-Handoff.ps1 version |
| `vendor`, `model`, `firmware_version` | the machine under test |
| `secureboot`, `bitlocker` | on / off / unknown at arm time; C: protection state at arm time |
| `payload`, `failmode` | the `BOOTX64.EFI` leaf (which payload); blank for a baseline run, else the fail-mode armed |
| `keypress_free`, `windows_returned` | y / n / na: did it reach the payload with no keypress; y / n: back in Windows normally after |
| `notes` | recovery prompt? logo hang? anything odd. From harness 0.2.0 the row begins with a harness-written `[harness: os=...; bitlocker-via=cmdlet\|manage-bde; fired-via=fired.txt\|grubenv\|none]` prefix, then the operator's words |

| Result | Meaning | Verdict |
|---|---|---|
| `fired-once` | payload ran (marker: `fired.txt` from the Shell payload, or `upg_fired=1` in `EFI/BOOT/grubenv` from the shim payload, harness 0.2.0+), one-shot cleared itself, boot order intact | **pass** |
| `ignored` | booted straight to Windows, order unchanged | fail-**safe** (and the *expected pass* for a fail-mode run) |
| `persisted` | payload ran but the one-shot did not clear, so it would boot the stick again | fail-**loud**: the prologue needs cleanup-on-return |
| `reordered` | firmware permanently changed the boot order | fail-**loud**: design input |
| `error` | the harness could not classify | investigate |

**What a pass requires.** Both VM firmwares (Hyper-V Gen 2 and QEMU+OVMF):
`fired-once` on the baseline, `ignored`/refused on the fail-modes. BitLocker
suspended: `windows_returned=y`, no recovery prompt in `notes`; the
`NoSuspend` fail-mode's outcome recorded verbatim either way (the prologue
suspends regardless, the cautious default). Every physical machine (at least
3 vendors beyond the G16): `fired-once`, or a *detectable* safe failure. A
`persisted` or `reordered` on real hardware does not fail the project: it adds
a required step to the shipping prologue. Record it and note the machine.

## `v1b-alongside.csv`: installing alongside a shrunk Windows (gate V1b, risk R21)

Linux is installed next to a shrunk Windows, and Windows must still boot
afterwards. Both share the **ESP** (EFI System Partition, where every
operating system keeps its boot loader). One row per run, appended by
`rig/vm/v1b.sh verdict` (`v1b-verdict.py`) from offline disk inspections
(`v1b-inspect.py`), the guest's shrink record and the boot markers both OSes
write to the OEMDRV volume (run-book: `rig/vm/README.md`). GRUB is Linux's
boot menu, **chainload** its handing over to Windows' own loader; **os-prober**
finds other systems for it; Anaconda is Fedora's installer; shim is the
Microsoft-signed first-stage loader for Secure Boot.

| Column | Meaning |
|---|---|
| `harness`, `secureboot` | v1b.sh / v1b-verdict.py version; on / off during the install **and** the boot cycles |
| `esp_size_mib`, `esp_free_before`, `esp_free_after`, `esp_added_bytes` | size of the Windows-made ESP that was reused; FAT free bytes before and after the install; bytes of files the install added |
| `fits_100mib_esp` | `computed-yes/NO`: would (Windows' own usage + added) fit a 100 MiB ESP. Arithmetic, exercised only if `esp_size_mib` is about 100 |
| `bootmgfw_intact` | y/n: `EFI/Microsoft/Boot/bootmgfw.efi` sha256 identical before the install, after it, and after every cycle |
| `preexisting_changed` | every file that existed on the ESP before the install and was modified or removed by it (Windows' own `BCD`/`BCD.LOG*`/`BOOTSTAT.DAT` excluded, because Windows rewrites those on boot) |
| `windows_via_grub` | count of Windows boots whose firmware `BootCurrent` was the *Fedora* entry: Windows reached through GRUB's chainload, not the firmware's own Windows entry |
| `windows_boots`, `linux_boots`, `os_prober_stock` | boot-marker rows after `install-done`; did the stock install list Windows in `grub.cfg`, and did it after `GRUB_DISABLE_OS_PROBER=false` + regenerate |

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | ESP had room, no pre-existing ESP file changed, Windows booted via GRUB and Linux booted, 2 or more cycles each | **pass for the firmware in the row**. With `secureboot=off` it closes plumbing only |
| `install-failed` | Anaconda never finished (no post-install inspection) | fail: capture the storage/anaconda logs |
| `esp-full` | the shared ESP had no room, or the install grew it | fail: design input (ESP size gate in `evaluate`) |
| `windows-files-changed` | `bootmgfw.efi` or anything else under `EFI/Microsoft/` was modified or removed | **fail-loud**: the safety net is compromised |
| `fallback-loader-replaced` | the five checks held, but the install replaced a Windows-placed file outside `EFI/Microsoft/` (in practice `EFI/Boot/bootx64.efi`, which shim-x64 overwrites) | not a bare pass. Design input: the converter must snapshot and restore it. The boot checks in the row still stand |
| `windows-unbootable-via-grub` | no Windows boot arrived through the Fedora entry | fail: os-prober / chainload broken on this firmware |
| `linux-unbootable` | Linux never booted | fail |
| `cycles-incomplete` | everything worked but fewer than 2 boots of each OS were recorded | incomplete, re-run the cycles |

**What a pass requires.** `pass-plumbing` (or `fallback-loader-replaced`
once the converter's own install step, which restores the fallback loader,
runs) on the QEMU rig (Secure Boot off, its only mode) **and** on a
Secure-Boot-enforcing firmware (Hyper-V Gen 2 or a physical machine). Every
physical machine (at least 3 vendors): `pass-plumbing` with `secureboot=on`,
or a *detectable* failure `evaluate` can steer to clean slate. A VM row never
closes R21's Secure Boot or vendor clauses (rule #5).

## `v3-bitlk-read.csv`: reading the kept BitLocker volume from Linux (gate V3, risk R19)

On the default path, Linux later copies the person's files out of the kept
Windows partition; if it is BitLocker-encrypted, Linux must unlock it and
read it back exactly. One row per config run, appended by `rig/hyperv/v3.sh
verdict` (`v3-verdict.py`), comparing the manifest Windows wrote before a
**full** shutdown (`guest/v3-plant.ps1`: sha256 + size of every file in a
planted corpus and under `C:\Users`) with the manifests the installed Fedora
wrote after unlocking the same partition with the recovery password
(`guest/v3-read.sh`: `cryptsetup open --type bitlk --readonly`, `mount -o
ro`, re-hash). The recovery password never appears in any file here.

| Column | Meaning |
|---|---|
| `harness`, `config` | v3.sh / v3-verdict.py version; the run's label (`xts128-usedspace`, `xts256-usedspace`, `xts128-full`, ...) |
| `windows_build`, `bitlocker_method`, `used_space_only`, `protectors` | the Windows build that encrypted; `EncryptionMethod` (XtsAes128 / XtsAes256 / ...); yes / no from `manage-bde -status`; key protector types (`Tpm+RecoveryPassword` is the Windows default) |
| `volume_status`, `partition_bytes`, `bitlk_volume_bytes` | `FullyEncrypted` (anything else is a mid-encryption volume, a different row); the partition's size vs the size BitLocker's own metadata records (they differ on every shrunk, keep-Windows volume) |
| `linux_context`, `kernel`, `cryptsetup`, `unlock` | where the read ran (the **installed** Fedora, settle-in's context, or a live environment); the versions on the reading side; ok / failed / not-reached |
| `ntfs_driver` | the driver that produced the compared manifest, one row each per run: `ntfs-3g` (FUSE), `ntfs3` (kernel), `ntfs3 (readahead 0)` (kernel, `blockdev --setra 0` on the dm device, a diagnostic pass) |
| `corpus_files`, `corpus_identical`, `corpus_mismatch`; `users_files`, `users_identical`, `users_mismatch`, `users_volatile` | the planted corpus, then `C:\Users`: hashed on both sides, byte-identical count, byte-different count. `volatile` = files Windows itself rewrites between the hash and the shutdown (registry hives, logs, caches, listed in the run's `artifacts/v3/verdict/users-ntfs3.txt`), reported but not held against the read |
| `notes` | cryptsetup's warnings verbatim, the ntfs-3g cross-check, anything odd |

When the verdict learns of a new cache Windows rewrites on its own, the run
is re-verdicted and the **later rows for the same config, kernel and driver
supersede the earlier ones**; the earlier `mismatch` rows stay as the record
of what the classifier did not yet know.

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | unlocked with the recovery password, mounted read-only, every corpus file and every non-volatile file under `Users` read back byte-identical (sha256 + size), no file unreadable on Linux that Windows could read | **pass for the config in the row**. A VM row closes plumbing only |
| `unlock-failed` | `cryptsetup open --type bitlk` refused the volume or the key | fail: the config is unsupported, so `evaluate` must steer it to decrypt-first or clean slate |
| `mount-failed` | unlocked, but the NTFS inside would not mount read-only | fail: investigate the driver / volume state |
| `mismatch` | at least one non-volatile file read back with different bytes | **fail-loud**: the trust-ending class (R19's "read wrong data"). Never softened |
| `read-errors` | no wrong bytes, but Linux could not read a file Windows could, or a file was missing | fail: the copy would be incomplete |
| `guest-crashed` | the reading OS crashed mid-read (kernel oops; the harness's last synced `stage=` and `trace-*.txt` name where) | **fail-loud** for that driver/kernel: a hung machine mid-pull. Design input (driver choice / kernel gate) |
| `incomplete` | a manifest is missing, the corpus was too small, or the pass never started (an earlier pass took the guest down) | re-run |

**What a pass requires.** `pass-plumbing` for all three configs V3 names
(XTS-AES-128 used-space-only, the Windows 10/11 default; XTS-AES-256;
full-disk), from an **installed** Fedora with the driver `settle-in` uses
(ntfs-3g, decided 2026-09-01, R19; a kernel-driver row is informational), and
on at least one physical BitLocker machine per Windows version targeted.
Used-space-only on a fragmented real disk is the residue no VM row closes.

## `v8-materialize.csv`: OneDrive placeholders are materialized at evaluate (gate V8, risk R8)

A OneDrive "free up space" file is a **placeholder**: its directory entry
shows the full size, but no bytes are on the disk, and pulled from Linux it
arrives empty. **Materializing** means forcing the real bytes onto the disk.
One row per run, appended by `evaluate/windows/Test-Materialize.ps1`, which
proves the harvester's `-Materialize` step (seam `-MaterializePath`) against
the **real** cloud files filter with ground truth: a temporary sync root
through the Cloud Files API (what OneDrive is built on), dehydrated
placeholders whose bytes only the harness knows, the harvester in a separate
process, then a hash of the NTFS volume. One placeholder is one the provider
refuses to serve, so the refuse arm runs every time.

| Column | Meaning |
|---|---|
| `timestamp` | as the harness wrote it; rows are transported verbatim |
| `harness`, `os_build` | Test-Materialize.ps1 version; the Windows build the filter belongs to |
| `provider` | `cfapi-test-provider` (the harness is the sync provider) or `onedrive` (the signed-in client, `-OneDrive`) |
| `files`, `bytes`, `placeholders_confirmed` | placeholders created and their total logical size; how many carried `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` **and** allocated 0 bytes on disk before the run |
| `materialized`, `bytes_verified` | files the harvester reported materialized; files whose on-disk sha256 equals the ground truth **and** whose placeholder attribute is gone **and** which allocate on disk |
| `refused_expected`, `refused_reported` | files the provider refuses to serve, and how many of those the harvester reported as failed |
| `harvest_exit`, `notes` | the harvester's exit code (0 = all materialized; 3 = at least one failure, the refusal); harness-written facts first (OS, provider, hydration policy, sizes), then FETCH_DATA counts and the harvester's summary line |

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | every servable placeholder materialized and byte-verified; the refused one reported failed with a non-zero exit | **pass for the filter and the harvester**. The provider was ours, so plumbing only (rule #5) |
| `pass` | the same with `provider=onedrive` | **pass**: the residue |
| `setup-failed` | the placeholders did not come out dehydrated. A harness or API problem; nothing learned about materialization | fix the harness |
| `wrong-bytes` | a file materialized but its bytes differ from the ground truth | **fail-loud**: the trust-ending class |
| `not-materialized` | a servable file stayed a placeholder, or was reported materialized without bytes on disk | fail: the harvester's judgment is wrong |
| `refusal-missed` | the unservable file was reported materialized, or a failure came back with exit 0 | **fail-loud**: "materialize, or refuse" would have written a job over an empty file |

**What a pass requires.** `pass-plumbing` on the rig and on at least one
physical machine; `pass` with `provider=onedrive` on a signed-in machine with
Files On-Demand (`-OneDrive` uploads a few MB, asks the client to free up
space, then materializes; only on a machine and account you own, never
unattended). Since the no-download decision (2026-09-26, R8) the launchers do
not use the materializer: that row proves a built but unused tool, and the
check that matters moved to `settle-in`.

## `r16-stick-writer.csv`: the stick writer refuses the wrong device (risk R16)

The writer (`upgrade-stick`; `Write-UpgradeStick.ps1` wrote lines 2 to 12) is the first component
that writes to a device; R16 is the risk that it writes the wrong one, before
the commit line. One row per run: every `-Plan` (read-only) and every
`-Write`. The rules, all of which must hold and each of which is reported:
USB bus and not HDD/SSD media; not the system or boot disk, holding no volume
Windows runs from; exactly one attached disk carries the `-Target` unique id;
size matches what the person was shown; online and writable; the person types
the device's current label (or model) to confirm. Right before `Clear-Disk`
the target is found again by unique id from a fresh list of disks, and the
cmdlets get that disk object, never a disk number.

| Column | Meaning |
|---|---|
| `writer`, `machine`, `os_build`, `mode` | the writer's version (`0.1.1` the script; `0.1.0 (Rust; ...)` the program); where it ran; `plan` (nothing written) or `write` |
| `disks_attached`, `usb_disks`, `target`, `expected_bytes` | how many disks the run saw, and how many on the USB bus; the unique id pointed at; the size the person was shown, in bytes (decimal GB x 10^9 when typed) |
| `decision`, `refusals` | `selected` or `refused`; when refused, every rule the pointed-at device broke |
| `written`, `verified` | y/n: was a disk erased and written; did every file read back against SHA256SUMS |
| `notes` | harness-written: elevation, then every attached disk with bus/media/size and the rules it broke (`writable` for a candidate), then the write's letters and file counts |

The result words are the `decision` column: `selected` (every rule held) or
`refused` (`refusals` names each broken rule). A `write` row passes only with
`decision=selected`, `written=y` and `verified=y`.

**What R16 closes requires.** The refusal matrix, fabricated (`-SelfTest`,
23 cases: the system disk pointed at, two sticks and a USB hard drive with the
right one selected, a USB SSD enclosure, cloned serials, wrong size,
off-by-one bytes, SD bus, SAS bus, offline, Windows To Go); live and
read-only (a real machine with a real stick, selected only when pointed at
with the right size, the internal disk refused; the rig with several disks,
every one refused); and the write, physical: several sticks **and a USB hard
drive attached at once**, `-Write` pointed at one, that one erased, written
and verified, the rest untouched (labels and file counts the same before and
after, in `notes`). No VM can run it: Hyper-V has no USB emulation.

## `v1-live-boot.csv`: the stick boots through the handoff and verifies, nothing installed (gate V1, reversible half)

One row per run, appended by `rig/hyperv/v1-verdict.py` from the V0 harness
row the guest wrote when Windows came back (`Test-Handoff.ps1 -Check -Auto`)
and the report the `%pre` verifier left on the stick
(`upgrade_/report/verify.json`, by `upgrade_/linux/verify.sh` inside
Anaconda's stage2; `%pre` runs before the installer touches any disk). The
chain: one-time boot entry, shim, GRUB records `upg_fired` and boots the
installer from the stick with `upg.mode=verify`, `%pre` finds the job's disk
by identity, checks the hardware, writes the storage `%include` and the
report, reboot, Windows. Bench: `rig/hyperv/v1.sh run`.

| Column | Meaning |
|---|---|
| `harness`, `secureboot` | v1.sh / v1-verdict.py version; on / off at arm time (from the V0 row) |
| `handoff_result`, `windows_returned` | the V0 row's result (`fired-once` is the only pass) and its `windows_returned` |
| `stage2_booted`, `storage_include` | y/n: a `verify.json` exists on the stick, so Anaconda's stage2 came up from the stick and ran our `%pre`; the `%include` the install would have used was written |
| `identity` | `pass` / `fail` / `not-reached`: the job's disk was found by unique id or serial **and** its size matched exactly |
| `esp`, `display`, `wifi`, `audio_firmware` | `pass` / `fail` / `skipped`: for keep-windows, an EFI partition holding `bootmgfw.efi` was found on that disk; the hardware checks (see `verify.sh` for each). `skipped` is "nothing to test on this machine", never "not checked" |
| `notes` | the V0 row's notes prefix, then the verifier's facts (kernel, disk, how it was matched, connector and mode, ESP device, the SecureBoot variable, and from harness 0.2.0 `desktop_image=pass/fail` with the image's size, sha256 verdict against the stick's `SHA256SUMS` and the read speed in MB/s: the cutover's read-back step and R17's counterfeit-flash test) |

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | handoff `fired-once`, stage2 booted from the stick, identity matched, storage include written, no hardware check failed, Windows returned | **pass for the firmware in the row**. A VM row (Secure Boot off on Hyper-V, no USB) closes plumbing only |
| `windows-not-returned` | no V0 row, or `windows_returned=n` | **fail-loud**: the reversible half did not come back |
| `handoff-failed` | the V0 row is not `fired-once` | see `v0-handoff.csv`'s vocabulary |
| `stage2-not-reached` | the entry fired but no report appeared: GRUB, the kernel, dracut or stage2 did not get as far as `%pre` | fail: capture the console |
| `identity-mismatch` | the verifier could not match the job's disk on this machine, or the size differed | on the rig a harness bug; on a real machine **the refusal working as designed** |
| `verify-incomplete` | identity matched but the include was not written, a hardware check failed, or (0.2.0+) the desktop image on the stick did not read back byte-identical | fail: read `verify.log` |

**What a pass requires.** `pass-plumbing` on the rig, then on an owned
physical machine with Secure Boot **on** (the signed chain that fired V0). A
physical row is made by feeding the stick's own files (`v0-handoff.csv`,
`upgrade_/report/verify.json`) to `v1-verdict.py` with the machine's firmware
string; the V0 row is also transported verbatim. The physical rows fill the
vendor matrix's "live boot" and "hardware verify" columns: one half-hour
visit per machine, read-only.

## `v2-install.csv`: the conversion itself, keep-windows, on the rig (destructive half, step 1)

One row per run, appended by `rig/hyperv/v2-verdict.py` from offline
inspections of the guest disk before the install, after it and after the boot
cycles (`rig/vm/v1b-inspect.py`: GPT and ESP file manifest with sha256), the
`outcome.json` the converter's `%post` wrote to the stick (validated against
`schemas/outcome.schema.json`), the boot markers both OSes left there, and
the V0 harness row. What runs is the product's own code: the kickstart (the
answer file that drives the Fedora installer unattended), the `%pre` verifier
and the `%post` checklist (`upgrade_/linux/{verify,outcome}.sh`). Bench:
`rig/hyperv/v2.sh run`. A physical row (harness `...-physical-prologue`) has
no offline inspection: the ESP is judged from the stick's snapshot and the
installer's `esp-after` manifest.

| Column | Meaning |
|---|---|
| `path`, `desktop`, `handoff_result` | from the job: `keep-windows`, `kde` / `gnome`; the V0 row's result for the arming reboot |
| `install_done`, `outcome_valid` | y/n: `outcome.json` exists with `status=completed`; it validates against the schema |
| `esp_size_mib`, `esp_free_before`, `esp_free_after`, `esp_added_bytes` | the shared ESP before and after |
| `bootmgfw_intact`, `microsoft_files_changed` | y/n: `EFI/Microsoft/Boot/bootmgfw.efi` sha256 identical before, after, and after the cycles; every pre-existing file under `EFI/Microsoft/` modified or removed (Windows' own `BCD*`/`BOOTSTAT.DAT` excluded), or `none` |
| `fallback_loader` | what sits at `EFI/Boot/bootx64.efi` after the install, as the checklist named it: `shim` (kept on purpose while Windows is kept), `shim-in-windows-slot` (shim placed in Windows' own slot, the reason in `notes`), `windows`, `other` |
| `snapshot_files`, `windows_entry_present`, `linux_first`, `grub_lists_windows` | files the `%pre` snapshot saved to the stick before the ESP was touched, Windows' fallback loader among them; the checklist's three firmware/GRUB facts from `outcome.json` |
| `windows_boots`, `linux_boots`, `notes` | boot-marker rows after `install-done` (Windows rows by the bench when the return check answers; Linux rows by the marker unit `%post` installed); outcome summary, ESP delta, boot counts, any changed files |

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | install completed, outcome valid, no Microsoft file changed, `bootmgfw.efi` intact, Windows entry present and GRUB lists it, shim in the fallback slot (or in Windows' slot) **with** the snapshot holding Windows' copy, 2 or more boots of each OS | **pass for the firmware in the row**. SB off on Hyper-V: plumbing only |
| `handoff-failed` | the arming reboot did not fire and no install ran. The V0 row for this leg reads `reordered` by design (the converter puts Fedora first in BootOrder), so `reordered` and `fired-once` both count as fired (line 2 predates that classifier fix of 2026-09-10 and stays as written) | see `v0-handoff.csv` |
| `install-failed` | no completed `outcome.json`: Anaconda stopped (a `%pre` refusal, a storage error, a bootloader error) | fail: read `report/anaconda.log`, `storage.log` |
| `outcome-invalid` | `outcome.json` does not validate | fail: the contract is wrong or the writer is |
| `windows-files-changed` | a Microsoft boot file changed, or `bootmgfw.efi` differs | **fail-loud**: the safety net is compromised |
| `esp-full` | the ESP had no room | design input |
| `installed-not-cycled` | the install completed and the ESP checks held, but no boot of either OS was recorded after `install-done` (a physical run before its boot cycles) | not a pass: the cycles are owed |
| `windows-unbootable-via-grub` | no Windows boot arrived, or GRUB does not list Windows | fail |
| `linux-unbootable` | the installed system never booted | fail |
| `fallback-loader-unrecorded` | the fallback slot is neither shim word, or the snapshot is missing | fail: rollback could not restore Windows' loader |
| `cycles-incomplete` | fewer than 2 boots of each OS | re-run the cycles |

**What a pass requires.** `pass-plumbing` on the rig, then on a physical
machine with Secure Boot on, with the boot cycles recorded (R21's checklist
all true, both systems reached with no key pressed).

## `r18-prologue.csv`: the prologue as product code: re-validate, the disk check, the shrink, the handoff (risk R18, step 1b)

The **prologue** is the Windows-side part of the conversion: after the typed
CONVERT and before the stick boots, it checks the machine again, gets the
disk healthy enough, shrinks Windows, and arms the handoff. One row per run,
appended by `rig/hyperv/prologue-verdict.py` from the prologue's record on
the stick (`upgrade_/prologue.json`, written at every stage change), its
return record (`upgrade_/prologue-return.json`: the handoff, classified when
Windows next boots), `outcome.json` validated against the schema, the bench's
fault injection (`fsutil dirty set C:`, which sets NTFS's "dirty" flag) and
the offline GPT inspections before and after. What runs is the one-click flow
(`RUN-CONVERT.cmd`, then `Invoke-Prologue.ps1 -Start`) against a copy of the
unshrunk install-day disk; the install then runs as in `v2-install.csv`,
whose verdict writes its own row for the same run. Bench:
`rig/hyperv/prologue.sh run`. A **rung** is one step on the ladder of disk
repairs, gentlest first (`spot-fix`, then `chkdsk-f`). The **fork** is the
branch chosen in advance (`keep-windows`, `clean-slate` or `stop`), taken
once the free space has been re-measured.

| Column | Meaning |
|---|---|
| `secureboot`, `dirty_injected`, `revalidated` | from the return record (`unknown` when a run never armed); y/n: the bench set NTFS's dirty flag on C: before the flow; job.json matched the live machine on every fact the prologue re-checks |
| `scan`, `disk_health` | what `Repair-Volume -Scan` (read-only) answered, verbatim (it chooses the rung); `Get-PhysicalDisk` HealthStatus read immediately before the repair was scheduled (anything but `Healthy` is a refusal) |
| `method`, `restarts`, `wininit_1001`, `found000`, `dirty_after` | `spot-fix` / `chkdsk-f` / `none`: the rung the prologue scheduled; restarts the check took; y/n: the boot-time check left its Wininit event 1001 (text in `notes`); whether it left a `found.000`; the flag `clean` / `dirty` / `unknown` after the check |
| `remeasured_gb`, `remeasured_by`, `diskpart_gb` | shrinkable space at the moment it mattered: the number branched on, which read-only path gave it, and diskpart's independent figure |
| `fork_taken`, `requested_bytes`, `freed_bytes`, `partition_shrunk` | `keep-windows` / `clean-slate` / `stop`: the pre-chosen fork, taken; the planned shrink; what `Resize-Partition` freed; y/n: the offline GPT shows C: smaller by exactly `freed_bytes` (`unreported` on a physical run, which has no offline inspection) |
| `hibernation_off`, `pagefile_off`, `bitlocker_before`, `bitlocker_suspended` | the mitigations the prologue applied (`n` when the mitigation never ran); BitLocker as the outcome records it |
| `armed`, `handoff_result`, `install_done`, `outcome_valid`, `record_in_outcome` | the one-shot entry was armed; the return record's classification (`reordered` is expected, because the converter puts Fedora first); as `v2-install.csv`; y/n: `outcome.json`'s `prologue` block equals the record the prologue left on the stick |
| `notes` | the prologue version and stage, the scan and chkntfs answers, the Wininit text, the shrink plan, the GPT delta, the outcome's stop reason if any; from prologue 0.4.0 the step-1b trigger (`trigger=dirty-flag`, `repair-queued` when the bit was clean and Windows had a repair queued (R18, 2026-09-17), or `none`); from 0.3.0 one `resume N (<stage>): <account> session <id> interactive <bool> explorer <bool> uptime <s> s stick after <s> s` entry per restart, and `bench AutoAdminLogon=0|1` |

A **walk-away row**: every `resume` entry in `notes` reads `NT AUTHORITY\SYSTEM
session 0 interactive False explorer False` and the bench line reads `=0`
(session 0 has no desktop, so nobody was signed in).

| Result | Meaning | Verdict |
|---|---|---|
| `pass-plumbing` | flag confirmed by the scan, health `Healthy`, a rung scheduled, restart taken, flag clear after, re-measured by a read-only path, fork `keep-windows`, C: shrunk by exactly the request (GPT agrees), handoff armed and fired, install completed, `outcome.json` valid and carrying the prologue's record | **pass for the firmware in the row**. A rig row closes plumbing, never the real-hardware clause (a real flag is the residue) |
| `stopped-<stage>` | the prologue wrote a stopped `outcome.json` at that stage: an honest refusal, the reason in `notes`. Seen: `stopped-arm-handoff`, `stopped-volume-check`, `stopped-shrink`, `stopped-confirm` | read it: on the rig a harness or product bug, on a real machine possibly the refusal working |
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

**What a pass requires.** `pass-plumbing` on the rig with the injected flag
(scan, guardrail, rung, restart, outcome read, re-measure, fork, shrink, arm,
install, record carried through). Then a real machine whose C: carries a real
flag Windows has kept through restarts (the Acer Aspire): the first physical
step-1b row and, if the re-measured number fits, the first physical install
with Windows kept. What a real flag does under the ladder is the residue no
test setup can fake (rule #5); `fsutil dirty set` is only our model of it.

## `r21-rollback.csv`: rollback: Windows first again, its fallback loader restored from the snapshot (risk R21)

Rollback undoes the boot changes so the machine starts Windows again as it
did before the conversion. One row per run, by `rig/hyperv/rollback-verdict.py`
from the rollback's record (`upgrade_/rollback.json`, written by
`upgrade-prologue rollback`, the window's "Roll back" choice, from the kept
Windows), the offline ESP manifests before the conversion, after the cycles
and after the rollback, and the bench's marker when Windows comes up
afterwards with **no key pressed**. Bench: `rig/hyperv/prologue.sh rollback`.
`--physical`: the ESP before is the stick's snapshot sums, the captures come from
`physical/collect-windows.ps1`, `boots.log` is assembled (first line `source,`).

| Column | Meaning |
|---|---|
| `record`, `restored`, `loader_matches_snapshot` | y/n: `rollback.json` exists; the rollback copied Windows' fallback loader back (it had been shim); offline, `EFI/Boot/bootx64.efi` after the rollback is byte-identical (sha256) to the pre-conversion inspection's |
| `windows_first`, `windows_direct_boot` | y/n: `{bootmgr}` leads the firmware display order after `bcdedit ... /addfirst`; the next start reached Windows with no key pressed (bench marker `direct-after-rollback`) |
| `linux_partitions_intact`, `efi_fedora_intact` | y/n: the GPT after the rollback equals the GPT after the cycles; every file under `EFI/fedora/` unchanged |
| `notes` | the shas and orders before/after, the GPT and `EFI/fedora` comparison |

**Result vocabulary.** `pass-plumbing` (rig) or `pass` (`--physical`) when
all of the above hold; else the first failing item by name: `record-missing`,
`not-restored`, `loader-mismatch`, `windows-not-first`, `windows-not-direct`, `linux-touched`, `efi-fedora-touched`.

**What a pass requires.** A rig row closes the plumbing of the restore half
of the R21 snapshot. The firmware clause is a physical-matrix column: does
*this* vendor's firmware honour `{fwbootmgr} displayorder` from Windows after
a Linux install put its own entry first?

## `walkaway-probe.csv`: the resume fires with nobody signed in (the walk-away clause of V0/V4; risk R24)

"Walk away" means the person clicks once and leaves: after each restart the
conversion must carry on by itself, before anyone signs in. This probe tests
only that. Written by **the prologue's own `-Probe` task, from session 0**
(`Invoke-Prologue.ps1 -Probe`, launched by `RUN-PROBE.cmd` on the kit), never
by a bench script and never by hand: it registers the same SYSTEM startup
task the conversion uses and restarts once; the task appends the row to the
stick's `upgrade_/walkaway-probe.csv` (and `upgrade_/probe.json`), queues the
sign-in notice and removes itself. Read-only: nothing on the disk changes but
the locked state directory and the one-shot task. A physical row is
transported verbatim (`tail -n +2`, the `harness` column inserted after
`timestamp`); rig rows come through `rig/hyperv/prologue.sh probe` the same
way.

| Column | Meaning |
|---|---|
| `timestamp`, `harness`, `prologue_version` | when the task wrote the row (after the restart); `0.1.0-hv` (rig) or `<prologue>-physical`, inserted at transport; the version that ran |
| `vendor`, `model`, `bios`, `os`, `secure_boot`, `stick_bus` | `Win32_ComputerSystem` / `Win32_BIOS` SMBIOSBIOSVersion; Windows caption and build; on / off / unknown at arm time; the stick's bus as Windows reports it (`USB` on a real machine, `SAS` on the Hyper-V rig) |
| `run_as`, `session_id`, `interactive`, `explorer_running` | the account the resume ran as (`NT AUTHORITY\SYSTEM` is the design); the resume's session (`0` means no desktop); `[Environment]::UserInteractive` in the resume; whether any `explorer.exe` existed (a signed-in desktop) |
| `uptime_s`, `stick_wait_s` | seconds from boot to the resume starting; seconds the resume polled before the stick's volume id appeared (real firmware enumerates USB late; the poll allows 120) |
| `notice`, `task_removed`, `notes` | `queued` (RunOnce notice for the next sign-in) or `shown` (a popup, because a session existed); y/n: the one-shot task unregistered itself; state stage, resume stage, the resume's UTC |

| Result | Meaning | Verdict |
|---|---|---|
| `resumed-unattended` | SYSTEM in session 0, no explorer, stick found, task removed | **pass for the firmware/edition in the row** |
| `resumed-attended` | a session existed when the resume ran: someone signed in early, or the task ran as the person | not a walk-away row; re-run without signing in |
| `stick-not-found` | the task ran but the stick's volume id never appeared within the poll | fail: USB enumeration slower than the poll, or the stick dropped (R16's loose-stick clause) |
| `task-not-removed` | everything else held but the task could not unregister itself | fail-loud: clean-up bug |

**What the probe closes.** The **resume mechanism** for that firmware and
edition: a SYSTEM startup task registered by the prologue fires before any
sign-in, sees a real stick, and cleans up. Not the conversion (no disk check,
shrink or handoff; those are `r18-prologue.csv`'s rows). A rig row closes
plumbing only: its stick is a SCSI disk, present from power-on. Residue (R24):
policy that blocks task registration, Fast Startup's hybrid shutdown,
BitLocker with a PIN.

## `v5-controller-mode.csv`: the "Storage controller mode" check on real Intel RST hardware, both directions (gate V5, risk R1)

Some Intel machines ship with the disk controller in RAID/RST mode (Intel
Rapid Storage Technology) or behind VMD (Volume Management Device), where
Linux often cannot see the disk. This file records whether the scanner's
check fires on real hardware when it should (**positive direction**) and
stays quiet when it should (**negative direction**). One row per scanner run
in one SATA mode, written by `rig/v5-verdict.py`, never by hand. **The
one-click path** (`RUN-STORAGE-MODE.cmd`, `upgrade-prologue storage-mode`) scans in
the mode the machine is in, restarts into the firmware setup for the person
to change SATA Mode, boots Safe Mode once through a copied boot entry, scans
again as SYSTEM with nobody signed in, asks for the mode back and scans a
third time; `rig/v5-verdict.py --from-run <folder>` then writes one row per
leg, the mode being what the harness **asked** the person to set. **The
two-file path** (a plain `RUN-SCANNER.cmd` run): `rig/v5-verdict.py
--sata-mode-set raid|ahci|absent <report> <capture>`, the mode the setup was
set to being the one operator input. Either way `result` compares the mode
word against the PCI class code the controller declared to Windows, so a
setting that did not take is `mode-mismatch`, not evidence; a `Synthetic`
capture is refused (`error`). Reports are discarded after the row; captures
are curated into `evaluate/windows/corpus/` with an `Expected` block per mode.

| Column | Meaning |
|---|---|
| `timestamp`, `harness`, `scanner_version`, `vendor`, `model`, `firmware`, `os` | the report's `ScannedUtc`; `v5-verdict <version>`; the scanner that produced the report; the machine from the report's `System` block (`BiosVersion`, OS caption + build) |
| `leg`, `sata_mode_set` | one-click path: 1, 2 or 3 (blank for a two-file row); `initial` (leg 1: whatever the machine was in), `raid` (RST Premium / Optane / RAID), `ahci`, or `absent` (two-file path: the setup has no SATA-mode option) |
| `controller`, `pci_id` | every Intel PCI mass-storage-class device (or one with an `iaStor*` service) in the capture, by Windows name, `;`-joined; its `vvvv:dddd` |
| `class_code`, `compatible_ids`, `driver_service` | the PCI class+subclass Windows published in `CompatibleIDs` (`0104 (RAID)`, `0106 (AHCI)`, `0108 (NVMe)`): the check's third signal; the full list, verbatim; the driver service bound (`storahci`, `stornvme`, `iaStorAC`, `iaStorAVC`, `iaStorVD`, ...) |
| `check_status`, `check_detail`, `verdict` | the `Storage controller mode` line of the report; the report's overall level |
| `flow_result` | one-click path: the harness's own verdict on the run: `restored` (three legs, original mode back), `mode-unchanged` (leg 2 saw the same mode: no option, or not saved), `not-restored`, `no-intel-controller`, `cancelled`, `error`; `single-run` for a two-file row |
| `resume_run_as`, `safe_boot` | who ran the resume that produced this leg (`SYSTEM unattended` is the design; `launcher` for leg 1); the Safe Mode boot before this leg, as its evidence records it: `y (user sign-in, option 1, runonce-signin)` means the person signed in at the Safe Mode screen and the `*`-RunOnce left the marker and restarted (Task Scheduler does not run the task in Safe Mode; rig 2026-09-14); `n` means no marker reached the resume |
| `fw_reboot` | how the restart into setup was done: `fw` (`shutdown /r /fw`, straight into the firmware setup) or `plain` (the firmware refused `/fw`; the person pressed the setup key) |
| `notes` | harness facts first (scan/capture times, controllers counted, classes seen, whether an RST or VMD service was bound; for the one-click path the stage, the mode seen and the cleanup read-back), then any `operator:` words |

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

**What a pass requires.** At least one physical machine with **both** a
`fail-fired` row and an `ok-passed` or `warn-rst-on-ahci` row, its RAID-mode
capture curated in the corpus with `Expected` `Storage controller mode: fail`
and marked real. The row says which signal fired: `class_code` `0104` with a
pre-VMD service (`iaStorAC`/`iaStorAVC`) is signal 3, the **pre-VMD RST
clause**; a `pci_id` from the kernel's `vmd.c` table, or `driver_service`
`iaStorVD`, is signal 1 or 2, **VMD proper** (11th gen+), which still takes
an 11th-gen-or-newer machine with RST on (R1 carries the residue). A rig run
(`rig/hyperv/prologue.sh storage-mode`) cannot change a SATA mode: its rows
are `no-intel-controller` with `flow_result` `mode-unchanged` and prove the
flow's mechanics only (read `safe_boot` and `resume_run_as`).

## `harvest-folder-map.csv`: what the folder map finds on real machines (roadmap item 3; risks R5, R6, R8, R26)

The **folder map** is the harvester's list of where the person's files live
(Documents, Pictures and the other known folders), and how big they are. One
row per read-only measurement with the harvester's `-FolderMapOut` (the step
the job writer runs). It is a measurement, not a pass: there is no `result`
column. It is the evidence R26 asks for about the folder map's coverage, and
the size a clean slate or the discard offer would have to stage.
Hand-written from the harvester's JSON. Personal file names stay out.

| Column | Meaning |
|---|---|
| `harvester`, `machine`, `os_build` | `HarvestVersion`; the machine measured |
| `folders_found`, `files`, `bytes` | the six known folders that exist, and their contents |
| `online_only`, `unreadable` | OneDrive online-only files found (R8; recorded as left in the cloud since job writer 0.11.0); sub-folders Windows would not list (R6; a job refuses on any) |
| `files_over_4gib`, `largest_file_bytes`, `onedrive_folders`, `other_profiles` | FAT32 cannot hold a file over 4 GB; how many of the six are redirected into OneDrive; other accounts' profiles (R5; a clean slate refuses on any) |
| `stick`, `needed_bytes`, `fits` | the stick volume measured against, what the folders need on it, the answer (`n/a` with no stick) |
| `notes` | how it was run, and what lies outside the map |

## `v9-erase.csv`: the one-click erase and install (gate V9, risk R27)

The path that erases every internal drive and installs Fedora, keeping
nothing; a 2-minute countdown in the installer is the last chance to cancel.
One row per arm, written by `rig/hyperv/v9-verdict.py` from the arm's own
evidence (`rig/hyperv/v9.sh`; never by hand). The rig guest is a copy of the
install-day Windows disk plus a blank 64 GiB second disk.

| Column | Meaning |
|---|---|
| `arm` | A refuse (a job naming an absent disk, armed by the V0 harness), B cancel (a key during the countdown), C erase (the countdown left alone), D not again (after C, the stick put first in the boot order and left in). An arm named with a number (`B3`, `C7`, `D3`) is judged by its letter |
| `prologue_version`, `verify_version`, `identity`, `countdown` | the code under test (`harness` = armed by Test-Handoff, not the prologue); verify.json: every drive the job names found by id and exact size; countdown.json: `elapsed`, `cancelled`, or `none` (never shown) |
| `outcome_status`, `stopped_at`, `commit_crossed`, `outcome_valid` | outcome.json, and whether it validates against the schema |
| `disks_unchanged`, `system_gpt_after`, `home_gpt_after` | both disks' GPT and first MiB identical before and after (v9-inspect.py, offline; partition tables only on a physical row); the partition tables afterwards |
| `fedora_booted`, `password_matches`, `home_on_second_disk` | the first Linux boot's marker line (rig-only bench instrumentation): it booted, the account's stored hash is the job's, `/home` is on the second disk |
| `graphical_login` | (from 2026-09-26) the person's choice held at first boot: `desktop` = graphical.target with the display manager running, `console` = multi-user.target with none; `not-recorded` on rows from before the check |
| `notes` | the kit, the arm's story, the stick's `converted` / `boot-install` markers, boot counts |

| Result | Meaning | Verdict |
|---|---|---|
| `refused-before-countdown` | arm A: verify.json `identity=fail`, no countdown record, disks unchanged | **pass for arm A** |
| `cancelled-untouched` | arm B: the countdown record says cancelled, `outcome.json` stopped at the countdown and schema-valid, disks unchanged | **pass for arm B** |
| `erased-installed` | arm C: the countdown elapsed, `outcome.json` completed and schema-valid with `crossed_utc` the countdown's end, the system disk = EFI + `/boot` + root, the home disk = one Linux filesystem, the first boot's password fingerprint = the job's hash, `/home` on the second disk | **pass for arm C** |
| `not-installed-again` | arm D: the stick carries `converted` and no `boot-install`, no new countdown ran, both disks unchanged, Linux booted again | **pass for arm D** (R35) |
| `fail` | anything else, every reason in `notes`. Kept, never removed | **fail** |

**What a pass requires.** All four arms on the rig for the kit under test,
then the physical rows: `erased-installed` (one install, the stick left in),
`not-installed-again` (a restart with the stick in) and `cancelled-untouched`.

## `settle-in-first-start.csv`: the first start of the new system (gate V10, risk R28)

One row per run, written by `rig/hyperv/settle-in-verdict.py` from the run's
own evidence (never by hand): what `v9.sh stick-pull` copied off the stick
(`outcome.json`, `job.json`) and the bench marker's capture from the first
Linux boot; on a physical run, gathered over SSH. From 2026-10-04 the notes
carry the remote-access (SSH) result.

| Column | Meaning |
|---|---|
| `installed`, `handoff` | settle-in was installed (`outcome.cutover.settle_in.installed`; the adapter checked SHA256SUMS); the Wi-Fi handoff from the stick: moved as expected, removed from the stick, no `wifi` folder left under `artifacts/credentials` |
| `clock_result`, `clock_why`, `installer_clock_error` | settle-in's clock result and why, as it said it (any result is recorded; `failed` fails the row; a correction that moved a right clock fails the row); the installer's error |
| `wifi_result`, `wifi_nm_parse`, `passwords_deleted` | the networks set up, by NetworkManager's own parse of the files settle-in wrote (every file mode 600; `partial` when only the connection list could be read); the Wi-Fi passwords deleted from the stick and the handoff folder |
| `ran_before_network`, `sessions` | settle-in finished before NetworkManager and chronyd started; every desktop sign-in reached its window manager (one that never does is a black screen) |
| `window`, `own_entry`, `button` | a desktop install: the first sign-in shows the window's own "showing the summary" line (so it reached the screen) and what the desktop did with its focus request (console: n/a); the conversion's own one-time "upgrade_" firmware entry removed, none left in the first boot's firmware capture; when before/after captures exist, the old-entry button removed exactly the stale entries, the running entry stayed first and the next boot started from it into the graphical sign-in (`n/a` when nothing was offered) |

| Result | Meaning | Verdict |
|---|---|---|
| `pass` | every check held, with real Wi-Fi networks | **pass for the machine in the row** (a rig row still closes plumbing only, rule #5) |
| `pass-plumbing` | every check held, with the rig's spoofed Wi-Fi networks (`--spoofed-wifi`) | plumbing |
| `fail` | a check failed, each named in the row and `notes`. When the verdict itself was wrong, the re-judged row follows | **fail** |

**What a pass requires.** `pass` on a real machine after one install: clock
corrected, the saved Wi-Fi connected by itself before anyone signed in, the
passwords deleted, remote access carried if Windows had it, the sign-in at
its desktop with the window in front, the conversion's own entry removed.

## `port-parity.csv`: the parity ledger for the Rust port (gate V13, risk R32)

Unlike the other files here, this one lists what is **owed**, not only what
happened. Before a PowerShell piece is ported, every piece of evidence it
earned gets a line: `piece` (the script, or part of it), `evidence` (the test
case, corpus recording or CSV row), `kind`, `rust_test` (the Rust test or row
that answers it), `result`, `date`; in full in
[../VALIDATION.md](../VALIDATION.md), V13. **Result vocabulary.** `owed`: the
Rust has not yet matched this evidence. `pass`: a Rust test or a re-run row
matches it; a line turns `pass` only on that match, never on reasoning. A
pass is every line `pass`; until then the script stays in use.

## `v13-rust-scanner.csv`: the Rust scanner on real machines (gate V13, risk R32)

One row per run of the Rust scanner beside the PowerShell scanner on a real
Windows machine, written from the two programs' own outputs; the captures
hold the machine's details and are never committed. `elevated` (y/n) and
`rust_build` (the commit; `(this commit)` from the working tree).
`facts_equal`: `yes` when `upgrade-scan --compare-facts` found every fact the
Rust read equal to what the PowerShell read in the same minute; `n/a` when
the row records one specific read instead (named in `notes`). `end_to_end`:
`SAME` when `upgrade-scan --replay <Rust capture> --against <PowerShell
capture>` found the report Rust prints from its own reads equal to
PowerShell's, line for line (`report_lines`, `checks`, `verdict` say what was
compared); `not run`; `n/a`. A pass is `facts_equal=yes` and
`end_to_end=SAME`, elevated, on each real machine the project has.

## `v11-walkaway.csv`: the walk-away way back to Windows (gate V11 step 4, risk R33)

The program on Linux downloads Windows from Microsoft's catalog, writes a
stick and starts from it once; the gate on the stick finds the drives, counts
down two minutes, and Windows Setup erases and installs with nobody at the
keyboard. One row per real run, written by `rig/hyperv/v11w-verdict.py` from
the records copied off the stick (`go-back.json`, `go-back-gate.json`,
`go-back-unattend.xml`, `go-back-installed.txt`) and the observations. Never
by hand.

| Column | Meaning |
|---|---|
| `gate_version`, `job_id`, `started` | the gate on the stick; the job it ran; how the stick was started (observed) |
| `stick_wait_s`, `countdown_s`, `gate_result` | seconds the gate waited for the stick; seconds of countdown before the cross or the cancel; `crossed` (the countdown ran out) or `cancelled` (a key came first) |
| `drives_matched`, `stick_spared`, `wipes`, `cancel`, `installed_marker` | every job drive matched one disk by serial or world-wide name; the answer file never names the stick; y/n: it wipes exactly the job's drives; `tested-cancelled` / `not-tested` (observed); y/n: Windows ran `SetupComplete` |
| `key_page`, `password_forced`, `wifi_at_first_start`, `activation` | observed: a product key page showed; the password was forced at the first sign-in; the new Windows was online at its first start; Windows' own activation words |

| Result | Meaning | Verdict |
|---|---|---|
| `windows-reached` | the gate crossed after a full countdown, every drive matched, the answer file wipes exactly those drives and never the stick, Windows ran `SetupComplete`, no product key page, the password forced, the program itself started the stick, the new Windows online at its first start | **pass** |
| `cancelled-untouched` | the key came before the countdown ran out, the gate removed the Wi-Fi files from the stick, no wipe-and-install followed, and the drives are unchanged (each drive's first MiB before and after) | **pass for the cancel arm** |
| `fail` | anything less, every reason in `notes` | **fail** |

## `v11-way-back.csv`: the guided stick for the way back (gate V11 step 3, risk R30)

One row per run of the way back's stick step: `settle-in`'s go-back writes
Microsoft's installer to a stick (or an image file on the rig) and Windows
Setup is started from it. Written from the writer's record and the rig's
observations. Columns: `step` (`3-stick`); `installer` and
`installer_sha256_in_table` (the installer used; y/n: its sha256 is in the
known table); `target` and `written` (y/n, or `stopped-at-read-back`);
`files_read_back`, `swm_parts`, `editions` (files read back; parts of the
split `install.swm`; its editions against `install.wim`); `setup_started`,
`secure_boot`, `installed`, `activation` (`not tested` on a VM, which has no
licence).

**Result vocabulary.** `pass-plumbing`: every file read back, the editions
the same as the original, Setup started with Secure Boot on and installed (a
rig row). `fail`: the writer stopped itself, or a check did not hold; the
reason in `notes`. Only a real stick in a real machine closes the step.

## `v12-window.csv`: the window's verify flow on the rig (gate V12, risk R31)

One row per run of `UPGRADE.exe`'s verify flow on the rig, written by
`rig/hyperv/v12-verdict.py` from the window's lines in the stick's
`upgrade_/convert.log`, the `%pre` verifier's report, the return check's row
and what the guest had left afterwards. Never by hand. Columns: `launched_as`
(`task-elevated`: a one-shot interactive task, no UAC click) and `renderer`
(the graphics backend it ended on; `wgpu` after an OpenGL refusal); `started`,
`reopen_registered`, `armed`; `handoff_result` (`no-row` when none),
`verify_identity`, `verify_image` (`not-reached`); `reopened`,
`result_heading`, `left_behind` (`none`).

**Result vocabulary.** `pass-plumbing`: Start logged, the reopen task
registered before the arm, the arm, the live boot's report, the handoff fired
once, the window reopened and logged a result matching what came back,
nothing left behind. Else the first failing link by name: `no-window-lines`,
`not-started`, `stopped-before-arm` (the window stopped itself before arming,
the stop in `notes`), `arm-not-logged`, `armed-without-reopen` (the window
must never do this), `not-reopened`, `handoff-mismatch`, `result-mismatch`,
`left-behind`. Seen so far: `stopped-before-arm`.

## `v13-rust-job.csv`: the Rust job writer beside New-Job.ps1 (gate V13, risk R32)

One row per side-by-side run of `upgrade-job` and `New-Job.ps1` on a real
machine, from the two programs' own outputs (`elevated` y/n, `rust_build` the
commit, as in `v13-rust-scanner.csv`). `facts_equal`: `upgrade-job facts`
beside `tools/Record-JobFacts.ps1`, field for field (`yes`, `yes, except <the
named differences>`, or `not run`). `wifi_equal`: the Wi-Fi block with its
keys (`yes` / `not run`). `job_equal`: a `job.json` written by each against
the same scan (`yes (<kept differences>)` / `not run (<why>)`). Every
difference is named in `notes`. A pass is `yes` on all three, elevated, with
only the differences kept on purpose.

## `v13-rust-prologue.csv`: the Rust prologue's reads beside Invoke-Prologue.ps1 (gate V13, risk R32)

One row per side-by-side run of `upgrade-prologue facts` and
`tools/Record-PrologueFacts.ps1` (`Invoke-Prologue.ps1`'s `Get-PrologueFacts`)
on a real machine, written from the two programs' own outputs (`elevated`,
`rust_build` as in `v13-rust-scanner.csv`). `facts_equal` reads `yes`, or
`yes, except <the named differences>`, each named in `notes`. A pass is every
fact the same, elevated, with only the differences kept on purpose (a record
in the state that nothing decides on).

## `r14-rebuild.csv`: the five Windows programs build to the same bytes twice (risk R14)

One row per program per run of `./rebuild-check.sh`, which clones HEAD into a
fresh directory, builds the five Windows programs there the same way (the
pinned toolchain, `--locked`, paths remapped) and compares their sha256 with
this checkout's. `commit`, `program`, `sha256_here`, `sha256_fresh_clone`,
`toolchain` say what was compared; `same` is the result: `y` (the bytes
match) or `n` (they differ). A pass is `y` for every program at one commit.
