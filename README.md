# upgrade_

**Move an ordinary computer from Windows to Linux, without needing to know how.**

Plug in a USB stick, make two choices, click convert. Come back to a working
Linux machine with your files, Wi-Fi and browsers intact. One stick is the
whole kit — there is no external drive anywhere in this design.

> **Status: the evaluator works. The conversion is designed, not built.**
> Today you can run the preflight scanner and get a real answer about your
> machine. Nothing in this repository writes to a disk yet — see
> [Status](#status) for what exists and what doesn't.

---

## Why

Windows 10 stopped getting security updates in October 2025. Windows 11's TPM
and CPU requirements stranded an enormous number of machines that are
mechanically and electrically fine. Those computers are not broken. They were
declared obsolete by someone else's product roadmap.

Most of them can run Linux for another five years. The obstacle was never
capability — it's that converting a machine currently requires knowing which
distribution suits your hardware, why the installer can't see your SSD, and what
to do when the speakers are silent afterwards. That knowledge is the barrier,
and it's the thing software can carry for you.

## How it works

Three modules. The boundary between them is **commitment**, not operating
system.

| | | |
|---|---|---|
| **`evaluate`** | Windows · reversible | Reads the machine, captures your choices, extracts everything that only exists while Windows does, and refuses anything it can't do safely. |
| **`upgrade_`** | Windows → Linux | The converter. Stages your files to the stick — or shrinks Windows aside and leaves them in place — then converts. |
| **`settle-in`** | Linux | Verifies the hardware actually works, hands over, stops. |

### The commit line

Exactly one moment in a conversion is irreversible, and Windows stays bootable
until it. On the **clean-slate** path (your files ride the stick) it is the
disk wipe — guarded by a two-minute human hardware check in the live session,
because that path has no rollback. On the **safety-copy** path (your files
never leave the internal disk; Windows is shrunk aside and kept) it is the
**reclaim** of the Windows partition, which happens only after first-boot
verification and your explicit consent. Everything earlier is additive, and a
failure at any earlier point simply boots Windows again.

Two rules follow. The interface says *"you can still cancel"* until that exact
moment and stops the instant it's crossed. And everything capable of refusing
must refuse **before** the line, because afterwards the only remaining safety
mechanism is a slow restore from a drive that might itself fail.

Full design in [docs/architecture.md](docs/architecture.md).

---

## What works today: `evaluate`

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

There is no override flag for a RED verdict, and there should never be one.
The converter runs from a single USB stick and refuses machines it cannot fit
— files too large for the stick and too little space to shrink Windows aside —
but the refusal is a **gap report**, not a door slam: exactly how many GB to
free or what stick size would change the answer. It will refuse machines with
Intel RST/VMD active, because that setting cannot be changed safely from
software on every vendor's firmware.

This still turns away real users, and that is the correct trade. A tool like this
earns trust once and spends it permanently the first time it destroys someone's
photos. Any component that writes to a disk has to clear a far higher bar than
the scanner does — which is why none of them are written yet.

---

## Status

| Component | State |
|---|---|
| `evaluate` — scanner (checks, verdict, distro recommendation) | **works**, tested on real hardware |
| `evaluate` — state harvester (locale, folders, Wi-Fi, browsers, capacity) | **works**, read-only |
| `evaluate` — artifact extraction (vendor firmware, BitLocker key) | designed |
| `evaluate` — intent capture UI, `job.json` | designed |
| `upgrade_` — prologue: imaging, staging, boot handoff | designed |
| `upgrade_` — cutover: partition, install, inject, restore | designed |
| `upgrade_` — rollback | designed |
| `settle-in` | designed |

Known unknowns are tracked openly in [docs/RISKS.md](docs/RISKS.md) — what is
unverified, what would happen if each risk is real, and what evidence would
close it. Read it before trusting any single check. Notably: **VMD detection has
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
  linux/                  cutover: install, inject, restore
settle-in/              module 3 — verify hardware, hand over, stop
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

Next, in order:

1. **The spine spike** — a hello-world conversion in a VM: boot handoff →
   live image → kickstart → Fedora. Proves the walk-away mechanism on
   evidence before anything is built on top of it.
2. `job.json` schema — the contract between `evaluate` and `upgrade_`
3. **Artifact extraction** — vendor firmware and keys. The one piece that
   cannot be added later on a user's machine: once Windows is gone, those
   files are unrecoverable.
4. Kickstart generator — `job.json` → a Fedora install (per-target seam for
   future distributions)
5. Live image — Fedora squashfs (KDE and GNOME) plus the cutover orchestrator
6. `settle-in`, including reclaim
7. Shrink, stick authoring, boot handoff hardening — **last, and reviewed
   hardest**

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
