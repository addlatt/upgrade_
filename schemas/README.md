# Contracts between modules

The three parts of the project only talk to each other through two files.
Think of them as work orders: one says what to do, the other says
what was done.

| File | Written by | Read by | Schema |
|---|---|---|---|
| `job.json` (+ `artifacts/`) | `evaluate` | `upgrade_`, `settle-in` | `job.schema.json` |
| `outcome.json` (+ logs) | `upgrade_` | `settle-in` | `outcome.schema.json` |

These are the only interfaces between modules. Everything else is internal.

Both carry a **version** in one string: `"schema": "job/1"` /
`"schema": "outcome/1"`. Sooner or later a USB stick written by one release
will be read by another. A module that meets a version it does not
understand must **refuse, not guess**. Guessing here means acting on a
misread instruction while holding a partitioning tool. `check.py` tests the
refusal as a negative test (a document that must fail), so nobody can
loosen the rule by accident.

Change these files carefully. `data/` is meant to change often through
drive-by PRs; this directory is not. A change here is a change to what a
component holding a partitioning tool will do.

## What the schemas refuse, on purpose

The schemas are JSON Schema draft-07 (a standard way to describe what a
valid JSON file looks like). They carry the project's rules as constraints,
not just shapes. A document that breaks one of these rules is invalid, and a
module that finds an invalid document stops:

- **No RED job**, except through `risk_acknowledgement` (decided
  2026-09-13, RISKS R23). A RED verdict is accepted only with that block
  present, its `statement` the fixed sentence word for word, and
  `overrides` naming one or both of `disk-health` / `volume-health`.
  Nothing else lifts a RED, and the block never names any other refusal.
- **The fork is chosen in advance** (RISKS R18, decided 2026-09-08).
  `fork.if_cannot_keep` is `clean-slate | stop`: what the converter does if
  the space it can shrink, re-measured after the disk check, does not fit
  Linux. The person answers this at `evaluate`; the prologue never asks. A
  volume that carries the dirty flag (Windows' "check this disk" mark) also
  requires `fork.volume_check_consented = true`: the person was told the
  prologue will run Windows' disk check, with its own restart, and that it
  must not be interrupted. Two more consents are optional booleans (absent
  means no consent): `fork.restore_points_consented` (2026-09-20) and
  `fork.usn_journal_consented` (2026-09-22). Each means the person was told,
  on the screen where CONVERT is typed, that Windows' restore points or
  NTFS's change journal are deleted if they are what stops the shrink.
- **keep-windows needs a Healthy disk and an ESP with room.** (The ESP is
  the small boot partition every UEFI machine has.) A job whose
  `intent.path` is `keep-windows` must carry
  `storage.physical_disk.health_status = Healthy` and
  `storage.esp.fits_alongside_install = true` (RISKS R21).
- **No silently empty cloud file** (RISKS R8 / V8). `harvest.cloud_files`
  records placeholders found and materialized (downloaded for real);
  `failed` is always 0, and `result` is
  `none-found | materialized | left-in-cloud`. `left-in-cloud` (decided
  2026-09-26) means online-only files were found and not downloaded: their
  bytes are in OneDrive, each folder's `cloud_only_files` says how many,
  and `settle-in` must skip them and reconnect OneDrive, never copy the
  stubs. "Refused" is not a result you can write: a refusal at `evaluate`
  produces no job at all.
- **A clean slate must fit the stick** (decided 2026-09-26, RISKS R26).
  `harvest.stick_fit` is required on every job. It records the stick
  volume's filesystem, cluster size and free bytes; the folders' bytes;
  what they need on that volume (cluster-rounded files, directories and
  manifest, or bytes × 1.02 if that is larger, plus 64 MB); files over
  4 GB (FAT32 cannot hold one); `fits`; and the gap.
  `intent.path = clean-slate` requires `fits = true`. A keep-windows job
  records it for the discard offer.
- **No truncated sizing** (RISKS R6), **no undetermined BitLocker state**
  (decided 2026-09-07), **no legacy BIOS**, **no unelevated run**.
- **The software inventory is private by where it lives** (decided
  2026-09-13). `harvest.software` lists the person's desktop programs and
  Store apps, names only, so `settle-in` can suggest Linux equivalents. It
  lives in `job.json` on the stick and in nothing that leaves the machine:
  not the text report, not the JSON report, not the machine capture, not
  `outcome.json`.
- **Secrets are files, not fields.** The BitLocker recovery key and the
  Wi-Fi keys live under `artifacts/credentials/`, and `job.json` holds only
  their relative paths (no `..`, no absolute paths). That way wiping the
  credentials means deleting one directory, and anyone can read `job.json`
  itself (RISKS R13). The account password is SHA-512 crypt or nothing.
- **Unknown fields are refused** (`additionalProperties: false`
  throughout). A field a reader does not know means the versions don't
  match. It is not noise.
- **The commit line is a fact.** `outcome.commit_line.crossed` must be
  `false` on `keep-windows` (there the line is the reclaim, in `settle-in`)
  and on any `stopped` run. A `completed` `clean-slate` run must say `true`,
  with a timestamp and `act = wipe`. `settle-in` reads this to decide
  whether it may still say "you can cancel".
- **A repair only on a Healthy disk.** `prologue.volume_check.disk_health_at_check`
  must be `Healthy` for `ran = true`.

- **An erase keeps nothing, and says so** (decided 2026-09-26, RISKS
  R27). `erase_consent` carries the typed sentence word for word, and names
  every drive to erase by identity (the system drive first, then an
  optional Healthy home drive). A clean-slate job carries `staged` or
  `erase_consent`, never both and never neither. The outcome records
  `cutover.countdown` (its end is the commit line), and a cancelled
  countdown is `stopped_at countdown`.
- **What the computer starts at is the person's choice** (2026-09-26).
  `intent.start_at` (`desktop` / `console`) is required, and a completed
  outcome's `install.boot_target` must match it. The Aspire's run 9 reached
  a text console nobody chose.

## Identity, and who re-checks it

`identity.system_disk` carries the serial, unique id and size of the disk
that holds C:. Before it does anything, the prologue re-checks the whole
`identity` block against the live machine. The cutover checks it again from
the live environment, because a stick moved to another computer must never
partition it. `stick` carries the same three facts for the USB device
`evaluate` wrote, so the prologue can refuse a different stick and the
cutover can verify every checksum against `stick.manifest` (RISKS R16, R17).

`outcome.job_id` must equal `job.job_id`. `settle-in` refuses a pair that
does not match.

## Checking a change

```
python3 schemas/check.py
```

This checks every document in `examples/` against its schema. Then it feeds
each schema the documents above that must be refused, and fails if any is
accepted. Add an example when a field gains a meaning; add a negative when a
rule is added. The examples are also the reference for what each writer
produces. `examples/job.keep-windows.json` is the Acer's shape (flagged
volume, consent given, BitLocker on, OneDrive placeholders materialized).

## Status

Schemas and examples exist (2026-09-08). Writers (2026-09-12): the job
writer `evaluate/windows/New-Job.ps1` writes `job.json`. The prologue
`upgrade_/windows/Invoke-Prologue.ps1` writes a stopped `outcome.json` at
any refusal, and writes the `prologue` block (via `upgrade_/prologue.json`
on the stick) that `upgrade_/linux/outcome.sh` carries into the completed
`outcome.json`. The same day, the prologue block gained
`volume_check.scan` and `.restarts`, `shrink.remeasured_by` and
`.remeasured_diskpart_gb`, an optional `staged` block and `handoff.marker`.
Added later: `volume_check.trigger` (2026-09-17), `resumes[]` (2026-09-13),
`shrink.restore_points_deleted` (2026-09-20), `shrink.usn_journal_deleted`
(2026-09-22), and `prologue.windows_update` with the `stopped_at` value
`windows-update` (2026-09-26, RISKS R25). Still not built: the
intent-capture step that fills `intent` and `fork` from the person's
answers (the launchers pass fixed values today), and the harvest of Wi-Fi
and browsers into `harvest`. The folder map is in (job writer 0.10.0,
2026-09-26): `harvest.folders`, `harvest.cloud_files` and
`harvest.stick_fit` come from the harvester's `-FolderMapOut`, and
`evaluate.harvest_version` names it. The examples here are what the
writers' self-tests must keep producing.
