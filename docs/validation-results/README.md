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
