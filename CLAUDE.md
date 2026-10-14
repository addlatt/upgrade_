# upgrade_: working guide

Read this first, every session. It carries the mission and the rules that are easy
to break without noticing. The design lives in `docs/architecture.md`, the unknowns
in `docs/RISKS.md`, the plan for closing them in `docs/VALIDATION.md`, where every
gate stands in `docs/STATUS.md`, what is next in `docs/ROADMAP.md`, and what
happened in `docs/log/`. But the *culture* below is what keeps this project
trustworthy, and you can't see it in the code.

## The mission

Move an ordinary computer from Windows to Linux, in one go, for someone who does not
know how. Plug in a USB stick, pick a desktop, click convert. Come back to a working
Linux machine with your files, Wi-Fi and browsers intact, and (by default) the old
system shrunk safely aside until they're sure.

The audience is non-technical people whose working Windows 10 machines were stranded
by Windows 11's hardware requirements and the October 2025 end of security updates.
The machines were never the obstacle: they run Linux fine. The obstacle is the
knowledge needed to get there. This project carries that knowledge for them.

**The vision works from any source; the code is Windows-only.** The docs and README
describe conversion as `Source → Linux`, but every line today reads a Windows
machine (PowerShell, `bcdedit`, BitLocker, `netsh`). Other source systems are a
future direction, not a v1 promise. Where the docs say "Windows", they mean the one
source that works now.

## The rules that are not negotiable

These are the project's spine. Quietly breaking one is how a tool like this hurts
someone.

1. **Refuse by default.** The only asset is that the report can be trusted. A
   scanner that says "probably fine" and is wrong is worse than no scanner, because
   the person acted on it and lost their data. There is **no override flag for a RED
   verdict.** When unsure between two severities, pick the more cautious one. If a
   check is wrong, fix the check. Don't remove it. There will be pressure to soften
   warnings (often from contributors whose own machine works). Resist it.
   **Amended (2026-09-13, RISKS R23):** one narrow exception exists, and it is not a
   flag. A person may type, verbatim, on a separate launcher, *"I confirm that I
   understand the risks and could lose data"*. That lifts exactly two refusals, the
   drive-health and volume-health ones, whose failure mode is losing that machine's
   own files, and only after the scanner has said RED in full. It lifts nothing
   whose failure mode is "cannot work" or "wrong machine". The sentence travels in
   `job.json` and `outcome.json`, every screen after it says DATA LOSS ACCEPTED, and
   no shorter form of consent is accepted anywhere. Widening this exception is the
   thing rule #1 forbids.

2. **Evidence, not argument.** A risk closes only when a primary source or a real
   machine confirms it, never because the reasoning sounds right. This applies to
   our own claims too. `RISKS.md` states, for each unknown, what would actually
   happen if it's real and what evidence would close it. Nothing in it is closed by
   a good paragraph.

3. **The commit line.** Exactly one moment per conversion is irreversible (a one-way
   door), and the source OS stays bootable until it. The interface must say "you can
   still cancel" until that exact moment, and stop the instant it's crossed. And
   **everything that can refuse must refuse before the line.** After it, the only
   safety left is a slow recovery that depends on hardware that might itself fail.

4. **Trust is spent once.** This tool earns trust once and loses it for good the
   first time it destroys someone's photos. Any component that writes to a disk must
   clear a far higher bar than one that reads. That's why the writers are built last
   and reviewed hardest.

5. **Spoof everything spoofable, and never confuse a spoof with evidence.** (Decided
   2026-08-22.) Everything that *can* be checked without real hardware *must* be, at
   three levels:
   - **logic:** detection functions fed made-up objects (the `-SelfTest` cases);
   - **recordings:** real machines' hardware lists captured with `-DumpMachine`,
     curated into `evaluate/windows/corpus/`, and replayed on every self-test run. A
     recording is ground truth for that machine, forever;
   - **simulated hardware:** VMs presenting spoofed devices, so the full Windows
     enumeration → WMI → scanner pipeline runs for hardware we don't own.

   Two rules keep this honest. First, **every contact with a real machine leaves a
   capture behind**: hardware reached once must stay testable forever. Second, **a
   spoofed pass closes plumbing, never a real-hardware clause.** A simulation is
   built from our model of the hardware, and the model is usually the thing in
   question (rule #2 in different clothes). Each risk names the part that can't be
   spoofed (its residue), and that residue still takes a real machine.

## Right now

The standing priority, above all feature work: validate the killers before building
anything that writes. Several things the whole project depends on exist only as
argument, which by rule #2 counts for nothing, so no component that writes to a disk
is built until the spine it depends on is proven on real hardware. The one current
target (as of 2026-10-11) is the keep-Windows path on the Aspire with shim in
Windows' slot. The return applied it by itself and the clock held on 2026-10-11
(R21, R36); still owed there: a real Windows update under the guard (R22) and the
rollback on this machine. Where every gate stands:
`docs/STATUS.md`. What is next, and the decisions owed: `docs/ROADMAP.md`. What
happened, run by run: `docs/log/`.

## How the design works (one paragraph)

Three modules, split on **commitment** (can this step still be undone?), not on OS.
`evaluate` (Windows, read-only) scans, harvests what only Windows can give (the
folder map, materialized cloud files, firmware, the BitLocker key), captures intent,
writes the stick, and refuses. `upgrade_` (the converter; Windows → Linux) does the
move. By **default** it shrinks Windows aside and installs Linux alongside, keeping
Windows as a rollback. Only on opt-in or a too-full disk does it wipe and stage
files to the stick instead. `settle-in` (Linux, first boot) verifies the hardware,
**pulls the user's files from the kept Windows partition** (default path), and
offers reclaim once everything is confirmed. No external drive anywhere: one stick
is the whole kit.

The module names are roles, not OSes: `evaluate` runs on the OS being left,
`settle-in` on the OS being arrived in. **Decided (2026-10-02, the owner): every OS
switch, in any direction, carries the Wi-Fi networks and their passwords as part of
the one click** (`architecture.md`, "What migrates"). A new direction is not
finished until it does.

## Layout

```
data/            hardware + distro knowledge base; community PRs land here
  devices.ps1      Wi-Fi/GPU/audio/storage quirks by PCI ID
  distros.ps1      distro kernel table (goes stale; verify against release notes)
  releases.ps1     the Linux releases the kit can carry, and what Secure Boot needs of each (measured by tools/measure-release.py, never typed)
  tables.json      the same three tables as JSON, for the Rust side (written by tools/export-tables.ps1, never edited)
evaluate/windows/  the corpus of machine recordings; the one PowerShell holdover (the stick writer; its phase 2 port retires it)
evaluate/scan/, harvest/, job/  the scanner, the harvester and the job writer in Rust (the product since 2026-10-11)
evaluate/scan/, evaluate/harvest/, evaluate/job/  the scanner's, harvester's and job writer's judging halves in Rust (held to the PowerShell word for word)
upgrade_/          the converter: prologue/ the prologue, handoff, rollback and the slot in Rust; kickstart/ the kickstart generator in Rust; windows/window/ the window (UPGRADE.exe), windows/handoff-payload/ the stick's EFI payload and the two .cmd starters; linux/ %pre verify + outcome
settle-in/         first-boot verify + file pull + reclaim (the file pull and reclaim are not built)
schemas/           job.json / outcome.json contracts (change rarely, review hard); rust/ reads them in Rust
docs/              architecture.md (how it works), RISKS.md (what is unproven), VALIDATION.md (how it is proven), research/ (devices, future paths)
  STATUS.md          where every gate and open risk stands: the one home for state
  ROADMAP.md         what is next, in what order, and the decisions owed: the one home for plans
  log/               what happened, one file a month, append-only; archive/ expired phase docs, each with its Archived line
  validation-results/  the CSVs (ground truth, append-only) and what their words mean; tools/docs-check.py the docs checker (./docs-check.sh)
dist/kit/          the kit, built by ./make-kit.sh (gitignored; the stick's contents)
.claude/skills/docs/  the docs skill, the one way to change a doc (SKILL.md, references/shapes.md, budgets.json)
```

## Working conventions

- **The Windows side is Rust** (decided 2026-09-27 and 2026-10-11, the owner;
  RISKS R32, VALIDATION V13): `upgrade-scan`, `upgrade-harvest`, `upgrade-job`,
  `upgrade-prologue` and the window `UPGRADE.exe`, built by `./build-rust.sh` with
  a pinned toolchain (`rust-toolchain.toml`; `rebuild-check.sh` proves the bytes).
  The PowerShell scripts left the product on 2026-10-11; what remains of it
  (the stick writer, `data/*.ps1`)
  keeps to Windows PowerShell 5.1 (no PS7 syntax) until its phase in
  `docs/RUST-PORT.md`, "What comes next", retires it.
- **Run `./port-check.sh` before any change lands.** It runs `schemas/check.py`,
  checks the data tables are fresh, checks the eleven frozen goldens are unchanged
  (`FROZEN-GOLDENS.sha256`: recordings of what the PowerShell answered, ground truth
  for the Rust forever), and runs every crate's tests. A golden changes only on
  purpose, from the Rust's own output, reviewed as a diff, with its new sha256
  recorded in the same commit. Add a case if you change verdict, detection, parsing
  or arithmetic logic. Live-OS reads stay behind collect/judge seams so the judging
  halves stay testable (rule #5).
- **Docs change only through the docs skill** (`.claude/skills/docs/SKILL.md`): one
  home per fact, history in `docs/log/`, state in `docs/STATUS.md`, plans in
  `docs/ROADMAP.md`. Run `./docs-check.sh` before any pull request that touches a
  `.md` file.
- **`data/*.ps1` is the contribution surface** until phase 3 moves it to JSON.
  Adding a device is a one-line PR with a cited source ("it should work" is not a
  source). The Rust reads `data/tables.json`, written from the tables by
  `data/tools/export-tables.ps1`; `./port-check.sh` fails when it is stale.
- **Never commit a machine report** (`upgrade-report-*.txt/.json`). They hold
  someone's hardware and account details. `.gitignore` covers them; keep it that
  way.
- **Never commit the V0 EFI binaries.** They are build inputs, gitignored, fetched
  as described in `upgrade_/windows/handoff-payload/README.md`.
- **Licence is GPL-3.0.** The trust model is "read the source"; copyleft keeps forks
  readable. Don't reintroduce permissively-licensed files without a reason.

## Working alongside other sessions (decided 2026-09-28, the owner)

Several Claude sessions often work in this repo at once. On 2026-09-27 that went
wrong twice: an accidental reset of the shared folder lost `rig/hyperv/v12.sh` and a
window fix before they were committed, and sessions pushing only their own commits
left the local `main` and GitHub's `main` 20 and 9 commits apart. Three rules keep
it from happening again:

- **Every session works in its own worktree** (a second checkout of the repo on its
  own branch), under `.claude/worktrees/<name>`. The shared folder stays on a clean
  `main` that nobody edits directly. Git ignores `.claude/worktrees/` through
  `.git/info/exclude`.
- **Work reaches `main` only through a pull request.** GitHub enforces it: `main`
  refuses direct pushes, force pushes and deletion. Push the branch, open the pull
  request, and the owner merges it. After a merge, bring the shared folder forward
  with `git merge --ff-only origin/main`, never a reset.
- **Remove a worktree only after its work is merged and its session is closed.**
  Removing it under a live session pulls the folder out from under that session.

Never push the local branch `backup-before-scrub`. It holds the history from before
personal details were removed (2026-09-26).

## How we write (decided 2026-09-26, the owner)

Every doc in this repo speaks with one voice: **straightforward, simple and
built on facts.** Write it the way you'd explain it to a smart friend who
has never opened a terminal.

- **Short sentences, plain words.** "Can this step still be undone?" beats
  "split on commitment". If a term of art is needed (ESP, kickstart,
  `bcdedit`), use it, and say what it is the first time.
- **No em dashes.** Use a full stop, a comma, a colon or brackets instead.
- **Facts first.** Dates, versions, file names, row numbers and results stay
  exact. Simplifying the words never softens a claim, a refusal or a gap.
  "Decided (YYYY-MM-DD): ..." lines keep their date.
- **Metaphors where they help, not as decoration.** One good picture (the
  commit line, a safety net, a one-way door) is worth a paragraph. Don't
  stack them.
- **No emoji.** For status, use the ASCII bars:
  `[####]` real machine · `[###.]` rig · `[##..]` built, untried ·
  `[#...]` planned · `[....]` not started · `[FAIL]` failed for real.
  Diagrams are mermaid or plain ASCII in a `text` block.
- **Honest about failures.** A fail stays in the record, in plain words,
  next to what fixed it.

## When the design changes

Keep the three docs in agreement: `architecture.md` (how it works), `RISKS.md`
(what's unproven), `VALIDATION.md` (how we'll prove it). A design change that
touches one usually touches all three. A claim in one that contradicts another is a
bug. **Record decisions with a date** in the relevant risk or doc
("Decided (YYYY-MM-DD): ..."). When a design move changes a risk's stakes, update
that risk's severity and reasoning in the same change. New killers get a new `R##`
entry. Don't let a consequence hide in prose. History goes to `docs/log/`, status to
`docs/STATUS.md`, plans to `docs/ROADMAP.md`; a living doc keeps one "Where it
stands" line linking there.

## Environment

This runs on a Windows machine via WSL. **`powershell.exe` (Windows PowerShell 5.1)
is reachable from the shell.** Use it to parse-check and run the scanner and harness
against the real target engine (`wslpath -w <file>` translates paths). Two real test
machines so far. The ASUS ROG Zephyrus G16 (Ryzen AI 9 HX 370, RTX 4060, MediaTek
MT7925, Cirrus CS35L56) is the development machine the scanner was built on. The
Acer Aspire A515-51G (InsydeH2O, dying SATA SSD) has run the conversion itself: the
handoff, the live boot, the prologue's stops, the erase and install (runs 9, 10 and
11), the way back, and the install beside Windows (2026-10-06). Most `fail` paths
are still synthetic (R2), so treat one green run as one data point, not proof.
