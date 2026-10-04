# The Rust port: roadmap

Decided (2026-09-27, the owner): Rust becomes the conversion's one language,
from the window down to the code that touches the disk. This page is the
plan for getting there: what exists, in what order it moves, and how each
move is proven.

Three docs already cover parts of this, and this page does not repeat them:

- `architecture.md`, "Stack": what was decided and the five steps.
- `RISKS.md` R32: what can go wrong (a port that decides differently and
  nobody sees it).
- `VALIDATION.md` V13: how a port is proven (the parity ledger).

## Where it stands (2026-10-04)

| Step | What | Status | Evidence |
|---|---|---|---|
| 0 | the window, `UPGRADE.exe` | `[###.]` stop path, `[##..]` reopen | V12, `v12-window.csv` |
| 1 | the schema library (`schemas/rust`) | `[##..]` built, no program uses it yet | 103 of 103 ledger lines `pass` |
| 2 | the scanner's judging half (`evaluate/scan`) | `[##..]` built, no program uses it yet | 103 of 103 self-test and corpus lines `pass`; 10 rig and physical lines `owed` |
| 3 | the read-only collectors (the scanner's reads, the harvester) | `[#...]` planned | ledger lines not written |
| 4 | the job writer, stick writer, kickstart generator | `[#...]` planned | ledger lines not written |
| 5 | the prologue, handoff, rollback | `[#...]` planned | ledger lines not written |

Nothing has been switched over. The stick still runs the PowerShell, and
every result in `docs/validation-results/` still belongs to the PowerShell
that earned it. Steps 1 and 2 are Rust code that sits beside the scripts
and is held to them by tests.

One command checks all of it:

```text
./port-check.sh
```

## The rules of the port

These come from CLAUDE.md and from R32. They are short so they can be
checked.

1. **Ledger first.** Before a piece is ported, every piece of evidence it
   has earned gets a line in `docs/validation-results/port-parity.csv`,
   marked `owed`. The ledger says what must be matched, not what happened
   to be tested.
2. **The script stays in use until its last line passes.** A PowerShell
   file is retired only when all its ledger lines read `pass`. Until then
   the window and the `.cmd` launchers keep calling it.
3. **Word for word.** A refusal, a warning and a reason say the same thing
   in Rust as in PowerShell. The tests compare the full text, not only the
   status.
4. **Never softer.** Where the Rust differs on purpose, it is stricter, and
   the difference is written down here ("Differences kept on purpose").
5. **A replay closes a decision, never a machine.** A test fed recorded
   output proves the Rust decides the same. What the firmware or the disk
   does still takes the machine (rule #5 in CLAUDE.md).
6. **Writers last.** Anything that writes to a disk is ported last and
   reviewed hardest (rule #4).

## How one piece is ported

Steps 1 and 2 used the same four moves. Every later piece uses them too.

```text
 1. cases      one list of inputs, taken from the piece's own self-test,
               plus extra inputs that reach the wording the self-test misses
 2. golden     the PowerShell (or Python) original answers every case,
               and its answers are written to a file
 3. Rust       the port answers the same cases
 4. compare    a cargo test requires the two to be equal, in full
```

The original is always the one that writes the golden file. Nobody types an
expected answer by hand. When the original changes, `./port-check.sh`
notices that the golden file is stale and fails until it is recorded again
(`./port-check.sh --record`), and then the Rust has to match the new
answers.

## The map: what exists and where it goes

The Windows side is about 10,600 lines of PowerShell. Sizes are line counts
on 2026-10-04.

| PowerShell today | Lines | Becomes | Step | Self-test today |
|---|---|---|---|---|
| `schemas/*.schema.json` + `check.py` | 3,300 | `schemas/rust` (crate `upgrade-schema`) | 1 | `check.py`, 103 checks |
| `upgrade-scan.ps1`, judging half | about 1,300 | `evaluate/scan` (crate `upgrade-scan`) | 2 | `-SelfTest`, 102 cases |
| `upgrade-scan.ps1`, reads and report | about 1,300 | `evaluate/scan`, a `collect` module and a program | 3 | none (live reads) |
| `Harvest-UpgradeState.ps1` | 1,279 | a harvest crate under `evaluate/` | 3 | `-SelfTest` |
| `New-Job.ps1` | 1,451 | a job writer crate | 4 | none of its own; rig and physical rows |
| `New-Kickstart.ps1` | 226 | part of the job writer crate | 4 | rig rows |
| `Write-UpgradeStick.ps1` | 514 | a stick writer (shares ideas with `settle-in/src/stickwrite.rs`) | 4 | `r16-stick-writer.csv` |
| `Read-Password.ps1` | 201 | part of the window | 4 | none |
| `Invoke-Prologue.ps1` | 2,124 | the converter's Windows half | 5 | rig and physical rows (`r18-prologue.csv`) |
| `Invoke-Rollback.ps1` | 238 | the converter's Windows half | 5 | `r21-rollback.csv` |
| `Test-Handoff.ps1` | 809 | the converter's Windows half | 5 | `v0-handoff.csv` |
| the `.cmd` launchers | about 1,200 | flows inside `UPGRADE.exe` | 5 | V12 |
| `data/*.ps1` | 379 | stays the edit surface for now (see "The data tables") | 2 | checked by the self-test |

Not on the list, and staying as they are: the rig scripts under `rig/`
(test equipment, not product), the diagnostic scripts on the kit
(`Diag-*.ps1`), and the test harnesses (`Test-Materialize.ps1`,
`Test-StorageMode.ps1`).

## Step 1: the schema library. Built 2026-10-04.

`schemas/rust` reads `job.json` and `outcome.json` in Rust.

- **The two schema files stay the contract.** The crate carries
  `job.schema.json` and `outcome.schema.json` inside itself and checks a
  document against them. There is no second copy of the rules in Rust that
  could drift.
- **A small reader, written here.** It knows exactly the schema keywords
  the two files use. A keyword it does not know refuses the schema when it
  loads. It has two dependencies (`serde_json`, and `regress` for the
  patterns). The trust model is "read the source", so a few hundred lines
  that can be read beat a large library that cannot.
- **A checked document is its own type.** `Job` and `Outcome` can only be
  made by passing the contract. Code that holds one holds a checked
  document.

Evidence: all 103 checks `check.py` runs are replayed (9 examples, 3 pairs,
91 refused documents). The refused documents are written by `check.py`
itself (`--dump-cases`), so both checkers judge the same documents. On top
of that, a differential run feeds 4,943 one-edit documents to both: same
answer on every one, except 54 where Rust is stricter (see below).

Not done: no program uses the crate yet. `settle-in` still reads its JSON
loosely. Moving `settle-in` and the window onto this crate is the next
small step (see "What comes next").

## Step 2: the scanner's judging half. Built 2026-10-04.

`evaluate/scan` holds every function in `upgrade-scan.ps1` (scanner 0.5.0)
that takes facts and says what they mean: the 19 checks, the parsers for
what Windows' tools print, the verdict, the kernel requirement and the
distribution recommendation.

It does not read the machine. It has no `main`. The PowerShell scanner is
still the scanner.

Evidence, all through `tests/parity.rs`:

- **167 cases, compared word for word** with what the PowerShell answers
  (`tests/golden.json`): every check's section, title, status, detail, note
  and remedy, in order, plus the verdict and the recommendation.
  - 100 are the PowerShell self-test's own cases, under their own names.
  - 3 are the machine recordings in `evaluate/windows/corpus/`.
  - 64 are extra cases that reach wording the self-test never reaches (a
    32-bit CPU, a Surface, a drive with every kind of error at once).
- **The self-test's own expectations are held directly too**, without the
  golden file: the expected statuses, the phrases that must and must not
  appear, the parsers' return values.
- `port-check.sh` checks that the PowerShell self-test and `cases.json`
  name exactly the same cases, so a case added to one is missed in the
  other at once.

Two things the comparison caught that reading would not have:

- PowerShell 5.1's `Sort-Object -Descending` puts equal entries in the
  reverse of their table order. With no kernel requirement the scanner
  recommends Linux Mint, Pop!_OS, Ubuntu LTS, in that order. The Rust does
  the same, and a test holds it there.
- `[math]::Round(31.95, 1)` is 32 and `[math]::Round(0.45, 1)` is 0.4. The
  Rust rounds the same way, checked against answers PowerShell gave.

Still `owed` for the scanner (10 ledger lines): its rig and physical rows
(`v5-controller-mode.csv` lines 2 to 10, `v1-live-boot.csv` line 4). Those
were earned by the scanner reading real machines, which is step 3. The
search for the scanner's rig and physical rows is not finished: before
step 3 starts, every results file is read again for rows the scanner had a
part in.

## Step 3: the read-only collectors. Planned.

This is where the Rust first touches a real Windows machine, read-only.

1. **The report.** Port `Write-UpgReport` (the text report). Proof: the
   PowerShell and the Rust print the same report, character for character,
   for every corpus recording.
2. **A program that replays.** `upgrade-scan.exe --replay capture.json`
   judges a recording and prints the report. No live reads yet. This is
   the first Rust scanner anyone can run, and it can do no harm.
3. **The reads, one at a time.** Registry values, WMI classes
   (`Win32_PnPEntity`, `Win32_ComputerSystem`, `MSFT_Partition`,
   `MSFT_PhysicalDisk`, `Win32_EncryptableVolume`), the event log, firmware
   variables, and the text of `fsutil`, `diskpart` and `bcdedit`. Each read
   keeps its raw answer, so every run leaves a capture behind (rule #5).
4. **Side by side on a real machine.** Run the PowerShell scanner and the
   Rust scanner on the same machine in the same minute and require the same
   JSON. First the G16, then the rig, then the Aspire. This is the row that
   closes the scanner's `owed` lines.
5. **The harvester.** Its ledger lines first (its `-SelfTest` cases and
   `harvest-folder-map.csv`), then the same four moves.

**Waits on (decided 2026-09-27):** V0's three more vendors and V9's
physical re-run. See "Open decisions" for the one question this raises.

## Step 4: the job writer, the stick writer, the kickstart. Planned.

- The kickstart generator first. It is a pure function (a job goes in, a
  text file comes out), so the four moves apply directly: same job, same
  kickstart, byte for byte.
- The job writer next. Its output is a `job.json`, and step 1 already
  checks those.
- The stick writer last in this step. It writes to a disk (the stick), so
  rule #4 applies: its rig rows and its two physical R16 rows are re-run
  with the Rust build before it replaces anything.

## Step 5: the prologue, the handoff, the rollback. Planned, last.

These change the internal drive and the firmware's boot settings. They
carry most of the project's physical evidence (the Aspire's runs 1 to 10).
A replay can prove the Rust makes the same decisions from the same recorded
tool output. It cannot prove the shrink, the BitLocker suspension or the
one-time boot entry. Each of those takes the rig, then the Aspire, again.

Before step 5 can be planned in detail, the PowerShell prologue has to
start keeping the raw output of every tool it calls (R32, "From now on
every physical run keeps the raw output"). That change is to the
PowerShell, and it is owed now, because every physical run made without it
is a run that cannot be replayed later.

When step 5's last line passes, the `.cmd` launchers and the scripts leave
the stick, and `UPGRADE.exe` is the whole Windows side.

## Differences kept on purpose

Each one is stricter than the original, or changes no decision.

| Where | The original | The Rust | Why |
|---|---|---|---|
| schema `format: date-time` | `check.py` skips it when Python's optional date library is missing (it is missing on the development machine) | always checked, calendar included | refuse by default |
| schema patterns | Python's regular expressions | ECMAScript's (what JSON Schema names) | `$` no longer accepts a trailing newline; `\d` is ASCII digits only. Both stricter |
| `Disk N` detail line | printed with the machine's own number format (`931,5 GB` on a German Windows) | always a dot (`931.5 GB`) | a display line only; no decision reads it |

## The data tables

`data/*.ps1` is the community's edit surface: adding a device is a one-line
change with a source. That does not change yet.

The Rust cannot read PowerShell, so `data/tools/export-tables.ps1` writes
the same tables to `data/tables.json`, and the Rust is built with that file
inside. `port-check.sh` fails when `tables.json` is older than the `.ps1`
files say. Never edit `tables.json` by hand.

When the PowerShell scanner is retired, the tables need a home that is not
PowerShell and is still a one-line edit. That format is not decided.

## Layout and build

```text
schemas/rust/        crate upgrade-schema: the contract reader
evaluate/scan/       crate upgrade-scan: the judging half
  tests/cases.json     the inputs (shared with the PowerShell recorder)
  tests/golden.ps1     asks the PowerShell scanner; writes golden.json
  tests/golden.json    the PowerShell's answers
data/tables.json     the data tables as JSON (written by a tool)
port-check.sh        the one command
```

Each crate stands alone with its own `Cargo.lock`, like `settle-in` and the
window. Both build for `x86_64-pc-windows-gnu` (checked 2026-10-04). The
tests run on Linux, so a session without Windows can still run them; only
recording the golden files needs `powershell.exe`.

Reproducible builds (R14) are owed before any Rust `.exe` replaces a
script: the bytes on the stick must be matched to the source by someone
else.

## What comes next

In order. The first three need no decision.

1. **Put the schema library to work.** `settle-in` and the window read
   `job.json` and `outcome.json` through `upgrade-schema`, and refuse what
   it refuses.
2. **The report and the replay program** (step 3, items 1 and 2). No live
   reads, so nothing waits on hardware.
3. **Make the PowerShell prologue keep raw tool output.** Owed since
   2026-09-27. The Aspire's run 11 should not be made without it.
4. **The kickstart generator** (step 4, first item): a pure function, and
   the next piece with a clean differential test.
5. The live reads (step 3, items 3 to 5), when the gate below opens.

## Open decisions (the owner's)

1. **What exactly waits for V0 and V9?** The 2026-09-27 decision says steps
   3 and later wait for V0's three more vendors and V9's physical re-run.
   Read strictly, that stops all Rust work past step 2. A narrower reading
   keeps what the wait protects (nothing the stick runs changes while the
   physical matrix is being filled) and still lets the port move: Rust
   pieces may be built and tested beside the scripts, and no script is
   replaced until the gate opens. Items 2 and 4 above assume the narrower
   reading. Not decided.
2. **The data tables' format** after the PowerShell scanner is retired.
3. **Beyond the Windows side.** The decision covers the Windows side. The
   Linux side still has shell (`upgrade_/linux/verify.sh`, `outcome.sh`,
   `make-kit.sh`) and Python (`schemas/check.py`, `data/tools/`). Whether
   those move to Rust too is not decided, and nothing here plans it.
