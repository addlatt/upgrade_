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

## R1 — VMD detection has never fired · critical · open (desk half closed 2026-08-22; level-3 spoof fired 2026-08-26)

**What.** RST/VMD detection was 10 hand-written PCI device IDs plus a guessed
`iaStorV*` service-name regex (`data/devices.ps1`,
`Get-UpgVmdDeviceIds`). None of it has ever matched real hardware.

**Reconciled against primary sources (2026-08-22).** The desk-research half of
the close condition is done; what it found justified the risk's severity:

- **The ID list was checked against the Linux kernel's VMD driver table**
  (`drivers/pci/controller/vmd.c`, `vmd_ids[]`, mainline master — fetched
  2026-08-22) and the PCI ID repository (pci-ids.ucw.cz). Three of our ten IDs
  were wrong: `8086:7ec0` is a **USB xHCI controller** (pci.ids: "Core Ultra
  200 Series Processors USB xHCI") — it would have produced a false RST/VMD
  RED on current Intel laptops — and `8086:2010` / `8086:e0b0` are not Intel
  devices in pci.ids at all. Five kernel-listed VMD IDs were missing:
  `28c0, 4c3d, b60b, b06f, b07f`. `8086:09ab` is confirmed real (pci.ids "RST
  VMD Managed Controller"; Intel article 000088762 documents the 9A0B/09AB
  pair) — it is Windows-visible only, which is why it is absent from vmd.c and
  why we keep it. The list now carries a per-ID source citation.
  Cross-checked against linux-hardware.org probes: `9a0b` appears on 211 real
  devices and `a77f` on 152, both bound to the `vmd` driver, so the flagship
  IDs exist in the wild; `4c3d` shows 0 probes there but stays on the kernel
  table's authority.
- **The `^iaStorV` regex was wrong in the dangerous direction.** The RST
  driver service family across generations is iaStor / iaStorV (Vista-era
  inbox), iaStorA / iaStorAC / iaStorAVC (RST ~11–17, including the
  Skylake–Comet Lake "RST Premium" NVMe-remap mode — Intel article 000059291;
  Microsoft's Win10 1903 RST compatibility hold), and iaStorVD (RST 18+, the
  VMD generation — Intel article 000057787). `^iaStorV` matched only the
  first and last of those: it **missed the entire pre-VMD RST family, which is
  the 2015–2020 population this tool exists for.**
- **Detection now uses three signals** (`Test-UpgStorageMode`): the
  kernel-reconciled ID list; the `iaStorVD` service (catches VMD IDs we don't
  know yet); and an Intel controller reporting PCI RAID class code `CC_0104`
  in its compatible IDs — the controller itself declaring RAID/remap mode
  (Microsoft, "Identifiers for PCI devices"). An `iaStor*` service on a
  non-RAID-class controller now warns instead of failing (RST software on an
  AHCI-mode controller — see R7 on over-refusal). The class-code mechanism was
  verified live on the G16: its NVMe controller reports `PCI\CC_0108` in
  `CompatibleID`, exactly the documented format, and correctly does not match.
- Six detection-level self-test cases now push fabricated PnP entries through
  the real `Test-UpgStorageMode` (VMD ID, 09ab child, unknown-ID-with-iaStorVD,
  CC_0104 RAID class, RST-on-AHCI warn, standard NVMe ok). All pass — but they
  are still synthetic.

**Level-3 spoof fired (2026-08-26).** The flagship check now fires through the
*real* Windows PnP → WMI → scanner pipeline, not just fabricated objects. On
the QEMU rig (`rig/vm/`), a patched `pci-testdev` (build-qemu-vmd.sh; QEMU
8.2.2) presents PCI `8086:9a0b` class `0104`; Windows enumerates it as an
unknown **RAID Controller** — instance `PCI\VEN_8086&DEV_9A0B&SUBSYS_00008086&
REV_00`, CompatibleIDs including `PCI\CC_010400` and `PCI\CC_0104`, no driver
bound so device Status is `Error`. Both the source scanner and the built
`dist/` fired identically: `[FAIL] Storage controller mode — Intel RST / VMD
active (RAID Controller)`, verdict RED. The device matched on signal 1 (the
`9a0b` ID from the kernel-reconciled list); signal 3 (RAID class `CC_0104` in
CompatibleIDs) is independently present as a backstop. Captured hardware-only
with `-DumpMachine` and curated as the permanent synthetic regression
`evaluate/windows/corpus/vm-qemu-q35-vmd-spoof-9a0b.json` (`Synthetic: true`),
which replays green on every `-SelfTest` alongside the G16 entry.

**This is plumbing, not evidence** (CLAUDE.md rule #5). The VM presents IDs
built from our own model of the hardware, so the run proves the
enumeration → parse → verdict path works end-to-end for a device that declares
these IDs — and proves nothing about what real RST hardware actually presents.
The real-hardware clause below is untouched; this capture must never be cited
against it.

**Still open — the half that needs hardware.** The check has never fired on a
real RST-enabled machine. The remaining test is exactly V5's: a borrowed Intel
laptop (11th gen or newer Dell/Lenovo ships with VMD/RST on by default) —
scanner must FAIL with RST/VMD enabled, then OK after the BIOS is switched to
AHCI. The only machine on hand, the ASUS G16, is AMD with standard NVMe and
cannot exercise the positive path; it only confirms the negative one.

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

**Mechanism built (2026-08-22).** The scanner now has `-DumpMachine`
(hardware-only capture) and a corpus replay in `-SelfTest`: every curated
capture in `evaluate/windows/corpus/` is replayed through the pure detection
checks on every run. The G16 is the first corpus entry. This is how R2's
machines will stay closed once reached — each machine met once is regression-
tested forever (CLAUDE.md rule #5). The risk itself stays open until the
spread of machines above actually exists in the corpus.

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

## R4 — Scanner and converter contradict each other · medium · code done, verify pending · design decision

**What.** The scanner's free-space check says "Not enough free space to install
Linux **alongside** Windows" and uses 25/60 GB dual-boot thresholds
(`evaluate/windows/upgrade-scan.ps1`, `Test-UpgDisk`). The converter lists dual-boot as an
explicit non-goal and replaces Windows entirely (at the time this was
written, via a since-removed external-drive design; now via the USB-only
paths).

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
scanner inside `evaluate`, which owns all refusals. That fixes ownership but
not the wording.

**Decided (2026-08-19).** The scanner keeps two personalities in one codebase:
standalone, a general advisory tool recommending across distributions;
embedded, the converter's `evaluate` mode with the converter's own gates. The
free-space check survives with new meaning under the USB-only design —
shrinkable space gates the safety-copy path, stick capacity gates the
clean-slate path.

**Wording reworked (2026-08-22).** `Test-UpgDisk` no longer emits the
dual-boot free-space fail or the "Backup drive needed" external-drive line.
It now reports `Disk in use` (info) and `Room to keep Windows` — the
shrinkable-space measurement via `Get-PartitionSupportedSize`, which gates the
safety-copy path. The "BEFORE YOU DO ANYTHING" block no longer tells anyone to
buy an external drive. Low free space is no longer a RED trigger — under
USB-only a full disk just means clean-slate-only, not "cannot convert".

**Still open** until the numbers are right, not just the words: the
clean-slate gate needs the harvester's folder sizing to answer "does your data
fit an N GB stick", which the scanner alone cannot compute. The scanner now
informs; `evaluate` still has to gate. Downgraded to medium — the dangerous
contradiction (telling people to buy a drive the product doesn't use) is gone.

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

**If real.** The path gate and the gap report are computed from a number that
is too small: a machine is steered onto the clean-slate path with files that
do not actually fit the stick, discovered at staging — or worse, trusted.

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
gone. Sharper under the keep-Windows default: files are pulled in `settle-in`
by reading the mounted NTFS *from Linux*, which has no OneDrive client — so a
placeholder that was not materialized beforehand cannot be filled at pull time,
only copied empty.

**Design response (2026-08-22).** `evaluate` must *materialize* placeholders —
force the download to real bytes while Windows is alive — not merely detect
them, because no later stage can. This is now stated in `architecture.md`
(evaluate's harvest duties) as a hard step: materialize, or refuse.

**Detection arm exercised (2026-08-22).** The harvester's `-SelfTest` now sets
a genuine `FILE_ATTRIBUTE_OFFLINE` on a real NTFS file and confirms
`Get-HarvestFolderStats` counts it as cloud-only — the real attribute read
through the real filesystem, not a fabricated object. Narrowed residue: the
`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` arm as the actual OneDrive cloud filter
sets it (only the filter can), and materialization itself.

**Closes when.** Materialization is implemented and tested on a machine with
"Free up space" files present — confirming the files carry real bytes on the
NTFS partition afterward — ideally also confirming the OneDrive pinned/unpinned
attribute bits (`0x00080000` / `0x00100000`) don't need to be part of the test.

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

**What.** `evaluate` writes secrets to the stick so the Linux side can act on
them: Wi-Fi passwords in cleartext (to recreate connections) and, under the
keep-Windows default, the **BitLocker recovery key** (so `settle-in` can unlock
and read the Windows partition).

**If real.** Anyone who picks up the stick has the user's home and work Wi-Fi —
and, worse, the key to their still-intact encrypted Windows disk.

**Timing constraint (2026-08-22).** The credential wipe cannot happen at the
end of cutover as first designed. On the keep-Windows path the recovery key is
needed later, by `settle-in`, to do the file pull — so the key (and the stick's
credential region) can only be scrubbed **after** `settle-in` finishes bringing
the files over. The wipe moves from end-of-cutover to end-of-settle-in-pull.
See `architecture.md`, Contracts.

**Mitigations planned.** Restricted ACLs on write, credential wipe at the
correct (later) moment, and telling the user plainly at intent capture.
**Closes when** those are implemented and verified, not merely designed.

## R14 — Supply chain · medium · open

**What.** A tool that runs as SYSTEM and repartitions disks is a high-value
target. Compromising a release binary owns every machine that runs it.

**Closes when.** Releases are reproducible, signed, and checksummed, and the
release process does not depend on a single unprotected credential.

---

# USB-only redesign · added 2026-08-19

The external drive was removed from the design: user files either ride the
stick (clean-slate path) or never leave the internal disk while Windows is
shrunk aside and kept until an explicit reclaim (safety-copy path). That
redesign retires the imaging risks and creates these.

## R15 — One-time UEFI boot handoff has never fired · critical · open (VM leg fired 2026-08-23)

**What.** Walk-away rests entirely on `bcdedit /set {fwbootmgr} bootsequence`
booting the stick exactly once. It has been tested on zero machines. Vendor
firmware is creative about removable-media entries: some ignore
`bootsequence` for USB devices, some re-enumerate and orphan the entry.

**If real.** Benign but total: the machine boots Windows, the user concludes
the tool did nothing. The product's core mechanism silently doesn't exist on
some fraction of hardware, and we don't know the fraction.

**VM leg fired (2026-08-23).** First evidence, on the QEMU+OVMF rig
(`rig/vm/`, firmware OVMF 2024.02): `Test-Handoff.ps1` records **`fired-once`**
— `bcdedit {fwbootmgr} bootsequence` one-time-booted the stick, the payload
ran and reset, Windows returned with no keypress, and the one-shot
self-cleared (`bootsequence clear: True`). Evidence in
`docs/validation-results/v0-handoff.csv`. Two bugs found and fixed getting
there, both of which would also have bitten the physical test:

1. **`startup.nsh` wrote its marker to the wrong volume.** The payload did a
   bare `fs0:`, which the UEFI Shell maps to the Windows ESP (also FAT), not
   the stick — so a *real firing* left no marker on the stick and the harness
   read it as a fail-safe. Any machine with an installed Windows has an ESP,
   so this was not VM-specific. Fixed to self-locate the stick by the volume
   that holds `startup.nsh`.
2. **The harness flagged its own test entry as a reorder.** `-Check` snapshotted
   the firmware boot order *before* deleting the temporary boot entry `-Arm`
   creates, so `after = before + our own entry` always looked like a reorder
   (`reordered`). Fixed to exclude the test `{guid}` from the comparison — a
   genuine firmware reorder of the real entries is still caught. (The
   pre-fix `reordered` row is left in the CSV, annotated, for the record.)

**Matrix progress (2026-08-23).** Baseline `fired-once` and `-FailMode NoFile`
→ `ignored` both pass on the rig (fires when it should, fails safe when the
payload is bad). The **Secure Boot rows (unsigned-refusal, signed-shim) cannot
run on this AMD/WSL2 rig**: SB-enforcing OVMF is the `.secboot` build, whose
QEMU firmware descriptor is `requires-smm`, and SMM crashes KVM on this host —
so the non-SMM OVMF we must use does not enforce SB (an unsigned shell booted
under the MS-keys varstore). The **BitLocker rows are also blocked here**: the
guest never detects a TPM (`Get-Tpm` → `TpmPresent: False`) even though QEMU
wires it correctly (`query-tpm` shows `tpm-crb` + emulator) and swtpm runs with
fresh state — the same non-SMM OVMF does not publish the TPM2 ACPI table (TPM
support, like SB enforcement, lives in the SMM-requiring `.secboot` build).
Rows 3–6 all move to the physical vendor matrix and the Hyper-V Gen 2 leg.

**Still open.** This rig validated only the SB-off, no-TPM paths (rows 1–2).
The close condition is unchanged: the Secure Boot and BitLocker rows on a host
that can run SMM-enabled OVMF (physical machines, or Hyper-V Gen 2), then
physical machines from **at least three vendors**, plus the Hyper-V leg that
`validation-results/README.md` also requires. A VM pass narrows R15; it does
not close it (CLAUDE.md rule #5).

**Closes when.** The spine spike (build order step 0) passes in a VM and on
physical machines from at least three vendors.

## R16 — Stick authoring can write the wrong device · critical · open

**What.** `evaluate` burns the live image with raw `\\.\PhysicalDrive`
writes. The user may have other USB devices attached.

**If real.** The tool destroys someone's data *before* the commit line — the
one failure mode the whole architecture exists to prevent, committed by the
component that promised to be safe.

**Closes when.** Device selection refuses non-removable buses, confirms size
and volume label with the user, refuses ambiguity outright — and the picker is
tested with multiple sticks and a USB hard drive attached simultaneously.

## R17 — Counterfeit or failing flash as the sole data carrier · high · open

**What.** On the clean-slate path the stick is briefly the only copy of the
user's files. Counterfeit sticks lie about capacity and silently discard
writes; cheap flash fails without warning.

**Scope narrowed (2026-08-22).** Only the clean-slate path stages user data to
the stick, and that path is now the opt-in / full-disk fallback, not the
default. On the default keep-Windows path the files never leave the internal
disk, so the stick is never the sole carrier. Impact-if-real stays high (data
gone after a wipe), but the population exposed to it shrank.

**If real.** Files verified as "staged" do not exist, discovered after the
wipe.

**Closes when.** The cutover's read-back checksum verification (which runs
before the commit line, while Windows still exists) is implemented as a hard
gate and demonstrated to catch a known-counterfeit stick.

## R18 — Windows shrink headroom is unmeasured · high · open

**What.** Immovable files — MFT, VSS store, pagefile, hiberfil — cap how far
`Resize-Partition` can shrink, often far short of free space. The
mitigations (disable pagefile/hibernation/system restore, reboot, retry) are
designed, not built.

**Partly addressed (2026-08-22).** The scanner now queries shrinkable space
via `Get-PartitionSupportedSize` (`Test-UpgDisk`) and reports it as `Room to
keep Windows`. Two constraints found on real hardware, both recorded here:

1. **The query needs Administrator.** Unelevated it returns "Access to a CIM
   resource was not available" — so the scanner caps this line the same way
   it caps BitLocker. This dents the V4 plan (see VALIDATION.md): "ship the
   scanner, measure the population for free" only yields shrink data from
   users who run elevated, which many won't. JSON reports should be filtered
   on `RanAsAdmin` before drawing conclusions.
2. **`SizeMin` includes immovable files** — which is exactly what we want (it
   *is* the real shrink floor), but it means a defragless machine reports a
   pessimistic number. The prologue's mitigations (disable pagefile/hiber,
   reboot) would raise it; the scanner reports the pre-mitigation floor and
   should say so.

**If real.** The safety-copy path is offered to machines that cannot deliver
it; the prologue fails late, after intent capture and hard confirmation —
recoverable, but exactly the walk-away-killing stop the design forbids.

**Closes when.** The safety-copy gate uses the shrinkable number (not free
space), verified on a fragmented real-world disk *with* the mitigations
applied, so the gate reflects achievable shrink rather than the cold floor.

## R19 — cryptsetup BITLK read is a new trust dependency · medium · open

**What.** The keep-Windows path (now the default) reads the BitLocker NTFS
volume via `cryptsetup` BITLK using the harvested recovery key, to copy the
user's files into Linux.

**Reframed (2026-08-22).** This risk used to read "unattended, on the only
copy, before the wipe" — the worst possible context. The design changed: the
read moved out of cutover and into `settle-in`, and on the keep-Windows path
Windows is never destroyed. So the read now happens (a) with the user present,
who can be asked about a stubborn unlock instead of the tool guessing; (b)
after the new Linux system has been verified working; and (c) with the Windows
partition fully intact as a backup — a failed read loses nothing, the user
reboots into Windows and retries. Same `cryptsetup` mechanism, far lower
stakes. Downgraded high → medium.

**If real.** Edge-case incompatibilities (key protector types, XTS variants,
used-space-only encryption) stop the copy — recoverable now, because Windows
is still there — or, the genuine remaining danger, read *wrong* data that then
checksums as what was (wrongly) read.

**Closes when.** Read-and-copy is verified against real BitLocker volumes
across Windows 10/11 defaults, both XTS-AES key sizes, and used-space-only
encryption; checksums are computed on the Windows side at harvest time (in
`evaluate`) so the Linux-side verify catches read corruption, not just copy
corruption. (Only the clean-slate path still reads user data before a
destructive step — and it does so inside Windows, where there is no BITLK
problem at all.)

## R20 — Browser profile porting is assumed, not verified · medium · open

**What.** The migration table promises Chrome/Edge bookmarks, history and
extensions transfer by copying the profile directory. Cookies and several
profile components are DPAPI-encrypted like the passwords are; version skew
between Windows and Linux builds can make the browser reset or refuse the
profile; extension state does not reliably survive a copy.

**If real.** "Your stuff silently didn't arrive" — the project's worst
failure shape, in the feature most users will check first.

**Closes when.** Real Windows→Linux profile transplants are verified per
browser and version pair, and `evaluate`'s claims are narrowed to what the
evidence supports.

## R21 — Installing alongside a shrunk Windows may not leave Windows bootable · critical · open

**What.** The keep-Windows path — now the **default** — installs Linux into
freed space and must leave the shrunk Windows fully bootable, because Windows
*is* the rollback and the file source. That depends on several things nobody
has tested together: reusing the existing Windows ESP without reformatting it,
fitting shim + GRUB + Fedora entries into an ESP that is often only 100 MB,
not clobbering `bootmgfw.efi`, and `os-prober` actually detecting the shrunk
Windows so it appears in the boot menu — all with Secure Boot enabled.

**Why it matters most now.** The whole safety-net promise of the default path
is "Windows is still here if anything goes wrong." If the alongside install
breaks Windows boot, that promise is false at the worst possible moment — the
user reaches for the fallback and it is gone. This is a harder problem than the
clean-slate wipe install, and the redesign made it the common case, not a
variant. It is the second-biggest project killer after the boot handoff (R15)
and had no entry until now.

**If real.** A "successful" conversion where the new Linux works but the kept
Windows will not boot: no rollback, no file source, and the user was
explicitly told they had both. Trust-ending.

**Closes when.** An alongside install is proven on real machines from several
vendors, Secure Boot on: Windows still boots from the menu afterward, GRUB
lists it, the shared ESP had room, and `bootmgfw.efi` is intact. This is the
keep-Windows half of the V1 gate and should be validated as its own Tier-1
item, not folded in as a "safety-copy variant". Fallback if it proves
unreliable on some firmware: those machines are steered to clean slate (which
never shares an ESP), and `evaluate` says so before committing.

---

# Resolved

Kept for the record — all four were in code that read correctly. All four were
also in code that had no tests: F1–F3 in the harvester, F4 in the app-risk
database. As of 2026-08-22 each is pinned by a self-test regression case
(harvester `-SelfTest` for F1/F3 and the truncation/SSID paths; scanner
`-SelfTest` for F4's "Microsoft Visual Studio Community 2022 must match").
F2 has no direct pin — it was an invocation bug in the netsh call itself,
which sits on the live side of the parse seam — but the seam now keeps the
parsing logic, where a silent empty result would hide, under test.

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
