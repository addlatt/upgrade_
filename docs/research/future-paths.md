# Future paths: how upgrade_ could one day support more devices

**Decided (2026-09-27, the owner):** document the feasible path for every
device family the research rated `plausible` or `hard`, so the long-term
plan says *how*, not only *whether*. The ratings and sources are in
[device-feasibility.md](device-feasibility.md).

**This is a plan on paper, not a promise and not evidence.** Nothing here is
built or tested. By rule #2, every step below is an unknown until a real
machine confirms it, and each path lists what must be proven first. None of
it comes before v1: the Windows path ships first (CLAUDE.md, "Right now").
Claims marked *(to source)* are known in general but were not checked
against a primary source for this page.

## What every path reuses

Today's conversion has seven steps. The question for each new device is
which steps carry over, which change, and which are new.

```text
  step                      today (Windows PC)                     reusable?
  ------------------------  -------------------------------------  ---------
  1 scan + refuse           upgrade-scan.ps1                       rules yes, code per source OS
  2 harvest + job           Harvest-UpgradeState.ps1, New-Job.ps1  job.json schema yes
  3 write the stick         Write-UpgradeStick.ps1                 per source OS
  4 prologue                Invoke-Prologue.ps1: check, shrink,    per source OS
                              pause encryption, one-time boot
  5 installer on the stick  Fedora + shim + verify.sh (%pre)       yes on x86; new image on ARM
  6 install + outcome       kickstart, %post checks, outcome.sh    yes on x86
  7 first start             settle-in (runs on any Linux, R28)     yes
```

Two things carry over everywhere, because they are rules, not code:

- **The commit line.** Exactly one moment can't be undone, and the old
  system stays bootable until it.
- **Refuse by default.** Each new source gets its own scanner rows, and an
  unknown machine is refused, not guessed.

The paths below are ordered by how much of that table they reuse.

---

## Path A: x86 handhelds (ROG Ally, Legion Go, MSI Claw and others)

**Rating:** plausible. **Source OS:** Windows. **Reuse:** almost everything.

These are small Windows PCs with UEFI firmware, so steps 1-7 are the same
code. What changes is that they're held in two hands, with a touchscreen
and game controls instead of a keyboard.

```text
same as today:  scan -> job -> stick -> prologue -> one-time boot -> installer -> settle-in
new:            input without a keyboard, at three moments
```

What's new:

1. **Typing without a keyboard.** The launchers ask for a typed sentence and
   a password. On a handheld that means Windows' on-screen keyboard. Owed:
   check the launchers work with it.
2. **The countdown's "any key cancels".** In the installer there is no
   on-screen keyboard. The cancel must work with a controller button or the
   power button *(to source: which buttons the installer's console sees)*.
3. **Screen orientation.** Some handheld panels are natively portrait and
   show up sideways in a text console *(to source per model)*. The
   countdown must be readable.
4. **Drivers.** Controller, fan and power controls may need newer kernels
   or extra packages. The scanner learns each model's needs, like it does
   for Wi-Fi today.
5. **Which Linux.** Fedora works as the base. Gaming-focused Fedora-based
   systems exist *(to source)*, but the product promise is plain Fedora;
   a gaming variant would be a later menu choice.
6. **R29.** Newer handhelds may be Secured-core, with the Microsoft
   third-party certificate off. The scanner must check.

**Prove first (proposed gates, not yet in VALIDATION.md):**

- a `-DumpMachine` capture of one handheld into the corpus;
- a V0 handoff row (the one-time boot fires, and fails safe);
- the countdown cancelled with no keyboard attached.

**Who's stranded:** handhelds that shipped with Windows 10 and can't take
Windows 11 *(to source: which models)*.

---

## Path B: old Surface devices (x86)

**Rating:** plausible. **Source OS:** Windows. **Reuse:** almost everything.

Eleven Surface models can't get Windows 11 (device-feasibility C37), so
their owners are exactly this project's audience. They're Windows PCs, so
steps 1-7 carry over. The catch is drivers.

The fork in the road:

```text
option 1: stock Fedora kernel      walk-away works, Secure Boot stays simple,
                                   but some hardware doesn't work
                                   (cameras, some keyboards/touchpads, suspend)
option 2: linux-surface kernel     much better hardware support, but it needs a
                                   MOK enrolment: a blue screen at the next start
                                   where a person types a password. Not walk-away.
```

The honest default is **option 1 with the gaps named up front**: the scanner
tells the person, before anything happens, "your camera won't work on
Linux", per model. That keeps rule #1 (the report is trustworthy) and the
one-click promise. Option 2 could become an extra step in `settle-in` later,
offered with plain words, never silently.

What's new:

1. Surface rows in `data/devices.ps1`: what works on the stock kernel, per
   model (source: linux-surface's feature tables, C35).
2. Surface firmware (UEFI) supports USB boot and a third-party certificate
   option (C34). Owed: does the one-time boot through `bcdedit` fire on
   Surface firmware? That's a V0 row.
3. Surface keyboards that detach (Pro models): the countdown needs the Type
   Cover attached, or a button.

**Prove first:** a V0 row on one Surface Pro 4 or Surface Laptop 1; a
capture into the corpus; the scanner's per-model "won't work" list checked
on that machine.

---

## Path C: Intel Macs without T2 (before 2018)

**Rating:** plausible, and **the best next source**. **Source OS:** macOS.
**Reuse:** steps 5-7 and every rule; steps 1-4 are new code.

Every one of these Macs is off current macOS (C10, C11), and they're plain
x86 with UEFI firmware, so the same Fedora image and installer work. The
work is a new front half that runs on macOS instead of Windows.

```text
  Windows today                    Mac equivalent                      status
  -------------------------------  ----------------------------------  ------------------
  Windows PowerShell 5.1 (ships)   zsh / bash 3.2 (ship with macOS)    same "no setup" rule
  Get-CimInstance (hardware)       system_profiler                     to build
  BitLocker                        FileVault (fdesetup)                to build
  Resize-Partition                 diskutil apfs resizeContainer       to build, to source
  bcdedit {fwbootmgr} bootsequence bless --setBoot --nextonly (C17)    to prove (SIP question)
  shim + Secure Boot               no Secure Boot on these Macs        unsourced, to prove
  Windows as the rollback          macOS; the Option-key boot picker   to prove
```

Step by step:

1. **Scan and refuse, on macOS.** The same judgement rules, new readers:
   Wi-Fi chip (Broadcom is common), GPU, disk, FileVault, free space. The
   working rule mirrors PowerShell 5.1: only tools that ship with macOS, no
   installs.
2. **Harvest.** Folders, time zone, Wi-Fi. Also one thing only macOS can
   give: **the FaceTime camera firmware**, which Linux's camera driver needs
   copied from macOS (C19). This is the same "harvest what only the old
   system knows" pattern as today.
3. **Write the stick** from macOS (`diskutil`), with the same wrong-device
   refusals as the R16 writer.
4. **Prologue.** Shrink the APFS container to make room, then the one-time
   boot with `bless --nextonly`. **The open question:** one source says
   `bless` only works from Recovery when SIP (System Integrity Protection)
   is on (C18, secondary). If that's true, walk-away breaks here, and the
   path needs a different way to reach the stick.
5. **Installer.** Today's Fedora x86_64 image and `verify.sh`, unchanged
   in principle. Identity by the Mac's serial number.
6. **Install beside macOS.** The Mac's own boot picker (hold Option) is a
   natural way back, and `bless` can make macOS the default again, which
   would be the Mac rollback.
7. **settle-in** as today (it runs on any Linux, R28).

The three hard problems, each would become its own risk:

- **Wi-Fi on first start.** Broadcom cards need the `broadcom-wl` driver
  from RPM Fusion's non-free repository (C20). That isn't in the Fedora
  image and normally gets built after install with internet access, which
  is exactly what's missing. The design rule is "works offline". Owed: a
  way to carry a driver built for the exact installed kernel on the stick,
  or a refusal for Macs whose Wi-Fi can't work offline.
- **Reading macOS's files from Linux.** Today's keep-Windows path copies
  files from the kept Windows partition on first start. Linux has no
  built-in APFS support *(to source: only out-of-tree read drivers exist)*,
  and FileVault-encrypted disks are unreadable from Linux anyway. So on Macs
  the files likely have to be **staged while macOS is still running**: to
  the stick, or to a shared partition both systems can read (exFAT).
- **Secure Boot.** These Macs seem to have no firmware Secure Boot to work
  with (no primary source found). The chain from firmware to Fedora would be
  unverified. The docs must say so plainly, not borrow the Windows path's
  "Secure Boot on" claim.

**Prove first:**

- does `bless --nextonly` fire, and fail safe, with SIP on (a V0-style row
  on one real Mac);
- the live boot with Wi-Fi checked;
- an APFS shrink measured by two paths, the same discipline as R18.

---

## Path D: Snapdragon X laptops (Windows on ARM)

**Rating:** plausible, but nobody is stranded on them yet. **Source OS:**
Windows. **Reuse:** steps 1-4 and 7; steps 5-6 need an ARM build.

These run supported Windows 11, so they're a future path, not a rescue. The
Windows side should carry over *(to source: that Windows PowerShell 5.1
ships on Windows on ARM)*.

What's new:

1. **An ARM (aarch64) Fedora image and an ARM shim** on the stick. Fedora 44
   supports specific models and picks the right device tree (the wiring map
   ARM needs) automatically (C29, C30). The scanner refuses every model not
   on that list.
2. **R29 is certain here, not a maybe.** Copilot+ PCs are Secured-core, and
   Lenovo says the Microsoft third-party certificate is off by default
   (C33). The path needs a person to change one firmware setting, with
   screenshots, before anything else.
3. **Harvest the DSP firmware.** Speakers and battery readings need firmware
   files copied from the Windows partition (C29). This fits the harvest
   step exactly.
4. **Keep Windows, always.** Fedora's own guide says to keep Windows for
   firmware updates (C29). So on these machines the erase path is refused
   and keep-Windows is the only path.

**Prove first:** R29 answered from Microsoft's documentation; one supported
model (for example a ThinkPad T14s Gen 6) through a live boot with audio
checked.

---

## Path E: Intel Macs with T2 (2018-2020)

**Rating:** hard. Everything in Path C, plus one step a person must do by
hand.

T2 Macs refuse to start from a USB stick until someone opens Recovery Mode
and changes two settings: security to "No Security", and "Allow booting
from external media" (C14, C15). There's no official way around that, and
there shouldn't be.

The honest version of this path:

1. **One guided manual step,** done before the conversion starts, with
   screenshots and plain words. It's the only step that isn't walk-away,
   and the launcher says so up front.
2. **A non-stock kernel.** Fedora on T2 Macs uses the t2linux community's
   build (C16), not Fedora's own. That breaks the "unmodified Fedora
   installer" line the project holds today, so it needs its own decision.
3. **Wi-Fi firmware copied from macOS** (C13), a harvest step as in Path C.
4. Weak spots stay named in the scan: suspend, camera, microphone (C13).

**Prove first:** everything in Path C, plus whether the t2linux Fedora
image can be verified byte for byte and trusted the way Fedora's own image
is.

---

## TVs: the path that works today

**Rating for the TVs themselves:** blocked (every smart TV checked). But
the reason people ask is real: TVs get stranded too, when streaming apps
drop old models (device-feasibility T1-T5).

So the TV path runs through a computer, which is something upgrade_ already
does:

```text
old Windows 10 laptop --(upgrade_)--> Fedora laptop --(HDMI)--> the TV
                                      Chrome on x86 gets full streaming DRM (T13)
```

What that would add, all inside today's design:

- **A "TV" choice** in the launcher menu, next to KDE, GNOME and the text
  console. It would install a desktop set up for a couch: large text,
  browser-based streaming ready to go, sound over HDMI *(to research: a
  TV-style KDE interface exists, and is it maintained)*.
- **settle-in checks the TV works:** the picture fills the TV screen and
  sound comes out of the TV, the same "speakers, not headphones" rule as
  today.

Generic Android TV boxes that can boot LibreELEC or CoreELEC (T6) stay out
of upgrade_. Like Apple Silicon and Asahi, the right answer there is to
point people at the project that already does it well.

---

## Suggested order, once v1 ships

```text
1  Path B  old Surface       Windows source, stranded owners, mostly data rows
2  Path A  x86 handhelds     Windows source, input changes only
3  Path C  Intel Macs        new source OS, biggest stranded group, most new code
4  TVs     the TV choice     a menu item and a settle-in check
5  Path D  Snapdragon X      waits until people are stranded on it
6  Path E  T2 Macs           waits for a decision on non-stock kernels
```

The order follows reuse first, then how many people are stranded. It's a
proposal for the owner to reorder, not a decision.

## Before any path starts

- **Each path gets its own risks and gates** in `RISKS.md` and
  `VALIDATION.md` the day it's chosen. They aren't there yet, so nothing
  here competes with v1's list.
- **Every contact with a real machine leaves a capture** (rule #5). The
  first step of every path is the same: one real device, scanned read-only,
  its capture added to the corpus.
