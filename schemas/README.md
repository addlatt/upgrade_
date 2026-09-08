# Contracts between modules

| File | Written by | Read by | Schema |
|---|---|---|---|
| `job.json` (+ `artifacts/`) | `evaluate` | `upgrade_`, `settle-in` | `job.schema.json` |
| `outcome.json` (+ logs) | `upgrade_` | `settle-in` | `outcome.schema.json` |

These are the only interfaces between modules. Everything else is internal.

Both are **versioned** by a single string: `"schema": "job/1"` /
`"schema": "outcome/1"`. A USB written by one release will eventually be read
by another — a module that meets a version it does not understand must
**refuse, not guess**. Guessing here means acting on a misunderstood
instruction while holding a partitioning tool. `check.py` asserts the
refusal as a negative test, so the rule cannot be loosened by accident.

Change these carefully. `data/` is meant to churn via drive-by PRs; this
directory is not. A change here is a change to what a component holding a
partitioning tool will do.

## What the schemas refuse, on purpose

The schemas are JSON Schema draft-07 and they carry the project's rules as
constraints, not just shapes. A document that breaks one of these is
invalid, and a module that finds an invalid document stops:

- **No RED job.** `scan.verdict` is `GREEN | YELLOW`. A RED machine never
  gets a job, and there is no override flag, ever (CLAUDE.md rule #1).
- **The fork is chosen in advance** (RISKS R18, decided 2026-09-08).
  `fork.if_cannot_keep` is `clean-slate | stop` — what the converter does
  if the shrinkable space it re-measures after the disk check does not fit
  Linux. The person answers this at `evaluate`; the prologue never asks. A
  volume that carries the dirty flag additionally requires
  `fork.volume_check_consented = true`: the person was told the prologue
  will run Windows' disk check, with its own restart, and that it must not
  be interrupted.
- **keep-windows needs a Healthy disk and an ESP with room.** A job whose
  `intent.path` is `keep-windows` must carry
  `storage.physical_disk.health_status = Healthy` and
  `storage.esp.fits_alongside_install = true` (RISKS R21).
- **No un-materialized cloud file** (RISKS R8 / V8). `harvest.cloud_files`
  records placeholders found and materialized; `failed` is a constant 0,
  `result` is `none-found | materialized`. "Refused" is not a writable
  result — a refusal at `evaluate` produces no job at all.
- **No truncated sizing** (RISKS R6), **no undetermined BitLocker state**
  (decided 2026-09-07), **no legacy BIOS**, **no unelevated run**.
- **Secrets are files, not fields.** The BitLocker recovery key and the
  Wi-Fi keys live under `artifacts/credentials/` and `job.json` holds only
  their relative paths (no `..`, no absolute paths), so the credential wipe
  is one directory and `job.json` itself can be read by anyone (RISKS R13).
  The account password is SHA-512 crypt or nothing.
- **Unknown fields are refused** (`additionalProperties: false`
  throughout). A field a reader does not know is version skew, not noise.
- **The commit line is a fact.** `outcome.commit_line.crossed` must be
  `false` on `keep-windows` (the line is the reclaim, in `settle-in`) and on
  any `stopped` run; a `completed` `clean-slate` run must say `true` with a
  timestamp and `act = wipe`. `settle-in` reads this to decide whether it
  may still say "you can cancel".
- **A repair only on a Healthy disk.** `prologue.volume_check.disk_health_at_check`
  must be `Healthy` for `ran = true`.

## Identity, and who re-checks it

`identity.system_disk` carries the serial, unique id and size of the disk
that holds C:. The prologue re-validates the whole `identity` block against
the live machine before it does anything; the cutover checks it again from
the live environment, because a stick moved to another computer must never
partition it. `stick` carries the same triple for the USB device `evaluate`
wrote, so the prologue can refuse a different stick and the cutover can
verify every checksum against `stick.manifest` (RISKS R16, R17).

`outcome.job_id` must equal `job.job_id`; `settle-in` refuses a pair that
does not match.

## Checking a change

```
python3 schemas/check.py
```

validates every document in `examples/` against its schema, then feeds each
schema the documents above that must be refused and fails if any is
accepted. Add an example when a field gains a meaning; add a negative when
a rule is added. The examples are also the reference for what each writer
produces — `examples/job.keep-windows.json` is the Acer's shape (flagged
volume, consent given, BitLocker on, OneDrive placeholders materialized).

## Status

Schemas and examples exist (2026-09-08); no writer emits them yet. The
harvester's `state.json` is the seed of `job.json`; the intent-capture
step that fills `intent` and `fork` is not built. When the writers land,
the examples here are what their self-tests must produce.
