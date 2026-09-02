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
| `notes` | recovery prompt? logo hang? anything odd |

### Result vocabulary

| Result | Meaning | Verdict |
|---|---|---|
| `fired-once` | payload ran, one-shot self-cleared, boot order intact | **pass** |
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
  *detectable* safe failure.

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
