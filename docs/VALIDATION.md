# Validation gates

What must be proven before building further. RISKS.md tracks what might be
wrong; this file is the ordered plan for finding out. Same rule applies:
nothing here closes by argument, only by evidence.

Ordering is by what dies if the answer is no, weighted by how little evidence
exists today. Each gate states the experiment, the pass criterion, and the
fallback design if it fails — because "we'll deal with it" is not a plan.

**The meta-rule: no component gets built on top of an unvalidated gate it
depends on.** The dependency map is at the bottom.

---

## V0 — The boot handoff fires · kills: walk-away itself · RISKS R15

The entire walk-away promise rests on `bcdedit /set {fwbootmgr} bootsequence`
booting a USB stick exactly once, on firmware from vendors who have never
heard of us. Evidence today: zero machines.

**Experiment.** Week one in a VM: OVMF UEFI, Windows guest, attach a stick
image as USB, run the four bcdedit commands, reboot. Then the physical
matrix: the ASUS G16 plus borrowed machines from at least three other vendors
(Dell, Lenovo, HP are the population). For each: does the stick boot exactly
once; after a deliberate live-environment failure, does the machine boot
Windows normally with no user action?

**Pass.** Fires on all tested firmware, or fails *safe* (Windows boots) on
the ones where it doesn't — with the failure detectable so the tool can say
so instead of silently doing nothing.

**If it fails.** Fallback is one manual step: "when it restarts, press the
key we show you." Walk-away degrades from perfect to press-one-key. Survivable
but the interface, docs and marketing all change — which is why this is V0.

**How to run.** The harness is `upgrade_/windows/Test-Handoff.ps1` (arm /
reboot / check, with `-FailMode` for the deliberate-failure paths); build the
stick per `upgrade_/windows/handoff-payload/README.md`; evidence lands in
`docs/validation-results/v0-handoff.csv`.

## V1 — Unattended install completes, Secure Boot on · kills: the conversion

The second half of the spine: a custom-composed live stick (Fedora's signed
shim/GRUB/kernel, untouched) boots with Secure Boot enabled and kickstart
drives Anaconda to a login screen with zero human input.

**Experiment.** Same spike as V0 — they are one build-order item. VM with
Secure Boot enforcing, then the physical matrix. Include the safety-copy
variant: install into freed space with `--onpart`, existing ESP reused, and
confirm both a 100 MB Windows-made ESP has room for shim+GRUB and
`bootmgfw.efi` still boots afterwards.

**Pass.** Hands-off from power-on to login, Secure Boot still enabled, and
(safety-copy variant) Windows still bootable from the boot menu.

**If it fails.** A known simpler shape exists: ship the *unmodified* Fedora
ISO on one partition and the kickstart on a second volume labeled `OEMDRV`,
which Anaconda picks up automatically. Less control, much less image
engineering, same signed chain. If custom composition fights us, fall back to
that rather than fighting.

## V2 — Extracted amp firmware makes speakers work · kills: the artifact pipeline

The docs' claim that `evaluate` must extract vendor firmware *now* because it
"cannot be added later" assumes extraction works at all: that the right blobs
can be pulled from the Windows driver store automatically and that the kernel
accepts them. Evidence today: the community does this by hand; nobody has
shown it end-to-end automated.

**Experiment.** On the test G16 itself — it has the exact CS35L56 hardware
the pipeline was designed for. Extract from its driver store, install Fedora
manually, place the firmware, play a sound through the *speakers*. This
validates extract → carry → inject on the hardware class that motivated it.

**Pass.** Audible speakers using only firmware harvested from that machine's
own Windows.

**If it fails.** The promise narrows: rely on linux-firmware upstream
coverage, and the scanner tells 2023+ laptop owners the truth about their
speakers instead of promising them. The "first impression is working
hardware" claim gets a hardware-generation asterisk.

## V3 — cryptsetup BITLK reads unattended · kills: safety-copy on modern defaults · RISKS R19

BitLocker is on by default on most machines the project targets. The
safety-copy path reads the user's only copy of their data through cryptsetup's
BITLK support, with nobody watching.

**Experiment.** Bench, all in VMs: Windows 11 with BitLocker defaults
(XTS-AES-128, used-space-only), hash every file from inside Windows, attach
the disk to Linux, unlock with the recovery key, re-hash, compare. Repeat for
XTS-AES-256 and full-disk encryption. Thousands of files, byte-identical or
it fails.

**Pass.** Identical hashes across all three configurations.

**If it fails.** Either decrypt-in-Windows-first (`manage-bde -off`; adds
hours, works) or BitLocker machines get clean-slate only. Both survivable,
both worse — and either changes the intent-capture UI, so we need the answer
before that UI exists.

## V4 — Real disks can actually shrink · kills: the safety-copy audience · RISKS R18

Safety-copy needs shrinkable space ≥ ~20 GB + user data. Immovable files
(MFT, VSS store) routinely cap shrink far below free space. If most real
machines can't shrink enough, safety-copy is a niche path and the design
leans almost entirely on clean slate + big sticks — which changes what stick
size we tell people to buy.

**Experiment.** Add the shrinkable-space query (the same one Disk Management
uses) to the scanner and ship it. Every scanner report then measures the
population for free. Pass judgment after ~20 real reports.

**Pass.** A meaningful fraction (say, a third) of scanned machines could host
Linux + their data in shrinkable space.

**If it fails.** Safety-copy stays in the design as the lucky path;
messaging, stick-size guidance and the intent UI reweight toward clean slate.

## V5 — VMD detection fires on real RST hardware · kills: scanner trust · RISKS R1

The flagship check has never matched anything. The project's only asset is
that its report is trustworthy, and this is the report's highest-stakes line.

**Experiment.** An afternoon: diff the ID list against the kernel's
`drivers/pci/controller/vmd.c` table and linux-hardware.org probes. Then one
machine: any 11th-gen+ Intel Dell or Lenovo laptop with RST enabled (they
ship that way) — scanner must say FAIL; flip it to AHCI — scanner must say
OK.

**Pass.** Both directions on at least one physical machine, list reconciled
with the kernel's.

**If it fails.** Fix the list and re-run; this one has no fallback because it
has no excuse — it's cheap.

## V6 — A signed binary can earn Defender's tolerance · kills: distribution · RISKS R12

Not a code question, a calendar question: reputation accrues with elapsed
time, so this validation *is* the mitigation.

**Experiment.** Start now: legal entity, OV certificate, sign something
trivial (the scanner wrapped in an exe is perfect), submit to Microsoft's
malware-analysis portal, distribute modestly, and measure SmartScreen
behavior monthly. By the time the converter exists, we know whether signed +
submitted + aged is sufficient — or whether EV/store distribution is needed.

**Pass.** The signed test binary downloads and runs on a stock machine
without SmartScreen interception.

**If it fails.** EV certificate, Microsoft Store packaging, or distribution
through repair-event channels with humans who can click past warnings. All
slower, all viable, all better known a year early.

## V7 — The scanner generalizes beyond one laptop · kills: the knowledge-base model · RISKS R2

Every check was verified on one ASUS G16. The community-table model only
works if reports from strange machines mostly confirm the tables.

**Experiment.** Ship the scanner publicly (it's ready modulo the R4 wording
rework), ask for reports: at least one Intel laptop, one Broadcom machine,
one pre-2015 machine, one BitLocker-on machine, one Surface. This also feeds
V4 for free.

**Pass.** Reports arrive and the verdicts survive contact — or the failures
are table gaps (one-line fixes) rather than logic failures.

---

## Lesser gates (validate when their component is built)

- **Counterfeit stick is caught** (R17): buy a known-fake stick, confirm the
  read-back verification fails it before the commit line.
- **Browser profile transplant matrix** (R20): real Windows→Linux moves per
  browser/version before `evaluate` promises anything.
- **Both desktops fit the stick**: compose the dual-squashfs image, weigh it,
  set the minimum stick size from the number, not a guess.
- **Windows reinstall fallback is real**: verify the digital-licence
  reactivation claim once, on the G16, so the clean-slate consent screen
  tells the truth.

## Dependency map — what is blocked on what

| Waiting on | Blocked work |
|---|---|
| V0 + V1 (the spine spike) | everything in `upgrade_/` and `settle-in/`; the live image; the kickstart generator beyond a stub |
| V2 | the artifact-extraction pipeline's scope (build order step 2) |
| V3 | the intent-capture UI's path logic; safety-copy cutover |
| V4 | stick-size guidance; intent UI weighting (ship scanner change now) |
| V5 | nothing — do it this week regardless |
| V6 | nothing — start the clock now; blocks only the eventual release |
| V7 | table confidence; multi-distro ambitions |

V5 and V6 start immediately because they cost an afternoon and a calendar
respectively. V0–V3 are the spike plus two bench tests, all parallelizable.
V4 and V7 ride the scanner's public release.
