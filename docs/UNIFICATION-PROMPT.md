# Prompt for the Rust unification sessions (roadmap item 19)

Expires when: roadmap item 19 is done (no PowerShell on the stick or in `data/`, the installer's `verify.sh` and `outcome.sh` and settle-in's starter in Rust, `port-parity.csv` all `pass`, RISKS R32 closed). Then this file moves to `docs/archive/`.

Paste everything below the line into a new Claude Code session opened in
`/home/addlatt/upgrade_`. It is written for a session that has not seen
this work before. Written 2026-10-13; the facts in it are as of commit
`1223382` on `main` (pull request #21 merged).

---

## The goal

Finish moving upgrade_ to one language. The owner decided it twice: Rust
is the conversion's one language (2026-09-27), and the PowerShell leaves
the product now (2026-10-11). The Windows side is done: `upgrade-scan`,
`upgrade-harvest`, `upgrade-job`, `upgrade-prologue`, `upgrade-kickstart`
and the window `UPGRADE.exe` are Rust, and a stick built by `make-kit.sh`
runs Rust from the first click to the restart. What is left is the edge of
the product: two PowerShell holdovers still on the stick, two diagnostic
scripts, the stick writer, the data tables, and the Linux side the
person's computer runs during the install. This is roadmap item 19, the
first of the owner's three checkpoints (19 Rust unification, 20 product
unification, 21 test optimization, in that order, decided 2026-10-12).

Not in scope: the rig's own Hyper-V PowerShell (`rig/hyperv/vm.ps1`,
`setup.ps1`, `new-vm.ps1`, the guest-side fault injection). It is the
harness, not the product, and stays (`docs/RUST-PORT.md`, "What comes
next"). Items 20 and 21 are later sessions; do not start them here.

## Read these first, in this order

1. `CLAUDE.md`, all of it. The five rules, the worktree rules, the voice.
2. `docs/RUST-PORT.md`, "What comes next" and "Open decisions". The three
   phases of the leaving; phase 1 is done, phases 2 and 3 are yours.
3. `docs/ROADMAP.md`, items 17 and 19, and "Decisions owed".
4. `docs/STATUS.md`, the rows for V13, R32, R16, V5, R9.
5. `docs/validation-results/port-parity.csv` and its entry in
   `docs/validation-results/README.md`: the ledger, 45 lines `owed`.
6. `.claude/skills/docs/SKILL.md`: the one way a doc changes.
7. `make-kit.sh`: what the stick carries today, line by line.

## How to work (not negotiable)

- **Own worktree, main only by pull request.** `git worktree add
  .claude/worktrees/<name> -b <name> origin/main`, work there, push the
  branch, open the pull request, the owner merges. Never edit the shared
  folder. After a merge, `git merge --ff-only origin/main` there.
- **`./port-check.sh` before anything lands.** It checks the schemas, the
  data tables' freshness, the eight frozen goldens
  (`FROZEN-GOLDENS.sha256`) and every crate's tests. A golden changes only
  on purpose, from the Rust's own output, reviewed as a diff, its new
  sha256 in the same commit.
- **Refuse by default; never softer.** Every refusal a script makes, the
  Rust makes, in the same words. A difference is allowed only when it is
  stricter, written down in `docs/RUST-PORT.md`, "Differences kept on
  purpose". No override for a RED verdict. The typed sentences are
  compared byte for byte.
- **Ledger first.** Before porting a piece, every line of evidence it has
  earned is in `port-parity.csv` as `owed`, committed first. A line turns
  `pass` only when a Rust test or a re-run row matches it. The original
  script records its own answers for the cases (`tests/cases.json`, a
  golden written by the original from WSL with `powershell.exe`), then
  the Rust answers the same inputs and a `cargo test` requires equality.
  `evaluate/job/tests/` is the fullest example. Delete the script only in
  the commit that lands its replacement and its golden.
- **Writers last, reviewed hardest** (rule #4). The stick writer writes a
  disk. It runs on the rig with the Rust build before it goes near a real
  machine, and its PowerShell stays on the stick as the way back until the
  Rust has its own rig rows. The Linux-side ports re-earn their rig rows
  the same way before the Aspire sees them.
- **Docs through the docs skill only.** One home per fact; history in
  `docs/log/2026-10.md`; state in `docs/STATUS.md`; plans in
  `docs/ROADMAP.md`. `./docs-check.sh` passes before any pull request that
  touches a `.md` file.
- **Captures are private.** Never commit a machine report
  (`upgrade-report-*`). Never write the owner's account name or university
  into a tracked file. Never push the branch `backup-before-scrub`.
- **Voice:** short sentences, plain words, no em dashes, no emoji, dated
  "Decided" lines, failures recorded next to what fixed them.

## What remains, in order

Each step is its own pull request. Numbers are line counts today.

1. **The diag tools to `upgrade-scan diag`** (`Diag-SecureBoot.ps1` 125,
   `Diag-Smart.ps1` 91, in `evaluate/windows/usb-kit/`). Read-only. Both
   read what the scanner already reads in Rust (the firmware's Secure
   Boot facts, the physical disk's reliability counters); the port is a
   subcommand that prints them. `make-kit.sh` copies the `.ps1` to the
   stick today (lines 198 and 201); it copies nothing after this. Smallest piece,
   start here.
2. **The storage-mode harness to Rust** (`Test-StorageMode.ps1`, 773;
   V5, `v5-controller-mode.csv`, `rig/v5-verdict.py`). It arms a Safe Mode
   boot through `bootsequence`, scans in both SATA modes, and writes the
   row. The prologue already has the pieces (`bcdedit`, the SYSTEM startup
   task, the stick's record files). Port the judging half first with
   cases, then the arming. Its rig arm (`rig/hyperv`) runs again with the
   Rust before the row counts. V5's warn row (`v5-controller-mode.csv`
   line 2, the Aspire) stays owed to the Rust until a re-run.
3. **The stick writer to an `upgrade-stick` crate** (`Write-UpgradeStick.ps1`
   514, `Copy-Kit.ps1` 85). It writes a disk: R16's rows run again
   (`r16-stick-writer.csv`, the wrong-device refusals first, then a real
   stick on the G16, the owner at the keyboard). Keep the PowerShell on
   the stick until those rows exist.
4. **The data tables to JSON** (`data/devices.ps1` 210, `distros.ps1` 62,
   `releases.ps1` 107, `export-tables.ps1` 47). Phase 3. `data/tables.json`
   already exists and the Rust reads it; the move makes it the source and
   drops the exporter. JSON is assumed; the owner's decision on the format
   is still listed under "Decisions owed" in `ROADMAP.md`, so ask once,
   then proceed on JSON if there is no answer. A device stays a one-line
   contribution with a cited source. `make-kit.sh`, `rig/vm/fetch-release.sh`
   and `data/tools/measure-release.py` read the tables; point them at the
   JSON. Then rewrite `CLAUDE.md`'s conventions and layout for a product
   with no PowerShell (through the docs skill; the 250-line budget).
5. **The installer's `verify.sh` (519) and `outcome.sh` (404) to Rust.**
   These run inside Anaconda's stage 2 on the person's machine: `verify.sh`
   is the refusal gate before the commit line (disk identity, hardware,
   the storage `%include`, the erase countdown), `outcome.sh` the boot
   checklist and installer adapter after it. A Linux binary on the stick,
   built for the installer's glibc (check Fedora 44's stage 2; a static
   musl build avoids the question). `settle-in/` already has Linux crates
   and `efi::write_var`; a new crate under `upgrade_/linux/` or a
   subcommand of a shared Linux binary, your call, written down. Rows
   re-earned on the rig: V1 (`v1-live-boot.csv`), V2 (`v2-install.csv`),
   R21 (`r21-rollback.csv`), V9 (`v9-erase.csv`). The kickstart generator
   (`upgrade_/kickstart/src/lib.rs` lines 150 and 201) names the two
   scripts; it changes with them and its golden is re-recorded on purpose.
6. **settle-in's starter** (`settle-in/linux/upgrade_-settle-in.sh`, 34):
   the console hook in `/etc/profile.d/`. It shows the summary at a text
   console and must stay out of the way of a display manager (the
   black-screen bug of 2026-09-27, rig run 2). A `settle-in console-hook`
   subcommand with the same logind check.
7. **Dropped (decided 2026-10-17, the owner; `RUST-PORT.md`).** Was: build and check tools, where feasible: `schemas/check.py` (421)
   into `schemas/rust`'s tests; `make-kit.sh`, `build-rust.sh`,
   `port-check.sh`, `rebuild-check.sh` stay shell unless a Rust tool is
   plainly better. Do not spend the session here.
8. **The 45 owed ledger lines.** Rows earned by the old scripts on the rig
   and the Aspire. Each turns `pass` by a re-run with the Rust, never by
   argument. The rig ones are yours; the Aspire ones need the owner.

After every step: the log entry, the STATUS rows (V13, R32, and the gate
the step re-earned), `port-parity.csv`, and item 17 or 19 in `ROADMAP.md`
if the plan changed. When step 8's last line passes and nothing in
`data/` or on the stick is PowerShell, R32 closes; move it under
`# Closed` in `RISKS.md` and archive `RUST-PORT.md` and this file.

## The machines, and how to reach them

- **The G16** is the machine this WSL runs on (ASUS ROG Zephyrus G16,
  Windows 11). `powershell.exe` is on the path. Copy an `.exe` built in
  WSL to a Windows folder before running it; a run from `\\wsl.localhost`
  hung. Elevation: `Start-Process powershell.exe -Verb RunAs -Wait` from
  `powershell.exe`; the owner sees an approval prompt. Say so in your
  message.
- **The Aspire** (Acer Aspire A515-51G, InsydeH2O, a dying SATA SSD) answers
  `ssh aspire` when it is awake; that session is elevated. It is in
  Windows today, with Fedora beside it and the kit stick off its USB bus.
  Ask before touching it; stop at once on "a system shutdown is in
  progress". Nothing in this prompt needs it until step 8.
- **The rig** is Hyper-V on the G16: `rig/hyperv/*.sh`, read its README
  first. The fresh image was re-saved 2026-10-12 with a bench password that
  never expires; other images from 2026-08-30 may still carry an expired
  one ("The credential is invalid" from PowerShell Direct). A fresh
  worktree needs `rig/hyperv/artifacts` and `rig/vm/artifacts` linked to
  the shared folder's, and `make-kit.sh` wants a clean tree.

## Traps already met (do not meet them again)

- PowerShell's `-eq`, `-match` and hashtable keys ignore case;
  `[math]::Round` is half to even; `Sort-Object -Descending` reverses equal
  items; `ConvertFrom-Json` gives decimals. The helpers are in
  `evaluate/scan/src/ps.rs` and `evaluate/job/src/val.rs`.
- WMI through COM needs the key property in the query and an input object
  for every method call (`evaluate/scan/src/collect/wmi.rs`).
- A fresh worktree's `windows-tests.sh` finds the `.exe` through cargo's
  metadata, not a fixed path; keep it that way.
- Hyper-V hands UTC to the guest's clock, so the rig's kept Windows reads
  seven hours off after a Linux boot (`windows-clock-wrong`); the bench
  owes a Time Synchronization switch-off. Not a product bug.
- On the Acer, `bcdedit`'s firmware order write does not survive a
  restart; efivarfs writes do. The rollback runs from Linux for that
  reason (decided 2026-10-12). Anything that orders the firmware from
  Windows must be checked on a real machine, not the rig.
- WSL's 9p cache goes stale on files Windows just wrote under `/mnt/c`;
  evict before reading.

## When you report

After each step: what is ported, the proof in numbers (cases, lines,
rows, machines), what is still owed and why, the docs touched, the pull
request. A chat session cannot run the Aspire, or anything that writes a
real disk, without the owner present. Build everything up to that line,
hand the owner the exact command, and record what came back.
