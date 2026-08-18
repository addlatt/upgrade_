# Known risks

This project's only asset is that its output is trustworthy. A scanner that
says "probably fine" and isn't is worse than no scanner, because the person
acted on it and lost their data.

So this file exists to keep known-unverified things visible rather than
comfortable. Every entry states what would actually happen if the risk is real,
and what would close it. Nothing here is closed by argument — only by evidence.

Severity:

- **critical** — can produce a confidently wrong answer on the thing the tool
  exists to do, or can destroy user data
- **high** — wrong advice, or silent partial failure
- **medium** — degraded output, or a defect users would notice and report
- **low** — internal fragility, likely to become a bug later

Status is `open` unless a primary source or a real machine has confirmed it.

---

## R1 — VMD detection has never fired · critical · open

**What.** RST/VMD detection is 10 hand-written PCI device IDs plus a guessed
`iaStorV*` service-name regex (`data/devices.ps1`,
`Get-UpgVmdDeviceIds`). None of it has ever matched real hardware.

**Why it matters most.** This is the flagship check. The README calls it "the
single most common false 'Linux won't install'". If the IDs are wrong, the
scanner prints `[ OK ] standard AHCI / NVMe - visible to Linux installers` on
precisely the machines it was written to catch.

**If real.** A false GREEN on the highest-stakes check. The user backs up,
wipes, boots the installer, and it shows no disks at all — the exact failure
the tool promised to prevent, now with Windows already gone.

**Closes when.** The ID list is checked against a primary source (Intel
datasheets, the kernel's `drivers/pci/controller/vmd.c` ID table, or
linux-hardware.org probes from RST-enabled machines), **and** the check fires
correctly on at least one machine with RST enabled.

## R2 — Single-machine test corpus · high · open

**What.** Everything verified end-to-end was verified on one laptop: an ASUS
ROG Zephyrus G16 (Ryzen AI 9 HX 370, RTX 4060, MediaTek MT7925). Every `fail`
path is synthetic — exercised only by `-SelfTest` with fabricated check
objects.

**Untested against real hardware:** Broadcom Wi-Fi refusal, ARM refusal,
free-space refusal, MBR partition-limit warning, BitLocker-enabled path,
cloud-only placeholder detection (see R12), and every vendor quirk.

**If real.** Unknown. That is the problem — we have no evidence either way for
most of the codebase.

**Closes when.** Reports exist from a spread of machines: at least one Intel
laptop, one with Broadcom Wi-Fi, one pre-2015 machine, one with BitLocker on,
one Surface. This is the single best argument for shipping the scanner early
and asking for reports.

## R3 — Distro kernel table is unverified and stale · high · open

**What.** `data/distros.ps1` marks Fedora (6.14) and Pop!_OS (6.9) with
`Approx=$true` — not checked against release notes. Ubuntu 26.04 LTS is absent
entirely.

**Why it matters.** This table produces the report's headline claim. The
scanner tells users "RULED OUT for shipping an older kernel: Linux Mint, Ubuntu
LTS, Pop!_OS" with total confidence, from data marked as a guess.

**If real.** If Ubuntu 26.04 LTS ships a kernel new enough, we steer people
away from the most popular and best-supported option for no reason — and the
recommendation degrades to openSUSE Tumbleweed and Arch, which the same table
rates as unsuitable for newcomers.

**Closes when.** Every entry is confirmed against the distribution's own
release notes, `Approx` is cleared, `$script:UpgDistroTableVerified` is updated,
and Ubuntu 26.04 is added or explicitly excluded with a reason.

## R4 — Scanner and converter contradict each other · high · open · design decision

**What.** The scanner's free-space check says "Not enough free space to install
Linux **alongside** Windows" and uses 25/60 GB dual-boot thresholds
(`evaluate/windows/upgrade-scan.ps1`, `Test-UpgDisk`). The converter lists dual-boot as an
explicit non-goal, replaces Windows entirely, and requires an external drive.

**If real.** The scanner answers a question the product no longer asks. A user
passes the free-space check, starts the converter, and is refused for a reason
the scanner never mentioned.

**Closes when.** A decision is made and both sides reflect it: either the
scanner stays a general dual-boot-aware tool and the converter states its extra
requirements up front, or the scanner becomes the converter's front end and its
disk advice is rewritten around external-drive capacity. It cannot silently be
both. **Decide before the WPF app wraps around it** — this gets expensive to
unwind afterwards.

**Partially addressed.** The module split (`docs/architecture.md`) places the
scanner inside `evaluate`, which owns all refusals including external-drive
capacity. That fixes ownership but not the wording: the free-space check still
advises about dual-boot thresholds the product no longer uses. Still open.

## R5 — Single-user assumption · high · open

**What.** `Harvest-UpgradeState.ps1` collects only the current user's folders,
browser profiles and account details.

**If real.** A family computer with three accounts migrates one and silently
abandons the other two. The conversion reports success. The other two people
lose everything on a machine that no longer has Windows on it.

This is a data-loss-shaped bug aimed squarely at the "anybody can do this"
audience, who are the most likely to share a machine.

**Closes when.** The harvester enumerates all local profiles, sizes them, and
either migrates all of them or refuses multi-user machines explicitly. Refusing
is an acceptable v1 answer. Silently migrating one is not.

## R6 — Truncated sizing undersizes the backup · medium · open

**What.** `Get-HarvestFolderStats` stops at 250,000 files and sets
`Truncated=$true`. The backup estimate is then low.

**If real.** The user buys or selects an external drive based on a number that
is too small, and the conversion fails partway through the imaging step — after
committing.

**Closes when.** The Phase A UI treats `Truncated` as a hard blocker rather
than a note, or sizing is made exact for the folders that feed the estimate.

## R7 — Broadcom vendor fallback over-refuses · medium · open

**What.** The vendor fallback returns `fail` for *every* unrecognised Broadcom
device (`Get-UpgWifiVendorFallback`, `14e4`).

**If real.** Some Broadcom parts work acceptably with `b43` or `brcmfmac`. We
tell those users their machine needs a replacement card. Over-refusal is the
safer direction of error, but "refuse by default" is meant to mean *refuse when
we don't know* — not *assert a failure we haven't established*.

**Closes when.** The fallback distinguishes "known bad" from "unknown Broadcom,
bring a USB Ethernet adapter just in case", and common working Broadcom IDs are
added to the exact-match table.

## R8 — Cloud-only detection positive path untested · medium · open

**What.** Placeholder detection checks `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`
and `FILE_ATTRIBUTE_OFFLINE`. On the test machine it returned 0, and an
independent check confirmed there genuinely are none. The code ran; it has
never found anything.

**If real.** OneDrive placeholder files copy as empty. The user's photos appear
to migrate and are 0 bytes on the other side, discovered long after Windows is
gone.

**Closes when.** Tested on a machine with "Free up space" files present, ideally
also confirming that the OneDrive pinned/unpinned attribute bits
(`0x00080000` / `0x00100000`) don't need to be part of the test.

## R9 — No CI; dist can drift from source · medium · open

**What.** `dist/upgrade-scan.ps1` is committed so users can download one file.
Nothing enforces that it matches `data/` and `evaluate/`. The self-test never runs
automatically.

**If real.** A contributor edits `data/devices.ps1`, forgets `./build.sh`,
and the published scanner runs stale data while the source looks correct.

**Closes when.** A GitHub Action runs `./build.sh`, fails if `dist/` changes,
and runs `-SelfTest` on every PR.

## R10 — NVIDIA detection matches rendered display text · low · open

**What.** `Get-UpgRecommendation` decides whether NVIDIA is present with
`$_.Detail -match 'NVIDIA'` across all checks — matching formatted output
rather than structured data.

**If real.** Not wrong today. But `NVIDIA High Definition Audio` exists as a
device on the test machine, and the moment any check prints device names into
`Detail`, distro recommendations change silently.

**Closes when.** Checks carry a structured vendor field and the recommendation
reads that instead of display strings.

## R11 — Secure Boot check verifies nothing · low · open

**What.** The check reports `ok` when Secure Boot is enabled and lists distros
known to work, but never confirms the *chosen* distribution's shim will be
accepted by this firmware.

**Closes when.** The converter validates the target's signed shim against the
machine before committing, or the check's wording drops the implied guarantee.

---

# Non-code risks

## R12 — Code signing and antivirus · critical · open

**What.** The finished converter elevates, reads BitLocker recovery keys,
exports Wi-Fi passwords in cleartext, images a disk, and rewrites boot
configuration. That is behaviourally an exact match for an infostealer followed
by ransomware.

**If real.** Unsigned, SmartScreen shows "Windows protected your PC" and
Defender may quarantine it outright. The target user — non-technical, cautious,
warned their whole life about exactly this — stops there permanently. No amount
of interface work substitutes.

**Closes when.** An OV or EV code-signing certificate is obtained (requires a
legal entity and a few hundred dollars a year), releases are signed and
reproducible, and the binary is submitted to Microsoft's malware-analysis portal
for allowlisting.

**Start early.** Reputation accrues as a function of elapsed time and install
volume, so the certificate is worth having before there is anything to sign.

## R13 — The USB becomes a credential store · medium · open

**What.** Phase A writes Wi-Fi passwords in cleartext to the USB stick so the
Linux side can recreate the connections.

**If real.** Anyone who picks up the stick has the user's home and work Wi-Fi.

**Mitigations planned.** Restricted ACLs on write, wiping the credential portion
at the end of Phase B, and telling the user plainly in Phase A. **Closes when**
those are implemented and verified, not merely designed.

## R14 — Supply chain · medium · open

**What.** A tool that runs as SYSTEM and repartitions disks is a high-value
target. Compromising a release binary owns every machine that runs it.

**Closes when.** Releases are reproducible, signed, and checksummed, and the
release process does not depend on a single unprotected credential.

---

# Resolved

Kept for the record — all four were in code that read correctly.

## F1 — `break` inside `ForEach-Object` terminated the whole script · fixed

`Get-HarvestFolderStats` used a 45-second stopwatch and `break` to bound work on
large folders. With no enclosing loop, PowerShell unwinds past the function and
**terminates the entire script — silently, with exit code 0**. No error, no
`state.json`, no indication anything went wrong. It would have fired for any
user with a large Documents or Pictures folder. Undetected because the test
machine's largest folder holds 447 files.

Replaced with `Select-Object -First`, which stops a pipeline correctly.

## F2 — `$args` collision reported zero Wi-Fi networks · fixed

A local variable named `$args` inside a function shadows PowerShell's automatic
`$args`. Splatting it passed nothing to `netsh`, which exported no profiles, and
the harvester cheerfully reported **0 networks on a machine with 14**. Renamed
to `$netshArgs`.

## F3 — UTF-8 SSIDs mangled · fixed

`Get-Content -Raw` reads as ANSI on PowerShell 5.1, turning `Addison's iPhone`
into `Addisonâ€™s iPhone`. Any SSID with a curly quote, accent or emoji would
have produced a NetworkManager profile that never connects — and it would look
like a broken Wi-Fi driver, not a text-encoding bug. Switched to
`XmlDocument.Load()`, which honours the declared encoding.

## F4 — Visual Studio rule could never match · fixed

`(?<!Microsoft )Visual Studio (?!Code)` excluded "Microsoft Visual Studio
Community 2022" — the actual product the rule was written to flag, and how
essentially every real install is named. Corrected to `Visual Studio (?!Code)`.
