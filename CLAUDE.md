# upgrade_ — working guide

Read this first, every session. It carries the mission and the rules that are
easy to violate without noticing. The design lives in `docs/architecture.md`,
the unknowns in `docs/RISKS.md`, and the plan for closing them in
`docs/VALIDATION.md` — but the *culture* below is what keeps this project
trustworthy, and it is not obvious from the code.

## The mission

Move an ordinary computer from Windows to Linux, in one go, for someone who
does not know how. Plug in a USB stick, pick a desktop, click convert — come
back to a working Linux machine with your files, Wi-Fi and browsers intact,
and (by default) the old system shrunk safely aside until they're sure.

The audience is non-technical people whose working Windows 10 machines were
stranded by Windows 11's hardware requirements and the October 2025 end of
security updates. The obstacle was never capability — those machines run Linux
fine — it's the knowledge required to get there. This project carries that
knowledge for them.

**Vision is source-agnostic; the implementation is Windows-only.** The docs and
README frame conversion as `Source → Linux`, but every line today reads a
Windows machine (PowerShell, `bcdedit`, BitLocker, `netsh`). Other source
systems are a future direction, not a v1 promise. Where the docs say "Windows",
they mean the one source that works now.

## The rules that are not negotiable

These are the project's spine. Breaking one quietly is how a tool like this
hurts someone.

1. **Refuse by default.** The only asset is that the report is trustworthy. A
   scanner that says "probably fine" and isn't is worse than no scanner,
   because the person acted on it and lost their data. There is **no override
   flag for a RED verdict.** When unsure between two severities, pick the
   more cautious one. If a check is wrong, fix the check — don't remove it.
   There will be pressure (often from contributors whose own machine works) to
   soften warnings. Resist it.
   **Amended (2026-09-13, RISKS R23):** one narrow exception exists, and it
   is not a flag. A person may type, verbatim, on a separate launcher, *"I
   confirm that I understand the risks and could lose data"*, and that lifts
   exactly two refusals — the drive-health and volume-health ones, whose
   failure mode is losing that machine's own files — after the scanner has
   said RED in full. It lifts nothing whose failure mode is "cannot work" or
   "wrong machine". The sentence travels in `job.json` and `outcome.json`,
   every screen after it says DATA LOSS ACCEPTED, and no shorter form of
   consent is accepted anywhere. Widening this exception is the thing rule
   #1 forbids.

2. **Evidence, not argument.** A risk closes only when a primary source or a
   real machine confirms it — never because the reasoning sounds right. This
   applies to our own claims too. `RISKS.md` states, for each unknown, what
   would actually happen if it's real and what evidence would close it.
   Nothing in it is closed by a good paragraph.

3. **The commit line.** Exactly one moment per conversion is irreversible, and
   the source OS stays bootable until it. The interface must say "you can still
   cancel" until that exact moment and stop the instant it's crossed. And
   **everything that can refuse must refuse before the line** — after it, the
   only safety left is a slow recovery that depends on hardware that might
   itself fail.

4. **Trust is spent once.** This tool earns trust once and loses it permanently
   the first time it destroys someone's photos. Any component that writes to a
   disk clears a far higher bar than one that reads. That's why the writers are
   built last and reviewed hardest.

5. **Spoof everything spoofable — and never confuse a spoof with evidence.**
   (Decided 2026-08-22.) Everything that *can* be validated without real
   hardware *must* be, at three levels: **logic** (detection functions fed
   fabricated objects — the `-SelfTest` cases), **recordings** (real machines'
   hardware enumerations captured with `-DumpMachine`, curated into
   `evaluate/windows/corpus/`, and replayed on every self-test run — a
   recording is ground truth for that machine, forever), and **simulated
   hardware** (VMs presenting spoofed devices, so the full Windows
   enumeration → WMI → scanner pipeline runs for hardware we don't own).
   Two rules keep this honest. First, **every contact with a real machine
   leaves a capture behind** — hardware reached once must stay testable
   forever. Second, **a spoofed pass closes plumbing, never a real-hardware
   clause**: a simulation is built from our model of the hardware, and the
   model is usually the thing in question (rule #2 in different clothes). Each
   risk names its unspoofable residue explicitly, and that residue still takes
   a real machine.

## Right now: validate the killers before building anything that writes

This is the current priority, above all feature work. Several things the whole
project depends on have **never been tested** — they exist as argument, which
by rule #2 counts for nothing. **No component that writes to a disk gets built
until the spine it depends on is proven on real hardware.** Full plan and
method in `docs/VALIDATION.md`; the killers, in order:

**Tier 1 — no product if these fail:**
- **V0 / R15 — the boot handoff fires.** Walk-away rests entirely on `bcdedit`
  `{fwbootmgr} bootsequence` booting the stick exactly once and failing safe to
  Windows otherwise. **First physical row fired 2026-09-08** — Acer Aspire
  A515-51G, Secure Boot **on**, signed payload, `fired-once`, no keypress, via
  the one-click `-Auto` flow (harness `upgrade_/windows/Test-Handoff.ps1`,
  stick built by `./make-kit.sh`). One vendor is not the matrix: **≥3 more
  vendors** (Dell, Lenovo, HP) still owed, plus the fail-safe rows on real
  firmware.
- **V1 — the unattended install completes, Secure Boot on.** Rig
  `pass-plumbing`; the physical live boot with Secure Boot on fired
  2026-09-12 (the Aspire); a physical install is still owed.
- **V1b / R21 — installing alongside a shrunk Windows leaves Windows bootable.** The
  default path keeps Windows as the safety net; if the alongside install breaks
  Windows boot (shared ESP too small, `bootmgfw.efi` clobbered, `os-prober`
  misses it) the safety net is a lie. Harder than the wipe install, and now the
  common case.

**Tier 2 — a core promise breaks (recoverable, but the default is broken):**
- **V4 / R18 — real disks can shrink enough.** Keep-Windows is the default and
  requires shrinkable space; if most disks can't free ~25 GB past immovable
  files, the default rarely applies. The scanner measures it (elevated only)
  by two independent read-only paths. **Decided (2026-09-08):** `evaluate`
  never repairs; the prologue clears NTFS's dirty flag - or a repair Windows
  has queued, the second trigger (2026-09-17) - as reversible prep, and
  branches on the fork the person chose in advance. **The Acer Aspire is the
  bad-conditions machine:** a dying SSD (SMART 187 = 725, hundreds of
  bad-block events; bad blocks are RED), kept by decision (2026-09-20), run
  under the acknowledged path (R23). **Where it stands (2026-09-26; R18 has
  the whole record, runs 1-7):**
  - The shrink ladder, all behind the typed CONVERT: hibernation and pagefile
    off with one restart, restored at every stop (0.5.1, **proven** run 6);
    restore points deleted with consent (`fork.restore_points_consented`; ran
    in run 6 and deleted 0 of 2 - 0.9.0 records Windows' answer and tries
    WMI, unfired); the change journal deleted with consent and created again
    (`fork.usn_journal_consented`, 0.8.0, **fired**, +1.1 GB).
  - Under `stop` a job is never a wipe (job writer 0.8.0, held in runs 6-7).
    A waiting Windows update is let finish, and nothing is armed while one
    waits (R25, prologue 0.9.0; markers read false in run 7 - detector
    unproven).
  - On the Aspire, runs 4-8 all stopped before anything irreversible; best
    9.5 of 25 GB; three runs named three different last unmovable files (the
    journal, System Restore's storage, `$Mft::$BITMAP`) because cold layouts
    drift with use. Never predict a shrink number - only a re-measure says
    (the "about 42 GB" of 2026-09-20 was wrong). **Keep-Windows is refused
    on this disk.**
  - **Designed (2026-09-26), not built:** the offer to discard Windows when
    it cannot be kept (fork *ask me then*; `architecture.md`, "When Windows
    cannot be kept"; R26, critical) - waits for the harvest and a first
    physical install.
  - The healthy-drive keep-Windows install (V1b's residue) needs another
    machine. R23 exists for the Aspire's owner; rule #1 above says how
    narrow.
- **V3 / R19 — the BITLK read in settle-in works.** How the default path
  delivers files: mount the kept Windows from installed Linux, unlock with the
  harvested key, copy. Bench-testable in VMs across BitLocker variants.
- **V2 — extracted amp firmware makes speakers work.** The "working hardware on
  first boot" promise. Testable on the G16 (it has the CS35L56).

**Tier 3 — silent data loss (the trust-ending class):**
- **V8 / R8 — OneDrive placeholders are materialized at `evaluate`.** The Linux-side
  pull has no OneDrive client, so a "free up space" stub not forced local
  beforehand copies over as 0 bytes. Must materialize, not just detect.
  **Built and plumbing-fired 2026-09-08:** harvester `-Materialize`
  (pin + read-through + three-fact verification, refuse on any failure);
  `Test-Materialize.ps1` is a real Cloud-Files-API provider — `pass-plumbing`
  on the rig and the G16. Residue: `-OneDrive` against a signed-in client.
  **Decided (2026-09-26, the owner's call): no download.** Online-only
  files are not copied at all - their bytes are in OneDrive, not on the
  disk. The job records them (`cloud_files.result = left-in-cloud`, job
  writer 0.11.0) and `settle-in` reconnects OneDrive instead; the danger
  that remains, and is `settle-in`'s to refuse, is copying a stub as if it
  were the file. `-Materialize` stays built, unused by the launchers.

**Tier 4 — kills adoption, not the mechanism:** V5 (VMD detection fires — an
afternoon, do it early; **2026-09-13:** the AHCI-side real row exists
(Aspire, `warn-rst-on-ahci` — iaStorAC on an AHCI-class controller, the R7
guard), the both-modes visit is one click — `RUN-STORAGE-MODE.cmd`, Safe Mode
through a copied boot entry booted once, resume as SYSTEM — and fired on the
rig and twice on the Aspire (2026-09-15: Safe Mode, marker, SYSTEM resume
all fired on real firmware; the setup screen was never reached — Acer's
firmware ignores boot-to-setup — so the RAID row is still owed; VMD proper
needs an 11th-gen+ machine, whose first scan is the FAIL row as shipped),
V6 (code-signing reputation — a calendar, start now),
V7 (scanner generalizes past the one test machine — ship it, collect reports).

Start V5 and V6 immediately (cheap / calendar-bound). V0+R21 are the spine
spike and block everything in `upgrade_/` and `settle-in/`.

**Vertical progress (2026-09-08):** schemas, V8 materialization, the R16
writer and the **live boot through the handoff** (`rig/hyperv/v1.sh`;
kickstart generator `upgrade_/windows/New-Kickstart.ps1`; `%pre` verifier
`upgrade_/linux/verify.sh`) all fired on the rig — `pass-plumbing` in
`docs/validation-results/v1-live-boot.csv`: one-shot entry → stick →
unmodified Fedora installer → identity + hardware verified → back to
Windows, nothing installed. **2026-09-09:** the stick carries Fedora's own
Workstation and KDE live squashfs (unmodified; `rig/vm/fetch-desktops.sh`),
the kickstart names the chosen one with its checksum, and `%pre` reads it
back byte-for-byte on the stick before anything is decided (R17's gate) —
row 3, `pass-plumbing`. **2026-09-10, the destructive half's first step:**
the converter's own kickstart installed Fedora KDE alongside the kept
Windows on the rig — `%pre` snapshots the ESP to the stick, `liveimg` from
the stick, `%post` runs the R21 boot-chain checklist and writes a
schema-valid `outcome.json` — `docs/validation-results/v2-install.csv`
row 3, `pass-plumbing` (`rig/hyperv/v2.sh`). **2026-09-12, first physical
live-boot row, Secure Boot on:** the Acer Aspire ran `RUN-VERIFY.cmd` —
scanner → `New-Job.ps1` (the job writer, first version) → kickstart →
handoff → installer through shim → identity by serial, display, **Wi-Fi
(28 networks)**, image read back at 22.6 MB/s → back to Windows
(`v1-live-boot.csv` row 4). Its C: then carried the dirty flag, so the
job was clean-slate (a verify-only run; nothing installed). **2026-09-12, the prologue as product code:**
`upgrade_/windows/Invoke-Prologue.ps1` + `RUN-CONVERT.cmd` — re-validate,
the R18 disk check (four guardrails, own restart, outcome recorded), the
two-path re-measure, the fork, the shrink, BitLocker suspension, the
handoff, a stopped `outcome.json` at every refusal; `outcome.sh` carries
its record into `outcome.json`. **Fired on the rig the same day**
(`docs/validation-results/r18-prologue.csv` row 4 `pass-plumbing`,
`v2-install.csv` row 4): injected flag → full boot-time check → 57.8 GB
by both paths → 25 GB freed → handoff → install → record carried into
`outcome.json`; and **rollback** (`Invoke-Rollback.ps1`, `ROLLBACK.cmd`;
`r21-rollback.csv` row 1 `pass-plumbing`). **2026-09-13, the walk-away
resume:** prologue 0.3.0 resumes as SYSTEM at startup (no sign-in; the
person's password is never taken — `architecture.md`, "the walk-away
resume"); `r18-prologue.csv` row 6 `pass-plumbing` with the rig's
autologon off, both resumes in session 0, 472 s from the check restart to
the first Linux boot with nobody at the keyboard; **and the first physical
row the same evening** — `RUN-PROBE.cmd` (read-only, one restart) on the
Aspire, Secure Boot on: SYSTEM in session 0, 38 s after boot, stick seen
5 s later (`walkaway-probe.csv` row 2). **2026-09-13, the Aspire's flag:** first physical step-1b row,
`stopped-volume-check` — the refusal path on a real machine, and the
drive diagnosis behind it (R18). Built the same day: the R18 guardrail
reads (scanner 0.2.0, prologue 0.2.0), the acknowledged-data-loss path
(R23; `RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd`), the software inventory in
`job.json` (`harvest.software`, private by placement), read-only drive
diagnostics on the kit (`DIAG-VOLUME.cmd`, `DIAG-SMART.cmd`), and two
physical R16 writes. **Next (2026-09-26):** the harvest into the job -
**folder map built the same day** (job writer 0.10.0, harvester 0.3.0:
`harvest.folders` + `harvest.stick_fit`, refusals for the wrong account,
unreadable folders, online-only files, and a clean slate that does not
fit the stick or has other profiles; unfired on a physical job); still
owed: the OneDrive download step in the launchers (consent text first),
staging moved to the exFAT partition, Wi-Fi, browsers; a physical keep-Windows
install on a machine with a healthy drive (not the Aspire); `settle-in`.

**Decided (2026-09-08): the build is a vertical, not a list.** One
front-to-back, one-click flow, reversible half first (schemas → OneDrive
materialization → stick writer → live image → hardware verify → back to
Windows, no commit line crossed), destructive half second. Trailblazed on
the rig, then on a machine we own and can image; a borrowed machine is only
ever a half-hour read-only visit that fills a column of the matrix. Full
statement in `docs/architecture.md`, "Build order". The user-facing
principle behind it: **a fully managed experience** — one click, one
consent, walk away.

## How the design works (one paragraph)

Three modules, split on **commitment**, not OS. `evaluate` (Windows, read-only)
scans, harvests what only Windows can give — the folder map, materialized cloud
files, firmware, the BitLocker key — captures intent, writes the stick, and
refuses. `upgrade_` (the converter; Windows → Linux) does it: by **default**
shrinks Windows aside and installs Linux alongside, keeping Windows as a
rollback; only on opt-in or a too-full disk does it wipe and stage files to the
stick instead. `settle-in` (Linux, first boot) verifies the hardware, **pulls
the user's files from the kept Windows partition** (default path), and offers
reclaim once everything is confirmed. No external drive anywhere — one stick is
the whole kit.

## Layout

```
data/            hardware + distro knowledge base — community PRs land here
  devices.ps1      Wi-Fi/GPU/audio/storage quirks by PCI ID
  distros.ps1      distro kernel table (goes stale; verify against release notes)
evaluate/windows/  scanner (upgrade-scan.ps1), harvester, V0 handoff harness
upgrade_/          the converter — windows/ prologue, rollback, kickstart, launchers; linux/ %pre verify + outcome
settle-in/         first-boot verify + file pull + reclaim (nothing built)
schemas/           job.json / outcome.json contracts (change rarely, review hard)
docs/              architecture.md, RISKS.md, VALIDATION.md, validation-results/
dist/              built single-file scanner (rebuild with ./build.sh)
```

## Working conventions

- **Windows PowerShell 5.1 only.** It's what ships on stock Windows 10/11.
  No PS7 syntax — no ternaries, no `??`, no `-Parallel`. If it needs a setup
  step, it doesn't run where it matters.
- **`data/*.ps1` is the contribution surface.** Adding a device is a one-line
  PR with a cited source ("it should work" is not a source). Keep it editable.
- **`./build.sh` inlines `data/` into `dist/upgrade-scan.ps1`.** Rebuild and
  commit `dist/` whenever `data/` or the scanner changes — nothing enforces
  this yet (R9), so it's on you.
- **Run both self-tests before any change lands:**
  `powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\upgrade-scan.ps1 -SelfTest`
  and the same for `.\Harvest-UpgradeState.ps1 -SelfTest`, from
  `evaluate/windows/`. Add a case if you change verdict, detection, parsing
  or arithmetic logic. Live-OS reads stay behind collect/judge seams so the
  judgment halves remain testable (rule #5).
- **Never commit a machine report** (`upgrade-report-*.txt/.json`) — they hold
  someone's hardware and account details. `.gitignore` covers them; keep it so.
- **Never commit the V0 EFI binaries** — build inputs, gitignored, fetched per
  `upgrade_/windows/handoff-payload/README.md`.
- **Licence is GPL-3.0.** The trust model is "read the source"; copyleft keeps
  forks readable. Don't reintroduce permissively-licensed files without a reason.

## When the design changes

Keep the three docs in agreement — `architecture.md` (how it works), `RISKS.md`
(what's unproven), `VALIDATION.md` (how we'll prove it). A design change that
touches one usually touches all three; a claim in one that contradicts another
is a bug. **Record decisions with a date** in the relevant risk or doc ("Decided
(YYYY-MM-DD): ..."), and when a design move changes a risk's stakes, update that
risk's severity and reasoning in the same change. New killers get a new `R##`
entry — don't let a consequence hide in prose.

## Environment

This runs on a Windows machine via WSL. **`powershell.exe` (Windows PowerShell
5.1) is reachable from the shell** — use it to parse-check and run the scanner
and harness against the real target engine (`wslpath -w <file>` to translate
paths). The single end-to-end test machine so far is an ASUS ROG Zephyrus G16
(Ryzen AI 9 HX 370, RTX 4060, MediaTek MT7925, Cirrus CS35L56) — every `fail`
path is otherwise synthetic (R2), so treat one green run as one data point, not
proof.
