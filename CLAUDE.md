# upgrade_: working guide

Read this first, every session. It carries the mission and the rules that
are easy to break without noticing. The design lives in
`docs/architecture.md`, the unknowns in `docs/RISKS.md`, and the plan for
closing them in `docs/VALIDATION.md`. But the *culture* below is what keeps
this project trustworthy, and you can't see it in the code.

## The mission

Move an ordinary computer from Windows to Linux, in one go, for someone who
does not know how. Plug in a USB stick, pick a desktop, click convert. Come
back to a working Linux machine with your files, Wi-Fi and browsers intact,
and (by default) the old system shrunk safely aside until they're sure.

The audience is non-technical people whose working Windows 10 machines were
stranded by Windows 11's hardware requirements and the October 2025 end of
security updates. The machines were never the obstacle: they run Linux
fine. The obstacle is the knowledge needed to get there. This project
carries that knowledge for them.

**The vision works from any source; the code is Windows-only.** The docs and
README describe conversion as `Source → Linux`, but every line today reads a
Windows machine (PowerShell, `bcdedit`, BitLocker, `netsh`). Other source
systems are a future direction, not a v1 promise. Where the docs say
"Windows", they mean the one source that works now.

## The rules that are not negotiable

These are the project's spine. Quietly breaking one is how a tool like this
hurts someone.

1. **Refuse by default.** The only asset is that the report can be trusted.
   A scanner that says "probably fine" and is wrong is worse than no
   scanner, because the person acted on it and lost their data. There is
   **no override flag for a RED verdict.** When unsure between two
   severities, pick the more cautious one. If a check is wrong, fix the
   check. Don't remove it. There will be pressure to soften warnings (often
   from contributors whose own machine works). Resist it.
   **Amended (2026-09-13, RISKS R23):** one narrow exception exists, and it
   is not a flag. A person may type, verbatim, on a separate launcher, *"I
   confirm that I understand the risks and could lose data"*. That lifts
   exactly two refusals, the drive-health and volume-health ones, whose
   failure mode is losing that machine's own files, and only after the
   scanner has said RED in full. It lifts nothing whose failure mode is
   "cannot work" or "wrong machine". The sentence travels in `job.json` and
   `outcome.json`, every screen after it says DATA LOSS ACCEPTED, and no
   shorter form of consent is accepted anywhere. Widening this exception is
   the thing rule #1 forbids.

2. **Evidence, not argument.** A risk closes only when a primary source or a
   real machine confirms it, never because the reasoning sounds right. This
   applies to our own claims too. `RISKS.md` states, for each unknown, what
   would actually happen if it's real and what evidence would close it.
   Nothing in it is closed by a good paragraph.

3. **The commit line.** Exactly one moment per conversion is irreversible
   (a one-way door), and the source OS stays bootable until it. The
   interface must say "you can still cancel" until that exact moment, and
   stop the instant it's crossed. And **everything that can refuse must
   refuse before the line.** After it, the only safety left is a slow
   recovery that depends on hardware that might itself fail.

4. **Trust is spent once.** This tool earns trust once and loses it for good
   the first time it destroys someone's photos. Any component that writes to
   a disk must clear a far higher bar than one that reads. That's why the
   writers are built last and reviewed hardest.

5. **Spoof everything spoofable, and never confuse a spoof with evidence.**
   (Decided 2026-08-22.) Everything that *can* be checked without real
   hardware *must* be, at three levels:
   - **logic:** detection functions fed made-up objects (the `-SelfTest`
     cases);
   - **recordings:** real machines' hardware lists captured with
     `-DumpMachine`, curated into `evaluate/windows/corpus/`, and replayed on
     every self-test run. A recording is ground truth for that machine,
     forever;
   - **simulated hardware:** VMs presenting spoofed devices, so the full
     Windows enumeration → WMI → scanner pipeline runs for hardware we don't
     own.

   Two rules keep this honest. First, **every contact with a real machine
   leaves a capture behind**: hardware reached once must stay testable
   forever. Second, **a spoofed pass closes plumbing, never a real-hardware
   clause.** A simulation is built from our model of the hardware, and the
   model is usually the thing in question (rule #2 in different clothes).
   Each risk names the part that can't be spoofed (its residue), and that
   residue still takes a real machine.

## Right now: validate the killers before building anything that writes

This is the current priority, above all feature work. Several things the
whole project depends on have **never been tested**. They exist only as
argument, which by rule #2 counts for nothing. **No component that writes
to a disk gets built until the spine it depends on is proven on real
hardware.** Full plan and method in `docs/VALIDATION.md`. The killers, in
order:

**Decided (2026-09-26, the owner's call): the next destructive target is
V9 / R27, the one-click erase and install.** Erase every internal drive,
install Fedora, keep nothing. It has its own launcher and typed sentence,
and a 2-minute cancellable countdown in the installer as the commit line.
The system goes on the C: drive and `/home` on a second internal drive.
Carrying files across is stage 2. **Built and passed on the rig
2026-09-26** (`v9-erase.csv`: refuse, cancel, erase. The first cancel froze
and was fixed in `verify.sh` 0.4.1; one erase ran over an Ubuntu-style
LVM). **The Aspire's run 9 (same night) erased and installed with nobody at
the keyboard, but booted to a TEXT login. That is a one-click failure**
(the kickstart lacked a graphical login; fixed in 0.3.0, and the rig
re-proven with the new `graphical_login` check). The person now picks KDE,
GNOME or the text console on the launcher (all three held on the rig), and
is told the sign-in name on screen. Physical row written (`v9-erase.csv`
line 11, fail). Findings owed: the installer's clock 4 h off (RTC in local
time), a stale Windows firmware entry, a raw traceback on a `%pre` refusal.
**Decided (2026-09-26, the owner):** the clock and Wi-Fi passwords are
harvested on Windows and applied by `settle-in` on first startup; the old
firmware entry is a button at the end of `settle-in` (`architecture.md`,
"The clock, Wi-Fi and the old boot entry"). Not built.
Next: the Aspire's physical row recorded, then a re-run that ends at the
desktop, and a plain-words screen for a `%pre` refusal (today it's
Anaconda's traceback). Design in `architecture.md`, "Erase and install".
**Decided (2026-09-27, the owner): there is always a way back to
Windows,** a new and empty one, and it says its cost first (most of these
machines can only go back to Windows 10). Its own "Go back to Windows"
program on the Linux side: a guided stick first, a walk-away reinstall
later (an erase, so rule #4). `evaluate` harvests the edition and how
Windows was activated, never a key (R30, V11; `architecture.md`, "The way
back to Windows"). The Aspire's follow-up run starts from a Windows the
owner reinstalls by hand: the first activation data point.

**Tier 1: no product if these fail.**
- **V0 / R15: the boot handoff fires.** Walk-away rests entirely on
  `bcdedit` `{fwbootmgr} bootsequence` booting the stick exactly once, and
  falling back safely to Windows otherwise. **First physical row fired
  2026-09-08:** Acer Aspire A515-51G, Secure Boot **on**, signed payload,
  `fired-once`, no keypress, via the one-click `-Auto` flow (harness
  `upgrade_/windows/Test-Handoff.ps1`, stick built by `./make-kit.sh`). One
  vendor is not the matrix: **≥3 more vendors** (Dell, Lenovo, HP) are
  still owed, plus the fail-safe rows on real firmware.
- **V1: the unattended install completes, Secure Boot on.** Rig
  `pass-plumbing`; the physical live boot with Secure Boot on fired
  2026-09-12 (the Aspire); a physical install is still owed.
- **V1b / R21: installing alongside a shrunk Windows leaves Windows
  bootable.** The default path keeps Windows as the safety net. If the
  alongside install breaks Windows boot (shared ESP too small,
  `bootmgfw.efi` overwritten, `os-prober` misses it), the safety net is a
  lie. Harder than the wipe install, and now the common case.

**Tier 2: a core promise breaks (recoverable, but the default is broken).**
- **V4 / R18: real disks can shrink enough.** Keep-Windows is the default
  and needs space that can be shrunk. If most disks can't free ~25 GB past
  files that can't be moved, the default rarely applies. The scanner
  measures it (elevated only) by two independent read-only paths.
  **Decided (2026-09-08):** `evaluate` never repairs. The prologue clears
  NTFS's dirty flag (or a repair Windows has queued, the second trigger,
  2026-09-17) as reversible prep, and branches on the fork the person chose
  in advance. **The Acer Aspire is the bad-conditions machine:** a dying SSD
  (SMART 187 = 725, hundreds of bad-block events; bad blocks are RED), kept
  by decision (2026-09-20), run under the acknowledged path (R23). **Where
  it stands (2026-09-26; R18 has the whole record, runs 1-7):**
  - The shrink ladder, all behind the typed CONVERT: hibernation and
    pagefile off with one restart, restored at every stop (0.5.1,
    **proven** run 6); restore points deleted with consent
    (`fork.restore_points_consented`; ran in run 6 and deleted 0 of 2.
    0.9.0 records Windows' answer and tries WMI, unfired); the change
    journal deleted with consent and created again
    (`fork.usn_journal_consented`, 0.8.0, **fired**, +1.1 GB).
  - Under `stop` a job is never a wipe (job writer 0.8.0, held in runs
    6-7). A waiting Windows update is allowed to finish, and nothing is
    armed while one waits (R25, prologue 0.9.0; markers read false in run
    7, so the detector is unproven).
  - On the Aspire, runs 4-8 all stopped before anything irreversible. Best
    was 9.5 of 25 GB. Three runs named three different last unmovable
    files (the journal, System Restore's storage, `$Mft::$BITMAP`),
    because the disk's layout shifts as the machine gets used. Never
    predict a shrink number: only a re-measure says (the "about 42 GB" of
    2026-09-20 was wrong). **Keep-Windows is refused on this disk.**
  - **Designed (2026-09-26), not built:** the offer to discard Windows
    when it cannot be kept (fork *ask me then*; `architecture.md`, "When
    Windows cannot be kept"; R26, critical). It waits for the harvest and
    a first physical install.
  - The healthy-drive keep-Windows install (V1b's residue) needs another
    machine. R23 exists for the Aspire's owner; rule #1 above says how
    narrow it is.
- **V3 / R19: the BITLK read in settle-in works.** This is how the default
  path delivers files: mount the kept Windows from the installed Linux,
  unlock it with the harvested key, copy. Bench-testable in VMs across
  BitLocker variants.
- **V2: extracted amp firmware makes the speakers work.** The "working
  hardware on first boot" promise. Testable on the G16 (it has the
  CS35L56).

**Tier 3: silent data loss (the class that ends trust).**
- **V8 / R8: OneDrive placeholders are materialized at `evaluate`.** The
  Linux-side pull has no OneDrive client, so a "free up space" stub not
  forced local beforehand copies over as 0 bytes. It must materialize
  (download for real), not just detect. **Built and plumbing-fired
  2026-09-08:** harvester `-Materialize` (pin + read-through + three-fact
  verification, refuse on any failure); `Test-Materialize.ps1` is a real
  Cloud-Files-API provider, `pass-plumbing` on the rig and the G16.
  Residue: `-OneDrive` against a signed-in client.
  **Decided (2026-09-26, the owner's call): no download.** Online-only
  files are not copied at all: their bytes are in OneDrive, not on the
  disk. The job records them (`cloud_files.result = left-in-cloud`, job
  writer 0.11.0) and `settle-in` reconnects OneDrive instead. The danger
  that remains, and that `settle-in` must refuse, is copying a stub as if
  it were the file. `-Materialize` stays built, unused by the launchers.

**Tier 4: kills adoption, not the mechanism.**
- **V5** (VMD detection fires; an afternoon, do it early). **2026-09-13:**
  the AHCI-side real row exists (Aspire, `warn-rst-on-ahci`: iaStorAC on an
  AHCI-class controller, the R7 guard). The both-modes visit is one click:
  `RUN-STORAGE-MODE.cmd`, Safe Mode through a copied boot entry booted
  once, then resume as SYSTEM. It fired on the rig and twice on the Aspire
  (2026-09-15: Safe Mode, marker and SYSTEM resume all fired on real
  firmware; the setup screen was never reached, because Acer's firmware
  ignores boot-to-setup, so the RAID row is still owed). VMD proper needs
  an 11th-gen+ machine, whose first scan is the FAIL row as shipped.
- **V6** (code-signing reputation: a calendar, start now).
- **V7** (the scanner generalizes past the one test machine: ship it,
  collect reports).

Start V5 and V6 immediately (cheap / calendar-bound). V0+R21 are the spine
spike and block everything in `upgrade_/` and `settle-in/`.

**Vertical progress (2026-09-08):** schemas, V8 materialization, the R16
writer and the **live boot through the handoff** (`rig/hyperv/v1.sh`;
kickstart generator `upgrade_/windows/New-Kickstart.ps1`; `%pre` verifier
`upgrade_/linux/verify.sh`) all fired on the rig, `pass-plumbing` in
`docs/validation-results/v1-live-boot.csv`: one-shot entry → stick →
unmodified Fedora installer → identity + hardware verified → back to
Windows, nothing installed.

- **2026-09-09:** the stick carries Fedora's own Workstation and KDE live
  squashfs (unmodified; `rig/vm/fetch-desktops.sh`). The kickstart names
  the chosen one with its checksum, and `%pre` reads it back byte for byte
  on the stick before anything is decided (R17's gate). Row 3,
  `pass-plumbing`.
- **2026-09-10, the destructive half's first step:** the converter's own
  kickstart installed Fedora KDE alongside the kept Windows on the rig.
  `%pre` snapshots the ESP to the stick, `liveimg` installs from the
  stick, `%post` runs the R21 boot-chain checklist and writes a
  schema-valid `outcome.json`. `docs/validation-results/v2-install.csv`
  row 3, `pass-plumbing` (`rig/hyperv/v2.sh`).
- **2026-09-12, first physical live-boot row, Secure Boot on:** the Acer
  Aspire ran `RUN-VERIFY.cmd`: scanner → `New-Job.ps1` (the job writer,
  first version) → kickstart → handoff → installer through shim → identity
  by serial, display, **Wi-Fi (28 networks)**, image read back at
  22.6 MB/s → back to Windows (`v1-live-boot.csv` row 4). Its C: then
  carried the dirty flag, so the job was clean-slate (a verify-only run;
  nothing installed).
- **2026-09-12, the prologue as product code:**
  `upgrade_/windows/Invoke-Prologue.ps1` + `RUN-CONVERT.cmd`. Re-validate,
  the R18 disk check (four guardrails, its own restart, outcome recorded),
  the two-path re-measure, the fork, the shrink, BitLocker suspension, the
  handoff, and a stopped `outcome.json` at every refusal. `outcome.sh`
  carries its record into `outcome.json`. **Fired on the rig the same
  day** (`docs/validation-results/r18-prologue.csv` row 4 `pass-plumbing`,
  `v2-install.csv` row 4): injected flag → full boot-time check → 57.8 GB
  by both paths → 25 GB freed → handoff → install → record carried into
  `outcome.json`. And **rollback** (`Invoke-Rollback.ps1`, `ROLLBACK.cmd`;
  `r21-rollback.csv` row 1 `pass-plumbing`).
- **2026-09-13, the walk-away resume:** prologue 0.3.0 resumes as SYSTEM
  at startup (no sign-in; the person's password is never taken.
  `architecture.md`, "the walk-away resume"). `r18-prologue.csv` row 6
  `pass-plumbing` with the rig's autologon off, both resumes in session 0,
  472 s from the check restart to the first Linux boot with nobody at the
  keyboard. **And the first physical row the same evening:**
  `RUN-PROBE.cmd` (read-only, one restart) on the Aspire, Secure Boot on:
  SYSTEM in session 0, 38 s after boot, stick seen 5 s later
  (`walkaway-probe.csv` row 2).
- **2026-09-13, the Aspire's flag:** first physical step-1b row,
  `stopped-volume-check`: the refusal path on a real machine, and the
  drive diagnosis behind it (R18). Built the same day: the R18 guardrail
  reads (scanner 0.2.0, prologue 0.2.0), the acknowledged-data-loss path
  (R23; `RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd`), the software inventory in
  `job.json` (`harvest.software`, private by placement), read-only drive
  diagnostics on the kit (`DIAG-VOLUME.cmd`, `DIAG-SMART.cmd`), and two
  physical R16 writes.
- **Next (2026-09-26):** the harvest into the job. **Folder map built the
  same day** (job writer 0.10.0, harvester 0.3.0: `harvest.folders` +
  `harvest.stick_fit`, refusals for the wrong account, unreadable folders,
  online-only files, and a clean slate that does not fit the stick or has
  other profiles; unfired on a physical job; since 0.11.0 online-only
  files are recorded as left in the cloud, not refused). Still owed: staging moved to
  the exFAT partition, Wi-Fi, browsers; a physical keep-Windows install on
  a machine with a healthy drive (not the Aspire); `settle-in`.

**Decided (2026-09-08): the build is a vertical, not a list.** One
front-to-back, one-click flow. Reversible half first (schemas → OneDrive
materialization → stick writer → live image → hardware verify → back to
Windows, no commit line crossed), destructive half second. Trailblazed on
the rig, then on a machine we own and can image. A borrowed machine is only
ever a half-hour read-only visit that fills one column of the matrix. Full
statement in `docs/architecture.md`, "Build order". The user-facing
principle behind it: **a fully managed experience.** One click, one
consent, walk away.

## How the design works (one paragraph)

Three modules, split on **commitment** (can this step still be undone?),
not on OS. `evaluate` (Windows, read-only) scans, harvests what only
Windows can give (the folder map, materialized cloud files, firmware, the
BitLocker key), captures intent, writes the stick, and refuses. `upgrade_`
(the converter; Windows → Linux) does the move. By **default** it shrinks
Windows aside and installs Linux alongside, keeping Windows as a rollback.
Only on opt-in or a too-full disk does it wipe and stage files to the stick
instead. `settle-in` (Linux, first boot) verifies the hardware, **pulls the
user's files from the kept Windows partition** (default path), and offers
reclaim once everything is confirmed. No external drive anywhere: one stick
is the whole kit.

## Layout

```
data/            hardware + distro knowledge base; community PRs land here
  devices.ps1      Wi-Fi/GPU/audio/storage quirks by PCI ID
  distros.ps1      distro kernel table (goes stale; verify against release notes)
evaluate/windows/  scanner (upgrade-scan.ps1), harvester, job writer, stick writer
upgrade_/          the converter: windows/ prologue, rollback, kickstart, launchers, the window (UPGRADE.exe, Rust), V0 handoff harness; linux/ %pre verify + outcome
settle-in/         first-boot verify + file pull + reclaim (nothing built)
schemas/           job.json / outcome.json contracts (change rarely, review hard)
docs/              architecture.md, RISKS.md, VALIDATION.md, validation-results/
dist/              built single-file scanner (rebuild with ./build.sh)
```

## Working conventions

- **Windows PowerShell 5.1 only.** It's what ships on stock Windows 10/11.
  No PS7 syntax: no ternaries, no `??`, no `-Parallel`. If it needs a
  setup step, it doesn't run where it matters.
  **Decided (2026-09-27, the owner):** the Windows side is being ported
  to Rust, one piece at a time (RISKS R32, VALIDATION V13). A script
  keeps these rules, and stays in use, until every line of its evidence
  in `docs/validation-results/port-parity.csv` reads `pass` in Rust.
- **`data/*.ps1` is the contribution surface.** Adding a device is a
  one-line PR with a cited source ("it should work" is not a source). Keep
  it editable.
- **`./build.sh` inlines `data/` into `dist/upgrade-scan.ps1`.** Rebuild
  and commit `dist/` whenever `data/` or the scanner changes. Nothing
  enforces this yet (R9), so it's on you.
- **Run both self-tests before any change lands:**
  `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\upgrade-scan.ps1 -SelfTest`
  and the same for `.\Harvest-UpgradeState.ps1 -SelfTest`, from
  `evaluate/windows/`. Add a case if you change verdict, detection, parsing
  or arithmetic logic. Live-OS reads stay behind collect/judge seams so the
  judging halves stay testable (rule #5).
- **Never commit a machine report** (`upgrade-report-*.txt/.json`). They
  hold someone's hardware and account details. `.gitignore` covers them;
  keep it that way.
- **Never commit the V0 EFI binaries.** They are build inputs, gitignored,
  fetched as described in `upgrade_/windows/handoff-payload/README.md`.
- **Licence is GPL-3.0.** The trust model is "read the source"; copyleft
  keeps forks readable. Don't reintroduce permissively-licensed files
  without a reason.

## Working alongside other sessions (decided 2026-09-28, the owner)

Several Claude sessions often work in this repo at once. On 2026-09-27
that went wrong twice: an accidental reset of the shared folder lost
`rig/hyperv/v12.sh` and a window fix before they were committed, and
sessions pushing only their own commits left the local `main` and
GitHub's `main` 20 and 9 commits apart. Three rules keep it from
happening again:

- **Every session works in its own worktree** (a second checkout of the
  repo on its own branch), under `.claude/worktrees/<name>`. The shared
  folder stays on a clean `main` that nobody edits directly. Git ignores
  `.claude/worktrees/` through `.git/info/exclude`.
- **Work reaches `main` only through a pull request.** GitHub enforces
  it: `main` refuses direct pushes, force pushes and deletion. Push the
  branch, open the pull request, and the owner merges it. After a merge,
  bring the shared folder forward with `git merge --ff-only origin/main`,
  never a reset.
- **Remove a worktree only after its work is merged and its session is
  closed.** Removing it under a live session pulls the folder out from
  under that session.

Never push the local branch `backup-before-scrub`. It holds the history
from before personal details were removed (2026-09-26).

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

Keep the three docs in agreement: `architecture.md` (how it works),
`RISKS.md` (what's unproven), `VALIDATION.md` (how we'll prove it). A design
change that touches one usually touches all three. A claim in one that
contradicts another is a bug. **Record decisions with a date** in the
relevant risk or doc ("Decided (YYYY-MM-DD): ..."). When a design move
changes a risk's stakes, update that risk's severity and reasoning in the
same change. New killers get a new `R##` entry. Don't let a consequence
hide in prose.

## Environment

This runs on a Windows machine via WSL. **`powershell.exe` (Windows
PowerShell 5.1) is reachable from the shell.** Use it to parse-check and
run the scanner and harness against the real target engine
(`wslpath -w <file>` translates paths). There are two real test machines
so far. The ASUS ROG Zephyrus G16 (Ryzen AI 9 HX 370, RTX 4060, MediaTek
MT7925, Cirrus CS35L56) is the development machine the scanner was built
on. The Acer Aspire A515-51G (InsydeH2O, dying SATA SSD) is the one that
has run the conversion itself: the handoff, the live boot, the prologue's
stops and the erase and install (run 9). Most `fail` paths are still
synthetic (R2), so treat one green run as one data point, not proof.
