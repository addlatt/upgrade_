# The Rust port: roadmap

Decided (2026-09-27, the owner): Rust becomes the conversion's one language,
from the window down to the code that touches the disk. This page is the
plan for getting there: what exists, in what order it moves, and how each
move is proven.

**Decided (2026-10-04, the owner): the whole port is built now, on the
branch `rust-port`, beside the scripts.** It no longer waits for V0's three
more vendors or V9's re-run (that was the 2026-09-27 plan). `main` keeps
validating the process in PowerShell. **As soon as that process has one
success on `main`, the project cuts over to Rust.** The reason is the same
as on 2026-09-27: one language controls the whole path. See "The branch"
and "The cut-over" below.

Three docs already cover parts of this, and this page does not repeat them:

- `architecture.md`, "Stack": what was decided and the five steps.
- `RISKS.md` R32: what can go wrong (a port that decides differently and
  nobody sees it).
- `VALIDATION.md` V13: how a port is proven (the parity ledger).

## Where it stands (2026-10-04)

| Step | What | Status | Evidence |
|---|---|---|---|
| 0 | the window, `UPGRADE.exe` | `[###.]` stop path, `[##..]` reopen; **since 2026-10-07 its verify flow runs the kit's Rust programs** (`upgrade-scan scan`, `upgrade-job write --verify-only --kickstart`, `upgrade-prologue verify-arm`), every line they print kept in `upgrade_\convert.log`; **2026-10-08: the convert flow is in it** (RUN-CONVERT.cmd's steps: the desktop choice, the Linux password hashed in the window with the job crate's own hasher, the job with its kickstart, the sign-in name, the typed `CONVERT` byte for byte, then `upgrade-prologue start`; the data-loss variant only when started with `--accepting-data-loss`, its own launcher, the sentence typed byte for byte, DATA LOSS ACCEPTED on every screen). On the rig (2026-10-08, `v12-window.csv` line 3, `stopped-before-arm`): the chooser, the welcome and the running screens drove by keyboard, `upgrade-scan.exe` ran under the window, and `upgrade-job.exe` refused the rig's stick ("the stick is on bus 'SAS', not USB", the R16 refusal; the script's row of 2026-09-28 stopped at the same place), shown on the stop screen in the job writer's words. Hyper-V has no USB, so the arm and the return are the G16's or the Aspire's to prove; a convert bench for the window is owed | V12, `v12-window.csv` |
| 1 | the schema library (`schemas/rust`) | `[##..]` built, no program uses it yet | 103 of 103 ledger lines `pass` |
| 2 | the scanner's judging half (`evaluate/scan`) | `[##..]` built, no program uses it yet | 103 of 103 self-test and corpus lines `pass`; 10 rig and physical lines `owed` |
| 3a | the scanner's whole-scan order, its text report, a program that replays recordings | `[##..]` built | 6 whole scans match the PowerShell scanner's own main section, report line for line |
| 3b | the harvester in Rust: the pure half and the filesystem reads (`evaluate/harvest`) | `[####]` the 12 filesystem cases pass on the G16 | 47 of 47 self-test lines `pass`; its rig and physical rows `owed` |
| 3c | the live reads: unprivileged half (system facts, device list, registry, installed programs) | `[####]` the G16, side by side: every fact Rust read equals what PowerShell read | `upgrade-scan --record` and `--compare-facts`, 2026-10-06 |
| 3d | the live reads: elevated half (disks, shrink room, volume health with the online scan, physical disk, BitLocker, boot partition, firmware variables and trusted keys, event log, SMART, the stick's boot files) | `[####]` the G16 and the Aspire, elevated, side by side: every fact equal | 2026-10-06; `v13-rust-scanner.csv` |
| 3e | the whole scanner in Rust, end to end: Rust reads the machine, Rust judges, the report is PowerShell's | `[####]` the G16 (SAME, 218 lines, 25 checks, YELLOW) and the Aspire (SAME, 166 lines, 26 checks, RED), both elevated | `upgrade-scan --replay rust.json --against powershell.json`, 2026-10-06 |
| 3f | the scanner as the product command: `upgrade-scan scan --json --out <dir>` writes the report files where `upgrade-scan.ps1 -Json -OutDir` writes them; `upgrade-scan dump-machine` writes the `-DumpMachine` capture | `[####]` the G16, both scanners back to back, elevated and not: the JSON reports SAME field for field, the text reports byte-identical | `upgrade-scan compare-reports`, 2026-10-07; `v13-rust-scanner.csv` line 7. The one difference kept on purpose in the judgment (the Windows 11 scan names) is below |
| 3g | the harvester's live half (`evaluate/harvest/src/live.rs`; `upgrade-harvest folder-map`): the known folders, whose desktop this is, the stick's volume facts, the browsers, the folder map as `-FolderMapOut` writes it | `[####]` the G16, both harvesters back to back, not elevated (the elevated run's UAC prompt was cancelled, so that run is still owed): the owner, all 6 folders, the cloud block, the stick and the fit SAME field for field; the browsers the same | `upgrade-harvest compare-maps`, 2026-10-07; `harvest-folder-map.csv` line 5 |
| 4a | the kickstart generator (`upgrade_/kickstart`) | `[##..]` built, no program uses it yet | 22 of 22 self-test lines `pass`; its rig and physical rows `owed` |
| 4b | the job writer's judging half (`evaluate/job`) | `[##..]` built, no program uses it yet | 126 of 126 self-test lines `pass`; rig and physical rows `owed` |
| 4c | the job writer's live half (`evaluate/job/src/live.rs`; `upgrade-job write`): every read of `Get-JobFacts`, the Wi-Fi export, `job.json` written only as a document that passed the contract, the password files, `ks.cfg` through the kickstart crate | `[####]` the G16, `upgrade-job facts` beside the script's `Get-JobFacts`, elevated and not: SAME field for field, the elevated-only reads and the Wi-Fi block with its keys included; the only difference is the two Store-signed non-removable apps, kept on purpose. A `job.json` beside the PowerShell's (needs the kit stick) is owed | `upgrade-job compare-facts`, 2026-10-07; `v13-rust-job.csv` lines 2 and 3. The stick writer is not started |
| 4d | the password hasher (`evaluate/job/src/password.rs`; `upgrade-job password`) | `[##..]` built; the prompt not yet run on a stick | 12 of 12 self-test lines `pass`, 34 recorded calls match, the specification's vectors held directly; its 3 physical rows `owed` |
| 5a | the prologue's judging half (`upgrade_/prologue`, crate `upgrade-prologue`): the R18 guardrails, the re-validation, the shrink plan and the fork, the ladder's rungs and consents, R25's update step, the erase path's start and return, the handoff classifier, the resume context, the state, the `prologue` block and a stopped outcome | `[##..]` built, no program uses it yet | 143 of 144 self-test lines `pass` (the 144th is the Wi-Fi scrub, a live action); 161 cases, 228 calls match word for word; every realistic stopped outcome passes `outcome.schema.json`; its 36 rig and 12 physical rows `owed` |
| 5b | the prologue's live half (`upgrade_/prologue/src/live.rs`, `flow.rs`; the program `upgrade-prologue start|resume|notify|abort|probe`): every read and every reversible write of the script, each tool call kept with its raw output (`tools.jsonl` in the state directory and in `upgrade_/report/` on the stick), the SYSTEM startup task as `schtasks` XML, the state directory locked with `icacls`; and the rollback (`rollback.rs`; `upgrade-prologue rollback`) | `[###.]` rig, the R18 arm passed: run 3 (2026-10-08, kit of commit e423cca) was the whole keep-Windows path with the disk check in it, nobody at the keyboard: the bench's dirty flag read, `chkdsk /f` armed and run at a restart (Wininit 1001, `dirty_after=clean`), both resumes as `NT AUTHORITY\SYSTEM` in session 0 (`run_as: SYSTEM` in the record), 57.9 GB re-measured by both paths, 25 GB freed (the partition table agrees), the handoff fired, Fedora 44 KDE installed beside Windows, Windows and Linux each booted twice, the return record carried into `outcome.json`. `r18-prologue.csv` line 14 and `v2-install.csv` line 8, both `pass-plumbing`. Run 2 the evening before (line 13, `flag-not-confirmed`; line 7) found the R25 restart undoing the bench's fault and the `run_as` defect (both fixed, see below). Its reads were proven beside `Get-PrologueFacts` on the G16, elevated and not (`v13-rust-prologue.csv` lines 2 and 3). The rollback (`r21-rollback.csv` line 4) and the walk-away probe (`walkaway-probe.csv` line 4, `resumed-unattended`, SYSTEM in session 0) passed the same day. Owed: the erase arms on the rig (`v9.sh`), then the Aspire. The rig harnesses take `PROLOGUE=rust`; `make-kit.sh` ships the binaries |
| 5c | the verify flow's handoff (`upgrade_/prologue/src/verify.rs`; `upgrade-prologue verify-arm|verify-check`, following `Test-Handoff.ps1` 0.3.1) | `[##..]` built; 39 of 39 recorded calls match, 33 of 33 self-test lines `pass`; no rig row yet (`PROLOGUE=rust rig/hyperv/v1.sh`, and the window's verify flow) | the rows: `v0-handoff.csv` lines 2 to 12, `v1-live-boot.csv` lines 2 to 5 | the handoff harness (`Test-Handoff.ps1`) is not ported: the prologue arms the handoff itself, and the window's verify flow still runs the script |

Nothing has been switched over. The stick still runs the PowerShell, and
every result in `docs/validation-results/` still belongs to the PowerShell
that earned it. Steps 1 and 2 are Rust code that sits beside the scripts
and is held to them by tests.

One command checks all of it:

```text
./port-check.sh
```

## The branch

All of the port lives on `rust-port` until the cut-over. Nothing from it
goes to `main` before then, so nothing the stick runs changes while the
PowerShell process is being validated.

`main` will keep changing (a fix after a physical run, a new check). The
port follows it like this:

1. Merge `main` into `rust-port` often.
2. Run `./port-check.sh`. If a script changed what it says, the recorded
   answers are stale and the check fails, naming the file.
3. Record again (`./port-check.sh --record`), read the diff, and change the
   Rust until it says the new thing too.

So a PowerShell fix can never be silently missing from the Rust: the check
fails until it is carried over.

## The cut-over

The trigger is the owner's: one success of the PowerShell process on
`main`. What the cut-over is, and is not:

- **It is the moment the Rust becomes the thing under test.** From then on
  the rig and the physical machines run the Rust build.
- **It does not carry evidence across.** Every rig and physical row was
  earned by the PowerShell that ran (rule #2). The parity tests prove the
  Rust decides the same and says the same. They cannot prove what a real
  disk or real firmware does when the Rust is the one asking. Those ledger
  lines stay `owed` until the Rust re-earns them.
- **The order at cut-over:** every self-test, corpus and differential line
  `pass` (before the cut-over, on this branch); then the rig harnesses
  (`rig/hyperv/*.sh`) re-run with the Rust build; then one physical run on
  the Aspire. The `.cmd` launchers and the scripts stay on the stick as the
  way back until the Rust has its own rig rows.

What makes the cut-over safe to be confident in is how much is closed
before it: everything a test can close without hardware. That is the work
of this branch.

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
| `Harvest-UpgradeState.ps1` | 1,279 | `evaluate/harvest` (crate `upgrade-harvest`; the program `upgrade-harvest folder-map`) | 3 | `-SelfTest`, 47 cases |
| `New-Job.ps1` | 1,451 | `evaluate/job` (crate `upgrade-job`; the program `upgrade-job write`) | 4 | `-SelfTest`, 126 cases |
| `New-Kickstart.ps1` | 226 | part of the job writer crate | 4 | rig rows |
| `Write-UpgradeStick.ps1` | 514 | a stick writer (shares ideas with `settle-in/src/stickwrite.rs`) | 4 | `r16-stick-writer.csv` |
| `Read-Password.ps1` | 201 | `evaluate/job/src/password.rs`, run as `upgrade-job password` until the window takes the prompt | 4 | `-SelfTest`, 12 cases |
| `Invoke-Prologue.ps1` | 2,124 | `upgrade_/prologue` (crate `upgrade-prologue`), then the converter's Windows half | 5 | `-SelfTest`, 144 cases; rig and physical rows (`r18-prologue.csv`, `v2-install.csv`, `v9-erase.csv`, `walkaway-probe.csv`) |
| `Invoke-Rollback.ps1` | 238 | `upgrade_/prologue/src/rollback.rs` (`upgrade-prologue rollback`); its rig row passed 2026-10-08 (`r21-rollback.csv` line 4) | 5 | `-SelfTest`, 14 cases; `r21-rollback.csv` |
| `Test-Handoff.ps1` | 809 | `upgrade_/prologue/src/verify.rs` (`upgrade-prologue verify-arm` and `verify-check`), the verify flow's handoff | 5 | `-SelfTest`, 33 cases; `v0-handoff.csv`, `v1-live-boot.csv` rows |
| the `.cmd` launchers | about 1,200 | flows inside `UPGRADE.exe`: all six are in (2026-10-08: verify, convert, erase, roll back, the walk-away probe, cancel; the data-loss variants behind `UPGRADE-ACCEPTING-DATA-LOSS.cmd`), untried on the rig in this form | 5 | V12 |
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

Items 1 and 2 are built (2026-10-04). `run::scan` is the scanner's main
section with the reads taken out, and `report::lines` is its text report.
The proof runs the PowerShell scanner's own main section, as written, with
only its reads replaced by a case's facts, and requires the same report,
line for line: six whole machines (the Aspire elevated, the G16 not
elevated, the VMD spoof, a clean ThinkPad, the same ThinkPad 199 days
later, an ARM machine with almost nothing readable). The program is
`upgrade-scan --replay machine.json`.

**A real machine, replayed (2026-10-04).** `tools/Record-Machine.ps1` runs
the PowerShell scanner's own main section with its own collectors on a
live machine, read-only, and keeps each collector's answer and the report
it printed in one file. `upgrade-scan --replay` on that file judges the
same facts in Rust and compares. First run, the G16, not elevated: SAME,
238 report lines and 25 checks, word for word. Second run, 2026-10-06, the
G16 elevated: SAME, 218 report lines and 25 checks. That run read what the
first could not: 76.5 GB of shrink room, BitLocker on for C:, 185.9 MiB
free on the boot partition, the firmware's six trusted signing authorities,
the drive's reliability counters. Third run, 2026-10-06, the Aspire on
Windows 11 again, elevated, over SSH: SAME, 166 report lines and 26 checks.
Its verdict is RED, from SMART alone this time: 748 uncorrectable reads
(725 on 2026-09-13), and a fresh Windows whose event log holds no bad-block
events yet.
Two machines, both verdicts, elevated and not. This proves the Rust
judges a real machine's real facts as the PowerShell does. It does not
prove the Rust can read them: that is items 3 and 4. The capture holds the
machine's program list and is never committed (`.gitignore` covers its
name).

**The first live reads in Rust (2026-10-06).** `evaluate/scan/src/collect`
reads a Windows machine through Microsoft's own bindings (the `windows`
crate): WMI over COM for the system facts and the device list, the registry
for Secure Boot, Fast Startup, the SBAT level and the installed programs.
`upgrade-scan --record` writes a capture in the recorder's shape;
`upgrade-scan --compare-facts rust.json powershell.json` compares two
captures fact by fact. On the G16, not elevated, run within a minute of
the PowerShell recorder: every fact Rust read is what PowerShell read. The
system facts, all 253 devices with their classes, drivers and compatible
IDs, Secure Boot, Fast Startup, and all 110 installed programs. One
difference showed up and was fixed on the spot: `Get-CimInstance` gives
dates in local time, and the Rust had kept the BIOS date in UTC. The same
evening on the Aspire, over SSH (elevated; the reads need no privilege):
SAME again, 131 devices and 13 programs. Two machines, no difference.

What this build does not read yet is named in the capture (`NotRead`) and
in the comparison, never guessed: elevation, the firmware's own SBAT copy,
the stick's boot files, the trusted keys, the resume facts, the disks,
volume health, the physical disk, BitLocker, the boot partition. Those are
step 3d. The build is `cargo zigbuild --release --target
x86_64-pc-windows-gnu` in `evaluate/scan`; run the `.exe` from a Windows
folder, not from `\\wsl.localhost`.

**The elevated reads (2026-10-06, the same evening).** The rest of the
scanner's reads, in Rust: whether the process is elevated; the firmware's
own SBAT level and its trusted signing authorities (the `db` variable,
read with the system-environment privilege and its certificates' names
parsed from DER); the resume facts (the Schedule service, the task policy,
`dsregcmd`); the disks, the volume and the room to shrink (the Storage
API's `GetSupportedSize`, with diskpart as the second path); the drive
that holds C: with its reliability counters, the System log's disk events
and ATA SMART; volume health (`fsutil`, the volume's status, NTFS event
98, Wininit 1001, the Chkdsk log); BitLocker; the boot partition (mounted
on a free letter, measured, unmounted). On the G16, elevated, both
recorders within a minute: every fact Rust read is what PowerShell read.

Three things the comparison caught on the way, all fixed: WMI gives an
object a usable path only when its key (`ObjectId`) is asked for, and
without one the shrink-room method and the counters' association both
failed; a WMI method needs an input-parameter object even with no
parameters, as `Invoke-CimMethod` sends one; WMI's error codes have to be
mapped to the words PowerShell uses (`Not supported`).

The online scan (`Repair-Volume -Scan`) came last, the same evening: the
Storage API's `Repair` method with `Scan` on, which only reads. Neither
machine had a reason to run it (no dirty flag, no queued repair), so it was
run on its own, elevated, on the G16's C: both ways: Rust `NoErrorsFound`,
PowerShell `NoErrorsFound`. The result's name is read from this machine's
own Storage module (`Volume.cdxml`), because Windows 10 and Windows 11 name
the results differently. **A finding for `main`:** on Windows 11 the names
are `ScanNoErrorsFound`, `ScanErrorsFoundNeedSpotFix` and so on, and the
PowerShell scanner's judgment looks for `ErrorsFound` or `ErrorsNotFixed`
exactly (Windows 10's names). A Windows 11 scan that found errors would not
be judged as having found them; the fact would still be in the note. The
Rust port mirrors the judgment as it is (parity), so the fix belongs in
`upgrade-scan.ps1` first.

The harvester's 12 filesystem cases followed (`evaluate/harvest/src/folders.rs`,
`tests/filesystem.rs`, run on Windows by `windows-tests.sh`): folder
sizes with the on-stick arithmetic (20,760 bytes for the self-test's tree,
to the byte), a real junction not followed, a folder this account may not
list counted as unreadable, the offline attribute, allocated bytes, the
read-through. All 12 pass on the G16.

The Aspire's elevated side-by-side
run came the same evening, over SSH: every fact Rust read is what
PowerShell read there too, including the ATA SMART block (748 uncorrectable
reads, 7 reallocated), the nine trusted authorities in its firmware and the
Storage API's 164.6 GB of shrink room. A second run on the Aspire caught
the machine shutting down for the owner's test and is void.

**End to end (2026-10-06, the G16).** `upgrade-scan --replay <Rust
capture> --against <PowerShell capture>` judges what Rust itself read and
compares the report with the one PowerShell made from its own reads, the
two recorders run back to back, elevated: SAME, 218 report lines and 25
checks, word for word. That is the scanner in Rust from the machine to the
report, with one read (the online scan) still PowerShell's alone. The
Aspire followed the same evening, elevated over SSH: SAME, 166 report lines
and 26 checks, verdict RED on both sides, from SMART that Rust read itself.

Items 3 to 5 no longer wait on V0 and V9 (decided 2026-10-04, above).
Item 4 needs a Windows machine; the G16 is the first.

### The harvester's pure half. Built 2026-10-04.

`evaluate/harvest` (following harvester 0.3.0) holds what needs no machine:
the time zone and sign-in name mappings, the two cloud-placeholder
judgments (R8), whether the folders fit the stick (R26), the backup
arithmetic, and the reader for one exported Wi-Fi profile. 41 cases match
the PowerShell's answers; 35 of them are the self-test's own.

The other 12 self-test cases need a real Windows filesystem: a junction, a
folder Windows will not list, the offline attribute, allocated bytes. They
stay `owed` and are named in `tests/owed-selftest.txt`. `port-check.sh`
requires every self-test case to be in the Rust's list or in that file, so
none can be forgotten.

One thing the port found in the PowerShell: `Get-HarvestWifi` drops a
profile it cannot parse without a word (an empty `catch`). A saved network
with no authentication element would vanish from the list. The Rust reader
returns an error for such a file, and the live half must list that network
as not carried. This harvester path is no longer the one a job uses (the
job writer reads Wi-Fi through the Native Wifi API and counts profiles two
ways), so nothing on the stick is affected today.

## Step 4: the job writer, the stick writer, the kickstart.

- **The kickstart generator: built 2026-10-04** (`upgrade_/kickstart`,
  following `New-Kickstart.ps1` 0.5.0). It is a pure function: a job goes
  in, a text file comes out. 48 inputs match the PowerShell: its 22
  self-test cases, every example job, every refusal (in the same words and
  the same order), and the manifest's edges. The text is equal byte for
  byte from the second line on. The converter's way in (`kickstart_for`)
  only takes a `Job` that passed the whole contract.
- **The job writer's judging half: built 2026-10-04** (`evaluate/job`,
  following `New-Job.ps1` 0.18.0). Facts and the person's choices go in; a
  job document or the refusals come out. It holds the path decision (keep
  Windows, clean slate, or no job), the typed data-loss statement (R23), the
  erase sentence and its drive list (R27), the folder-map refusals (R5, R6,
  R8, R26), the release check (R34), and the records a job carries (clock,
  licence, SSH keys, Wi-Fi rows, installed programs). 965 calls match the
  PowerShell: all 126 self-test cases with their own inputs, and 15 more
  groups for the branches the self-test does not reach. Every refusal is
  word for word. Every job document is equal field for field, in the same
  order. And every job it writes from the self-test's machine passes
  `job.schema.json`, checked with the step 1 library.
- Its live half is not built: reading the machine, writing `job.json` and
  the Wi-Fi password files, calling the harvester.
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
| kickstart, first line | `generated by New-Kickstart.ps1 0.5.0` | `generated by upgrade-kickstart 0.1.0 (Rust; follows New-Kickstart.ps1 0.5.0)` | a file should say what wrote it. A comment; nothing reads it |
| kickstart, way in | its own shape checks (they accept `JOB/1`, any case) | the converter's way in needs a `Job` that passed the whole schema | never softer |
| installed programs in a job | sorted by the machine's language rules (PowerShell's `Sort-Object`) | sorted by the lower-cased name, the same on every machine | the list is an inventory; nothing decides on its order. Past the 2,000 cap the two could keep different entries |
| Store apps marked non-removable (2026-10-07) | `Get-AppxPackage`'s `NonRemovable` flag, whose source Windows does not expose; dropped from the inventory | read as membership of Windows' inbox-application list in the registry, which covers 44 of the 46 on the G16; the two Store-signed ones (Windows Security, App Installer) are listed | an inventory, nothing decides on it; two Microsoft apps more is the whole difference |
| the prologue's facts `Hiberfil` and `Pagefile` (2026-10-07) | `Test-Path C:\hiberfil.sys`: False even elevated, because the provider cannot open a file locked to SYSTEM | whether the file is there (`GetFileAttributes`): True when hibernation and the pagefile are on | the Rust answers the question asked; a record in the state (`Facts.hiberfil`), nothing decides on it |
| the prologue's evidence times (2026-10-07) | NTFS event 98 and the last check are carried with the event's full precision (`2026-10-08T07:38:46.3872739Z`) | to the second (`2026-10-08T07:38:46.0000000Z`): the shared event reader keeps seconds | the same instant to the second; the freshness comparison and every decision are unchanged. The words of `RepairQueuedWhy` differ in the fraction |
| the prologue's "is NTFS's request fresh" (2026-10-07) | `[DateTime]::Parse` of the evidence's round-trip text, then `ToUniversalTime()`: a time with no zone is read as this machine's local time | a time with no zone is read as UTC | the prologue's evidence always carries the `Z`; the self-test's one naked date gives the same answer either way. A text that is not a time makes the script throw; the Rust takes the request as standing (never a skip) |
| the job writer's refusals at the door | a missing stick or scan folder, a wrong `-Desktop`, or a failed read makes the script throw (a traceback, exit 1) | the same words as a plain refusal, exit 2; a job is written only after `upgrade_schema::Job` accepted it | stricter and plainer; the script wrote without checking the contract |
| a job's `evaluate.version` | `0.18.0` | given by the program that writes the job | a record should say what wrote it |
| `Disk N` detail line | printed with the machine's own number format (`931,5 GB` on a German Windows) | always a dot (`931.5 GB`) | a display line only; no decision reads it |
| the password typed twice (2026-10-07) | `-cne`, which compares by the culture's rules: `é` and `e` + a combining accent count as the same, and the pair is accepted | the two entries are compared byte for byte; that pair is refused with "the two entries are not the same" | stricter, and right: Fedora's sign-in compares bytes, so the accepted pair would have locked the person out. `tests/password.rs` holds it by name |
| the JSON files' form: the report, the folder map (2026-10-07) | `ConvertTo-Json`: a byte order mark, CRLF, .NET dates (`\/Date(...)\/`), a one-item array written as the item | plain UTF-8, LF, dates as `2025-05-19T20:00:00` local time, arrays always arrays; one field more saying what wrote the file (`Scanner`, `Harvester`) and, in the report, `ReadErrors` only when a read failed | the same values in a plainer form. Every reader (`New-Job.ps1`, the window, the Rust) accepts both; nothing decides on `System.BiosDate`. The text report is byte-identical, mark and line ends included |
| "Volume health", the online scan's result (2026-10-07) | judged under Windows 10's names only (`ErrorsFound`, `ErrorsNotFixed`); on Windows 11 a scan that found errors (`ScanErrorsFoundNeedSpotFix`, `ScanErrorsFixedOnlineAlsoNeedSpotFix`, `ScanErrorsFoundAndFixedOnline`) read as ok, the word left in the note | both sets of names mean errors found: WARN, "online scan reported: ..."; and `ScanNoErrorsFound` gets the "its own log contradicts" clause like `NoErrorsFound` | stricter. `storage::SCAN_FOUND_ERRORS`; `tests/parity.rs` holds the four cases by name, and fails the day the PowerShell says the same, so the exception cannot outlive its reason |

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
upgrade_/kickstart/  crate upgrade-kickstart: job.json -> ks.cfg
evaluate/harvest/    crate upgrade-harvest: the harvester's pure half
evaluate/job/        crate upgrade-job: facts -> job.json or refusals
evaluate/scan/       crate upgrade-scan: the judging half
  tests/cases.json     the inputs (shared with the PowerShell recorder)
  tests/golden.ps1     asks the PowerShell scanner; writes golden.json
  tests/golden.json    the PowerShell's answers
  src/main.rs          upgrade-scan --replay machine.json
data/tables.json     the data tables as JSON (written by a tool)
port-check.sh        the one command
```

Each crate stands alone with its own `Cargo.lock`, like `settle-in` and the
window. Both build for `x86_64-pc-windows-gnu` (checked 2026-10-04). The
tests run on Linux, so a session without Windows can still run them; only
recording the golden files needs `powershell.exe`.

**Rebuilding the same bytes (R14; built 2026-10-08).** The bytes on the
stick must be matched to the source by someone else, so the build is
pinned and checked:

- `rust-toolchain.toml` at the root names the one rustc (1.93.1) and the
  two targets; rustup picks it up by itself.
- zig 0.13.0 (sha256 in `upgrade_/windows/window/build.sh`) and
  cargo-zigbuild 0.23.4 link the Windows target; `--locked` holds every
  crate to its `Cargo.lock`.
- Every program's release profile strips symbols and uses one codegen
  unit, and `build-rust.sh` and the window's `build.sh` remap the source
  and registry paths that rustc would otherwise write into the binary
  (`--remap-path-prefix`), so a build from any directory gives the same
  bytes.
- `KIT-MANIFEST.txt` names the five programs' sha256 and the toolchain
  line (`build-rust.sh --toolchain`).
- `./rebuild-check.sh` is the proof: it clones HEAD into a fresh
  directory, builds the five programs there and compares their sha256
  with this checkout's, one row per program in
  `docs/validation-results/r14-rebuild.csv`. A second person runs the
  same script on their machine; a `same=n` row is a finding, not a
  rounding error. First run: owed (the rig was busy when this was built).

## The cut-over sessions (from 2026-10-07)

`docs/CUTOVER-PROMPT.md` is the brief for the sessions that cut the whole
active path over, in this order: the scanner's product command (done, 3f),
the password hasher (done, 4d), the harvester's live half (done, 3g), the
job writer's live half (built, 4c; a `job.json` side by side owed), the prologue's judging half (built, 5a), the prologue's
live half with the handoff and the rollback (their ledger lines written),
the launchers as flows in `UPGRADE.exe`, the kit, the docs. The prologue's
live half is built (5b): what it needs next is the rig (`PROLOGUE=rust
rig/hyperv/prologue.sh run`, then `v9.sh`), and only then the Aspire.

**The verify flow's handoff (5c, 2026-10-07):** `Test-Handoff.ps1`, the V0
harness that RUN-VERIFY.cmd and the window arm the one-shot boot with, is
`upgrade_/prologue/src/verify.rs`: the fail modes, the payload table, the
evidence row and the logon task for the return check on top of the
prologue's own classifier, manage-bde parse, GRUB block and stick lookup.
Its 33 self-test cases and 6 port cases are replayed against the script
(`tests/verify.rs`, `verify-golden.json`: 39 of 39 match). The live half
(`upgrade-prologue verify-arm --stick X: --auto --payload shim
--suspend-bitlocker`, and `verify-check`, which the arm registers as a
logon task for the person) follows Invoke-Arm and Invoke-Check step for
step, every tool call recorded. Built, not yet run: the rig's `v1.sh arm`
takes `PROLOGUE=rust`, and the window's verify flow calls it. The ledger
had 12 lines for this script that were not cases (`Arm`, `Check`,
`SelfTest`, the task name, caught by a loose pattern on 2026-10-07); they
were removed, so the script's 48 lines are its 33 cases and 15 rows.

**Built in from the first line (5b):** every tool the Rust prologue calls
is kept with its command line, exit code and both output streams, in
`tools.jsonl` beside the state and in `upgrade_/report/` on the stick.
The PowerShell never did this, and its physical runs cannot be replayed
(R32). A later port, or a later reader, can replay the Rust's decisions
against what the tools really printed.

**The rig's first Rust run (2026-10-07 evening, `PROLOGUE=rust
rig/hyperv/prologue.sh run`, kit of commit 2174a97):** the Rust prologue
stopped at re-validation, `volume_health.repair_queued: job says 'False',
machine says 'True'`. Windows was right and so was the Rust: on this
guest (Windows 10 Pro 19045) `fsutil dirty set C:` logs NTFS event 98 at
once and `Get-Volume` reports `Full Repair Needed`; the rig's stand-in
job (`rig/hyperv/v1-job.py`) wrote `repair_queued: false` without
looking. The PowerShell prologue, run on the same guest state minutes
later, read the same repair-queued state word for word and would have
stopped on the same job. Fixed in the harness: the stand-in now reads
the volume's status and event 98 as the product's job writer does. The
same PowerShell run then met R25 (the fresh rig disk has a Windows update
waiting), which the Rust run exercises next. A stop at re-validation is
the prologue doing its job (rule #1); it was recorded, not softened.

**The rig's second Rust run (2026-10-07, later the same evening, kit of
commit 2174a97):** the whole keep-Windows path ran with nobody at the
keyboard, and three things came out of it.

- *The update restart undid the bench's fault.* The fresh rig disk has a
  Windows update waiting for a restart (CBS `RebootPending`; the
  PowerShell prologue read the same minutes earlier). The R25 gate let it
  finish before anything changed, exactly as designed. But the bench's
  dirty flag (`fsutil dirty set C:`) did not survive that clean restart:
  the guest's System log shows NTFS event 98 "Volume C: is healthy" at the
  next mount and no autochk (no Wininit 1001) at that boot. The prologue
  then read a clean volume with no repair queued and judged the check not
  needed, as the script would have. The row is honest
  (`flag-not-confirmed`), and the harness now lets a pending update finish
  before it injects the flag (`prologue.sh update-clear`), so the R18 arm
  tests the check again. Every earlier rig row predates the gate (0.9.0,
  2026-09-26), which is why nobody had met this.
- *`run_as` was wrong.* The Rust named the account with `GetUserNameExW`,
  which calls SYSTEM `WORKGROUP\UPGRIGHV$`; the script's
  `WindowsIdentity.GetCurrent().Name` says `NT AUTHORITY\SYSTEM`. So the
  record's two resumes read `run_as: user` for a SYSTEM task in session 0.
  Fixed: `upgrade_scan::collect::win::account_name` names the token's own
  SID, as .NET does. The row's raw notes keep the machine account as it
  was read.
- *The verdict's `record` test could not pass after a return.* The record
  on the stick carries every resume, including the one the return adds
  after the install, while `outcome.json` carries the block as it was at
  the arm. Since 4908fdf (2026-09-13 20:03Z, four hours after the last
  rig return) plain equality had been impossible; `prologue-verdict.py`
  now asks that every other key be equal and that the outcome's resumes
  be the first of the record's. Run 2 re-judged: `record=y`. A harness
  defect, not a prologue one.

**The rig's third Rust run (2026-10-08, kit of commit e423cca):**
`pass-plumbing` on both rows, with the disk check in the path this time.
The bench's `update-clear` step read the fresh disk's pending update
directly (`cbs=True wu=False`, then `cbs=False` after one restart), which
is the primary evidence for the finding above. One parity gap came out of
the row's evidence: the Rust wrote the volume's status as `53263` where
the script's row 6 (the same image and the same fault) had `Full Repair
Needed`. The number is `0xD00F`, missing from the status table copied
from `Get-Volume`'s words. The prologue's repair-queued trigger looks for
the word "repair" in that status, so on a machine with no NTFS event 98
the Rust would have missed a queued repair the script sees: the less
cautious side. Mapped now (`collect::storage::operational_status`), with
the run as its source; the neighbouring codes (Scan Needed, Spot Fix
Needed) have not been seen raw and stay numbers until they are.

**A finding from 5a for `main`:** `Get-PrologueRepairMethod` knows only
Windows 10's scan names. On Windows 11 a scan that found errors
(`ScanErrorsFoundNeedSpotFix`) with no other evidence makes the prologue
*refuse* the disk check, and `ScanNoErrorsFound` is not the spot-fix
either. That is the safe side (never repair on a guess), so the Rust keeps
it exactly; the fix belongs in `Invoke-Prologue.ps1` first, as the
scanner's did. The log, the volume's status and NTFS event 98 still
choose the full check on their own. The Windows 11 scan names are fixed in the Rust (above); the fix
to `upgrade-scan.ps1` on `main` is still owed there.

## What comes next

In order. The first three need no decision.

1. **Put the schema library to work.** `settle-in` and the window read
   `job.json` and `outcome.json` through `upgrade-schema`.
2. **Make the PowerShell prologue keep raw tool output** (on `main`), and
   fix the scan-result names for Windows 11 in `upgrade-scan.ps1` (the Rust
   already judges them, above). Owed since 2026-09-27. Every physical run
   made without it is one the Rust cannot replay later.
3. **The prologue's judging half** (its guardrails and its fork), the same
   way as the job writer's.
4. The job writer's live half, then the stick writer, then the prologue's live half, handoff and rollback (step 5).

## Open decisions (the owner's)

1. **Which run counts as the "one success"** that triggers the cut-over.
   The owner names it when it happens; it gets a dated line here. Note
   (2026-10-04): `main` now records the Aspire's run 11 as "the first clean
   conversion". Whether that is the success meant is the owner's to say.
   What the Rust has today is the deciding, not the doing: no Rust has read
   a live machine or written to a disk yet.
2. **The data tables' format** after the PowerShell scanner is retired.
3. **Beyond the Windows side.** The decision covers the Windows side. The
   Linux side still has shell (`upgrade_/linux/verify.sh`, `outcome.sh`,
   `make-kit.sh`) and Python (`schemas/check.py`, `data/tools/`). Whether
   those move to Rust too is not decided, and nothing here plans it.
