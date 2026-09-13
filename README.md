# upgrade_

** Convert your machine to Linux**

Plug in a USB stick, pick a desktop, click convert. Come back to a working
Linux machine with your files, Wi-Fi and browsers intact — and, by default,
your old system shrunk safely aside until you're sure. One stick is the whole
kit.

> **Status (2026-09-13): the whole conversion exists as code and has run
> end to end on the rig; one physical machine has run it as far as its
> failing drive allowed.** Scan → job → kickstart → the prologue (disk
> check, shrink, boot handoff) → Fedora installed beside Windows → rollback,
> each step written by the product's own scripts and each one leaving an
> evidence row. It is not a release: the physical vendor matrix is one
> machine wide, `settle-in` (the first boot on Linux) is not built, and the
> converter is unsigned. See [Status](#status).

---

## Why

Your os should not control your hardware.
Anybody should be able to upgrade their machine without friction.

## How it works

Three modules. 

| | | |
|---|---|---
| **`evaluate`** | Source · reversible | Reads the machine, captures your choices, extracts everything that depends on the source system for its existence, and refuses anything it can't do safely. |
| **`upgrade_`** | Source → Linux | The converter. By default shrinks the source aside and installs Linux next to it; wipes and stages files to the stick only if you ask, or if the disk is too full to keep both. |
| **`settle-in`** | Linux | Verifies the hardware works, brings your files home from the kept partition, hands over, stops. |

**Today the source is Windows.** The framing is deliberately source-agnostic —
the *shape* (read, commit, settle) has nothing Windows-specific about it — but
every line of the current implementation reads a Windows machine: PowerShell,
`bcdedit`, BitLocker, `netsh`. Other source systems are a future direction,
not a v1 promise. Where this README and the design docs say "Windows", they
mean the one source that works now.

### The commit line

Exactly one moment in a conversion is irreversible, and the source OS stays
bootable until it. By default the converter **keeps the old system**: it
shrinks that partition aside, installs Linux next to it, brings your files
across on first boot, and only then — after you have confirmed everything works
— offers to **reclaim** the space. That reclaim is the irreversible moment, and
it is your explicit choice. Until it, the old system is a full rollback a
boot-menu away.

Only if you ask to wipe the old system outright — or if the disk is too full to
keep both — does the irreversible moment become a **disk wipe** instead, and
that path is guarded by a two-minute hardware check with you present, because
it has no rollback. Everything before either line is additive; a failure
earlier simply boots the old system again.

Two rules follow. The interface says *"you can still cancel"* until that exact
moment and stops the instant it's crossed. And everything capable of refusing
must refuse **before** the line, because afterwards the only remaining safety
mechanism is a slow restore from a drive that might itself fail.

Full design in [docs/architecture.md](docs/architecture.md).

---

## What works today

**The vertical, on the rig and once on a real machine.** One kit stick
(`./make-kit.sh`, written by the R16 stick writer) carries everything.
`RUN-CONVERT.cmd` on it runs the scanner, writes `job.json`, generates the
kickstart, asks for one typed word, and hands over to the prologue
(`Invoke-Prologue.ps1`): it re-validates the job against the live machine,
runs Windows' own disk check with its own restart if the volume is
flagged, re-measures the room by two read-only paths, takes the fork the
person chose in advance, shrinks C:, suspends BitLocker for one restart
and arms the one-time boot handoff. The stick then boots Fedora's
installer (Secure Boot on, signed shim), which verifies the machine's
identity, reads the desktop image back byte for byte, snapshots the boot
files, installs alongside Windows into the freed space, checks the boot
chain and writes `outcome.json`. `ROLLBACK.cmd` puts Windows first again
from that snapshot. Every step writes a row under
[docs/validation-results/](docs/validation-results/), by a harness, never
by hand — including the runs that failed.

**Two guardrails learned from the first physical machine (2026-09-13).**
Its SSD said `Healthy` while Windows had logged 261 bad blocks on it and
the drive itself reported 725 uncorrectable reads; the scan cmdlet said
"no errors" while its own log said "found problems". The scanner now
reads the disk error log, the SMART counters, the volume's own status and
Windows' check log, and a drive like that is RED. The prologue refused it
before any of that was read, for a weaker reason — the refusal path is the
most-tested part of the tool.

### The scanner: `evaluate`

A read-only scanner. It tells you whether this specific machine can move to
Linux, what will break, which distribution to use, and what to do first. It
changes nothing, encrypts nothing, deletes nothing, and sends nothing anywhere.

The scanner is deliberately two tools in one codebase: run standalone, it is a
general advisory tool that recommends across distributions; run inside the
converter, it becomes `evaluate` — the converter's own gates, its capacity
checks, and (in v1) Fedora-only messaging. Same checks, one flag.

The valuable output is not a yes/no. It's **the kernel version your hardware
needs**, and which popular distributions fail to meet it.

That distinction matters more than anything else here. A first-time user is
almost always pointed at Linux Mint or Ubuntu LTS. On a 2024-or-newer laptop
those ship a kernel too old for the Wi-Fi card, and the user boots into a system
with no wireless, concludes Linux is broken, and reinstalls Windows. Nothing was
broken. They picked a release from before their laptop existed.

| Check | Why it's there |
|---|---|
| **Intel RST / VMD** | The SSD is invisible to every Linux installer. Looks like a broken installer, is actually a BIOS setting. The single most common false "Linux won't install". |
| **Wi-Fi chipset** | Broadcom cards need a driver you can't download without a network connection you don't have yet. Recent MediaTek cards need kernel 6.7+. |
| **Graphics** | New AMD APUs need a matching recent kernel or you get a black screen. NVIDIA needs a distro that installs the proprietary driver for you. |
| **Smart audio amps** | Cirrus and TI amps mean headphones work and the internal speakers are silent. Extremely common on 2023+ laptops. |
| **BitLocker** | Resize an encrypted disk without the recovery key and the data is gone permanently. |
| **Fast Startup** | Windows hibernates instead of shutting down, leaving the partition unsafe to resize. Shutting down does not clear it. |
| **Free space & staging size** | Which conversion path fits this machine: files-on-the-stick, or shrink-Windows-aside. (The shipped scanner still uses dual-boot-era wording here — being reworked, see RISKS R4.) |
| **Installed software** | Adobe, Office, CAD, kernel anti-cheat games. The honest answer is sometimes "don't convert this machine." |

Unrecognised devices are listed at the end of the report so they can be
contributed back.

### Running it

Download `dist/upgrade-scan.ps1`, then in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File .\upgrade-scan.ps1
```

The report prints to the screen and saves to your Desktop.

```powershell
-Json          # also write machine-readable JSON
-NoFile        # print only, save nothing
-OutDir <path> # save somewhere other than the Desktop
-SelfTest      # run the built-in logic tests
```

**Run it as Administrator if you can.** Without elevation, Windows refuses to
report BitLocker status, and the scanner cannot tell you whether your disk is
encrypted — the one unknown that can cost you everything. It still runs fine
unelevated; it just caps its verdict and says so.

---

## First principle

**Be honest about machines we can't do safely, and refuse them.**

There is no override flag for a RED verdict. One narrow, dated exception
exists (2026-09-13, [RISKS R23](docs/RISKS.md)): a person who has already
copied their files off a machine refused for its *drive* may type a fixed
sentence on a separate launcher; it lifts those two refusals and no other,
and everything it writes afterwards says DATA LOSS ACCEPTED. Widening it is
what the rule forbids. The converter runs from a single USB stick and refuses machines it cannot fit
— files too large for the stick and too little space to shrink Windows aside —
but the refusal is a **gap report**, not a door slam: exactly how many GB to
free or what stick size would change the answer. It will refuse machines with
Intel RST/VMD active, because that setting cannot be changed safely from
software on every vendor's firmware.

This still turns away real users, and that is the correct trade. A tool like this
earns trust once and spends it permanently the first time it destroys someone's
photos. Any component that writes to a disk has to clear a far higher bar than
the scanner does — which is why they were written last, behind the evidence,
and why each refusal is tested harder than each success.

---

## Status

| Component | State |
|---|---|
| `evaluate` — scanner (checks, verdict, distro recommendation) | **works**, tested on real hardware |
| `evaluate` — state harvester (locale, folders, Wi-Fi, browsers, capacity) | **works**, read-only |
| `evaluate` — OneDrive placeholder materialization (V8) | **works**, plumbing-fired against a real Cloud Files provider |
| `evaluate` — job writer (`job.json`, the software inventory) | **works**; harvest of folders/Wi-Fi/browsers into the job, the BitLocker key and the intent UI still owed |
| `evaluate` — stick writer (R16) | **works**, two physical writes verified |
| `upgrade_` — prologue: re-validate, disk check, shrink, BitLocker, boot handoff | **works** on the rig (`r18-prologue.csv`); one physical row, a refusal |
| `upgrade_` — cutover: identity, image read-back, ESP snapshot, install alongside, boot-chain check, `outcome.json` | **works** on the rig (`v2-install.csv`), Secure Boot off there; the physical Secure-Boot-on install is owed |
| `upgrade_` — rollback (Windows side) | **works** on the rig (`r21-rollback.csv`) |
| `upgrade_` — clean-slate path (wipe) | stops before the wipe on purpose: its human gate is not built |
| `settle-in` — hardware verify, file pull, reclaim, software matching | designed; the BITLK read is bench-proven (`v3-bitlk-read.csv`) |
| Physical vendor matrix | one machine (Acer, InsydeH2O); Dell, Lenovo, HP owed |
| Code signing | not started; unsigned binaries look like malware to Defender |

Known unknowns are tracked openly in [docs/RISKS.md](docs/RISKS.md) — what is
unverified, what would happen if each risk is real, and what evidence would
close it. The ordered plan for closing the load-bearing ones — what must be
proven before anything else gets built — is
[docs/VALIDATION.md](docs/VALIDATION.md). Read both before trusting any single
check. Notably: **VMD detection has
never fired on real hardware**, and it guards the most consequential case.

Also on the list, and not a code problem: the finished converter will need a
code-signing certificate. Behaviourally it elevates, reads recovery keys,
writes raw USB devices, resizes partitions and rewrites boot configuration —
indistinguishable from malware to Defender. Unsigned, it gets quarantined and
the people it's built for stop there.

---

## Development

```
data/                   knowledge base — most contributions land here
  devices.ps1             hardware: Wi-Fi, GPU, audio, storage quirks
  distros.ps1             distribution kernel table (goes stale — refresh it)
schemas/                contracts between modules (change rarely, review hard)

evaluate/               module 1 — read the machine, capture intent, refuse
  windows/                scanner + state harvester
upgrade_/               module 2, "the converter" — does the conversion
  windows/                prologue: stage to stick or shrink aside, boot handoff
  linux/                  cutover: install, inject, (clean-slate) restore
settle-in/              module 3 — verify hardware, pull files home, hand over
  linux/

build.sh                inlines data/ into dist/upgrade-scan.ps1
dist/upgrade-scan.ps1   the built single file people download
docs/architecture.md    how the three modules fit together
docs/RISKS.md           what is unverified and what would close it
```

Run from source with `evaluate/windows/upgrade-scan.ps1` (it loads `data/` from
disk), or build the standalone single file:

```bash
./build.sh
```

Targets Windows PowerShell 5.1, which ships on every Windows 10 and 11 install.
No PowerShell 7 syntax — no ternaries, no null-coalescing. If it doesn't run on
a stock machine, it doesn't run where it matters.

Before opening a PR:

```powershell
.\evaluate\windows\upgrade-scan.ps1 -SelfTest
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Adding a device is a one-line change to
a table in `data/`. That's the point — the hardware database is the part that
only gets good with many people, and it's the part that makes every future
report better.

The most useful thing you can contribute right now is **a report from a machine
that isn't mine.** Everything here has been verified against a single laptop;
every refusal path is currently tested only synthetically.

## Roadmap

Done, with rows: the spine spike, the schemas, the kickstart generator,
the live image on the stick with byte-for-byte read-back, the stick
writer, the prologue (disk check, shrink, handoff), the alongside
install with the boot-chain checklist, rollback. Next, in order:

1. **A physical keep-Windows install, Secure Boot on** — on a machine with
   a healthy drive (the first candidate's SSD is failing). This is the
   V1b residue and the row the whole default path waits for.
2. **`settle-in`** — hardware verify on first boot, the file pull from the
   kept Windows partition (BITLK unlock, copy, checksum), the "you had
   these programs" list with Linux equivalents from the software
   inventory, the Linux-side rollback, and reclaim.
3. **The harvest into `job.json`** — folders, Wi-Fi profiles, browser
   profiles, the BitLocker key, and the intent-capture screen that asks
   the person for a desktop, a password and the fork.
4. **The vendor matrix** — Dell, Lenovo, HP visits: scan, handoff, live
   boot, hardware verify, half an hour each, read-only.
5. **Code signing** — a calendar item, not a code item; start now.
6. A **rescue mode** for machines refused for their drive: the staging
   step alone, reading what can be read onto the stick with checksums.

Longer term: hardware data seeded from
[linux-hardware.org](https://linux-hardware.org) probes rather than hand-curated,
and opt-in outcome reporting so the database learns from real conversions.

## Licence

GPL-3.0 — see [LICENSE](LICENSE).

A note on that choice: this project's trust model is "read the source", and
copyleft guarantees every fork stays readable. The nightmare scenario is a
closed fork that quietly softens the refusals while wearing this project's
earned trust; GPL-3.0 is the licence that forbids it. (It was MIT briefly —
switched while the contributor count was one, exactly when the old README
said it would be cheap.)
