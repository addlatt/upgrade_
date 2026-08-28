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
  `NoSuspend` fail-mode → recovery prompt in `notes` (proving suspension is
  load-bearing).
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
