# Validation gates

What must be proven before building further. RISKS.md tracks what might be
wrong; this file is the ordered plan for finding out. Same rule applies:
nothing here closes by argument, only by evidence.

Ordering is by what dies if the answer is no, weighted by how little evidence
exists today. Each gate states the experiment, the pass criterion, and the
fallback design if it fails — because "we'll deal with it" is not a plan.

Gates are grouped into the same **tiers** used in `CLAUDE.md` and `RISKS.md`,
so the three agree:

- **Tier 1 — no product if these fail:** V0, V1, V1b
- **Tier 2 — a core promise breaks (recoverable):** V4, V3, V2
- **Tier 3 — silent data loss:** V8
- **Tier 4 — kills adoption, not the mechanism:** V5, V6, V7

The V-numbers are stable identifiers, not a priority order — read the tier, not
the number. **The meta-rule: no component gets built on top of an unvalidated
gate it depends on.** The dependency map is at the bottom.

**Method note (2026-08-22): spoof everything spoofable.** Each gate splits
into a part testable without the real thing — detection logic against
fabricated objects, replays of captured real-machine enumerations
(`-DumpMachine` → `evaluate/windows/corpus/`), spoofed devices in VMs — and
an unspoofable residue that only a real machine or primary source can close.
The spoofable part gets automated tests that run on every `-SelfTest`; the
residue is what the gate's **Pass** line means. A green simulation narrows a
gate; it never closes one, because the simulation is built from the very
model of the hardware the gate exists to question. CLAUDE.md rule #5 carries
the full statement.

---

# Tier 1 — no product if these fail

## V0 — The boot handoff fires · kills: walk-away itself · RISKS R15

**VM leg fired (2026-08-23).** On the QEMU+OVMF rig (`rig/vm/`) the baseline
SB-off run records `fired-once`: one-time boot, payload ran, Windows returned
no-keypress, one-shot self-cleared. Two bugs fixed en route (payload `fs0:`
marker misdirection; harness counting its own test entry as a reorder) — both
would have hit the physical test too; see RISKS R15. Remaining on this rig:
the SB-on fail-modes and the BitLocker matrix. Then the physical vendor matrix
and the Hyper-V Gen 2 leg — a VM pass narrows V0, it does not close it.

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

The second half of the spine, and the shared base both paths need: a
custom-composed live stick (Fedora's signed shim/GRUB/kernel, untouched) boots
with Secure Boot enabled and kickstart drives Anaconda to a login screen with
zero human input.

**Experiment.** Same spike as V0 — they are one build-order item. VM with
Secure Boot enforcing, then the physical matrix. This gate covers the base
install to a login screen; the alongside-specific concerns are V1b.

**Pass.** Hands-off from power-on to login, Secure Boot still enabled.

**If it fails.** A known simpler shape exists: ship the *unmodified* Fedora
ISO on one partition and the kickstart on a second volume labeled `OEMDRV`,
which Anaconda picks up automatically. Less control, much less image
engineering, same signed chain. If custom composition fights us, fall back to
that rather than fighting.

## V1b — Installing alongside a shrunk Windows leaves Windows bootable · kills: the default path's safety net · RISKS R21

**VM leg fired (2026-08-27).** On the QEMU+OVMF rig (`rig/vm/v1b.sh`, SB off —
the only mode this host can run): C: shrunk 32 GiB, Fedora 42 kickstarted into
the gap reusing the Windows ESP unformatted, and all five checks held — ESP
had room (+6.2 MB, 260 MiB ESP), `bootmgfw.efi` byte-identical throughout,
Windows reached through the GRUB menu three times (its own `BootCurrent`
named Fedora's entry), Linux booted five times, every cycle a fresh QEMU.
Recorded as `fallback-loader-replaced`, not a bare pass: the install
overwrote Windows' `EFI/Boot/bootx64.efi` with shim, and the run also caught
Windows re-taking the boot order after a servicing pass and the firmware
dropping OS boot entries — five design inputs, detailed in RISKS R21. Row:
`docs/validation-results/v1b-alongside.csv`. Remaining: the Secure-Boot-on
chainload, a ~100 MiB ESP row, and the physical vendor matrix — a VM pass
narrows V1b, it does not close it.

**Decided (2026-08-30)** from those findings (RISKS R21 has the list): shim
keeps the fallback slot and rollback restores Windows' copy from a snapshot
the prologue takes; post-install boot-chain verification is a cutover step
with results in `outcome.json`; os-prober set explicitly; `evaluate` gates on
≥ 32 MiB free on the ESP; the boot-order takeover is its own risk (R22) with
a settle-in re-assert unit. **Owed code before anything writes:** the
scanner's ESP check (free space, and that the Windows Boot Manager entry
points at it — elevated only, like the shrink query), with self-test cases.
The bench row turns `pass-plumbing` only once the converter's own install
step runs on it.

The default keep-Windows path installs Linux into freed space and **must leave
the shrunk Windows fully bootable**, because Windows is both the rollback and
the file source. This is harder than the wipe install and, since the redesign,
it is the common case — not a "variant" of V1. It earns its own Tier-1 gate.

**Experiment.** Alongside install on real machines from several vendors,
Secure Boot on: `--onpart` into the freed space, **reuse the existing Windows
ESP without reformatting it**, add shim + GRUB, run `os-prober`. Check each of:
the ~100 MB Windows-made ESP had room for the added entries; `bootmgfw.efi` is
untouched; Windows still boots from the GRUB menu; Linux boots; both survive a
few power cycles.

**Pass.** After the install, *both* systems boot from the menu, Secure Boot
still enabled, on every machine in the matrix.

**If it fails.** Machines whose firmware or ESP can't take the alongside
install are steered to **clean slate** (which never shares an ESP — it wipes
and lays down a fresh layout), and `evaluate` says so before committing. The
default simply doesn't apply to those machines; the product still converts
them, without the safety net.

**How to run.** The bench is `rig/vm/v1b.sh` (run-book in `rig/vm/README.md`):
offline disk inspections before/after (`v1b-inspect.py`), the guest-side
shrink (`rig/vm/guest/v1b-shrink.ps1`), the kickstart (`rig/vm/v1b-ks.cfg`)
auto-loaded from an OEMDRV volume, boot markers written by each OS, and
`v1b.sh verdict` turning all of it into the CSV row. On a physical machine the
same pieces apply with the machine's own disk in place of the qcow2 — the
inspector needs a block-device reader instead of `qemu-img dd`.

# Tier 2 — a core promise breaks (recoverable, but the default is broken)

Three gates here, all recoverable failures that nonetheless break a core
promise: **V4** (do disks shrink enough — gates whether the default path even
applies), **V3** (the BITLK read that delivers files on the default path), and
**V2** (firmware that makes speakers work). Kept in V-number order below.

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

## V3 — cryptsetup BITLK reads · kills: the default path's file delivery · RISKS R19

BitLocker is on by default on most machines the project targets. The
keep-Windows path — now the default — delivers the user's files by reading the
kept Windows partition through cryptsetup's BITLK support, in `settle-in`. The
redesign made this *recoverable* (user present, Linux verified, Windows intact
as backup — see R19), but if it's flaky the default experience is broken for
everyone on modern BitLocker machines.

**Experiment.** Bench, all in VMs: Windows 11 with BitLocker defaults
(XTS-AES-128, used-space-only), hash every file from inside Windows, attach
the disk to Linux, unlock with the recovery key, re-hash, compare. Repeat for
XTS-AES-256 and full-disk encryption. Thousands of files, byte-identical or
it fails. Test from an *installed* Fedora (where `settle-in` runs), not only
the live environment.

**Pass.** Identical hashes across all three configurations.

**If it fails.** Either decrypt-in-Windows-first (`manage-bde -off`; adds
hours, works) or BitLocker machines get clean-slate only. Both survivable,
both worse — and either changes the intent-capture UI, so we need the answer
before that UI exists.

## V4 — Real disks can actually shrink · kills: whether the default even applies · RISKS R18

Keep-Windows is the **default** and needs shrinkable space ≥ ~20 GB + user
data. Immovable files (MFT, VSS store) routinely cap shrink far below free
space. If most real machines can't shrink enough, the default rarely applies
and the design leans almost entirely on clean slate + big sticks — which
changes what stick size we tell people to buy and how often anyone gets the
safety net at all.

**Experiment.** Add the shrinkable-space query (the same one Disk Management
uses) to the scanner and ship it. Every scanner report then measures the
population for free. Pass judgment after ~20 real reports.

*Done (2026-08-22):* the query is in `Test-UpgDisk` as `Room to keep Windows`.
Caveat found on real hardware: `Get-PartitionSupportedSize` needs
Administrator (RISKS R18) — so only elevated runs carry a number. Filter the
JSON corpus on `RanAsAdmin=true` before judging, and note the sample will skew
toward users willing to elevate.

**Pass.** A meaningful fraction (say, a third) of *elevated* scanned machines
could host Linux + their data in shrinkable space.

**If it fails.** Keep-Windows becomes the lucky path rather than the default;
messaging, stick-size guidance and the intent UI reweight toward clean slate.

# Tier 3 — silent data loss (the trust-ending class)

## V8 — OneDrive placeholders are materialized at evaluate · kills: file integrity on the default path · RISKS R8

On the default path files are pulled from the mounted Windows partition *by
Linux*, which has no OneDrive client. A "free up space" placeholder not forced
local beforehand copies over as **0 bytes** — the user's photos arrive empty,
discovered later. `evaluate` must *materialize* them (force the download while
Windows is alive), not merely detect them, because no later stage can.

**Experiment.** On a machine with OneDrive "free up space" files present:
confirm `evaluate` detects them, forces them local, and that they carry real
bytes on the NTFS partition afterward. Confirm the pinned/unpinned attribute
bits (`0x00080000` / `0x00100000`) don't need to be part of the detection.

**Pass.** No cloud-only stub survives into the pulled data as a 0-byte file.

**If it fails.** `evaluate` refuses machines with un-materializable
placeholders rather than silently copying empties — refuse-by-default applies:
better to turn someone away than to lose their photos.

# Tier 4 — kills adoption, not the mechanism

## V5 — VMD detection fires on real RST hardware · kills: scanner trust · RISKS R1

The flagship check has never matched anything. The project's only asset is
that its report is trustworthy, and this is the report's highest-stakes line.

**Experiment.** An afternoon: diff the ID list against the kernel's
`drivers/pci/controller/vmd.c` table and linux-hardware.org probes. Then one
machine: any 11th-gen+ Intel Dell or Lenovo laptop with RST enabled (they
ship that way) — scanner must say FAIL; flip it to AHCI — scanner must say
OK.

**Desk half done (2026-08-22).** ID list reconciled against `vmd.c` (mainline
master) and pci.ids: three bogus IDs removed (`7ec0` was a USB controller — a
false-RED landmine on Core Ultra 200 machines; `2010`/`e0b0` aren't Intel
devices), five kernel IDs added (`28c0, 4c3d, b60b, b06f, b07f`), `09ab` kept
with an Intel citation (article 000088762). The `^iaStorV` service regex was
replaced — it missed the whole pre-VMD RST family (iaStorA/iaStorAC/iaStorAVC,
the Skylake–Comet Lake remap generation) — with three signals: kernel ID list,
`iaStorVD` service, and PCI RAID class code `CC_0104` from CompatibleID
(format verified live on the G16). Six detection-level self-test cases feed
fabricated PnP entries through the real check; all pass. Full evidence trail
in RISKS R1.

**Level-3 spoof done (2026-08-26).** The full Windows PnP → WMI → scanner
pipeline now fires on simulated hardware. A patched QEMU `pci-testdev`
(`rig/vm/`) presents PCI `8086:9a0b` class `0104`; Windows enumerates it as an
unknown RAID Controller (CompatibleIDs `PCI\CC_010400` / `PCI\CC_0104`), and
both the source scanner and the built `dist/` return `[FAIL] Storage
controller mode — Intel RST / VMD active`, verdict RED. Captured hardware-only
and curated as the synthetic corpus regression
`evaluate/windows/corpus/vm-qemu-q35-vmd-spoof-9a0b.json`. This closes the
**plumbing** — enumeration, parsing and verdict all work end-to-end — but it
is built from our own model of the IDs, so per CLAUDE.md rule #5 it narrows V5
without closing it. See RISKS R1 for the full trail.

**Pass.** Both directions on at least one physical machine, list reconciled
with the kernel's. The reconciliation and the level-3 plumbing are done; **what
remains is the physical machine**: a borrowed Intel laptop with RST/VMD enabled
— FAIL with it on, OK after switching to AHCI. The G16 (AMD, standard NVMe)
cannot exercise the positive path, and neither can the spoof: the synthetic
capture is the residue's regression test, not a substitute for it.

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

# Lesser gates (validate when their component is built)

Real, but they degrade rather than kill, or only touch the fallback path:

- **Counterfeit stick is caught** (R17): buy a known-fake stick, confirm the
  read-back verification fails it before the commit line. *Fallback path only
  now — data rides the stick only on clean slate.*
- **Browser profile transplant matrix** (R20): real Windows→Linux moves per
  browser/version before `evaluate` promises anything.
- **Both desktops fit the stick**: compose the dual-squashfs image, weigh it,
  set the minimum stick size from the number, not a guess.
- **Windows reinstall fallback is real**: verify the digital-licence
  reactivation claim once, on the G16, so the clean-slate consent screen
  tells the truth.

# Dependency map — what is blocked on what

| Waiting on | Blocked work |
|---|---|
| V0 + V1 + V1b (the spine spike) | everything in `upgrade_/` and `settle-in/`; the live image; the kickstart generator beyond a stub |
| V1b specifically | the default keep-Windows cutover (alongside install); if it fails on a machine, that machine is clean-slate-only |
| V2 | the artifact-extraction pipeline's scope (build order step 2) |
| V3 | the intent-capture UI's path logic; the settle-in file pull |
| V4 | stick-size guidance; intent UI weighting (ship scanner change now) |
| V8 | the settle-in file pull's integrity guarantee; `evaluate`'s materialize-or-refuse step |
| V5 | nothing — do it this week regardless |
| V6 | nothing — start the clock now; blocks only the eventual release |
| V7 | table confidence; multi-distro ambitions |

V5 and V6 start immediately because they cost an afternoon and a calendar
respectively. V0 + V1 + V1b are the spine spike — one VM build-order item,
now including the alongside install that keeps Windows bootable. V3 and V8 are
bench tests, parallelizable. V4 and V7 ride the scanner's public release.
