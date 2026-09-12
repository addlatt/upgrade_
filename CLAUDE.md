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
   flag for a RED verdict, ever.** When unsure between two severities, pick the
   more cautious one. If a check is wrong, fix the check — don't remove it.
   There will be pressure (often from contributors whose own machine works) to
   soften warnings. Resist it.

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
- **V1b / R21 — installing alongside a shrunk Windows leaves Windows bootable.** The
  default path keeps Windows as the safety net; if the alongside install breaks
  Windows boot (shared ESP too small, `bootmgfw.efi` clobbered, `os-prober`
  misses it) the safety net is a lie. Harder than the wipe install, and now the
  common case.

**Tier 2 — a core promise breaks (recoverable, but the default is broken):**
- **V4 / R18 — real disks can shrink enough.** Keep-Windows is the default and
  requires shrinkable space; if most disks can't free ~20 GB past immovable
  files, the default rarely applies. Scanner measures it (elevated only) by two
  independent read-only paths. **First physical machine (2026-09-08) could not
  be measured at all: its C: carried NTFS's dirty flag**, which Windows refuses
  to shrink past — the scanner's new `Volume health` check now names that
  directly. **Decided (2026-09-08):** `evaluate` never repairs; the `upgrade_`
  prologue clears the flag as reversible prep (scan → refuse if the disk is
  not Healthy → spot-fix or `chkdsk /f` → restart → re-measure → the fork the
  user pre-chose). Owed code; RISKS R18 has the guardrails. The first
  guardrail's read landed 2026-09-08: the scanner's `Disk health` line
  (`Get-PhysicalDisk HealthStatus`; Unhealthy is RED).
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

**Tier 4 — kills adoption, not the mechanism:** V5 (VMD detection fires — an
afternoon, do it early), V6 (code-signing reputation — a calendar, start now),
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
(`v1-live-boot.csv` row 4). Its C: still carries the dirty flag, so the
job forced clean-slate. **2026-09-12, the prologue as product code:**
`upgrade_/windows/Invoke-Prologue.ps1` + `RUN-CONVERT.cmd` — re-validate,
the R18 disk check (four guardrails, own restart, outcome recorded), the
two-path re-measure, the fork, the shrink, BitLocker suspension, the
handoff, a stopped `outcome.json` at every refusal; `outcome.sh` carries
its record into `outcome.json`. Rig bench `rig/hyperv/prologue.sh`, rows
in `docs/validation-results/r18-prologue.csv`. Next: rollback from the
snapshot, the Aspire's real flag.

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
upgrade_/          the converter — windows/ prologue, linux/ cutover (nothing built)
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
