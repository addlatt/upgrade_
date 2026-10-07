# Prompt for the cut-over sessions

Paste everything below the line into a new Claude Code session opened in
`/home/addlatt/upgrade_`. It is written for a session that has not seen
this work before. Written 2026-10-07; the facts in it are as of commit
`97476be` on `rust-port`.

---

## The goal

Cut the whole active path of upgrade_ over from PowerShell to Rust, now,
so the product does not split into a PowerShell half and a Rust half. When
this is done, a stick built by `make-kit.sh` runs Rust from the first click
to the restart, and no `.ps1` or `.cmd` on the stick is on the path a
person takes. The owner decided this (2026-10-04, `docs/RUST-PORT.md`):
build the full port on the branch `rust-port`, then cut over.

You are continuing that work. Several sessions' worth is already done and
this prompt tells you exactly what remains. Expect this to take more than
one session; commit after every piece and push `rust-port`.

## Read these first, in this order

1. `CLAUDE.md` (the whole file: the mission, the five rules, the worktree
   rules, the writing voice).
2. `docs/RUST-PORT.md`: the roadmap, the method, the status table, the
   differences kept on purpose, the findings.
3. `docs/RISKS.md`, section R32; `docs/VALIDATION.md`, section V13.
4. `docs/validation-results/port-parity.csv` (the ledger) and
   `docs/validation-results/v13-rust-scanner.csv`.
5. The crates' `lib.rs` files: `schemas/rust`, `evaluate/scan`,
   `evaluate/harvest`, `evaluate/job`, `upgrade_/kickstart`, and the window
   `upgrade_/windows/window`.

## How to work (not negotiable)

- **Own worktree.** `git worktree add .claude/worktrees/<name> -b <name>
  origin/rust-port`, then work there. Never edit the shared folder.
  Commit to your branch and push it; when a piece is complete, merge it
  into `rust-port` (fast-forward or merge) and push `rust-port`. Nothing
  goes to `main` until the owner says the cut-over is done; then it is one
  pull request from `rust-port`.
- **Refuse by default; never softer.** Every refusal the PowerShell makes,
  the Rust makes, in the same words. A difference is allowed only when it
  is stricter, and it is written down in `docs/RUST-PORT.md` under
  "Differences kept on purpose". There is no override for a RED verdict and
  the two typed sentences (`RISK_STATEMENT`, `ERASE_STATEMENT` in
  `evaluate/job/src/lib.rs`) are compared byte for byte.
- **Ledger first.** Before porting a piece, every line of evidence it has
  earned is in `port-parity.csv` as `owed` (the rows are already there for
  the scanner, harvester, job writer and kickstart; write them for the
  prologue, rollback, handoff, password hasher and launchers before you
  start on those). A line turns `pass` only when a Rust test or a re-run
  row matches it.
- **The four moves, per piece:** one list of inputs (`tests/cases.json`);
  the PowerShell original records its own answers (`tests/golden.ps1`,
  run from WSL with `powershell.exe`); the Rust answers the same inputs;
  a `cargo test` requires equality in full. `./port-check.sh` runs all of
  it and must pass before a commit. Look at `evaluate/job/tests/` for the
  fullest example.
- **Live reads are proven side by side.** `upgrade-scan --record` and
  `tools/Record-Machine.ps1` on the same machine in the same minute, then
  `upgrade-scan --compare-facts rust.json powershell.json`, then
  `upgrade-scan --replay rust.json --against powershell.json`. Copy that
  pattern for every new live read.
- **Writers last, reviewed hardest** (CLAUDE.md rule #4). Anything that
  writes to a disk, the firmware or the boot configuration is re-run on
  the rig (`rig/hyperv/*.sh`) with the Rust build before it goes near a
  physical machine, and its PowerShell stays on the stick as the way back
  until the Rust has its own rig rows.
- **Captures are private.** Any file matching `upgrade-report-*` is a
  machine report (hardware, disk facts, program lists) and is gitignored.
  Record the result in a results CSV; never commit the capture.
- **Writing voice:** short sentences, plain words, no em dashes, no emoji,
  dated "Decided" lines, failures recorded next to what fixed them.

## What is already in Rust (do not redo)

| Piece | Where | Proven by |
|---|---|---|
| The `job.json` / `outcome.json` contract | `schemas/rust` (crate `upgrade-schema`) | all 103 `check.py` checks; 4,943 one-edit documents judged the same |
| The scanner, whole: every read, every judgment, the report | `evaluate/scan` (crate `upgrade-scan`, binary `upgrade-scan.exe`) | end to end on the G16 and the Aspire, elevated: Rust reads, Rust judges, PowerShell's report line for line (`v13-rust-scanner.csv`) |
| The harvester: mappings, judgments, folder sizes, cloud placeholders, the read-through | `evaluate/harvest` (crate `upgrade-harvest`) | 41 recorded cases; 12 Windows-only filesystem tests (`windows-tests.sh`) |
| The job writer's judging half: refusals, path, statements, erase drives, records, the job document | `evaluate/job` (crate `upgrade-job`) | 965 recorded calls, word for word and field for field |
| The kickstart generator | `upgrade_/kickstart` (crate `upgrade-kickstart`) | 48 recorded inputs, byte for byte from line 2 |
| The window, verify flow only | `upgrade_/windows/window` (`UPGRADE.exe`) | rig row; it still runs the scripts as child processes |

The ledger stands at 406 `pass`, 20 `owed`.

## The active path today, exactly

What the stick's launchers run (`upgrade_/windows/handoff-payload/RUN-*.cmd`,
every call wrapped in `Invoke-Logged.ps1`):

1. `upgrade-scan.ps1 -Json -OutDir upgrade_\reports` (the report; the job
   writer reads the newest `upgrade-report-*.json` there).
2. `Read-Password.ps1` (asks for the new account's password, writes its
   SHA-512 crypt hash to a file; the password itself is never written).
3. `New-Job.ps1` (reads the report, the machine, the Wi-Fi API; runs
   `Harvest-UpgradeState.ps1 -FolderMapOut` in a child process; writes
   `upgrade_\job.json`, the Wi-Fi password files, and runs
   `New-Kickstart.ps1` for `ks.cfg`).
4. `Invoke-Prologue.ps1` (the converter's Windows half: the disk check and
   its restart, hibernation and pagefile off, restore points and the
   change journal with consent, the shrink, BitLocker suspension, the
   handoff through `Test-Handoff.ps1 -Arm`, the SYSTEM resume task,
   `outcome.json` at every stop).
5. `Invoke-Rollback.ps1` (undo before the commit line) and
   `CANCEL-CONVERSION.cmd`.
6. The typed sentences, the countdown words, the KDE/GNOME/console choice
   and the fork choice live in the `.cmd` files themselves.

The stick writer (`Write-UpgradeStick.ps1`) is run by the owner through
`make-kit.sh`, not by the person; it can stay PowerShell for the cut-over
and be ported after. Same for `Copy-Kit.ps1`, the `Diag-*.ps1` tools and
everything under `rig/`.

## What remains, in order

Each item: ledger lines, cases and golden where the piece has a self-test
or recorded output, the Rust, `port-check.sh` green, a commit, a line in
`docs/RUST-PORT.md`'s status table.

1. **One fix in the scanner's judgment.** `evaluate/scan/src/storage.rs`,
   `volume_health`: the online scan's result is matched against Windows
   10's names (`ErrorsFound`, `ErrorsNotFixed`); Windows 11 names them
   `ScanErrorsFoundNeedSpotFix` and the like (the full table is in
   `collect/storage.rs`, `repair_status_names`). Judge both sets. This is
   stricter than the PowerShell; record it under "Differences kept on
   purpose" and make the parity test expect it.

2. **The scanner as a product command.** `upgrade-scan scan --json
   --out <dir>` writes `upgrade-report-<model>-<stamp>.txt` and `.json`
   exactly where and as `upgrade-scan.ps1 -Json -OutDir` writes them (the
   JSON shape is at the end of `upgrade-scan.ps1`: `ScannerVersion`,
   `ScannedUtc`, `System`, `RanAsAdmin`, `RequiredKernel`, `Verdict`,
   `Recommended`, `Checks`, `Releases`, `UnmatchedIds`). The job writer
   reads that file. Prove it side by side on the G16.

3. **The password hasher** (`Read-Password.ps1`, 201 lines): a console
   prompt, SHA-512 crypt (`$6$`, 5000 rounds as the script does), written
   to the file the launchers expect. Pure logic: the script's own self-test
   carries the specification's vectors; port the hash against those and
   against answers recorded from the PowerShell.

4. **The harvester's live half** (`Harvest-UpgradeState.ps1`, the parts not
   in `evaluate/harvest`): the known folders (`SHGetKnownFolderPath`,
   OneDrive redirection), who owns the desktop (the signed-in session's
   SID against the process SID; `Get-HarvestOwner`), the stick's volume
   facts for the fit (filesystem, cluster size, free bytes), the browsers.
   Output: the folder map JSON the job writer reads (`-FolderMapOut`),
   same field names. Prove it side by side: run the PowerShell with
   `-FolderMapOut` and the Rust on the G16, compare.

5. **The job writer's live half** (`New-Job.ps1`, `Get-JobFacts` and
   `main`): the machine facts (most are the scanner's collectors; share
   them), the Wi-Fi profiles through the Native Wifi API with the
   plaintext key (`WlanGetProfile`, the C# in `Get-JobWlanProfiles` says
   exactly how) and the second count from disk, BitLocker with the
   `manage-bde` fallback, the stick's identity, the clock, the licence
   facts (never a key), SSH keys, the software inventory; then writing
   `job.json` (through `upgrade_schema::Job` so nothing unchecked is ever
   written), the Wi-Fi password files, and `ks.cfg` through
   `upgrade_kickstart`. Prove it: the PowerShell and the Rust write a job
   on the G16 from the same report; the two `job.json` files must be
   equal except `job_id`, the timestamps and `evaluate.version`.

6. **The prologue's judging half** (`Invoke-Prologue.ps1`, 2,124 lines).
   Read it whole first. Separate what decides (the R18 guardrails, the
   two-path re-measure, the fork, the shrink ladder's rungs and their
   consents, R25's waiting update, the stop conditions, the `outcome.json`
   contents at every stop) from what acts. The ledger lines are its rows
   in `r18-prologue.csv`, `v2-install.csv`, `v9-erase.csv`,
   `walkaway-probe.csv`, `r21-rollback.csv`, plus its own `-SelfTest`
   (`Invoke-SelfTest`, near line 1849; `make-kit.sh` runs it). Cases, golden, Rust, as for the job
   writer. The rollback's judging half the same way.

7. **The prologue's live half, the handoff and the rollback.** This is the
   one-way door. Every action keeps the raw output of the tool it called
   (`bcdedit`, `diskpart`, `manage-bde`, `chkdsk`, `vssadmin`, `fsutil`,
   `schtasks`) in the outcome's artifacts, so a later port can be replayed
   against it; the PowerShell never did this, and it is the one lesson of
   this port worth building in from the first line. The handoff is
   `bcdedit /set {fwbootmgr} bootsequence` plus the return-check task
   (`Test-Handoff.ps1`, which the window already imitates in
   `window/src/flow.rs`). The SYSTEM resume task is `schtasks` XML (the
   prologue has it). Prove it on the rig: `rig/hyperv/v1.sh`, `v2.sh`,
   `v9.sh`, the R18 and R21 harnesses, each pointed at the Rust build, each
   appending its row as today. Only then the Aspire, and only with the
   owner at the keyboard.

8. **The launchers as flows in `UPGRADE.exe`.** The verify flow exists
   (`window/src/flow.rs`, `words.rs`). Add convert, convert accepting data
   loss, erase and install, erase accepting data loss, rollback, cancel,
   probe. The typed sentences are compared byte for byte, the words come
   from the decisions and never the other way round (R31), and every
   screen after the acknowledgement says DATA LOSS ACCEPTED. The window
   calls the Rust crates directly; `Invoke-Logged.ps1`'s job (one log of
   everything) moves inside.

9. **The kit.** `make-kit.sh` builds `UPGRADE.exe` and `upgrade-scan.exe`
   (or one binary) with `cargo zigbuild --release --target
   x86_64-pc-windows-gnu` and puts them at the stick's root; `verify.sh`
   and `outcome.sh` on the Linux side are unchanged. The `.cmd` and `.ps1`
   files come off the stick when the rig rows of step 7 are green; until
   then they stay as the way back and the README says so. Builds must be
   reproducible (R14): write down how a second person rebuilds the same
   bytes.

10. **Docs and the ledger.** `docs/RUST-PORT.md` status table,
    `RISKS.md` R32, `VALIDATION.md` V13, `architecture.md` "Stack",
    `CLAUDE.md`'s layout and the "Windows PowerShell 5.1 only" convention
    (which becomes "Rust; the retired scripts are history"). Every ledger
    line `pass`, or `owed` with the reason in the line.

## The machines, and how to reach them

- **The G16** is the machine this WSL runs on (ASUS ROG Zephyrus G16,
  Windows 11). `powershell.exe` is on the path. An `.exe` built in WSL
  must be copied to a Windows folder (`$env:TEMP`) before it is run; one
  run from `\\wsl.localhost` hung. To run something elevated, start it
  with `Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList ...`
  from `powershell.exe`: Windows shows the owner an approval prompt and
  the result comes back when it is answered. Say in your message that you
  did this.
- **The Aspire** (Acer Aspire A515-51G, Windows 11 again since 2026-10-06,
  a dying SATA SSD, RISKS R18/R23) answers `ssh aspire` (the host entry
  and key are in `~/.ssh/config`); that session is elevated. The owner
  uses it for physical tests and it goes down without notice: **ask before
  touching it**, and stop at once if a command fails with "a system
  shutdown is in progress". Files go to `C:\Users\<user>\upgrade-record\`
  with `scp`.
- **The rig** is Hyper-V on the G16; the harnesses are `rig/hyperv/*.sh`
  and their guest scripts. Read `rig/hyperv/README.md` before using it.

## Traps already met (do not meet them again)

- PowerShell's `-eq`, `-match`, `-in`, `-contains` and hashtable keys
  ignore case; `-cmatch` and `-ceq` do not. `[math]::Round` rounds half to
  even. `{0:N2}` adds thousands separators and rounds half away from zero
  on the 15-digit form. `Sort-Object -Descending` reverses the order of
  equal items. `ConvertFrom-Json` gives decimals, which print `0.0` where
  a double prints `0`. These are all in `evaluate/scan/src/ps.rs` and
  `evaluate/job/src/val.rs`; use them.
- WMI through COM: an object has a usable path only when its key property
  (`ObjectId` in the Storage namespace) is in the query; a method call
  needs an input-parameter object even with no parameters; WMI's error
  codes must be mapped to the words PowerShell prints (`Not supported`).
  All in `evaluate/scan/src/collect/wmi.rs`.
- The recorder scripts load a script's functions by cutting its text
  before its main section; a `[switch]` parameter in that text
  type-locks a variable of the same name in the recorder (`$json`).
  `-is [pscustomobject]` is true for any wrapped value; test
  `GetType().Name`. A dictionary entry named `keys` hides `.Keys`; use
  `.PSBase.Keys`.
- Free space, temperatures and shrink room drift between two recorders a
  minute apart; `--compare-facts` already allows for that.

## When you report

After each piece: what is ported, what the proof was (numbers: cases,
lines, checks, machines), what is still owed and why, what you changed in
the docs, and the commit. At the end of the cut-over, before the pull
request: the list of every `.ps1` and `.cmd` that is off the active path,
the rig rows the Rust build earned, and the one physical run the owner
made with it.

What a chat session cannot do: run the rig or the Aspire without the owner
present for anything that writes. Build everything up to that line, hand
the owner the exact command, and record what came back.
