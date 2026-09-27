# Known risks

This project has one asset: people can trust what it tells them. A scanner
that says "probably fine" and is wrong is worse than no scanner at all,
because the person believed it and lost their data.

So this file keeps the things we have not yet proven in plain sight, even
when that is uncomfortable. Every entry says what would really happen if the
risk is real, and what would close it. Nothing here is closed by a good
argument. Only evidence closes a risk.

Severity:

- **critical**: can give a confidently wrong answer on the very thing the
  tool exists to do, or can destroy someone's data
- **high**: wrong advice, or a part that fails silently
- **medium**: worse output, or a defect people would notice and report
- **low**: fragile inside, likely to become a bug later

Status is `open` unless a primary source (the original document, such as a
vendor datasheet or the kernel's own code) or a real machine has confirmed it.

A few words used all through this file:

- **the rig**: our test bench of virtual machines (Hyper-V and QEMU). A pass
  there proves the moving parts connect. It is not a real machine.
- **`pass-plumbing`**: the result word for "the plumbing works on the rig".
  It never closes a real-hardware question (CLAUDE.md rule #5).
- **a row**: one line in a results file under `docs/validation-results/`.
  Each real test leaves one.
- **the Aspire**: the Acer Aspire A515-51G, the laptop most physical rows
  in this file come from. **The G16**: the ASUS ROG Zephyrus G16, our first
  end-to-end test machine.

---

## R1: VMD detection has never fired · critical · open (desk half closed 2026-08-22; level-3 spoof fired 2026-08-26; AHCI-side real row and the one-click both-modes harness 2026-09-13)

**What.** Some Intel machines run their disk through Intel RST (Rapid
Storage Technology) or VMD (Volume Management Device), storage modes that
can hide the disk from a Linux installer. The scanner is meant to catch this.
Its detection was 10 hand-written PCI device IDs (the numbers a piece of
hardware reports about itself) plus a guessed `iaStorV*` service-name
pattern (`data/devices.ps1`, `Get-UpgVmdDeviceIds`). None of it has ever
matched real hardware.

**Checked against primary sources (2026-08-22).** The desk-research half of
the close condition is done. What it found justified the severity:

- **The ID list was checked against the Linux kernel's VMD driver table**
  (`drivers/pci/controller/vmd.c`, `vmd_ids[]`, mainline master, fetched
  2026-08-22) and the PCI ID repository (pci-ids.ucw.cz). Three of our ten
  IDs were wrong:
  - `8086:7ec0` is a **USB xHCI controller** (pci.ids: "Core Ultra 200
    Series Processors USB xHCI"). It would have given a false RST/VMD RED on
    current Intel laptops.
  - `8086:2010` and `8086:e0b0` are not Intel devices in pci.ids at all.

  Five VMD IDs from the kernel list were missing: `28c0, 4c3d, b60b, b06f,
  b07f`. `8086:09ab` is confirmed real (pci.ids "RST VMD Managed
  Controller"; Intel article 000088762 documents the 9A0B/09AB pair). Only
  Windows sees it, which is why it is absent from vmd.c and why we keep it.
  The list now cites a source for each ID. Cross-checked against
  linux-hardware.org probes: `9a0b` appears on 211 real devices and `a77f`
  on 152, both bound to the `vmd` driver, so the main IDs exist in the wild.
  `4c3d` shows 0 probes there but stays, on the kernel table's authority.
- **The `^iaStorV` pattern was wrong in the dangerous direction.** The RST
  driver's service name changed across generations:
  - iaStor / iaStorV (Vista-era, built into Windows)
  - iaStorA / iaStorAC / iaStorAVC (RST ~11-17, including the Skylake to
    Comet Lake "RST Premium" NVMe-remap mode: Intel article 000059291;
    Microsoft's Win10 1903 RST compatibility hold)
  - iaStorVD (RST 18+, the VMD generation: Intel article 000057787)

  `^iaStorV` matched only the first and last of those. It **missed the whole
  pre-VMD RST family, which is the 2015-2020 population this tool exists
  for.**
- **Detection now uses three signals** (`Test-UpgStorageMode`):
  1. the ID list, now matched to the kernel's;
  2. the `iaStorVD` service (catches VMD IDs we don't know yet);
  3. an Intel controller reporting PCI RAID class code `CC_0104` in its
     compatible IDs. That is the controller itself declaring RAID/remap
     mode (Microsoft, "Identifiers for PCI devices").

  An `iaStor*` service on a controller that is not RAID-class now warns
  instead of failing (RST software on an AHCI-mode controller; see R7 on
  over-refusal). The class-code mechanism was checked live on the G16: its
  NVMe controller reports `PCI\CC_0108` in `CompatibleID`, exactly the
  documented format, and correctly does not match.
- Six detection-level self-test cases now push made-up PnP entries through
  the real `Test-UpgStorageMode` (VMD ID, 09ab child, unknown ID with
  iaStorVD, CC_0104 RAID class, RST-on-AHCI warn, standard NVMe ok). All
  pass, but they are still synthetic.

**Level-3 spoof fired (2026-08-26).** A spoof is a fake device we build to
test the pipeline. The main check now fires through the *real* Windows PnP
-> WMI -> scanner pipeline, not just made-up objects. On the QEMU rig
(`rig/vm/`), a patched `pci-testdev` (build-qemu-vmd.sh; QEMU 8.2.2) shows
PCI `8086:9a0b` class `0104`. Windows lists it as an unknown **RAID
Controller**: instance `PCI\VEN_8086&DEV_9A0B&SUBSYS_00008086&REV_00`,
CompatibleIDs including `PCI\CC_010400` and `PCI\CC_0104`, no driver bound,
so the device Status is `Error`. The source scanner and the built `dist/`
both fired the same way: `[FAIL] Storage controller mode`, detail `Intel
RST / VMD active (RAID Controller)`, verdict RED. The device matched on
signal 1 (the `9a0b` ID from the kernel-matched list). Signal 3 (RAID class
`CC_0104` in CompatibleIDs) is present too, as a backstop. It was captured
hardware-only with `-DumpMachine` and kept as the permanent synthetic
regression test `evaluate/windows/corpus/vm-qemu-q35-vmd-spoof-9a0b.json`
(`Synthetic: true`). It replays green on every `-SelfTest`, next to the G16
entry.

**This is plumbing, not evidence** (CLAUDE.md rule #5). The VM shows IDs we
built from our own model of the hardware. So the run proves the path
(enumerate -> parse -> verdict) works end to end for a device that declares
these IDs. It proves nothing about what real RST hardware shows. The
real-hardware clause below is untouched, and this capture must never be
cited against it.

**The negative direction on real Intel RST silicon (2026-08-27, re-read
2026-09-13).** The Aspire (Kaby Lake-R, Sunrise Point-LP PCH), in its
firmware's AHCI mode, shows `PCI\VEN_8086&DEV_9D03`, CompatibleIDs
`PCI\CC_010601` / `PCI\CC_0106`, with Acer's RST driver **iaStorAC** bound
to it. That is exactly the case the R7 guard was written for: `[WARN] Intel
RST driver present, controller not in RAID mode (iaStorAC)`, verdict
YELLOW, **not** `[OK]`. So on this machine the AHCI-side row is
`warn-rst-on-ahci`.

The scanner's note "this combination has not been confirmed on real
hardware yet" is no longer true in one respect. The Fedora installer booted
through the handoff on this same machine and found the disk by serial
(`v1-live-boot.csv` row 4, 2026-09-12). So with iaStorAC on an AHCI-class
controller, Linux does see the disk. The status stays `warn`: one machine is
one data point. The capture is `corpus/acer-aspire-a515-51g.json` (Expected
`warn`). The row is `v5-controller-mode.csv` row 1, written by
`rig/v5-verdict.py` from the 2026-09-13 report and capture.

**The positive direction: built as one click, fired on the rig (2026-09-13,
UTC 09-14).** The Aspire has no VMD (that is 11th gen and newer). But its
setup screen may offer "Intel RST Premium with Intel Optane". In that mode
the same controller shows up as `8086:282A`, class code `0104`: the third
signal, never fired on real hardware.

Flipping a SATA mode by hand takes four `bcdedit` lines (`bcdedit` is
Windows' tool for editing its boot settings), two Safe Mode boots and
tapping a setup key at the right moment. So the visit was made one click:

1. `RUN-STORAGE-MODE.cmd` -> `evaluate/windows/Test-StorageMode.ps1` scans
   (leg 1).
2. It copies the Windows boot entry with `safeboot minimal`, takes the copy
   out of the boot menu, and boots it exactly once through the boot
   manager's one-time `bootsequence`. Safe Mode loads every installed
   storage driver, so the changed controller gets its driver bound. That is
   the documented way round `INACCESSIBLE_BOOT_DEVICE`.
3. It registers the SYSTEM startup task the prologue uses (R24), sets a
   `*`-prefixed RunOnce entry that only acts in Safe Mode, and restarts
   **straight into the firmware setup** (`shutdown /r /fw`; a plain restart
   if the firmware refuses).
4. The person changes SATA Mode there and saves. Windows boots the copy in
   Safe Mode. The person signs in and it restarts by itself.
5. The resume scans as SYSTEM before anyone signs in (leg 2), asks for the
   original mode back, and does it all once more (leg 3).

Every way out removes the copy, the sequence, the RunOnce and the task, and
the record lands on the stick. What the rig taught (`rig/hyperv/prologue.sh
storage-mode`, runs 4-5; the record is in the harness's
`storage-mode.json`):

- `bcdedit /copy` also adds the copy to the boot menu. That gives a
  two-entry, 30-second menu on every boot until cleanup. The harness removes
  it from `displayorder`; `bootsequence` boots it anyway.
- Windows accepts `shutdown /r /fw` on Hyper-V. The firmware honours it by
  stopping at "No boot devices were found" (it has no setup screen). The
  flag works once. A real firmware opens its setup instead. The harness
  records which restart it used (`fw` / `plain`).
- **Task Scheduler does not run a SYSTEM boot-trigger task in Safe Mode**,
  not even with a `SafeBoot\Minimal\Schedule` entry (run 5 sat at the Safe
  Mode sign-in for 42 minutes). The `*`-prefixed RunOnce restarts the
  moment someone signs in (runs 4 and 5). It leaves a marker (UTC time, who,
  the SafeBoot option, uptime) that the next resume adds to the record. So
  the Safe Mode sign-in is the person's one extra step per mode change. The
  Schedule idea is dropped.
- The SYSTEM resume on the normal boot: session 0 (the background session
  with nobody signed in), unattended, 7 s after boot, the stick seen after
  6 s, leg 2 scanned, cleanup read back from the guest (no task, no
  RunOnce, no safeboot entry, no bootsequence).
- Two rig traps, for the record:
  - PowerShell 5.1 turns a native command's error output into a fatal error
    under `$ErrorActionPreference = 'Stop'`. Shutdown's Win32 error 203
    killed run 3 before the fallback could run. Every native call now goes
    through a helper that reads the exit code.
  - Hyper-V gives the guest the host's local time as its hardware clock.
    The time-sync service pulls it back hours at a later boot, and Task
    Scheduler then holds every boot-trigger task until real time catches
    up. The bench sets the guest clock first.

A VM has no SATA mode to flip. So the rig's rows in `v5-controller-mode.csv`
are `no-intel-controller` with `flow_result` `mode-unchanged`: plumbing for
the flow, nothing for the check (rule #5).

**Two physical runs of the one-click flow (2026-09-15, the Aspire, rows
6-9 of `v5-controller-mode.csv`).** Both ended `mode-unchanged`. The setup
screen was never reached, so the SATA mode never changed and the RAID row is
still owed. What the runs proved on real firmware, from the harness's own
record:

- The copied boot entry boots Safe Mode exactly once (both runs).
- The `*`-RunOnce at the Safe Mode sign-in wrote its marker (option 1, 126 s
  after boot) and restarted (run 2). In run 1 the person cut the Safe Mode
  boot short with the power button. That works too: the driver binding
  happens before the sign-in.
- The resume ran as SYSTEM in session 0, 27 s and 21 s after boot, with the
  stick seen after 3 s and 2 s. Leg 2 scanned before anyone signed in.
- Everything armed was removed. Run 1 read back clean. Run 2's stick record
  ends before the cleanup line (stick pulled or dropped), and the next boot
  finishes it.

And two firmware findings:

- **InsydeH2O V1.21 refused `shutdown /r /fw` with Win32 error 203 in run 1
  and accepted it in run 2, then booted straight on both times.** This
  firmware ignores the boot-to-setup request. So on Acer the setup key (F2,
  tapped from the moment the screen goes dark) is the only way in, and the
  person did not catch it either time.
- **Windows 11 Home shows no power icon on the Safe Mode sign-in screen**,
  so the harness now says "hold the power button".

Both scans in both runs: `8086:9d03`, class `0106`, iaStorAC, `warn`. That
is the AHCI-side row four more times. The Aspire's overall verdict is RED
because of its dying drive (R18), which has nothing to do with this check.

**Still open: the half that needs hardware.** The check has never fired on a
real machine in RAID/RST mode. Whether the Aspire's setup even shows SATA
Mode is unknown until someone reaches its Main tab (F2 at power-on; Acer
hides the option on some models until Ctrl+S).

- If it is there, one more run of `RUN-STORAGE-MODE.cmd` with F2 tapped at
  each restart gives the row: expected `8086:282a`, `CC_0104`, `iaStorAC`
  or `iaStorAVC` bound, `[FAIL]`, RED. That is `fail-fired` on signal 3, the
  **pre-VMD RST clause**.
- If it is absent, the Aspire is done and the row needs another vendor.

Either way VMD proper is untouched. An 11th-gen-or-newer machine with RST on
(kernel `vmd.c` IDs, `iaStorVD`) ships that way, so its first scan is the
FAIL row with no firmware change at all. V5 is Tier 4 and blocks nothing.
Two full runs of the flow on the Aspire is enough of an owner's time
(decided 2026-09-15: skip unless the one-minute F2 look finds the option).
The only other machine on hand, the G16, is AMD with standard NVMe and
cannot test the positive path.

**Why it matters most.** This is the flagship check. The README calls it
"the single most common false 'Linux won't install'". If the IDs are wrong,
the scanner prints `[ OK ] standard AHCI / NVMe - visible to Linux
installers` on exactly the machines it was written to catch.

**If real.** A false GREEN on the highest-stakes check. The person backs up,
wipes, boots the installer, and it shows no disks at all. That is the exact
failure the tool promised to prevent, and now Windows is already gone.

**Closes when.** The ID list is checked against a primary source (Intel
datasheets, the kernel's `drivers/pci/controller/vmd.c` ID table, or
linux-hardware.org probes from RST-enabled machines), **and** the check fires
correctly on at least one machine with RST enabled.

## R2: Single-machine test corpus · high · open

**What.** Everything tested end to end was tested on one laptop: the ASUS
ROG Zephyrus G16 (Ryzen AI 9 HX 370, RTX 4060, MediaTek MT7925). Every
`fail` path is synthetic. It has only been run by `-SelfTest` with made-up
check objects.

**Not yet tested on real hardware:** Broadcom Wi-Fi refusal, ARM refusal,
free-space refusal, MBR partition-limit warning, the BitLocker-enabled path,
cloud-only placeholder detection (see R8), and every vendor quirk.

**If real.** Unknown. That is the problem: for most of the code we have no
evidence either way.

**Closes when.** Reports exist from a spread of machines: at least one Intel
laptop, one with Broadcom Wi-Fi, one pre-2015 machine, one with BitLocker on,
one Surface. This is the best single argument for shipping the scanner early
and asking for reports.

**Mechanism built (2026-08-22).** The scanner now has `-DumpMachine` (a
hardware-only capture) and a corpus replay in `-SelfTest`. Every curated
capture in `evaluate/windows/corpus/` is replayed through the pure detection
checks on every run. The G16 is the first corpus entry. This is how R2's
machines stay covered once reached: each machine met once is tested again
on every run, forever (CLAUDE.md rule #5). The risk itself stays open until
the spread of machines above really exists in the corpus.

## R3: Distro kernel table is unverified and stale · high · open

**What.** `data/distros.ps1` marks Fedora (6.14) and Pop!_OS (6.9) with
`Approx=$true`, meaning nobody checked them against release notes. Ubuntu
26.04 LTS is missing entirely.

**Why it matters.** This table produces the report's headline claim. The
scanner tells people "RULED OUT for shipping an older kernel: Linux Mint,
Ubuntu LTS, Pop!_OS" with total confidence, from data marked as a guess.

**If real.** If Ubuntu 26.04 LTS ships a new enough kernel, we steer people
away from the most popular, best-supported option for no reason. The
recommendation then falls back to openSUSE Tumbleweed and Arch, which the
same table rates as unsuitable for newcomers.

**Closes when.** Every entry is confirmed against the distribution's own
release notes, `Approx` is cleared, `$script:UpgDistroTableVerified` is
updated, and Ubuntu 26.04 is added or explicitly left out with a reason.

## R4: Scanner and converter contradict each other · medium · code done, verify pending · design decision

**What.** The scanner's free-space check says "Not enough free space to
install Linux **alongside** Windows" and uses 25/60 GB dual-boot thresholds
(`evaluate/windows/upgrade-scan.ps1`, `Test-UpgDisk`). The converter lists
dual-boot as an explicit non-goal (something it will not do) and replaces
Windows entirely (when this was written, through an external-drive design
since removed; now through the USB-only paths).

**If real.** The scanner answers a question the product no longer asks. A
person passes the free-space check, starts the converter, and is refused for
a reason the scanner never mentioned.

**Closes when.** A decision is made and both sides reflect it. Either the
scanner stays a general, dual-boot-aware tool and the converter states its
extra requirements up front, or the scanner becomes the converter's front
end and its disk advice is rewritten around external-drive capacity. It
cannot quietly be both. **Decide before the WPF app wraps around it.** This
gets expensive to undo afterwards.

**Partly addressed.** The module split (`docs/architecture.md`) puts the
scanner inside `evaluate`, which owns all refusals. That fixes who owns it,
but not the wording.

**Decided (2026-08-19).** The scanner keeps two personalities in one
codebase. Standalone, it is a general advisory tool that recommends across
distributions. Embedded, it is the converter's `evaluate` mode, with the
converter's own gates. The free-space check survives with a new meaning under
the USB-only design: shrinkable space gates the safety-copy path, and stick
capacity gates the clean-slate path.

**Wording reworked (2026-08-22).** `Test-UpgDisk` no longer gives the
dual-boot free-space fail or the "Backup drive needed" external-drive line.
It now reports `Disk in use` (info) and `Room to keep Windows`, the
shrinkable-space measurement from `Get-PartitionSupportedSize`, which gates
the safety-copy path. The "BEFORE YOU DO ANYTHING" block no longer tells
anyone to buy an external drive. Low free space is no longer a RED trigger:
under USB-only, a full disk just means clean-slate only, not "cannot
convert".

**Still open** until the numbers are right, not just the words. The
clean-slate gate needs the harvester's folder sizing to answer "does your
data fit an N GB stick", which the scanner alone cannot work out. The
scanner now informs; `evaluate` still has to gate. Downgraded to medium: the
dangerous contradiction (telling people to buy a drive the product doesn't
use) is gone.

## R5: Single-user assumption · high · open

**What.** `Harvest-UpgradeState.ps1` collects only the current user's
folders, browser profiles and account details.

**If real.** A family computer with three accounts moves one and silently
leaves the other two behind. The conversion reports success. The other two
people lose everything, on a machine that no longer has Windows on it.

This is a data-loss bug aimed straight at the "anybody can do this"
audience, who are the most likely to share a machine.

**Closes when.** The harvester lists all local profiles, sizes them, and
either moves all of them or refuses multi-user machines outright. Refusing
is an acceptable v1 answer. Silently moving one is not.

**Partly built (2026-09-26, harvester 0.3.0 / job writer 0.10.0).**

- The folder map lists every other profile on the machine (local, domain
  and Microsoft Entra accounts; `Win32_UserProfile`, not special).
- The job writer **refuses a clean-slate job** when any exist, and names
  them, since the wipe would delete their files.
- A keep-Windows job is still written (their files stay in the kept
  Windows), and the job writer says plainly that their files are not in it.
  The reclaim in `settle-in` will have to refuse on the same fact.
- The same step closes a second way to take the wrong person's files: an
  elevated run under **another account**. (UAC on a standard account asks
  for an administrator's password, and the elevated process then reads the
  administrator's folders.) The job writer compares the elevated account
  with the owner of the desktop in its session, by SID, and refuses when
  they differ or no desktop owner is found.

Still open: whether a Windows leftover such as `defaultuser100000` (found on
the Aspire, 2026-09-26) should count. See R26's measurement.

## R6: Truncated sizing undersizes the backup · medium · open

**What.** `Get-HarvestFolderStats` stops at 250,000 files and sets
`Truncated=$true`. The backup estimate is then too low.

**If real.** The path gate and the gap report are worked out from a number
that is too small. A machine is steered onto the clean-slate path with files
that do not really fit the stick. That is found out at staging, or worse,
trusted.

**Closes when.** The Phase A UI treats `Truncated` as a hard blocker rather
than a note, or sizing is made exact for the folders that feed the estimate.

**Refusal built (2026-09-26, job writer 0.10.0), and a sibling found.** The
job writer refuses any folder that hit the cap. Building it turned up a
second way to size low that nothing counted: `Get-ChildItem -Recurse
-ErrorAction SilentlyContinue` silently skips a sub-folder Windows will not
list. The prologue's staging copies with the same call, so those files would
also be left off the stick. The harvester now counts such folders
(`Unreadable`, with the first paths) and the job writer refuses on any.

Checked on real Windows PowerShell 5.1 (G16, 2026-09-26) before relying on
it:

- A recursive listing does **not** follow junctions or directory symlinks.
  (A link to a whole other tree is not counted. That is correct: it is not
  the person's folder.)
- It **does** walk OneDrive's cloud directories, which are reparse points
  too.

Self-tests use a real junction and a real folder with a deny rule. On the
G16 and the Aspire the count was 0 in all six folders.

## R7: Broadcom vendor fallback over-refuses · medium · open

**What.** The vendor fallback returns `fail` for *every* Broadcom device it
does not recognise (`Get-UpgWifiVendorFallback`, `14e4`).

**If real.** Some Broadcom parts work well enough with `b43` or `brcmfmac`.
We tell those people their machine needs a new card. Over-refusal is the
safer way to be wrong, but "refuse by default" is meant to mean *refuse when
we don't know*, not *claim a failure we haven't shown*.

**Closes when.** The fallback tells "known bad" apart from "unknown
Broadcom, bring a USB Ethernet adapter just in case", and common working
Broadcom IDs are added to the exact-match table.

## R8: Cloud-only detection positive path untested · medium · open (materialization built; cfapi leg fired 2026-09-08)

**What.** OneDrive can keep a file "online-only": on disk there is just a
placeholder (a stub with the name and size, but no contents). Placeholder
detection checks `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` and
`FILE_ATTRIBUTE_OFFLINE`. On the test machine it returned 0, and a separate
check confirmed there really are none. The code ran; it has never found
anything.

**If real.** OneDrive placeholder files copy as empty. The person's photos
seem to move across and are 0 bytes on the other side, found out long after
Windows is gone. It is sharper under the keep-Windows default: `settle-in`
pulls files by reading the mounted NTFS *from Linux*, which has no OneDrive
client. So a placeholder that was not downloaded beforehand cannot be filled
at pull time, only copied empty.

**Design response (2026-08-22).** `evaluate` must *materialize*
placeholders (force the download to real bytes while Windows is still
alive), not just detect them, because no later stage can. This is now stated
in `architecture.md` (evaluate's harvest duties) as a hard step: materialize,
or refuse.

**Detection arm tested (2026-08-22).** The harvester's `-SelfTest` now sets
a genuine `FILE_ATTRIBUTE_OFFLINE` on a real NTFS file and confirms
`Get-HarvestFolderStats` counts it as cloud-only. That is the real attribute
read through the real filesystem, not a made-up object. What was left: the
`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` arm as the real OneDrive cloud filter
sets it (only the filter can), and materialization itself.

**Materialization built and its plumbing fired (2026-09-08).** The
harvester now has `-Materialize`:

- Every placeholder under the user folders is pinned (`attrib +P -U`,
  "always keep on this device", so the client will not turn it back into a
  stub before settle-in pulls it).
- It is read through to the end, with a timeout.
- It is judged on three facts: placeholder attributes gone, every byte
  read, bytes allocated on the volume (`GetCompressedFileSize`). All three,
  or it is **not** materialized. One failure makes the harvest **refuse**
  (exit 3, no job).

The harness (`evaluate/windows/Test-Materialize.ps1`) is a real sync
provider on the Cloud Files API (cfapi, the Windows API OneDrive is built
on). It creates genuine placeholders with known true bytes and refuses to
serve one of them. The harvester runs in a separate process, and the bytes
on NTFS are hashed afterwards.

`pass-plumbing` on the rig (Windows 10 19045) and the G16 (Windows 11
26200): 6/6 servable files byte-identical and fully allocated, the refused
one reported failed, exit 3 (`docs/validation-results/v8-materialize.csv`).
The `RECALL_ON_DATA_ACCESS` arm is now exercised by the real filter (attrs
`0x401620` before, `0x80420` after: pinned, reparse point, no recall bit).
The pinned/unpinned bits are confirmed **not** part of detection (a
self-test case locks that in). Two cfapi facts learned: parent directories
must themselves be placeholders, and a placeholder's name is bare, relative
to its own directory.

**In the job (2026-09-26, job writer 0.10.0).** The job writer reads the
folder map and **refuses** any job while online-only files remain: found and
not downloaded, a download that failed, or files online-only again when read
after the download. The download itself is the job writer's `-Materialize`,
which no launcher passes yet. It changes the machine (it downloads, and pins
"always keep on this device"), and the launchers write the job *before* the
typed CONVERT, so it waits for consent text the owner approves. Measured
2026-09-26: 0 online-only files in the six folders on the G16 and on the
Aspire (whose Desktop, Documents and Pictures are redirected into a
work-or-school OneDrive).

**Decided (2026-09-26, the owner's call): online-only files are not
downloaded; `settle-in` reconnects OneDrive.** Their bytes already live in
OneDrive, so leaving them there loses nothing. The loss this risk names was
only ever copying the 0-byte stub as if it were the file. Job writer 0.11.0
no longer refuses on them. It records `cloud_files.result = left-in-cloud`
and the count per folder, and says at job time that they stay in OneDrive.

The risk moves to `settle-in`, with its own unproven residue:

- **How a placeholder looks through ntfs-3g from Linux** (a reparse point,
  an empty file, an error? Never read). settle-in must recognise it or
  refuse the pull, and match the job's count.
- Whether a **work-or-school OneDrive** (the Aspire's is a university's)
  lets a third-party Linux client sign in at all (the web always works).
- The prologue's clean-slate staging, which copies with a plain read. On
  Windows that downloads a placeholder or fails. It should skip them the
  same way (owed before the discard offer).

The materializer stays built and proven at plumbing level, unused.

**Closes when.** `Test-Materialize.ps1 -OneDrive` passes against a signed-in
OneDrive with Files On-Demand. That is the part no test provider can stand
in for (CLAUDE.md rule #5): the client's own stub/download behaviour, its
response to `attrib +P`, and a fetch stalled by the network. The harness runs
that leg. It uploads a few MB to the account and is only ever run on a
machine the person owns.

## R9: No CI; dist can drift from source · medium · open

**What.** `dist/upgrade-scan.ps1` is committed so people can download one
file. Nothing makes sure it matches `data/` and `evaluate/`. The self-test
never runs automatically. (CI, continuous integration, means tests that run
by themselves on every change.)

**If real.** A contributor edits `data/devices.ps1`, forgets `./build.sh`,
and the published scanner runs old data while the source looks correct.

**Closes when.** A GitHub Action runs `./build.sh`, fails if `dist/`
changes, and runs `-SelfTest` on every PR.

## R10: NVIDIA detection matches rendered display text · low · open

**What.** `Get-UpgRecommendation` decides whether NVIDIA is present with
`$_.Detail -match 'NVIDIA'` across all checks. It matches formatted output
rather than structured data.

**If real.** Not wrong today. But `NVIDIA High Definition Audio` exists as a
device on the test machine. The moment any check prints device names into
`Detail`, distro recommendations change silently.

**Closes when.** Checks carry a structured vendor field and the
recommendation reads that instead of display text.

## R11: Secure Boot check verifies nothing · low · open

**What.** The check reports `ok` when Secure Boot is on and lists distros
known to work. It never confirms that this firmware will accept the *chosen*
distribution's shim (the small signed loader that lets Linux boot with
Secure Boot on).

**Closes when.** The converter checks the target's signed shim against the
machine before committing, or the check's wording drops the implied
promise.

---

# Non-code risks

## R12: Code signing and antivirus · critical · open

**What.** The finished converter runs as administrator, reads BitLocker
recovery keys, exports Wi-Fi passwords in plain text, images a disk, and
rewrites boot settings. In behaviour, that is an exact match for an
infostealer (malware that steals passwords) followed by ransomware.

(From 2026-09-13 the prologue also registers a SYSTEM startup task and an
HKLM `RunOnce` entry, to resume with nobody signed in. Those are textbook
signs antivirus looks for in malware that wants to survive a restart. That
exposure is R24's, with its evidence there.)

**If real.** Unsigned, SmartScreen shows "Windows protected your PC" and
Defender may quarantine it outright. The target person (non-technical,
careful, warned their whole life about exactly this) stops there for good.
No amount of interface work makes up for it.

**Closes when.** An OV or EV code-signing certificate is obtained (it needs
a legal entity and a few hundred dollars a year), releases are signed and
reproducible, and the binary is sent to Microsoft's malware-analysis portal
for allowlisting.

**Start early.** Reputation builds with elapsed time and number of
installs, so the certificate is worth having before there is anything to
sign.

## R13: The USB becomes a credential store · medium · open

**What.** `evaluate` writes secrets to the stick so the Linux side can use
them: Wi-Fi passwords in plain text (to recreate connections) and, under
the keep-Windows default, the **BitLocker recovery key** (so `settle-in` can
unlock and read the Windows partition).

**If real.** Anyone who picks up the stick has the person's home and work
Wi-Fi, and worse, the key to their still-intact encrypted Windows disk.

**Timing constraint (2026-08-22).** The credential wipe cannot happen at the
end of cutover as first designed. On the keep-Windows path the recovery key
is needed later, by `settle-in`, to do the file pull. So the key (and the
stick's credential area) can only be wiped **after** `settle-in` finishes
bringing the files over. The wipe moves from end-of-cutover to
end-of-settle-in-pull. See `architecture.md`, Contracts.

**Decided (2026-09-26, the owner): Wi-Fi passwords are harvested and
applied by `settle-in` on first startup** (`architecture.md`, "The clock,
Wi-Fi and the old boot entry"). To shorten the time they sit on the stick,
the installer's last step moves them from the stick into a root-only folder
on the installed system and deletes them from the stick. `settle-in`
applies them and deletes that copy. Credentials on the stick then live from
the harvest to the end of the install (minutes, with the stick in the
machine) rather than until someone remembers to wipe it.

**Mitigations planned.** Restricted file permissions on write, the
credential wipe at the correct (later) moment, and telling the person
plainly when they choose what to do. **Closes when** those are built and
verified, not just designed.

## R14: Supply chain · medium · open

**What.** A tool that runs as SYSTEM and repartitions disks is a valuable
target for attackers. Whoever tampers with a release binary owns every
machine that runs it.

**Closes when.** Releases are reproducible, signed and checksummed, and the
release process does not depend on a single unprotected credential.

---

# USB-only redesign · added 2026-08-19

The external drive was removed from the design. Now the person's files
either ride on the stick (the clean-slate path), or never leave the internal
disk while Windows is shrunk to one side and kept until the person
explicitly reclaims the space (the safety-copy path). That redesign retires
the disk-imaging risks and creates the ones below.

## R15: One-time UEFI boot handoff has never fired · critical · open (VM leg fired 2026-08-23)

**What.** Walking away rests entirely on one command,
`bcdedit /set {fwbootmgr} bootsequence`, booting the stick exactly once. It
asks the firmware (the machine's built-in startup software, UEFI) to start
from the stick on the next boot only. Vendor firmware is creative about
removable-media entries: some ignore `bootsequence` for USB devices, some
re-scan and lose the entry. Untested on any physical machine until
2026-09-08, when the first one fired (below).

**If real.** Harmless but total. The machine boots Windows and the person
concludes the tool did nothing. The product's core mechanism silently
doesn't exist on some share of hardware, and we don't know how big that
share is.

**VM leg fired (2026-08-23).** First evidence, on the QEMU+OVMF rig
(`rig/vm/`, firmware OVMF 2024.02). `Test-Handoff.ps1` records
**`fired-once`**: `bcdedit {fwbootmgr} bootsequence` booted the stick one
time, the payload ran and reset, Windows came back with no keypress, and the
one-shot cleared itself (`bootsequence clear: True`). Evidence in
`docs/validation-results/v0-handoff.csv`. Two bugs were found and fixed on
the way. Both would also have bitten the physical test:

1. **`startup.nsh` wrote its marker to the wrong volume.** The payload did a
   bare `fs0:`, which the UEFI Shell maps to the Windows ESP (the small EFI
   System Partition the firmware boots from; also FAT), not the stick. So a
   *real firing* left no marker on the stick, and the harness read it as a
   fail-safe. Any machine with Windows installed has an ESP, so this was not
   VM-specific. Fixed: it now finds the stick by the volume that holds
   `startup.nsh`.
2. **The harness flagged its own test entry as a reorder.** `-Check` took
   its snapshot of the firmware boot order *before* deleting the temporary
   boot entry `-Arm` creates. So `after = before + our own entry` always
   looked like a reorder (`reordered`). Fixed: the test `{guid}` is left out
   of the comparison. A genuine firmware reorder of the real entries is
   still caught. (The pre-fix `reordered` row stays in the CSV, with a note,
   for the record.)

**Matrix progress (2026-08-23).** Baseline `fired-once` and
`-FailMode NoFile` -> `ignored` both pass on the rig. It fires when it
should and fails safe when the payload is bad.

- The **Secure Boot rows (unsigned-refusal, signed-shim) cannot run on this
  AMD/WSL2 rig.** OVMF that enforces Secure Boot is the `.secboot` build.
  Its QEMU firmware descriptor is `requires-smm`, and SMM crashes KVM on
  this host. So the non-SMM OVMF we must use does not enforce Secure Boot
  (an unsigned shell booted under the MS-keys varstore).
- The **BitLocker rows are blocked here too.** The guest never detects a TPM
  (`Get-Tpm` -> `TpmPresent: False`), even though QEMU wires it correctly
  (`query-tpm` shows `tpm-crb` + emulator) and swtpm runs with fresh state.
  The same non-SMM OVMF does not publish the TPM2 ACPI table (TPM support,
  like Secure Boot enforcement, lives in the SMM-requiring `.secboot`
  build).

Rows 3-6 all move to the physical vendor matrix and the Hyper-V Gen 2 leg.

**Hyper-V leg fired rows 3, 5, 6 (2026-08-30).** `rig/hyperv/`, Gen 2
guest, real Secure Boot + vTPM (a virtual security chip), stick as a SCSI
VHDX (no USB emulation, so the removable-media clause stays physical).
Three rows in `v0-handoff.csv`:

- **Row 3** (`SecureBootUnsigned`, Secure Boot on): **`ignored`**. The
  firmware refused the unsigned payload silently and fell through to
  Windows, boot order intact. The fail-safe half of V0 holds under real
  Secure Boot.
- **Row 5** (BitLocker on, suspension armed, Secure Boot off per run-book):
  **`fired-once`**. The payload ran off the stick, the one-shot cleared
  itself, no recovery prompt, protection resumed by itself.
- **Row 6** (`NoSuspend`): **`fired-once` with NO recovery prompt**. This
  was the pre-registered finding, recorded word for word. The one-shot
  fires and *resets*, and the next Windows boot is a normal one. So the
  PCRs (the TPM's measurements of what booted) are unchanged at unseal
  time, and the TPM unseals. On this firmware, suspension is **not**
  needed to protect against the handoff itself. It guards the prologue's
  other changes (and the Secure Boot toggle, observed). Whether any vendor
  firmware measures the attempted one-shot into a sealed PCR stays with the
  physical matrix. The prologue keeps suspending anyway (cheap, and the
  cautious default).

Row 4 (signed shim) cannot be run meaningfully here. The `MicrosoftWindows`
db refuses shim at the firmware, and the row cannot tell that apart from
"shim ran and rebooted". It is left to firmware whose db holds both CAs.

**Still open.** This rig proved only the Secure-Boot-off, no-TPM paths
(rows 1-2). The close condition is unchanged: the Secure Boot and BitLocker
rows on a host that can run SMM-enabled OVMF (physical machines, or Hyper-V
Gen 2), then physical machines from **at least three vendors**, plus the
Hyper-V leg that `validation-results/README.md` also requires. A VM pass
narrows R15. It does not close it (CLAUDE.md rule #5).

**Decided (2026-09-07): harness 0.2.0, before the first physical row.**
Reviewing the harness for the Acer Aspire A515-51G run found three gaps a
real machine would have hit. All are fixed and locked in by the harness's
new `-SelfTest`:

1. **Home editions have no `Get-BitLockerVolume`.** The BitLocker
   PowerShell module ships on Pro/Enterprise/Education only. A Home machine
   with Device Encryption (BitLocker under another name) *on* read as
   `unknown`, and 0.1.0 then armed without suspending and without a
   warning. 0.2.0 falls back to `manage-bde -status C:` (present on every
   edition; English "Protection Status" line only, so translated output
   stays `unknown`).
2. **Unknown BitLocker state now refuses to arm**, and so does BitLocker on
   without `-SuspendBitLocker` (unless `NoSuspend` *is* the experiment). No
   flag overrides either. Make the state known instead. The prologue
   inherits this refusal (`architecture.md`, prologue step 4).
3. **The shim payload records itself.** 0.1.0's signed row depended on a
   human seeing the reboot, and the harness then classified it `ignored`.
   Now `grub.cfg` does `save_env upg_fired` into a pre-created
   `EFI/BOOT/grubenv` on the stick, and `-Check` reads it like `fired.txt`.
   `-Arm` resets the block first. **Fired on the QEMU rig the same day**
   (Secure Boot off, the only mode that rig runs): the firmware started the
   entry, shim loaded the install-media GRUB, `save_env` rewrote `grubenv`
   on the FAT stick (`upg_fired=1` read back from Windows), the one-shot
   cleared, and `-Check` classified `fired-once` from `grubenv`, with no
   human answer needed. GRUB printed a harmless `bli.c` partition-UUID error
   (MBR stick) and shim a fallback-to-default-loader line. Both are in the
   row. Plumbing only: whether a firmware that *enforces* Secure Boot lets
   the same `save_env` through is the physical shim row's job.

The 0.2.0 baseline (Shell stick) was re-run on the same rig as a regression
row: `fired-once` via `fired.txt`. So the harness changes did not disturb
the path the earlier rows proved. Both rows are in `v0-handoff.csv`.

**Harness 0.3.0: the one-click (fully managed) flow, VM leg fired
2026-09-07.** The user philosophy is a managed experience: plug in, one
double-click, one "Yes", walk away. 0.3.0 makes the handoff test work that
way:

- `-Arm -Auto` registers a one-shot **elevated logon task** and reboots.
- On return the task **runs `-Check` itself**. It classifies the result,
  removes its own task and the boot entry, asks the single human fact (was
  a key needed?) in a popup that **times out to `unknown`**, and writes the
  row to the stick.

This is the prologue's own "walk away, clean up on return" shape, built here
first (rule #4: the writers come later, but this reversible boot-config
piece belongs to them). One stick now carries both payloads (`EFI/BOOT`
signed shim, `EFI/SHELL` unsigned shell), so nothing is re-flashed between
rows. Fired on the QEMU rig: `fired-once`, `mode=auto`, unattended, the
popup timing out to `unknown`, which is correct when no one answers.

The only human steps a machine cannot remove: the one UAC "Yes" (never
bypassed) and, for the *unsigned* matrix rows only, toggling Secure Boot in
firmware (no API exists). Neither touches the product's signed one-click
path. Six new self-test cases lock in the payload-path and stick-relocation
logic (the stick can come back under a different drive letter; the check
finds it by volume id). Plumbing only until a physical machine runs it.

**FIRST PHYSICAL MACHINE: fired 2026-09-08.** Acer Aspire A515-51G
(i7-8550U, firmware **V1.21**, Windows 11 Home 22631, **Secure Boot on**,
BitLocker off, dual SSD+HDD, Intel RST software on an AHCI-mode
controller). Run through the one-click flow exactly as a non-technical
person would: plug in the stick built by `./make-kit.sh`, double-click
`RUN-TEST.cmd`, one UAC consent, walk away. Result: **`fired-once`,
`keypress_free=y`, `windows_returned=y`, `mode=auto`, `fired-via=grubenv`.**

What that row really shows, and what it does not:

- **The mechanism works on real vendor firmware.** `bcdedit {fwbootmgr}
  bootsequence` booted a **real removable USB stick** one time. No VM leg
  could reach that clause (Hyper-V Gen 2 has no USB emulation; the QEMU
  rig's `usb-storage` is still an emulation). Acer's firmware honoured the
  one-shot, used it up, and left the boot order intact.
- **Secure Boot enforcing, signed payload, no MOK games** (MOK: the Machine
  Owner Key, a way to enroll your own keys). This firmware's db holds both
  CAs (unlike Hyper-V's templates, where it is one or the other), so shim
  -> GRUB ran verified with nothing enrolled by us. The signed chain is the
  product's path, and it is the one that ran.
- **The whole managed flow held on hardware.** The elevated logon task
  survived the reboot, ran the return check by itself, classified from the
  `grubenv` marker the payload wrote to the FAT stick, removed its own task
  and the test boot entry, and wrote the row to the stick. No console, no
  second UAC, no drive letter typed.
- **The person's own words for the boot:** it "restarted without any button
  clicks in the boot loader." Signing back into Windows afterwards is
  normal and is not what `keypress_free` measures.
- **What it does NOT show:**
  - One vendor is not the matrix (**≥3 more**: Dell, Lenovo, HP, still
    owed).
  - The **fail-safe rows have never run on real firmware** (`NoFile`, and
    `SecureBootUnsigned`, which needs the unsigned Shell payload with Secure
    Boot on).
  - **No physical BitLocker row exists.** This machine has BitLocker off,
    so whether any vendor firmware measures the attempted one-shot into a
    sealed PCR is still entirely open. The prologue keeps suspending anyway.

Two side firsts from the same run, both on real hardware:

- The scanner's **ESP gate** (R21 item 4) fired for the first time outside
  a VM: `Boot partition (ESP): 44 MiB free; Windows boots from it`,
  comfortably over the 32 MiB threshold.
- `Get-BitLockerVolume` **worked on a Home edition**
  (`bitlocker-via=cmdlet`), so the `manage-bde` fallback built in 0.2.0 was
  not needed here. That is one machine, not proof the concern is wrong. The
  fallback stays, and a Home machine with Device Encryption *on* is still
  the case that would use it.

**Fixed straight after (harness 0.3.1, 2026-09-08).** A CSV the harness
creates *fresh on a stick* carried a UTF-8 byte-order mark (BOM, a hidden
marker at the start of the file) in front of its header's first column name
(PS 5.1's `Out-File -Encoding UTF8`). So a naive parser reads that column
as `\ufefftimestamp`. Data rows never carried one and the transported row
was unaffected. But this is shipping code (the prologue writes to the stick
too), so the header is now written with an explicit no-BOM encoder, locked
in by two self-test cases. The transport procedure (append data rows only,
never the stick's header) is written down in `validation-results/README.md`.

Also recorded: the harness starts each row's notes with the OS edition, the
BitLocker source and the marker source. The one-click launchers pass the
stick's own drive letter, so a physical operator types nothing.

**Closes when.** The spine spike (build order step 0) passes in a VM and on
physical machines from at least three vendors.

## R16: Stick authoring can write the wrong device · critical · open (writer built, refusal matrix fired 2026-09-08)

**What.** `evaluate` writes the live image with raw `\\.\PhysicalDrive`
writes, straight onto the device. The person may have other USB devices
plugged in.

**If real.** The tool destroys someone's data *before* the commit line.
That is the one failure the whole architecture exists to prevent, done by
the part that promised to be safe.

**Built (2026-09-08): `evaluate/windows/Write-UpgradeStick.ps1`.** Choosing
the target is a pure function over every attached disk (23 made-up cases in
`-SelfTest`). A disk is chosen only if all of these hold:

- USB bus, and not HDD/SSD media. (A USB hard drive or SSD enclosure is
  somebody's backup, so it is refused even when pointed at.)
- Not the system/boot disk, and holding no volume Windows runs from.
- Exactly one attached disk carries the `-Target` unique id. (Cloned serials
  mean ambiguity, which means refused.)
- Its size matches what the person was shown: exact bytes from a job, ±10 %
  of the packaging's decimal GB from a human.
- Online and writable.
- The person types the device's current label (or model). Never a bare
  "yes".

Disk numbers are not accepted as a target. Right before `Clear-Disk`, the
write path finds the target again by unique id from a fresh list and hands
the storage commands the object itself. So plugging something in between
the plan and the write cannot move it. Then: MBR, FAT32 boot partition
(active) + exFAT staging, copy, and every file read back against
`SHA256SUMS`. Every run (`-Plan`, read-only, and `-Write`) adds a row to
`docs/validation-results/r16-stick-writer.csv` listing every attached disk
and the rules it broke.

**Live, read-only (2026-09-08).**

- On the G16 the real 8 GB stick is chosen only when pointed at with the
  right size, refused for a 32 GB claim, and the system NVMe is refused on
  five counts when pointed at.
- On the rig with four SAS disks attached (system, OEMDRV, two blank VHDX
  "sticks"), every disk is refused, the pointed-at one for its bus.

One finding for `job.json`: a stick's `UniqueId` on Windows is the USBSTOR
device path **with the host name added on the end**
(`…&0&_&0:<host-name>`), and its `SerialNumber` can be empty. So the identity the prologue
re-checks is the unique id as listed on the same machine, and the serial is
a secondary field.

**Closes when.** The physical write row: several sticks **and a USB hard
drive attached at once**, `-Write` pointed at one stick. That one is
erased, written and verified, and every other device is untouched. A VM
cannot run it (no USB emulation; every VHDX is refused for its bus first).
That row is exactly the rule-#5 residue.

**First physical write (2026-09-13).** One real stick (General UDisk 8 GB)
was written and verified from the G16 host. A first attempt found that
`Clear-Disk` leaves a real stick MBR-initialized, where the rig's VHDX had
come back RAW (`Initialize-Disk` refused; writer 0.1.1 initializes only a
RAW disk). Both rows are in `r16-stick-writer.csv`. Several devices still
owed.

## R17: Counterfeit or failing flash as the sole data carrier · high · open

**What.** On the clean-slate path the stick is, for a while, the only copy
of the person's files. Counterfeit sticks lie about their size and silently
throw away writes. Cheap flash fails without warning.

**Scope narrowed (2026-08-22).** Only the clean-slate path puts user data on
the stick, and that path is now the opt-in / full-disk fallback, not the
default. On the default keep-Windows path the files never leave the internal
disk, so the stick is never the only copy. The impact if real stays high
(data gone after a wipe), but fewer people are exposed to it.

**If real.** Files checked as "staged" do not exist, found out after the
wipe.

**The read-back gate exists (2026-09-09).** `upgrade_/linux/verify.sh` (the
kickstart's `%pre`; the kickstart is the answer file that drives the Fedora
installer, and `%pre` is the script it runs before installing) reads the
chosen desktop image on the stick back against the stick's own `SHA256SUMS`
in the live session. It is the same code path that will read the staged
files back on the clean-slate path. In install mode a mismatch is a refusal
(exit 23, `%pre --erroronfail`), before the commit line, while Windows still
exists. It also records the stick's read speed, the honest basis for the
time estimate the person sees before walking away. Rows in
`docs/validation-results/v1-live-boot.csv` carry `desktop_image=` and the
MB/s in `notes`. Not yet shown on a counterfeit stick.

**First physical read-back (2026-09-12).** On the Aspire the 2.6 GB KDE
image read back byte-identical from the 8 GB "General UDisk" at **22.6
MB/s**: about two minutes, and the number the time estimate must be built
from. The same stick, on the G16 that morning, dropped off the bus twice
under a steady 5 GB write, with a burst of Windows disk errors (event 51),
until it was pushed back in. A loose connection this time, but exactly how a
failing stick looks, and the read-back gate is what would catch a silent
version of it.

**Closes when.** The cutover's read-back checksum check (which runs before
the commit line, while Windows still exists) is built as a hard gate (done)
and shown to catch a known-counterfeit stick.

## R18: Windows shrink headroom is unmeasured · high · open (prologue disk-check step built 2026-09-12; shrink ladder built, runs 4-7 physical, keep-Windows refused on the Aspire 2026-09-26)

**What.** To keep Windows, we shrink its partition to make room for Linux.
But some files cannot be moved: the MFT (NTFS's master file table), the VSS
store (System Restore's shadow copies), the pagefile and the hibernation file
(hiberfil). They cap how far `Resize-Partition` can shrink, often far short
of the free space. Think of a bookshelf where a few books are glued in
place: you can only slide the shelf's end in as far as the last glued book.

The ways round this were designed here first. They are now built and have
run on real hardware:

- hibernation and pagefile off with a restart (0.5.0), restored at every
  stop (0.5.1);
- restore points, with consent (0.6.0/0.9.0);
- the change journal, with consent (0.8.0).

The dated entries below carry each one.

**Partly addressed (2026-08-22).** The scanner now asks how much can be
shrunk through `Get-PartitionSupportedSize` (`Test-UpgDisk`) and reports it
as `Room to keep Windows`. Two limits found on real hardware, both recorded
here:

1. **The query needs Administrator.** Unelevated it returns "Access to a
   CIM resource was not available". So the scanner caps this line the same
   way it caps BitLocker. This dents the V4 plan (see VALIDATION.md): "ship
   the scanner, measure the population for free" only gets shrink data from
   people who run it elevated, and many won't. JSON reports should be
   filtered on `RanAsAdmin` before drawing conclusions.
2. **`SizeMin` includes unmovable files.** That is exactly what we want (it
   *is* the real shrink floor), but it means a machine that has not been
   defragmented reports a gloomy number. The prologue's mitigations (turn
   off pagefile/hibernation, restart) would raise it. The scanner reports
   the floor before mitigation and should say so.

**A guess of our own caught and removed (2026-09-08).** When
`Get-PartitionSupportedSize` failed, the scanner swallowed the error in an
empty `catch` and then printed a *cause*: "Fast Startup or a dirty volume can
cause this". Nothing had shown that. It was a plausible guess dressed up as a
finding, and **our own rig contradicts it**. Both rig guests ran with Fast
Startup **on** (`HiberbootEnabled=1`, recorded in the V3 rows), and the same
command measured shrinkable space fine (50.3 GB and 58.2 GB, in
`v1b-alongside.csv`). The Acer Aspire A515-51G reported "could not measure"
on both of its scans, and we did not know why, because the reason was thrown
away before it reached the report. Rule #2 applies to our own claims. This
one would have sent a person to change a power setting that was never shown
to be the problem.

Fixed in the scanner:

- The collecting half now keeps the error text and which call raised it.
- The judging half prints *what Windows said* and explicitly declines to
  name a cause. Two self-test cases lock in that the text carries Windows'
  reason and never mentions Fast Startup.
- **A second, independent, read-only measurement was added:** `diskpart`
  `shrink querymax`. This is the Virtual Disk Service path that Disk
  Management itself uses, separate from the Storage Management API path the
  command takes. It runs when the first path refuses (elevated only, 60 s
  hard timeout, output parsed against a line captured word for word on the
  rig: `The maximum number of reclaimable bytes is:   17 GB (17417 MB)`). A
  number from that path is labelled `via diskpart` and still carries the
  first path's refusal, so a returned report says both what worked and what
  did not.

**The Aspire answered the same day (2026-09-08), and both paths agree.**

- Storage API: *"Cannot shrink a partition containing a volume with
  errors."*
- diskpart: *"Use Chkdsk to fix the corruption problem, and then try to
  shrink the volume again."*

The C: volume carries NTFS's **dirty flag** (a bit Windows sets when it
thinks the disk needs checking), and Windows refuses to measure or shrink a
flagged volume. Fast Startup was never the cause. The guessed remedy would
have sent the person to a power setting and left the real blocker in place.
Two consequences, both built the same day:

1. **`Volume health` is now a scanner check** (read-only, elevated only).
   `fsutil dirty query C:` always runs (instant; the parser was written
   against the rig's real line `Volume - C: is NOT Dirty`). `Repair-Volume
   -Scan` (online, never repairs) runs only when the flag is set or the
   shrink query refused. A flagged volume is a **warn** that names `chkdsk
   C: /f` and says plainly that the converter must run that step before it
   can measure. The shrink line points at it. Nine self-test cases,
   including that translated or error output parses to `unknown`, never
   `clean`.
2. **Decided (2026-09-08): `evaluate` never repairs; the prologue may.**
   Keep-Windows must clear this blocker *itself*. The user philosophy is a
   fully managed experience, not a manual step. But the two modules split
   the job by whether it can be undone, the same way the architecture
   splits everything else:
   - **`evaluate` refuses to promise.** It detects the flag (`Volume
     health`), says plainly that Windows needs a disk check before anyone
     can know whether it can be kept, and **never runs one**. Its "made no
     changes" line is the trust contract. Instead it asks the person for the
     **fork** up front: *if the re-measured number fits, keep Windows; if
     not, clean slate or stop. Which?* That answer goes into `job.json`. No
     guess, and the person has agreed to the branch before anything happens.
   - **The `upgrade_` prologue runs the check and takes the fork.** It is a
     reversible prep step with its own restart, between "re-check the job
     against the live machine" and "shrink" (`architecture.md`, prologue
     steps 1-2). Consent is real (the person has said "convert this
     machine"). Refusal is still possible afterwards (it sits before the
     commit line, so a "stop" leaves Windows as it was, plus a finished
     disk check). And the number it branches on is the true one, measured
     at the moment it matters.
   - **Guardrails, all four, in the prologue:**
     1. the read-only online scan confirms the flag first;
     2. **refuse outright if the physical disk does not report `Healthy`**
        (a flag from a dying drive is a different situation, and repair
        activity can finish it off);
     3. prefer Windows' spot-fix (offline for seconds, fixes only what the
        scan logged); full `chkdsk /f` only when the scan logged real
        errors;
     4. the check's real outcome (Wininit event 1001 text, any `found.000`)
        recorded in `outcome.json`.

     The person is told, on the arm screen, that the restart may be slow and
     must not be interrupted.
   - **Consequences accepted with eyes open.** A full repair can cut short
     or delete files it cannot make sense of (fragments land in
     `found.000`). On a stale flag with no real corruption, the common case,
     nothing is lost. That is why scan-then-spot-fix is the default. We ran
     it, so we own the outcome. That is what the record is for.
   - **The test kit gains nothing that writes on the scanner side.** The
     step comes from the harness's family (`Test-Handoff.ps1`: one
     reversible change, its own restart, clean-up on return), which the
     prologue grew from. **Owed code.** The disk-health read
     (`Get-PhysicalDisk HealthStatus`, read-only) goes into the scanner
     first, since it is useful anyway. **Landed 2026-09-08** as the `Disk
     health` check:
     - It reads the physical disk holding C: (matched by `DeviceId`, then by
       `UniqueId`). `HealthStatus` is readable **unelevated** (confirmed on
       the G16). The reliability counters (uncorrected read/write errors,
       wear, power-on hours) are elevated-only and carried as facts, never
       judged.
     - Healthy -> ok. Warning -> warn, keep-Windows not offered, clean slate
       still possible. Unhealthy -> **fail** (RED, no override: copy the
       files off, replace the drive). Unreadable or unrecognised -> unknown,
       never ok.
     - Six seam cases plus a verdict case lock it in. `job.json` requires
       `Healthy` for a keep-Windows job (`schemas/job.schema.json`), and
       `outcome.json` requires `Healthy` at the moment of any repair.

**If real.** The safety-copy path is offered to machines that cannot deliver
it. The prologue fails late, after the person chose and firmly confirmed.
That is recoverable, but it is exactly the kind of stop that kills walking
away, which the design forbids.

**The fork, taken by code (2026-09-12).** The Aspire's C: still carries the
dirty flag (`Cannot shrink a partition containing a volume with errors`).
The first job writer (`New-Job.ps1`) did what the decision says: shrinkable
unmeasured -> keep-Windows not offered -> `intent.path = clean-slate`,
`path_reason = forced-no-room`, with the ESP and disk health both recorded
as fine. The prologue's disk-check step that would turn that into a real
measurement is still owed.

### The prologue's disk-check step (built 2026-09-12)

`upgrade_/windows/Invoke-Prologue.ps1` does step 1b exactly as decided
above:

1. `Repair-Volume -Scan` (read-only) first, its answer recorded word for
   word.
2. `Get-PhysicalDisk` HealthStatus read again right before anything is
   scheduled. Anything but `Healthy` is a stop.
3. The rung chosen by the scan:
   - `NoErrorsFound` -> `Repair-Volume -SpotFix`, confirmed by `chkntfs`,
     falling back to scheduling `chkdsk C: /spotfix` if the command did not
     take;
   - `ErrorsFound`/`ErrorsNotFixed` -> `chkdsk C: /f`;
   - anything else -> refuse.
4. Its own restart, resumed by a one-shot elevated task.

**Since 0.3.0 (decided 2026-09-13) the resume is a SYSTEM task at startup,
not a logon task.** Every rig row before it had the resume fire only because
the rig guest signs in automatically, and on the Aspire's row a person
signed in. So the walk-away half of the promise had never really been
tested. The 0.3.0 resume:

- runs before, and without, a sign-in;
- polls for the stick by volume id;
- locks its state folder to SYSTEM and Administrators before the task
  exists (a SYSTEM task over a folder ordinary users can write to is a way
  to take over the machine);
- records who ran it and whether a session existed (`state.Resumes`);
- queues its messages as a `RunOnce` notice for the next sign-in.

The alternative, holding the person's Windows password for an automatic
sign-in, was considered and refused (`architecture.md`, "the walk-away
resume").

**Fired the same day** (`r18-prologue.csv` row 6, `pass-plumbing`,
automatic sign-in off in the guest, `query user` empty throughout):

- Both resumes ran as `NT AUTHORITY\SYSTEM` in session 0, no explorer, 10 s
  and 6 s after boot, the stick found 4 s and 3 s later.
- The Wininit 1001 text was read with no sign-in at all. (The earlier "lands
  after logon" was just the rig's automatic sign-in following boot by
  seconds.)
- The RunOnce notice was queued and the task removed on return. The state
  folder's permissions read back as SYSTEM + Administrators full, Users
  read.
- The whole check -> resume -> shrink -> handoff -> install -> first Linux
  boot took 472 s with nobody at the keyboard.

**The physical residue closed the same evening** with the read-only probe
(`RUN-PROBE.cmd`, `walkaway-probe.csv` row 2): the Acer Aspire, Secure Boot
on, Windows 11 Home, a real USB stick. SYSTEM in session 0, 38 s after boot,
the stick seen 5 s later, notice queued, task removed. What remains is other
vendors' firmware, one probe row each.

Also seen on this row: with the 0.2.0 evidence read, the rig's injected flag
now leads to `chkdsk /f`, not the spot-fix. `Repair-Volume -Scan` on a
`fsutil dirty set` volume logs "found problems" and marks it "Full Repair
Needed". So the rig's model of a flag has moved closer to the Aspire's.

On return, the Wininit 1001 text, any `found.000` and the flag are read
again. If the flag survives the spot-fix, the prologue rescans and steps up
to `/f` only if that rescan logs errors, once. Otherwise it stops and says
so. Then both read-only measurements, the fork from `job.json`, the shrink.

**The job writer changed with it (0.2.0).** A flagged volume on a Healthy
disk with room on the ESP is now a `keep-windows` job with the fork pending,
not a forced clean slate. The 0.1.0 writer's forced `clean-slate` on the
Aspire (above) jumped ahead of the very fork the decision saves for the
prologue's measurement.

Self-tests lock in every rung, the gate, the fork, the plan arithmetic and
the record's shape. The rig bench (`rig/hyperv/prologue.sh`) injects the flag
with `fsutil dirty set C:`, and the row goes to
`docs/validation-results/r18-prologue.csv`.

**Fired on the rig the same day** (`r18-prologue.csv` rows 2-4). The first
run stopped on a prologue bug, and its refusal path (C: grown back, stopped
outcome, credentials wiped) is row 2. Row 4 is `pass-plumbing`:

- the injected flag confirmed by the scan, `Healthy` read, the spot-fix
  scheduled;
- at the restart, Windows ran its own **full** three-stage check on the
  flagged volume (Wininit 1001, 6 s, "found no problems"). So on a dirty
  bit, autochk (Windows' boot-time disk checker) does the whole check,
  whichever rung was scheduled;
- both paths then measured 57.8 GB, the fork kept Windows,
  `Resize-Partition` freed exactly 25 GB, the install completed, and
  `outcome.json` carried the prologue's record unchanged.

**Residue, stated plainly:** the rig's flag is our model of the Aspire's: a
stale bit with no corruption behind it. What a *real* flag, kept by Windows
through several restarts, does under the ladder is the part no spoof can
answer. The Aspire is the machine that answers it.

### The Aspire's drive (2026-09-13 to 2026-09-17)

**The Aspire answered (2026-09-13), and the answer is the dying-drive
branch.** First physical step-1b row (`r18-prologue.csv` row 5,
`stopped-volume-check`). The prologue confirmed the flag, read `Healthy`,
scheduled the spot-fix and restarted. It came back to a volume still
flagged, **no boot-time check logged**, a `found.000` already on C:, and a
rescan that again returned `NoErrorsFound`. It refused to step up on a
guess. A read-only diagnostic (`DIAG-VOLUME.cmd`, kept out of the repo
because it names the person's files) then showed what the prologue's two
signals had missed:

1. **`Repair-Volume -Scan`'s return value is not the truth.** Every one of
   its runs on this machine (the scanner's, the prologue's, and Windows' own
   daily ones since at least 2026-08-27) returned `NoErrorsFound`. For the
   same runs, the Chkdsk provider logged "Examining 18 corruption records
   ... corruption found ... Windows has examined the list of previously
   identified potential issues and **found problems**. Please run chkdsk
   /scan to fully analyze the problems and queue them for repair." The
   online scans list cross-linked attribute records, a corrupt `$UsnJrnl`,
   files owning the same clusters, and `$I30` indexes whose multi-sector
   headers read as zeros. All "queued for offline repair", daily, for weeks.
   `Get-Volume` says it plainly: `HealthStatus Warning`, `OperationalStatus
   Full Repair Needed`. **Both the scanner and the prologue must read the
   event log and the volume's own status, not the command's string.**
2. **`Get-PhysicalDisk` HealthStatus `Healthy` is not enough.** The System
   log holds **30 `disk` event 7 ("has a bad block") entries on
   `\Device\Harddisk1\DR1`**, the internal SSD that holds C:, from
   2026-09-12 20:17, while the disk still reports Healthy. Zeroed sector
   headers plus bad blocks is the fingerprint of failing flash. This is the
   exact case guardrail 2 was written for ("a flag from a dying drive is a
   different situation, and repair activity can finish it off"), and the
   guardrail's one read did not see it.
3. **Windows 11 does not run the boot-time check on its own** for a dirty
   volume here. `BootExecute` is the default `autocheck autochk *`, the
   volume has been dirty for weeks, and there is no Wininit 1001 in 30 days.
   The flag stays because the offline repair never runs unless a person
   asks for it (Windows' "Restart to repair drive errors"), not because the
   check failed.

**Decided (2026-09-13):**

- The disk-health guardrail gains a second read: `disk` events 7 / 51 / 153
  on the physical disk holding C: in the last 30 days. Any bad-block event
  is **RED** in the scanner and a refusal in the prologue, no override
  (rule #1).
- The volume-health check reads `Get-Volume`'s `OperationalStatus` and the
  latest Chkdsk-provider event's verdict ("found problems" / "queued for
  offline repair"), and treats either as the flag's *reason*, replacing the
  command's string.
- The prologue's rung chooser uses the same facts: corruption records
  queued for offline repair are "real errors" (the `/f` rung), a flag with
  none is the spot-fix rung, and a disk with bad blocks gets no rung at all.

For the Aspire the product answer is the one R18 already wrote: copy the
files off this drive and replace it. The prologue's refusal was right. The
reasons it had were weaker than the real ones. Owed code: scanner (`Disk
health`, `Volume health`), prologue guardrails 1-2, the job schema's
`volume_health.scan` meaning.

**Diagnosis confirmed by SMART (2026-09-13, `DIAG-SMART.cmd`, read-only).**
SMART is the drive's own health log. The SK hynix HFS256G39TND (SATA,
firmware 30001P10, 8,566 h, 9,375 power cycles) reports:

| SMART attribute | Value |
|---|---|
| 187 Reported_Uncorrectable | **725** |
| 5 Reallocated_Sector_Ct | **7** (196 Reallocated_Event_Count 7) |
| 184 End-to-End_Error | **639** |
| 195 Hardware_ECC_Recovered | raw 59.5 million, normalized value has touched **1** (worst) |
| 199 UDMA_CRC_Error_Count | **0** |
| 188 Command_Timeout | 0 |

Its own failure-prediction flag is `False`: every normalized value still
sits above the vendor's lax thresholds, which is exactly why
`Get-PhysicalDisk` says `Healthy`. The System log holds **261 bad-block
events (disk id 7) on that disk since 2026-08-27** and 165 NTFS corruption
events (id 55). At the very boot after the prologue's spot-fix restart, NTFS
said it in words (id 98, 2026-09-13 10:10:04): *"Volume C: needs to be taken
offline to perform a Full Chkdsk. Please run CHKDSK /F"*. The 1 TB HDD
beside it is clean on every counter.

**Verdict: the SSD's flash is failing (media errors the drive's ECC cannot
correct, reaching the computer), not its connector.** Replace it; copy the
files off first. Two more reads for the guardrail code: NTFS event 98 is
Windows' own "full chkdsk needed" statement and belongs in the rung chooser
as the "real errors" signal. And SMART 187/5/197/199 (via
`MSStorageDriver_FailurePredictData`, SATA only) can name *why* a drive is
refused.

**2026-09-17, the clean bit with a queued repair (the Aspire, first run of
the acknowledged path).** The owner ran
`RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd` with nothing on the SSD to lose.

- The scan said RED (Disk health: 368 bad-block events, SMART 187 = 736),
  Volume health WARN (the NTFS 98 of 2026-09-13 still standing,
  `Repair-Volume -Scan` "NoErrorsFound").
- And `fsutil dirty query` said **NOT Dirty**. The bit set on 09-13 had
  cleared, with NTFS's 09-13 request for a full chkdsk still inside the
  30-day window. (**Corrected 2026-09-20:** this entry first said
  `Get-Volume` still read "Full Repair Needed". That was 09-13's reading. On
  09-17 and 09-20 the volume read `OK` / `Healthy`, and the week-old event
  98 was the only "repair queued" signal.)
- In that state `Get-PartitionSupportedSize` answered **0 GB shrinkable, no
  error, 33.6 GB free**. Not the "volume with errors" error of 09-12, but a
  number.
- The job writer (0.4.0) took the number: `clean-slate`, `forced-no-room`.

Three consequences, none expected:

1. The acknowledged path could not reach a keep-Windows install on this
   machine at all. Clean slate stages and stops at the unbuilt wipe gate.
2. The prologue's step 1b was keyed on the dirty bit alone and would have
   skipped the check. Yet the scanner's own Volume-health text says that
   check is the only thing that clears this.
3. The run ended right after the job was written, and nothing on the stick
   said why, because the launchers logged nothing.

**Corrected 2026-09-20:** this was read at the time as a kickstart failure.
The second run's `convert.log` shows the job step exiting 0 and no kickstart
step at all. The launcher's own refusal text had an unescaped `)` inside its
`if` block, which closes the block in cmd, so `pause` and `exit /b 1` ran
every time. The data-loss launcher had never run past step 3 anywhere (the
rig drives the prologue directly). Reproduced in a nine-line script; fixed
with `^(` / `^)`; `make-kit.sh` now refuses any launcher with the pattern.
The sentence is case-sensitive. A first attempt in lowercase was refused,
correctly.

**Decided (2026-09-17):** Windows' own "repair queued" statement
(`Get-Volume` OperationalStatus naming a repair, or NTFS event 98 within 30
days) is a first-class fact next to the dirty bit:
`job.storage.volume_health.repair_queued`, required in the schema, checked
again by the prologue, needing the same consent.

- With it set and the shrink answer below the Linux minimum, the job writer
  (0.5.0) records `shrinkable_gb` **null**, with the answer and the reason in
  `shrink_error`, and the path is keep-windows pending the check. That is
  the 2026-09-08 rule extended to the second of Windows' two statements.
- The scanner (0.3.1) reports such a number as "not a trustworthy number"
  (info) instead of "too little room" (warn).
- The prologue (0.4.0) runs step 1b on either trigger
  (`volume_check.trigger` = `dirty-flag` | `repair-queued`). The evidence
  still chooses the rung (here `chkdsk-f`, as before). On the repair-queued
  trigger, "clean after" proves nothing: the check must have run (Wininit
  1001) and `Get-Volume` must no longer name a repair, or it stops at
  `volume-check`.
- Every launcher step now runs through `Invoke-Logged.ps1`, which appends
  what the step printed to `upgrade_\convert.log` on the stick.

R23's width is unchanged: nothing new is lifted.

**Second run (2026-09-20), as far as the launcher bug let it go.** On the
real machine the scanner said "Windows answered 0 GB, but a full disk check
is queued - not a trustworthy number". The job writer wrote `keep-windows
(default)`, shrinkable unmeasured, DATA LOSS ACCEPTED, lifting disk-health
and volume-health (465 bad-block events by then). Those are the first three
steps of the decision, fired on hardware.

### Runs 3 and 4 on the Aspire (2026-09-20)

**Third run (2026-09-20), the first physical row of the acknowledged path:
`stopped-volume-check`** (`r18-prologue.csv` row 7, `0.4.0-physical`; job
and outcome both schema-valid, the outcome carrying the acknowledgement).
All five launcher steps ran and logged. Prologue 0.4.0:

- re-validation matched; trigger `repair-queued`;
- disk gate lifted by the acknowledgement (465 bad-block events);
- the evidence chose `chkdsk-f`; Windows accepted it ("This volume will be
  checked the next time the system restarts", chkntfs `scheduled`);
- restart; the resume ran as SYSTEM in session 0, nobody signed in, 36 s
  after boot, the stick seen 3 s later. That is the walk-away resume on the
  product path, on real firmware.

Then the stop, and it is the right one: **no Wininit 1001 after two minutes
of polling, and a 36-second boot leaves no room for a full check to have
run.** The prologue refused to measure and wrote its stopped outcome. Two
things this leaves open, neither closed by argument:

1. **A boot-time check that Windows accepted did not run, for the second
   time on this machine** (09-13: the spot-fix restart, no 1001 either).
   The rig's guest is Windows 10; this is Windows 11 Home 22631. If
   scheduled checks do not run on current Windows 11, step 1b's whole ladder
   fails for every flagged machine in the audience, not just this one.
   Evidence that would tell us: `BootExecute` and `chkntfs C:` read after
   scheduling and again after the restart, the Wininit and Chkdsk events of
   that boot, whether the "press any key to skip" screen appeared, and a
   Windows 11 guest on the rig with an injected flag.
2. **The 0 GB answer is unexplained.** With the volume reading `OK` and the
   bit clean, "a repair is queued" rests on one week-old event. An untested
   candidate: clusters NTFS has marked bad (`$BadClus`) sit near the end of
   the volume and pin the shrink floor. Windows names the last unmovable
   file in Defrag event 259 after a shrink query. Read-only, and owed before
   anyone reasons further.

Owed: a read-only diagnostic visit for both (extend `DIAG-VOLUME.cmd`), the
Windows 11 rig row, and the keep-Windows install on a healthy drive.

**The diagnostic, the same evening (2026-09-20, read-only, over SSH to the
Aspire, the check-restart boot still the current one). Windows' own records
answered both questions, and both answers overturn the 2026-09-17
reading:**

1. **The 0 GB is `hiberfil.sys`.** Storage API: SizeMin = size. diskpart
   `shrink querymax`: 0 B. Defrag event 259: *"The last unmovable file
   appears to be: \hiberfil.sys::$DATA"*, last cluster `0x3b562fe` (the end
   of the volume), shrink potential target LCN `0x30db0ab`, about 42 GB once
   it is gone. Not the SSD, not a repair: the cold floor, with a
   hibernation file parked at the last cluster. That is exactly what the
   prologue's hibernation-off-then-re-measure step exists for, the step
   after the one this run stopped at.
2. **There was no repair queued.** The full boot-time check **ran on
   2026-09-15 18:01** (a restart during the storage-mode visit): autochk's
   own log `Chkdsk20260915220115.log`, `found.011`, and a **Wininit 1001**
   at 18:02:32. That is what cleared the bit and set the volume to `OK`.
   The NTFS 98 of 09-13 was history by 09-17. The "repair queued" trigger
   (event 98 within 30 days) fired on a repair already done, and the
   scanner's Volume-health warning had the same flaw. It also answers the
   wider worry: a scheduled boot-time check **does** run on this Windows 11
   machine and **does** log Wininit 1001. Today's scheduled `/f` on a volume
   with nothing to repair left no trace (no autochk log, no 1001, volume
   mounted 2 s after kernel start), and the schedule is gone from
   `chkntfs`. Why is unknown and, with the trigger corrected, not a state
   the converter should reach.

The stop itself stands. Asked to prove a check ran, the prologue could not,
and refused. What was wrong was upstream: sending it there.

**Decided and built (2026-09-20), job writer 0.6.0, prologue 0.5.0,
scanner 0.3.2:**

1. An event 98 counts as "repair queued" only if no completed boot-time
   check (Wininit 1001, or autochk's own log under `System Volume
   Information\Chkdsk`) comes after it. An event is history, not state.
   `Get-Volume`'s status and the dirty bit are state and always count. The
   same freshness rule feeds the prologue's rung chooser and the scanner's
   Volume-health check, which now reports such a volume `ok` and says when
   the check completed.
2. When the cold shrink number is below the minimum, all three read what
   Windows names as the last unmovable file (Defrag 259; `shrink querymax`,
   which only reports, prompts the event if the Storage API did not):
   `job.storage.last_unmovable_file`. If it is `hiberfil.sys`,
   `pagefile.sys` or `swapfile.sys` (the files the prologue already turns
   off before its second measurement), the job is keep-windows with the
   fork pending, and the scanner says so (info) instead of steering to clean
   slate. Anything else (`$Mft`, a restore point, `$BadClus`, a person's
   file) is still "no room".

The disk gate and the ESP gate are untouched, and R23 lifts nothing new. The
2026-09-17 `repair_queued` fact stays, for a repair that really is queued.
Owed: the rerun.

**Fourth run (2026-09-20 evening, kit 08a852a): `stopped-shrink`**
(`r18-prologue.csv` row 8, `0.5.0-physical`; job and outcome both
schema-valid; the run's files copied off the stick and hash-matched before
anything was read). Everything the two fixes predicted fired on the real
machine:

- the scanner RED on Disk health alone (465 bad-block events), Volume
  health `ok`;
- the job keep-windows with the fork pending, lifting `disk-health` only;
- the prologue's trigger `none` ("a boot-time check completed after it ...
  not a queued repair");
- cold 0 GB by both paths, "Windows names the last unmovable file:
  \hiberfil.sys";
- hibernation and pagefile off; restart 1; the resume as SYSTEM in session
  0, 20 s after boot, the stick seen 2 s later.

Then the honest stop: **re-measured 7.2 GB (Storage API) / 3.3 GB
(diskpart) against the 25 GB Linux needs**, fork `stop`, stopped outcome
written, resume task removed, no boot entry left. The commit line was never
approached. What the run overturns and exposes:

1. **Corrected (2026-09-20): "about 42 GB once it is gone" was wrong, and it
   was an argument.** That figure was read off Defrag 259's "shrink
   potential target" LCN. The target is where Windows would like to get to.
   What removing the named file really buys is only the distance to the
   *next* unmovable file, and nothing reports that until the first one is
   gone. (Back to the bookshelf: unglue one book and the shelf only slides
   as far as the next glued one.) With `hiberfil.sys` and the pagefile gone,
   the next one is **`\System Volume Information\{...}{3808876b-...}`,
   shadow-copy (System Restore) storage**, at cluster `0x3985c3f`. That is
   exactly the 7.2 GB the Storage API answered (`vssadmin`: three shadow
   copies, 4.31 GB used, the newest made 2026-09-20 15:20 local). The
   scanner's and job writer's "pending the prologue's own re-measure"
   wording was right to promise nothing. The handoff note's "~42 GB" was
   not.
2. **V4's first mitigated number from a real disk is a no:** 48.5 GB free,
   7.2 GB shrinkable after every mitigation the prologue has. Shadow-copy
   storage is not on the mitigation ladder. Shrinking or deleting it
   destroys restore points, which is not reversible prep. So whether the
   prologue may ever touch it is a decision, not a fix, and is not made
   here. One machine, a dying one: a data point, not a rate.
3. **The two read-only paths disagreed because the machine changed between
   them.** The person saw Windows "come back", took the run for failed and
   signed in (logon 23:59:48Z). Slack started by itself at 00:00:17Z and at
   00:00:33Z created a cache file at cluster `0x3a82d73`, open and so
   unmovable. That is exactly diskpart's 3.3 GB at 00:00:51Z (Defrag 259
   names it; `fsutil volume querycluster` confirms both clusters). The stop
   used the larger number and would have stopped on either. Two findings: a
   sign-in during the re-measure can lower the number, and **the walk-away
   resume shows a person at the lock screen nothing at all**. "You can walk
   away" reads as "it failed" to someone who stayed.
4. **Bug: a stop after the mitigation does not undo it, and says it did.**
   `Stop-Prologue` grows C: back, re-enables BitLocker and removes the boot
   entry, but never restores hibernation or the pagefile. The message says
   "Windows is as it was". Read after the stop: `HibernateEnabled` 0, no
   `pagefile.sys`, automatic management off, no pagefile setting. A Windows
   with no pagefile at all. Worse, `Invoke-ProloguePagefileOff` deletes the
   existing pagefile settings without recording them, so a custom setup
   could not be put back. Owed: record the before state, restore it at every
   stop and abort, a self-test case, a rig row. No refusal may leave the
   machine worse than it found it (rule #3).

Owed after this run: item 4's fix; something on screen during an unattended
resume (R24); the shadow-storage decision, with more machines' numbers
before it; and, still, the keep-Windows install on a healthy drive.

**Built (2026-09-20), prologue 0.5.1: item 4.** Before hibernation or the
pagefile is touched, the prologue records how they were
(`state.Shrink.Before`: `HibernateEnabled`, automatic management, any custom
pagefile settings).

- `Stop-Prologue` and `-Abort` put back exactly what this run turned off, to
  what it was. A machine that had hibernation off, or no pagefile, is not
  given one. A state with no record (0.5.0) gets the pagefile back on
  automatic and is told hibernation was left off.
- The return after a completed install restores the pagefile only.
  Hibernation stays off there by design (the kept volume must be
  mountable).
- The stop window says "Windows is as it was" only when that is true. It
  says "once it has restarted" when a pagefile is pending, and "NOT fully
  put back", with the reason, when a restore failed.

Ten self-test cases on the judging half (99 pass). `prologue-verdict.py`
writes before/restored into the row's notes. **Not closed:** the acting half
(`powercfg /h on`, `Set-CimInstance`, re-creating `Win32_PageFileSetting`)
has run nowhere. It closes on a row whose stop follows the mitigation, with
the settings read back after the next restart: the rig, and the Aspire, whose
next run will stop at the same place for the same reason.

**Decided (2026-09-20, the owner's call): the Aspire keeps its dying SSD.**
It is the project's bad-conditions machine, what someone in the field with
an old computer might really plug the stick into, and it goes on being run
as it is. This replaces the same day's plan to swap the drive next. What
does not change: a drive or stick failure mid-run is a note here, never a
fail row. And rows from this machine never stand in for the healthy-drive
keep-Windows install (V1b's residue), which is still owed and now needs a
different machine or a later swap.

**Decided and built (2026-09-20, the owner's call): restore points in the
way of the shrink are deleted, with consent, by the conversion.** "We don't
want a user to ever have to run commands, and this blocks conversion."
Prologue 0.6.0, job writer 0.7.0. The judge
(`Get-PrologueRestorePointStep`) says `delete` only when all four hold:

1. the re-measured number does not fit;
2. Windows itself names System Restore's shadow-copy storage as the last
   unmovable file (the exact `System Volume Information\{...}{3808876b-...}`
   form from run 4, nothing else under that folder);
3. `job.fork.restore_points_consented` is true (a job written before the
   field existed reads as no consent);
4. it has not been done in this run.

Then `vssadmin delete shadows /for=C: /all`, the counts before and after go
into the record and `outcome.json` (`shrink.restore_points_deleted`), and
one more re-measure before the fork. Both launchers say it in plain words on
the screen where CONVERT is typed: restore points are Windows' own undo
history, not the person's files, and deleting them cannot be undone.

The cost, stated: this is the first thing the prologue does before the
commit line that a stop cannot undo. It is a modest loss (Windows stays
bootable, no personal file is touched), and the person is told before the
word. Rule #3's promise is now "one irreversible moment, plus the restore
points you were told about". Seven self-test cases on the judge. The acting
half (`vssadmin`) has run nowhere. It closes on the Aspire's next run, where
run 4's evidence says this is exactly the file in the way.

### Runs 5 to 8 on the Aspire (2026-09-22 to 2026-09-26)

**Fifth run (2026-09-22, kit 3dbf910, prologue 0.6.0, job writer 0.7.0):
`stopped-confirm`** (`r18-prologue.csv` row 9, `0.6.0-physical`; job and
outcome both schema-valid; the run's files copied off the stick and
hash-matched, all ten, before anything was read).

Before it, with the owner's consent and over SSH, hibernation and automatic
pagefile management were put back after run 4 and the machine restarted
(the one change allowed). Read back: `hiberfil.sys` 5.1 GB, `pagefile.sys`
1.9 GB, volume clean, one restore point left of run 4's three (Windows ages
them out at its 4.75 GB cap).

The run itself took 52 seconds and no restart: scan RED on Disk health alone
(465 bad-block events), the sentence, CONVERT, and then not the flow this
entry's owners expected. Neither new piece of code fired: 0.5.1's restore
and 0.6.0's restore-point deletion are still unfired.

1. **The cold layout was not run 4's, and the expected flow was an
   argument.** Cold: 3.2 GB by both paths, with Windows naming
   **`\$Extend\$UsnJrnl:$J`, NTFS's change journal** (Windows' running log
   of recent file changes), as the last unmovable file, last cluster
   `0x3a84077` (Defrag 259, four times, 22:13:54Z-22:23:21Z). The volume
   has 62,219,007 clusters of 4 KB: (62,219,007 - 61,358,199) x 4 KB = 3.28
   GB, the number both paths gave. At run 4's re-measure nothing unmovable
   lay past `0x3985c3f` except Slack's open cache at `0x3a82d73`. So this
   journal extent was placed after 2026-09-21 00:00:51Z, in the tail that
   run 4's mitigation had emptied. The re-created `hiberfil.sys` landed
   elsewhere. The predicted "cold 0 GB naming hiberfil.sys" (the handoff
   note and the run card) was reasoning from the last run's layout. A cold
   layout is not stable between runs: ordinary use puts new unmovable files
   into freed space at the end of the volume.
2. **Defect: the person chose "stop" and the job said "wipe".** The job
   writer judged the cold number: 3.2 GB, pinned by a file not in
   `Test-JobShrinkMitigable` (hibernation, page and swap only). So
   `intent.path = clean-slate`, `path_reason = forced-no-room`, with
   `fork.if_cannot_keep = stop` right next to it in the same job. The
   prologue's fork treats a clean-slate job as clean slate "whatever the
   number", and started staging. The launcher had told the person, on the
   screen where CONVERT is typed, that the Windows partition would be shrunk
   and Linux installed beside it. The job it wrote carried the opposite
   operation. This was by design in the writer (its self-test expects forced
   clean slate under `stop`), and it contradicts the schema's own
   description of `fork` ("chosen here, in advance, by the person - never
   guessed at run time") and the consent text. What stopped the run was not
   a check. It was that clean slate's confirm gate is not built yet ("the
   prologue will not arm an unattended wipe"). Rule #1: a person's "stop"
   may never become a wipe. A job that cannot keep Windows under `stop` must
   refuse, before CONVERT is asked for.
3. **Defect: "your files are staged" after staging none.** The job carries
   no folder map yet ("not in this job: folders"). So the staging step
   found 0 folders, wrote an empty `staging/SHA256SUMS`, and the stop said
   "Your files are staged on the stick with checksums". Harmless today only
   because the wipe is unbuilt. Once the gate exists it is the
   trust-ending class (R8's family: an empty copy that reads as a complete
   one). Clean slate must refuse when the job lists no folders, and no
   message may claim files were staged when none were.
4. **What held.** The scanner's verdict; the acknowledgement lifting
   `disk-health` only; re-validation (all seven facts); the two independent
   measurements agreeing; the refusal to arm an unattended wipe; a truthful
   `outcome.json` apart from item 3's sentence; and Windows as found. Read
   back over SSH at 22:38Z: hibernation on, pagefile on (automatic), one
   restore point, no resume task, no one-time boot entry, C: the same size.

Owed after this run: items 2 and 3 fixed with self-test cases before the
Aspire runs again, and a decision on the next rung, since the change journal
now pins this disk cold. Deleting it (`fsutil usn deletejournal`) loses
Windows' record of recent file changes (not the person's files; indexers and
sync clients re-scan), and Windows re-creates it, possibly somewhere else
near the end. Whether that frees anything, and what sits behind it, only a
re-measure can say (the shadow storage of run 4 is one known candidate).

**Fixed (2026-09-22), job writer 0.8.0 and prologue 0.7.0: items 2 and
3.**

- Item 2: `Get-JobPath` takes the fork. Under `stop` it returns keep-windows
  whenever the disk (Healthy or acknowledged) and the ESP allow it. A small,
  unmeasured or unmitigable cold number goes to the prologue's re-measure
  and fork, which stops. Otherwise it returns no path at all, which
  `New-JobDocument` turns into a named refusal before CONVERT ("Windows
  cannot be kept on this machine (...), and you chose to stop rather than
  wipe it"). Forced clean slate remains only under `if_cannot_keep =
  clean-slate`. The prologue's `Get-PrologueFork` also takes `path_reason`
  and stops a `forced-no-room` clean-slate job whose fork is not
  clean-slate. Run 5's own `job.json` would now stop at the fork, before
  staging.
- Item 3: `Get-PrologueStageRefusal` refuses clean slate when the job lists
  no folders (before anything is written to the stick), and when folders
  were listed but 0 files were staged. The confirm stop now states the count
  staged.
- Self-tests: the job writer's forced-clean-slate cases now pass
  `-IfCannotKeep clean-slate`, with new cases for `stop` (run 5's facts as
  scanned give keep-windows with no staged block; a Warning disk and a full
  ESP are named refusals; a sweep of 180 disk states under `stop` gives no
  clean-slate job). The prologue gains five (111 pass). Scanner, harvester,
  kickstart and schema checks pass.

**Not closed:** judged on made-up facts only (rule #5, logic level). The
refusal paths have run on no machine. What it means for the Aspire as it
stands: run 6 would get a keep-windows job. The prologue would re-measure,
take its mitigation rung (hibernation and pagefile off, one restart),
re-measure, and, if the change journal still pins the floor, stop and put
both back. That is the row 0.5.1 is still owed. `RUN-VERIFY.cmd`, which
passes no fork and so means `stop`, now gets keep-windows jobs on full disks
too, and a refusal on a disk whose ESP or health rules out keeping Windows.

**Decided and built (2026-09-22, the owner's call): the change journal in
the way of the shrink is deleted, with consent, by the conversion.** This
is the next rung the fifth run named. Prologue 0.8.0, job writer 0.9.0. The
judge (`Get-PrologueUsnJournalStep`) says `delete` only when all of these
hold:

- the re-measured number does not fit;
- Windows names `\$Extend\$UsnJrnl` (the exact Defrag 259 form from run 5,
  `:$J:$DATA` stream included) as the last unmovable file;
- `job.fork.usn_journal_consented` is true (a job written before the field
  existed reads as no consent);
- it has not been done since the last restart. Windows creates the journal
  again, so the pagefile rung's restart can put it back in the way, and
  then once more is allowed.

The act is `fsutil usn deletejournal /n C:` (Microsoft's documented form;
`/n` returns once the journal is disabled), after `fsutil usn queryjournal
C:` records its sizes (32 MB / 8 MB on the Aspire). Then one re-measure.
The journal is created again with those sizes (`fsutil usn createjournal m=
a=`) right after a successful shrink, and by every stop and abort. Both
launchers say it on the CONVERT screen. `RUN-CONVERT.cmd`'s "nothing else is
deleted before Linux is installed" now comes after it, so it stays true.

What it costs: Windows' record of recent file changes. (Microsoft's own
caution: the Indexing Service and replication must rescan the volume, and
disabling "can take several minutes, and it can continue after the system
restarts".) No file of the person's. The owner, on the acknowledged
launcher: it is "essentially the user saying, fuck it". But this rung is a
consent on both launchers, like restore points. It does not widen R23's
acknowledgement, which still lifts exactly two refusals.

Eight self-test cases on the judge, the parser (the Aspire's real
`queryjournal` text) and the restore plan (120 pass). The acting half has
run nowhere. The syntax was taken from Microsoft's documentation, not run,
because running even `fsutil usn deletejournal`'s help on the G16 was
refused by the session's safety check. **Not closed.** Unknowns only a real
run answers: whether deleting it frees the extent Defrag named, how fast
Windows recreates it and where, and what sits behind it (run 4's shadow
storage is one candidate).

**Sixth run (2026-09-23, kit 537e093, prologue 0.8.0, job writer 0.9.0):
`stopped-shrink`** (`r18-prologue.csv` row 10, `0.8.0-physical`; job and
outcome schema-valid; eleven files copied off the stick and hash-matched
before anything was read; the machine read over SSH on 2026-09-26, once SSH
was reinstalled, because Windows' feature update had removed OpenSSH
Server). Between runs 5 and 6 Windows updated itself from build 22631 to
26200 (25H2). The ESP's free space fell from 44 MB to 37.8 MB with it (the
job's gate is 32 MB). The first launch stopped at the sentence, not typed
exactly: the refusal as built. The second ran:

1. **Job writer 0.8.0's fix held on a real machine.** Cold 8.4 GB pinned by
   `$UsnJrnl`, fork `stop`, and the job was keep-windows (`default`), not a
   wipe.
2. **The change-journal rung fired, twice: once per boot, as built.**
   `fsutil usn deletejournal /n C:` exit 0 both times. The number went 8.4
   -> 9.5 GB (Defrag 259: the journal's last cluster `0x3938060`, the next
   file's `0x38f2e57`). The journal was created again with its recorded
   sizes at the stop, and read back as 32 MB / 8 MB on 09-26. So deleting
   it frees what Windows named (about 1.1 GB here), and Windows re-created
   it in the same area across a restart (after the restart Defrag named it
   again at `0x39356ff`).
3. **Restore-point deletion ran and deleted nothing.** "2 before, 2 after".
   No VSS or volsnap event in the window. The shadow copies are
   client-accessible (the kind Microsoft's documentation says `vssadmin
   delete shadows` can delete). `/quiet` hides every message (Microsoft's
   page), and 0.6.0 records no exit code, so this run cannot say why. It is
   not argued here. Worse, the next pass logged "restore points were already
   deleted and their storage is still named", a false sentence. What held:
   nothing else was touched on the strength of it, and the fork stopped.
4. **The restart let a waiting Windows update finish, and Windows restarted
   twice more on its own.** Restart 1 (`shutdown.exe`, 20:31:02Z,
   "upgrade_:") was followed by TrustedInstaller, "Operating System: Upgrade
   (Planned)", at 20:33:31Z (3 s into the resumed re-measure), and again at
   20:34:44Z (System 1074; KB5129195, 26200.9457, reported installed at
   20:38:06Z). No bugcheck, no unexpected-shutdown event, no bad-block event
   in the window. The SYSTEM resume fired again after the extra restarts
   (20:35:59Z) and carried on from its saved state. Here it cost nothing.
   After a shrink, or with the handoff armed, it would not be harmless: a
   new risk, **R25**.
5. **Prologue 0.5.1's restore is proven on a real machine.** Before:
   hibernation on, automatic pagefile (as put back on 09-22). At the stop:
   journal created again, hibernation back on, pagefile back to automatic.
   Read back on 2026-09-26 after two restarts: `HibernateEnabled` 1,
   `hiberfil.sys` 5.1 GB, automatic pagefile, `pagefile.sys` present, the
   journal 32 MB, no resume task, no boot entry, C: the same size.
6. **V4's number for this disk: 9.5 GB, pinned by System Restore's
   storage** (two shadow copies at the time; two new ones since, made by
   Windows Update on 09-25 and 09-26). 25 GB is needed. What lies behind the
   shadow storage is unknown until it is gone.

Owed before run 7: the restore-point act made to say what happened (exit
code, the output without `/quiet`'s silence, a count that must drop, and no
"already deleted" unless it did); R25's refusal.

**Built (2026-09-26), prologue 0.9.0: item 3.** The restore-point act keeps
vssadmin's exit code and any error it raises. If the count did not drop, it
deletes each shadow copy of C: through its WMI object (`Win32_ShadowCopy`,
`Remove-CimInstance`) one by one, and records Windows' answer for each. The
count is read before, after vssadmin, and at the end. A pure verdict
(`Get-PrologueRestorePointVerdict`: deleted-all / deleted-some /
deleted-none / none-there / unknown) makes every log line. The second pass
now says "deleting restore points was already tried in this run (restore
points: deleted NONE of 2 ...)" where 0.8.0 said "already deleted". Five
self-test cases, one of them run 6's own record. The WMI path is Windows'
documented object, not yet run on any machine. Why vssadmin deleted nothing
on the Aspire is still unknown, and the next run records it.

**Seventh run (2026-09-26, kit 29a1d9f, prologue 0.9.0): `stopped-shrink`
at 7.2 GB** (`r18-prologue.csv` row 11, `0.9.0-physical`; job and outcome
schema-valid; nine files copied off the stick and hash-matched first).

- Scan RED on Disk health alone; keep-windows under `stop`.
- R25's gate, before any change, read all three markers false ("nothing is
  waiting for a restart"). That is the first physical reading, with no
  waiting update to compare it against.
- Cold 7.2 GB by both paths. Windows named neither the journal nor System
  Restore's storage, but **`\$Mft::$BITMAP`, the bitmap of NTFS's own
  master file table**, last cluster `0x39833cf` in all six Defrag 259 events
  (15:54-16:01Z): (62,219,007 - 60,306,383) x 4 KB = 7.3 GB, the number
  measured.
- The pagefile rung ran (one restart, the SYSTEM resume 38 s after boot),
  and the re-measure was 7.2 GB with the same file named. Fork `stop`;
  hibernation and the pagefile put back (the pagefile at the next restart,
  as the stop says).
- Neither 0.9.0 restore-point path nor the journal rung was reached. The
  file in the way now sits nearer the end than both.

What this run says, and only this: on this disk, today, the last unmovable
file is NTFS metadata that nothing on the prologue's ladder touches, and that
no Windows tool this project has evidence for can move. The MFT's valid data
grew from 1.72 GB (2026-09-22) to 1.98 GB, and its bitmap now lies in the
tail earlier runs had emptied. Three runs, three different files (journal,
System Restore's storage, the MFT's bitmap), the same disk. A cold layout on
a used, fragmented disk moves with ordinary use, and each rung buys only the
distance to the next file. **For this machine the keep-Windows path is
refused, correctly.** That is the case the discard offer (R26, designed, not
built) exists for. Whether the MFT's bitmap can be moved by a documented
Windows interface is an open question, not an argument this entry makes.

**Closes when.** The safety-copy gate uses the shrinkable number (not free
space), checked on a fragmented real-world disk *with* the mitigations
applied, so the gate reflects the shrink you can really get rather than the
cold floor. Status (2026-09-26): one such disk exists, the Aspire,
mitigated, and it is a "no" (best 9.5 of 25 GB; runs 5-7). What closes this
risk is VALIDATION V4's Pass line: the share of elevated machines that can,
which only more machines answer.

**The folder map raises the target (2026-09-26, job writer 0.10.0).** Until
now every job carried an empty folder list, so the prologue's shrink target
was `linux_min_gb` alone (25 GB). Its rule was always 25 GB plus the
harvested bytes × 1.2 (the files are pulled into Linux before Windows is
reclaimed). With the folders in the job, the rule applies for real. On the
Aspire (16.6 GB of folders) the target becomes about **45 GB**, not 25.
Keep-Windows was already refused there at 9.5 GB, so no verdict changes. But
every earlier "N of 25 GB" in this entry was measured against the smaller
target.

**Eighth run (2026-09-26, kit 594569f, job writer 0.11.0, prologue 0.9.1):
`stopped-shrink`** (`r18-prologue.csv` row 12; evidence in gitignored
`rig/hyperv/artifacts/aspire-r23-2026-09-26-run8/`). Planned as a
stop-before-CONVERT test of the folder map. The owner typed CONVERT, so the
whole prologue ran. The first launch stopped on a mistyped sentence (the
refusal works).

- The **first physical job with the folder map**. Written in a real launcher
  window, so the desktop-owner check passed (elevated `<name>` = signed-in
  `<name>`). Six folders, 15.43 GiB, 0 online-only, `stick_fit` false
  (FAT32, one file over 4 GB). Job and outcome both schema-valid.
- The prologue confirmed the target: **Linux needs 43.5 GB** (25 + 15.43 ×
  1.2).
- Cold 7.2 GB behind `$Mft::$BITMAP`. After the pagefile restart, **2.3 GB**
  behind the change journal (it moved into the freed area's way). The
  journal rung deleted it and the number went back to 7.2 GB,
  `$Mft::$BITMAP` again. Stop, with the journal, hibernation and the
  automatic pagefile put back.
- The R25 gate read nothing waiting (one check).
- The scanner's count grew: 158 bad-block events in 30 days, SMART 187 =
  748 (741 on 09-20, 742 on 09-22, 745 on 09-23, per the scan
  reports kept with each run; 725 was the 09-13 `DIAG-SMART.cmd` reading).

Owed, a third time: reading back that the pagefile returns after a restart
(no restart happened between runs 7 and 8, so run 8 began with none in use
and the setting on automatic).

## R19: cryptsetup BITLK read is a new trust dependency · medium · open (VM leg fired 2026-09-01)

**What.** The keep-Windows path (now the default) reads the BitLocker NTFS
volume from Linux through `cryptsetup` BITLK (cryptsetup's BitLocker
support), using the harvested recovery key, to copy the person's files into
Linux.

**Reframed (2026-08-22).** This risk used to read "unattended, on the only
copy, before the wipe": the worst possible setting. The design changed. The
read moved out of cutover and into `settle-in`, and on the keep-Windows path
Windows is never destroyed. So the read now happens:

- (a) with the person present, who can be asked about a stubborn unlock
  instead of the tool guessing;
- (b) after the new Linux system has been checked and works;
- (c) with the Windows partition fully intact as a backup. A failed read
  loses nothing: the person restarts into Windows and tries again.

Same `cryptsetup` mechanism, far lower stakes. Downgraded high -> medium.

**If real.** Edge-case mismatches (key protector types, XTS variants,
used-space-only encryption) stop the copy. That is recoverable now, because
Windows is still there. The real remaining danger: it reads *wrong* data,
which then checksums as whatever was (wrongly) read.

**Closes when.** Read-and-copy is checked against real BitLocker volumes
across Windows 10/11 defaults, both XTS-AES key sizes, and used-space-only
encryption. Checksums are worked out on the Windows side at harvest time (in
`evaluate`), so the Linux-side check catches read corruption, not just copy
corruption. (Only the clean-slate path still reads user data before a
destructive step, and it does so inside Windows, where there is no BITLK
problem at all.)

**VM leg fired (2026-09-01): `rig/hyperv/v3.sh`, rows in
`docs/validation-results/v3-bitlk-read.csv`.** The setup:

- Reader: the *installed* Fedora 42 of the V1b guest (settle-in's context).
- Target: the same guest's Windows 10 22H2 C:, BitLocker XtsAes128,
  used-space-only, TPM + recovery-password protectors, encrypted *then*
  shrunk (the product's order).
- Files: a planted corpus of 2,850 files (0 B to 200 MiB + 1, cluster-edge
  sizes, NTFS-compressed, sparse, Unicode and 200-character names, 20-deep
  paths), plus all 4,734 files under `C:\Users`. All hashed on Windows, then
  unlocked with the 48-digit recovery password and hashed again on Linux.

The other two setups V3 names (XTS-AES-256 used-space-only, and XTS-AES-128
"full") were built the same way (encrypt, then shrink) on copies of the
clean disk and read from the same installed Fedora as data disks. Same
outcome: corpus 2,850/2,850 under every driver.

What the run showed:

1. **The unlock works and is not fussy.** `cryptsetup open --type bitlk`
   (2.7.5 and 2.8.4) takes the recovery password on stdin or through
   `--key-file`, with or without a trailing newline, and refuses a key with
   one digit changed. Every file read back byte-identical through
   **ntfs-3g**: corpus 2,850/2,850, Users identical except files Windows
   itself rewrote between the hash and the shutdown (see 5).
2. **The kernel `ntfs3` driver on Fedora 42's install kernel (6.14.0-63)
   crashes (an "oops") reading this volume.** Five runs out of five, always
   the same signature (`page_cache_ra_unbounded` jumping through a bad
   pointer during a plain file read). The crash site moved between runs
   (`medium/`, `large/`, two different PNGs under `Users`), the reader was
   killed each time, and the whole guest froze in two of the five. After
   `dnf upgrade` to the current F42 kernel (6.19.14-108) the same reads
   complete and are byte-identical, readahead on or off. **Decided
   (2026-09-01): `settle-in` reads the kept Windows volume through ntfs-3g
   (FUSE, a driver that runs as an ordinary program), never the in-kernel
   `ntfs3`.** A fault in an ordinary-program driver is a failed process the
   person can be told about. A kernel crash mid-pull is a hung machine, and
   the pull runs on the *install* kernel, before any update. Reopen only
   with a kernel gate that is itself evidence.
3. **cryptsetup warns on every shrunk volume**: `BitLocker volume size
   85775613952 does not match the underlying device size 51415875584`.
   BitLocker's own metadata still records the size before the shrink.
   cryptsetup limits the mapping to the partition and everything reads
   correctly. This is the keep-Windows path's *normal* state: `settle-in`
   must expect the warning, not fail on it.
4. **Two file-type traps a naive copy falls into.**
   - (a) Windows' zero-byte SYSTEM-attributed files
     (`CryptnetUrlCache\Content\…`,
     `SystemCertificates\My\AppContainerUserCertRead`, 18 under one fresh
     profile) show up as **FIFOs** (pipes) under ntfs-3g. A plain `open()`
     waits forever (the harness hung an hour on one). The copy must `lstat`
     first and skip anything that is not a regular file.
   - (b) App-execution aliases under `AppData\Local\Microsoft\WindowsApps`
     are reparse points Windows hides. ntfs3 shows them as empty regular
     files. The folder map from `evaluate` should carry the reparse-point
     list, so the copy skips them by name.
5. **Harvest-time checksums are only as good as the quiet period.**
   OneDrive, Edge's WebView, Search and the content-delivery service kept
   writing under `AppData` until shutdown. 66 files changed or appeared
   between the Windows hash and the Linux read. With those services stopped,
   a crypt32 URL-cache metadata entry still changed (same size, new bytes,
   read identically by both Linux drivers). None was user data. But the same
   gap exists in `evaluate`'s harvest: hash the person's folders last, after
   stopping OneDrive (R8 territory), and treat `AppData` mismatches as
   information rather than read failures.
6. What the rig **cannot** say (rule #5 residue): real disks (fragmented
   used-space-only volumes, 4Kn sectors, half-encrypted states), Windows
   11's BitLocker (the guest is Windows 10 22H2), other vendors' recovery-key
   handling, and "full-disk" as a real disk experiences it. On the rig's
   thin VHDX, a "Fully Encrypted" whole-volume conversion grew the image by
   only ~2.7 GB, so the free space was never rewritten.

Severity stays **medium**: the mechanism works, the failures found are
design inputs with cheap fixes, and Windows stays intact throughout.

## R20: Browser profile porting is assumed, not verified · medium · open

**What.** The migration table promises that Chrome/Edge bookmarks, history
and extensions carry over by copying the profile folder. But cookies and
several other parts of the profile are encrypted with DPAPI (Windows'
per-user encryption), just like the passwords. A version gap between the
Windows and Linux builds can make the browser reset or refuse the profile.
And extension state does not reliably survive a copy.

**If real.** "Your stuff silently didn't arrive": the project's worst kind
of failure, in the feature most people will check first.

**Closes when.** Real Windows->Linux profile moves are checked for each
browser and version pair, and `evaluate`'s claims are narrowed to what the
evidence supports.

## R21: Installing alongside a shrunk Windows may not leave Windows bootable · critical · open (VM leg fired 2026-08-27; the converter's own install fired 2026-09-10)

**What.** The keep-Windows path (now the **default**) installs Linux into
the freed space. It must leave the shrunk Windows fully bootable, because
Windows *is* the rollback and the source of the files. That depends on
several things nobody has tested together, all with Secure Boot on:

- reusing the existing Windows ESP without reformatting it;
- fitting shim + GRUB + Fedora entries into an ESP that is often only
  100 MB;
- not overwriting `bootmgfw.efi` (the Windows boot loader);
- `os-prober` (the tool GRUB uses to find other systems) really detecting
  the shrunk Windows, so it appears in the boot menu.

**Why it matters most now.** The whole safety-net promise of the default
path is "Windows is still here if anything goes wrong". If the alongside
install breaks Windows boot, that promise is false at the worst moment: the
person reaches for the net and it is gone. This is harder than the
clean-slate wipe install, and the redesign made it the common case, not a
variant. It is the second-biggest project killer after the boot handoff
(R15), and had no entry until now.

**If real.** A "successful" conversion where the new Linux works but the
kept Windows will not boot: no rollback, no file source, and the person was
explicitly told they had both. Trust-ending.

**VM leg fired (2026-08-27).** First evidence, on the QEMU+OVMF rig
(`rig/vm/`, OVMF 2024.02 non-SMM, **Secure Boot off**). `rig/vm/v1b.sh`
shrank the guest's C: by 32 GiB with `Resize-Partition`, installed a Fedora
42 netinst into the freed space through a kickstart with `part /boot/efi
--onpart=sda1 --noformat`, and drove both systems through power cycles. Row
in `docs/validation-results/v1b-alongside.csv`, result
**`fallback-loader-replaced`**. The five stated checks all held, but the
run is not recorded as a bare pass, because the install changed a file
Windows had placed (finding 1). What held:

- **(a) ESP room.** The reused Windows-made ESP took the install: +7 files,
  +6,218,358 B (`EFI/fedora/`: shim, mm, grub, grub.cfg, BOOTX64.CSV; and
  `EFI/Boot/fbx64.efi`). Windows' own footprint was 28.07 MB; 34.29 MB used
  after. **The 100 MB case was not tested.** This guest's ESP is 260 MiB
  because `autounattend.xml` asked for it. The arithmetic says Windows'
  default of ~100 MiB would still hold ≈34 MB
  (`fits_100mib_esp=computed-yes`), but that is a computed column, not a
  run. A ~100 MiB ESP row is still owed.
- **(b) `bootmgfw.efi` byte-identical** (sha256 `d1f7e351…`) before the
  shrink, after the install, and in every boot row from both systems. Every
  other file under `EFI/Microsoft/` also unchanged, except Windows' own
  BCD/BOOTSTAT logs.
- **(c) Windows boots from the GRUB menu.** os-prober found it
  (`/dev/sda1@/EFI/Microsoft/Boot/bootmgfw.efi`), GRUB listed "Windows Boot
  Manager (on /dev/sda1)", and three Windows sessions were reached through
  it. Proven from *inside Windows* by reading the firmware's `BootCurrent`
  variable: it named Fedora's `Boot0002`, meaning the chainload path (GRUB
  handing over to Windows' loader), not Windows' own entry.
- **(d) Linux boots** (five sessions) and **(e) both survive power
  cycles**. Every cycle was a fresh QEMU process, with markers written by
  each system itself to the OEMDRV volume, never by hand.

Five findings, all design inputs. The five checks alone would have named
none of them:

1. **The install replaces `EFI/Boot/bootx64.efi`.** Windows Setup places a
   copy of `bootmgfw.efi` there (1,604,016 B). It is the removable-media
   fallback path the firmware uses when its boot entries are lost. Fedora's
   `shim-x64` overwrites it with shim (949,424 B) plus `fbx64.efi`.
   `bootmgfw.efi` is intact and Windows stays bootable, but "Windows' files
   on the ESP are untouched" is false as a blanket claim. What it means for
   the design: the prologue must snapshot the whole `EFI/Boot` +
   `EFI/Microsoft` tree before the install, and rollback/reclaim must
   restore `bootx64.efi`. There is a real trade-off to decide (dated
   decision pending):
   - Shim in the fallback slot means a wiped boot-entry list still reaches
     GRUB. **Observed:** after the firmware had dropped both OS entries,
     launching `EFI/Boot/bootx64.efi` went shim -> `fbx64.efi` ->
     `BOOTX64.CSV`, re-created the "Fedora" entry and showed the GRUB menu
     with Windows on it.
   - Windows' copy there means a wiped list boots Windows only.

   Either way it must be explicit.
2. **Windows registers itself again and takes the boot order.** After one
   Windows session that applied a waiting update at shutdown, the firmware
   held a *new* `Boot0009 "Windows Boot Manager"` first in `BootOrder`
   (Windows' `bcdedit {fwbootmgr}` showed `{bootmgr}` first, Fedora second).
   The next power-on booted Windows directly: no GRUB menu, Linux out of
   reach without the firmware's boot-menu key. A plain Windows session
   without an update did **not** flip it. `settle-in` must put the Linux
   entry first again after every Windows session (`efibootmgr -o …`), and
   the text the person sees must say that a Windows update can hide Linux
   until then.
3. **Firmware can drop OS boot entries wholesale.** Before Anaconda (the
   Fedora installer) ran a single `efibootmgr` call, the Windows entry was
   already gone (Anaconda's own `storage.log` proves the order). Cause on
   this rig: OVMF's fw_cfg boot-order handling (QEMU `bootindex=`) deletes
   *every* OS-created `Boot####`. Confirmed by booting into OVMF's Boot
   Manager with the CD at `bootindex=0` (Fedora and Windows both absent)
   versus with an extra USB device and no `bootindex` (both present). A rig
   artifact, but the class is real on vendor firmware too. So the
   converter's post-install step must *check* the Windows entry exists and
   re-create it (`efibootmgr -c -L "Windows Boot Manager" -l
   '\EFI\Microsoft\Boot\bootmgfw.efi'`) rather than assume the install left
   it alone. Windows re-created its own later (finding 2), so the safety
   net healed itself here, by luck of timing.
4. **os-prober was on by default in Fedora 42's Anaconda install.** The
   stock `/etc/default/grub` has no `GRUB_DISABLE_OS_PROBER` line, and the
   stock `grub.cfg` already carried the Windows entry. Setting it to `false`
   and regenerating changed nothing. Do not rely on that: set it explicitly
   in the kickstart, and keep the `os_prober_stock` column so a distro that
   flips the default shows up as a row, not a surprise.
5. **The ESP's GPT entry name was rewritten** ("EFI system partition" ->
   "EFI System Partition") even with `--noformat`. Type GUID, unique GUID
   and extent unchanged. Cosmetic, recorded because "reuse without
   touching" was the claim and the GPT entry *was* touched.

**Decided (2026-08-30):** the findings above become design, recorded in
`architecture.md` (cutover steps 8, 9, 11; rollback; settle-in) and owed as
code before any writer is built:

1. *Fallback loader:* shim stays in `EFI/Boot/bootx64.efi` while Windows is
   kept (losing the boot entries then still reaches GRUB and both systems,
   as observed). The prologue first snapshots `EFI/Boot` + `EFI/Microsoft`
   and the `Boot####` set to the stick. **Rollback restores Windows' copy.**
   The bench result flips from `fallback-loader-replaced` to
   `pass-plumbing` only when the bench runs the converter's own install
   step, which takes that snapshot.
2. *Checking after the install is a step, not an assumption:* Windows entry
   present (re-create it if the firmware dropped it), Linux entry first,
   `bootmgfw.efi` matches the snapshot, `grub.cfg` lists Windows. Every
   result written to `outcome.json`.
3. *os-prober is set explicitly* in the kickstart, never left to the
   distro's default.
4. *`evaluate` gate:* ≥ 32 MiB free on the ESP (5× the measured 6.2 MB),
   and the ESP is the volume the Windows Boot Manager entry points at.
   Otherwise steer to clean slate. **Landed (2026-08-30):**
   `upgrade-scan.ps1` "Boot partition (ESP)". Collection (`Get-UpgEspFacts`,
   elevated-only, mounts the system partition and resolves the `{bootmgr}`
   device line) sits behind a seam, apart from judgment (`Test-UpgEsp`).
   Six self-test cases (ok / full-ESP warn / wrong-volume warn /
   unresolvable unknown / unelevated info / failed-query unknown), `dist/`
   rebuilt. A warn steers to clean slate. RED is never involved: the
   machine still converts.
5. *Windows taking back the boot order* is a standing hazard for the whole
   life of the dual boot, not an install-time fact. Split out as **R22**.

**Hyper-V leg opened (2026-08-30).** `rig/hyperv/`: a Gen 2 guest with real
Secure Boot, a vTPM and a **100 MiB ESP** (Windows Setup's default) is
installed. First finding before any run: Hyper-V's two Secure Boot dbs are
one-or-the-other (the Windows CA *or* the third-party UEFI CA that signs
shim; measured by A/B, table in `rig/hyperv/README.md`). So a real machine's
db, which holds both CAs, cannot be copied there. The Secure-Boot-on
*chainload* clause can still be tested by enrolling the Windows PCA into
shim's MokList. The db-composition clause and the vendor matrix stay
physical.

**Hyper-V leg fired: the ~100 MiB ESP row (2026-08-31).**
`rig/hyperv/v1b.sh`, Hyper-V UEFI Release v4.1, Secure Boot off. (The
guest's locked `MicrosoftWindows` template refuses shim, so the plain dual
boot needs Secure Boot off here. The Secure-Boot-on chainload is the MOK
experiment, still owed.) Windows Setup's default **100 MiB ESP
took the install with room to spare**: Windows' own footprint 28.3 MB,
Fedora added the same 7 files / 6,218,358 B as on QEMU, 65,994,752 B still
free after. `fits_100mib_esp` is now tested, no longer only computed. All
five checks held (2 Windows boots, both with `BootCurrent` = Fedora's entry;
2 Linux boots; `bootmgfw.efi` byte-identical in every row). Result
`fallback-loader-replaced` again: shim replaced `EFI/Boot/bootx64.efi`, so
finding 1 is reproduced on a second firmware. What this leg adds:

- **BitLocker, as the product will meet it (the QEMU guest had no TPM).**
  C: FullyEncrypted XtsAes128, Tpm + RecoveryPassword protectors. Shrinking
  the encrypted volume worked unchanged. Suspended `-RebootCount 1` before
  the installer boot, per run-book. The first GRUB-chainloaded Windows boot
  used up the suspension and **turned protection back On by itself,
  re-sealing against the shim -> GRUB -> `bootmgfw.efi` path**. The second
  chainloaded boot then **unsealed silently with protection On**: no
  recovery prompt anywhere, protectors and recovery password intact
  throughout. On this firmware the prologue's suspend-once flow is exactly
  right for the alongside install.
- **Hyper-V's UEFI kept the OS `Boot####` entries.** Unlike OVMF's fw_cfg
  path (which deleted them all, finding 3), the Windows entry survived the
  installer boot untouched. Anaconda put Fedora first in `BootOrder` and
  Windows stayed present. Entry deletion is firmware behaviour, not a
  constant, so the converter's post-install check-and-recreate step stays.
- Finding 4 (os-prober on by default in stock F42) and finding 5 (the ESP's
  GPT name case-changed despite `--noformat`) both reproduced.
- **Rig hazard found and fixed mid-run.** WSL's /mnt/c 9p page cache served
  hours-old VHDX pages (a pre-update `bootmgfw.efi`) to the offline
  inspector, and a `cp` baked them into a backup. It was caught because the
  guest's own hash disagreed. Every WSL read of a Windows-written file now
  clears the page cache first (`posix_fadvise DONTNEED`, in `v1b-inspect.py`
  and `v1b.sh`). The spoiled backup was deleted and taken again. Detail in
  `rig/hyperv/README.md`.

Row 2 of `docs/validation-results/v1b-alongside.csv`.

**Hyper-V leg: the Secure-Boot-on chainload fired (2026-08-31).** The one
Secure-Boot-on clause a VM on this host can reach. Second guest `UPGRIGMOK`
(`MicrosoftUEFICertificateAuthority` template, so the firmware trusts shim).
Fedora was installed alongside **under Secure Boot enforcing**
(shim/GRUB/kernel all checked by the firmware; `bootmgfw.efi`
byte-identical `d1f7e351…`; os-prober found Windows). Then the Microsoft
Windows Production PCA 2011 certificate, taken from this guest's own
`bootmgfw.efi` signature, was enrolled into shim's MokList. Result, both
directions:

- **Negative:** with the PCA *not* enrolled, GRUB -> `chainloader
  bootmgfw.efi` is refused: `bad shim signature` (shim checked db + MokList
  and declined). Windows cannot be booted through GRUB. The Secure Boot gate
  is real on the chainload path.
- **Positive:** with the PCA enrolled (`mokutil --test-key` -> already
  enrolled), the same GRUB entry boots Windows 10 to the desktop. Secure
  Boot enforcing was confirmed from Fedora (`mokutil --sb-state`) *and*
  Windows (`Confirm-SecureBootUEFI` -> True). BitLocker turned itself back
  on, no recovery prompt.

**What it does NOT show:** a real machine's db holds *both* CAs and boots
`bootmgfw.efi` from Windows' own firmware entry, with no shim/MokList in the
path. Hyper-V's one-or-the-other templates cannot express that. So this
proves only the chainload *check*, not the db composition, and it leaves
Windows' own firmware entry refused under this template. The db-composition
clause and the ≥3-vendor physical matrix stay open. Full record and the
what-it-does-not-show list:
`docs/validation-results/v1b-mok-chainload-2026-08-31.md`.

**Still open: this rig cannot close them.** Secure Boot enforcement lives in
the SMM-requiring OVMF build that crashes KVM on this AMD/WSL2 host (see
R15), so the QEMU run had Secure Boot off. The shim -> GRUB ->
`bootmgfw.efi` chainload with Secure Boot *on* fired 2026-08-31 through the
MOK experiment (above), and the ~100 MiB ESP row fired the same day. What a
VM on this host still **cannot** show: a real machine's db holding *both*
the Windows CA and the UEFI CA. That combination lets vendor firmware boot
Windows from its own entry with no shim in the path (Hyper-V's templates are
one-or-the-other). That db-composition clause and the ≥3-vendor physical
matrix are what the close condition really names. A VM pass narrows R21. It
does not close it (CLAUDE.md rule #5).

**Closes when.** An alongside install is proven on real machines from
several vendors, Secure Boot on: Windows still boots from the menu
afterwards, GRUB lists it, the shared ESP had room, and `bootmgfw.efi` is
intact. This is the keep-Windows half of the V1 gate, and should be proven
as its own Tier-1 item, not folded in as a "safety-copy variant". Fallback if
it proves unreliable on some firmware: those machines are steered to clean
slate (which never shares an ESP), and `evaluate` says so before committing.

**The converter's own install, with the decided design built in
(2026-09-10).** `upgrade_/linux/verify.sh` snapshots `EFI/Boot` and
`EFI/Microsoft` (every file, sha256) plus the firmware's `Boot####` entries
to the stick before Anaconda touches the ESP. `upgrade_/linux/outcome.sh`
runs the checklist in `%post` (the script the installer runs after
installing):

- Windows Boot Manager entry present (re-created with `efibootmgr` if the
  firmware dropped it; Hyper-V kept it on all three runs this time);
- Linux first in `BootOrder`;
- `bootmgfw.efi` against the snapshot;
- `grub.cfg` lists Windows;
- the fallback slot named (shim, kept on purpose).

It writes every result to `outcome.json`. Three rig runs in
`docs/validation-results/v2-install.csv`; row 3 `pass-plumbing`. Residue
unchanged: Secure Boot on, vendor firmware.

**The restore half is built (2026-09-12).**
`upgrade_/windows/Invoke-Rollback.ps1` (`ROLLBACK.cmd` on the stick, run
from the kept Windows) does every read first: job, outcome (`keep-windows`,
Windows kept, snapshot named), this disk's identity against the job, the
snapshot's checksum for `EFI/Boot/bootx64.efi`, and what the slot holds
now. Then the two writes:

1. the file back from the snapshot (its own checksum checked before and
   after; the shim copy saved to `upgrade_/rollback/`);
2. `{bootmgr}` first in `{fwbootmgr}` `displayorder`.

It deletes nothing. 14 self-test cases. Rig row 1 (2026-09-12)
`pass-plumbing` in `docs/validation-results/r21-rollback.csv`: Windows'
loader back byte for byte, `{bootmgr}` first, a keyless start reached
Windows, GPT and `EFI/fedora` untouched. The Linux-side twin comes with
`settle-in`.

## R22: Windows servicing re-takes the firmware boot order · medium · open

**What.** On the keep-Windows path Windows stays installed. Windows Update's
boot-file servicing registers the *Windows Boot Manager* firmware entry
again and puts it **first** in `BootOrder`. Seen on the QEMU rig
(2026-08-27, during V1b): one Windows session that applied a waiting update
at shutdown was enough. The next power-on booted Windows directly with no
GRUB menu, while a plain Windows session without an update changed nothing.
This is not a one-time install event. It can happen again at any Windows
update, for as long as Windows is kept.

**If real** (it is; the only question is how often on vendor firmware). The
person restarted into Windows once to fetch something, an update ran, and
now the machine "went back to Windows". Linux looks gone, and the files they
already pulled look gone with it. Nothing is lost (Fedora's entry and
partitions are untouched), but a non-technical person cannot know that, and
this is exactly the person the project exists for. It damages trust, but
loses no data: medium.

**Decided (2026-08-30).** `settle-in` installs a boot-time unit that puts
the Linux entry first again whenever a Windows session moved it. The welcome
screen names the firmware boot-menu key for the one boot where the person
has to step in. Reclaim removes both along with Windows. See
`architecture.md`, settle-in.

**Closes when.** The unit is built, and a Windows update on a real machine
is followed by a Linux boot with no action from the person. And the physical
matrix shows whether any vendor firmware ignores the re-assertion (some
firmware pins its own order; that would move this to "press one key", like
V0's fallback).

---

# Resolved

Kept for the record. All four were in code that read correctly, and all four
were also in code that had no tests: F1-F3 in the harvester, F4 in the
app-risk database. As of 2026-08-22 each is locked in by a self-test
regression case (harvester `-SelfTest` for F1/F3 and the truncation/SSID
paths; scanner `-SelfTest` for F4's "Microsoft Visual Studio Community 2022
must match"). F2 has no direct test. It was a bug in how the netsh call
itself was invoked, which sits on the live side of the parse seam. But the
seam now keeps the parsing logic, where a silent empty result would hide,
under test.

## R23: The acknowledged-data-loss path exists · high · open (decided 2026-09-13; in use on the Aspire since 2026-09-20)

**What.** Rule #1 said there is no override for a RED verdict, ever. On
2026-09-13, after the Acer Aspire's SSD was diagnosed as failing (R18) and
the owner asked for a way to go ahead anyway, the project owner decided to
build one, narrowly:

- A separate launcher (`RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd`) on which the
  person types, word for word and with the same capitals, *"I confirm that I
  understand the risks and could lose data"*.
- The job writer then accepts a RED verdict **only if every failing
  hardware check is one of two**: `Disk health`, `Volume health`. It records
  `risk_acknowledgement` (`statement`, `accepted_utc`, `overrides`) in
  `job.json`.
- The prologue requires the same sentence typed for its run, and lifts
  exactly the listed refusals (the disk-health gate, the persistent-flag
  stop).
- Every screen and record says DATA LOSS ACCEPTED. `outcome.json` carries
  the block.
- The schemas encode the limits (`schemas/check.py`). Each of these is
  refused: a paraphrase, an empty override list, `identity` as an override,
  RED without the block, a repair on a non-Healthy disk without the block.

**What it never lifts.** Identity mismatch, a moved or non-USB stick,
legacy BIOS, an unknown BitLocker state, RST/VMD, a failed image read-back,
a failed ESP snapshot, a schema-invalid job. Their failure is "cannot work"
or "wrong machine", not "this machine's own files".

**If real (the risk).** The exception is how the first destroyed-photos
incident happens. The sentence gets pasted from a forum, or typed by the
person whose files are on the line because some tool told them they were
being overcautious. The safeguards are the sentence itself (no short form,
no flag), the separate launcher (the normal path never shows an escape
hatch), the loud record, and this entry. **The path is for machines whose
files are already copied off.** The launcher says so three times.

**Closes when.** It never closes. It is a standing cost. What can change is
its width, and the rule is that it does not widen.

## F1: `break` inside `ForEach-Object` terminated the whole script · fixed

`Get-HarvestFolderStats` used a 45-second stopwatch and `break` to limit
work on large folders. With no loop around it, PowerShell unwinds past the
function and **ends the entire script, silently, with exit code 0**. No
error, no `state.json`, no sign anything went wrong. It would have fired for
anyone with a large Documents or Pictures folder. It went unnoticed because
the test machine's largest folder holds 447 files.

Replaced with `Select-Object -First`, which stops a pipeline correctly.

## F2: `$args` collision reported zero Wi-Fi networks · fixed

A local variable named `$args` inside a function hides PowerShell's
built-in `$args`. Passing it on sent nothing to `netsh`, which exported no
profiles, and the harvester cheerfully reported **0 networks on a machine
with 14**. Renamed to `$netshArgs`.

## F3: UTF-8 SSIDs mangled · fixed

`Get-Content -Raw` reads as ANSI on PowerShell 5.1, turning `<name>'s
iPhone` into `<name>â€™s iPhone`. Any network name (SSID) with a curly
quote, accent or emoji would have made a NetworkManager profile that never
connects. And it would look like a broken Wi-Fi driver, not a text-encoding
bug. Switched to `XmlDocument.Load()`, which honours the declared encoding.

## F4: Visual Studio rule could never match · fixed

`(?<!Microsoft )Visual Studio (?!Code)` excluded "Microsoft Visual Studio
Community 2022": the very product the rule was written to flag, and how
nearly every real install is named. Corrected to `Visual Studio (?!Code)`.

## R24: The walk-away resume: a SYSTEM startup task and a RunOnce entry · high · open (decided 2026-09-13)

**What.** The prologue restarts Windows at least once before the handoff
(the disk check; the pagefile re-measure) and must carry on with nobody at
the keyboard. The fork is chosen in advance in `job.json`, so nothing after
the typed word needs a person.

Until 2026-09-13 the resume was an elevated *logon* task in the person's
session. Every row that had ever fired had someone signed in (the rig signs
in automatically; the Aspire's owner signed in). So the walk-away half of
the promise had never been tested.

Decided 2026-09-13: the resume runs as `NT AUTHORITY\SYSTEM` at **startup**
(`AtStartup`, `ServiceAccount`, `StartWhenAvailable`). It polls for the
stick by volume id, records who ran it and whether a session existed
(`state.Resumes`), and queues anything a person should read as a one-shot
HKLM `RunOnce` notice for the next sign-in.

Holding the person's Windows password for an automatic sign-in was
considered and refused (`architecture.md`, "The walk-away resume"):

- Microsoft-account holders often sign in with a PIN.
- Windows 11 accounts can be passwordless.
- The secret would sit on a disk settle-in later mounts.
- A third-party password box looks exactly like phishing.

**Two stakes that are not R18's.**

1. **Antivirus exposure (with R12).** A SYSTEM `AtStartup` task registered
   by an unsigned script, plus an HKLM `RunOnce` value, is exactly the
   "survive a restart" pattern Defender and SmartScreen score. If Defender
   removes the task or the RunOnce entry, the conversion stalls at the
   sign-in screen with Windows intact (the task is one-shot; `state.json`
   stays; `-Resume` can be run by hand). A stall, not a loss, but the
   walk-away promise is broken on that machine.
   - **Evidence that it trips:** a Defender detection during a probe or a
     conversion, on the rig or a physical machine, recorded in
     `walkaway-probe.csv` / `r18-prologue.csv` notes (event log
     `Microsoft-Windows-Windows Defender/Operational` 1116/1117 naming the
     task or the script).
   - **Fallback if it does:** `shutdown /g` (Automatic Restart Sign-On,
     what Windows Update uses: Windows holds the credential, TPM-protected
     where available), with the resume back on an at-logon trigger. Or the
     signed release (R12), which is the real answer.
2. **The privilege surface.** A task that runs as SYSTEM from a script
   under `ProgramData` hands full control of the machine to any standard
   user who can replace the script. The safeguard: `Protect-StateDir`
   strips inherited permissions and grants SYSTEM and Administrators full
   control, Users read. It **refuses to register the task** if any
   Users/Everyone/Authenticated write permission survives. The safeguard is
   evidence, not argument: the resume records the permissions as read back
   (`state.StateDirAcl`, from 0.3.1), and the rig row of 2026-09-13 read
   them back as exactly that. The task is removed by every way out (return,
   stop, abort, probe).

**Residue, stated plainly.**

- **BitLocker with a PIN or startup key:** the machine cannot boot without
  a person, whatever the task does. The resume runs only after the volume is
  unlocked. Not a fault of the mechanism, but the walk-away promise does not
  apply to those machines, and `evaluate` should say so (the harvester reads
  protector types; owed: name it in the report).
- **TPM measurements:** the disk-check restart itself changes nothing the
  TPM measures. The row on the rig (BitLocker off) cannot say what a
  TPM-only machine does at that restart. The Aspire probe (BitLocker off)
  cannot either. A BitLocker-on physical row is owed before the promise
  covers it.
- **Fast Startup:** a hybrid *shutdown* is not a restart. The prologue uses
  `shutdown /r`, which is a real restart whatever the setting, and
  `AtStartup` fires on it. Untested claim; owed: one row with Fast Startup
  on.
- **Managed devices:** a domain / Entra / Intune policy that forbids task
  creation or runs scripts through AppLocker breaks the resume before the
  restart, where the prologue *can* refuse. Scanner 0.3.0 reads the
  Schedule service, the Task Scheduler creation policy and the join state
  (info/warn). The job writer has that fact before the prologue meets it.

**Evidence so far.**

- `r18-prologue.csv` row 6 (rig, automatic sign-in off, both resumes SYSTEM
  in session 0, 472 s from the check restart to the first Linux boot,
  `query user` empty throughout).
- `walkaway-probe.csv` row 1 (rig) and row 2 (Acer Aspire A515-51G,
  InsydeH2O V1.21, Windows 11 Home 22631, Secure Boot on, a real USB stick:
  SYSTEM in session 0, 38 s after boot, the stick seen 5 s later, notice
  queued, task removed). The probe is the half-hour-visit row for every
  borrowed vendor (VALIDATION V0's matrix).
- On the product path, the Aspire's own conversion runs (`r18-prologue.csv`
  rows 7, 8, 10 and 11, 2026-09-20..26) each resumed as SYSTEM in session 0
  with nobody signed in. Run 6 did so through two Windows Update restarts it
  did not ask for (R25).

**Closes when.** Probe rows from ≥3 more vendors, one BitLocker-on row, one
Fast Startup row, and no Defender detection across them. Or a signed
release (R12), after which the antivirus half no longer matters.

## R25: Windows Update restarts the machine during the prologue · high · open (found 2026-09-23; gate built 0.9.0; detector fired on the rig 2026-09-26)

**What.** The prologue restarts Windows before the commit line (the disk
check, the pagefile rung) and relies on Windows restarting only when the
prologue asks. On the Aspire's sixth run (R18), the prologue's own restart
let a waiting cumulative update (KB5129195) finish. Windows' installer
(TrustedInstaller, "Operating System: Upgrade (Planned)") then restarted the
machine **twice more**, the first time 3 s into the resumed re-measure.
Windows had also moved from build 22631 to 26200 between runs 5 and 6, with
no one asking. Updates arrive on their own schedule. A machine being
converted is an ordinary Windows until the commit line.

**If real** (it is; the only open question is when it bites).

- Before the shrink, it costs a re-run of the measurement. The resume
  carried on from its saved state, as built.
- After the shrink, before the handoff, an update restart is harmless to
  the plan but can change what the prologue already measured.
- **With the one-time boot armed, an update restart could use it up.** The
  firmware would boot the stick while Windows expected to come back and
  finish updating. What Windows does then (resume the update at the next
  Windows boot, roll the update back, or be left unbootable, which is the
  safety net) is not known. Nothing here has been observed, and it is the
  question this risk exists for.

Until it is answered it is treated as the worst of those: a broken safety
net before the commit line. High.

**Decided (2026-09-26, the owner's call: "get the update out of the way")
and built, prologue 0.9.0: refuse, by default, before it can bite.** The
prologue reads Windows' pending-restart state before it changes anything,
and again right before it arms the handoff. It does not arm, or start,
while Windows says an update is waiting for a restart. The signs Windows
gives must be confirmed on a real machine before any of them is trusted
(rule #2: the registry keys commonly cited are not a documented contract).

As built:

- `Invoke-UpdateGate` reads three markers (Component Based Servicing
  `RebootPending` and `RebootInProgress`, Windows Update `Auto
  Update\RebootRequired`), records each check with all three values, and
  counts any one as pending.
- Before anything changes, and again before the shrink, a waiting update
  gets a restart of the prologue's own (stage `update-restart`). The SYSTEM
  resume then gives Windows up to 10 minutes to finish, through any
  restarts of its own, and checks again. At most three. After that the run
  stops at `windows-update` with the reason.
- Right before the arm there is no restart: a waiting update there stops
  the run, and the stop grows C: back.
- Both launchers tell the person the computer may restart first to let an
  update finish.
- `outcome.json` carries `prologue.windows_update` (checks, pending_seen,
  restarts; schema and two negative cases). Five self-test cases on the
  judge (130 pass).

Unproven: whether those markers are set when an update is waiting on real
Windows 11 26200. The Aspire's next run records them at every gate, which is
the evidence this entry is waiting for.

**Fired on the rig (2026-09-26, V9 runs, Windows 10 19045).** The rig's
install-day disk copy carries a real waiting update (CBS `RebootPending` =
True). Prologue 0.10.0 read it at `before-changes`, restarted once, resumed
as SYSTEM, read nothing waiting after the restart and again before arming,
then armed: `prologue.windows_update` = 3 checks, pending seen, 1 restart.
A real pending state, not an injected one, but Windows 10 on the rig.
Windows 11 26200 is still owed.

**Closes when.** A physical row in which a waiting update exists at CONVERT
and the prologue neither arms nor shrinks until it has cleared, and a rig
row that injects the pending state.


## R26: The offer to discard Windows after a failed shrink · critical · open (designed 2026-09-26, not built)

**What.** When keep-Windows cannot fit and the person chose *ask me then* at
CONVERT, the prologue stops as it does today. Then, at the next sign-in, it
offers to delete Windows and keep only the listed folders, staged to the
stick (`architecture.md`, "When Windows cannot be kept"). It is the owner's
design, prompted by the Aspire (R18): a disk that cannot free 25 GB after
every mitigation, and a person left with "stop" or a wipe they had to choose
blind.

**If real.** What can go wrong. Each one is a trust-ending way to lose
files:

- **A file outside the folder map is gone.** Anything not in the known
  folders (a `C:\Projects`, a second user's profile, mail stores, a game's
  saves) is not staged. The window names the rule, and a person will still
  not read it. The folder map's coverage is the first thing to prove.
- **The stick is the only copy for a while.** Cheap flash that lies about
  its size or drops out under writes (the Aspire's stick does; R17) turns
  "staged" into lost. The read-back in the live session is a hard gate. It
  has to be proven with a failing stick, not argued.
- **Consent under frustration.** The offer comes after something the person
  wanted did not work. The design's answers: a typed sentence, "Keep
  Windows" as the default, the numbers on screen, and the two-minute check
  at the wipe. Whether people read them is not something this project can
  prove alone.
- **Stale numbers.** Days can pass between the stop and the yes. The design
  measures again when the window opens, and again in the prologue. A gap
  between staging and the wipe (files changed after staging) is lost work:
  small, but it must be named in the window.
- **Programs and their licences.** Installed programs are not carried (the
  software inventory lists them). Product keys and licences that live only
  in Windows are lost with it.
- **OneDrive online-only files** copy as empty stubs if read naively (R8).
  Decided 2026-09-26: they are not downloaded; they stay in OneDrive. The
  staging must skip them and never copy a stub as the file, or refuse.

**Decided (2026-09-26).**

- Stop is the default at every step.
- The offer is made only on the failure branch, to a person who chose *ask
  me then*.
- No offer when the files do not fit the stick.
- The consent is the typed sentence, word for word, separate from R23's,
  and it never lifts a refusal.
- A "yes" only re-runs the normal chain (job writer, kickstart, prologue)
  with a new job carrying the sentence and the numbers shown.
- The wipe stays in the live session, behind the checksum read-back and the
  human check.

Building waits for the harvest (folder map), staging at real size, the
live-session check, the cutover restore and `settle-in`, and for a first
physical install row (V1/V1b). CLAUDE.md rule #4.

**The harvest half built, and the Aspire measured (2026-09-26).** The folder
map is in the job, and `harvest.stick_fit` answers "do the files fit on the
stick" (job writer 0.10.0; `architecture.md`, "one stick, honestly sized").
Read-only over SSH:

- The Aspire's six folders hold **16.6 GB** (15.4 GiB) in 35,351 files.
  Desktop, Documents and Pictures are redirected into a work-or-school
  OneDrive; 0 online-only, 0 unreadable. 10.2 GiB of it is in Downloads,
  including one 5.9 GB file.
- On today's 8 GB stick (FAT32 kit volume, 1.71 GiB free) **the offer would
  not be made**. The files need at least 15.8 GiB, and the 5.9 GB file can
  never go on FAT32 (FAT32 cannot hold a file over 4 GB). It would take a
  stick with at least ~17 GB free, staged to exFAT.
- The coverage question, answered for one machine. Outside the map sit:
  70 GB of AppData (mainly a Docker disk, 36 GB, and a **WSL Ubuntu disk,
  4.9 GB, that holds the person's Linux-side files**), an Outlook cache
  (can be downloaded again), ~2.2 GB of tool folders in the profile, and
  38.9 GB of `Windows.old` (the 2026-09-23 feature update).
- One other profile was found: `defaultuser100000`, a Windows setup
  leftover with no local account and 0.11 GB. A clean-slate job refuses on
  it today (R5). That is the cautious direction until a rule for setup
  leftovers has evidence.

Row: `docs/validation-results/harvest-folder-map.csv`. Found while building:
the prologue stages to the FAT32 kit volume, not the exFAT `UPGDATA`
partition. Moving it is owed before the offer is built.

**Closes when.** On the rig: a failed shrink -> the offer -> the typed yes
-> staged and read back -> wipe install -> every staged file restored with
matching checksums, and the "files do not fit" and "stick drops" paths
refusing before the wipe. Then one physical row, on a machine whose owner
has chosen to lose Windows. Plus evidence of the folder map's coverage on
real machines (what share of a person's files it finds).

## R27: The one-click erase and install · critical · open (decided 2026-09-26; all three arms pass on the rig 2026-09-26; physical run 9 failed on a text login, fixed; re-run owed)

**What.** A launcher that, after one typed sentence and a password, erases
every internal drive and installs Fedora with nothing kept
(`architecture.md`, "Erase and install"). It is the first path whose commit
line destroys everything on the machine. The owner chose it as the first
destructive path to prove end to end: the mechanism before the migration.

**If real.** What can go wrong:

- **The wrong drive is erased.** A stick moved to another computer, a USB
  disk mistaken for an internal one, a drive swapped since the job was
  written. The answer is identity. Every drive to be erased is named in the
  job by serial, unique id and size. The prologue checks them again in
  Windows, and `verify.sh` again in the installer: exact size, or refuse.
  The installer is told to use only those drives. (Written before the
  build. Since fired: rig arm C, and the Aspire's run 9 matched both drives
  by identity. One machine with two drives, not a matrix.)
- **The countdown does not show, or does not cancel.** `%pre` normally logs
  to a file. The countdown has to reach the screen and read a key from it
  inside Anaconda's environment. Must be seen on the rig and on the
  Aspire, both arms: cancel (Windows comes back untouched) and time out
  (the erase). (Written before the build. Since seen: both arms on the rig;
  the time-out on the Aspire, run 9, 120.0 s. Cancel on real hardware is
  still owed.)
- **Consent is a sentence people type without reading.** The sentence says
  exactly what happens, the countdown is the second chance, and the
  launcher is separate from every other one. Whether that is enough cannot
  be proven here.
- **A failing system drive (the Aspire's) fails the install half-way.**
  After the commit line there is no Windows to go back to. The stick can
  run the install again. On the Aspire this is accepted under R23 and is
  itself evidence.
- **The password.** Hashed on Windows (SHA-512 crypt, written in PowerShell
  5.1, no library) and never stored in clear. A wrong hash means an account
  nobody can sign in to. Checked against the published test vectors and by
  signing in.

**Decided (2026-09-26, the owner's answers).**

- Two drives: the system on the drive holding C:, `/home` on the second
  internal drive.
- A 2-minute countdown where any key cancels and silence goes ahead.
- The sentence "I confirm that everything on this computer will be deleted
  and nothing will be kept", on its own launcher, with R23's sentence also
  typed on a RED machine.
- Carrying files across is stage 2.

**Built (2026-09-26).** Job writer 0.12.0, `Read-Password.ps1` 0.1.0,
prologue 0.10.0, `verify.sh` 0.4.1, `outcome.sh` 0.3.0, the two
`RUN-ERASE-AND-INSTALL` launchers, schema `erase_consent` /
`cutover.countdown`. **Fired on the rig the same day**
(`docs/validation-results/v9-erase.csv`, `rig/hyperv/v9.sh`):

- **A, refuse** (`refused-before-countdown`). A job naming a home disk that
  is not attached. The installer found the system disk, named the missing
  one, wrote no storage plan and stopped. Both disks unchanged, Windows came
  back. (0.4.0. The refusal comes before the countdown, which is all 0.4.1
  changed.) Finding: the screen shows Anaconda's raw Python traceback for a
  `%pre` refusal. A person needs a plain sentence there (owed).
- **B, cancel. The first attempt FAILED, and that row stays.** With 0.4.0's
  bash countdown (`read -t 1 -n 1`), a key pressed near its end left bash
  5.2.37 stuck in `read(2)` on tty6 with its timeout dead (kernel
  `wait_woken`, `FIONREAD` 0). It froze at 0:02, neither cancelling nor
  erasing. Nothing was erased. A power-off brought Windows back, and the
  prologue's return recorded "Windows came back before the countdown
  ended". The same run showed tty6 at 12 columns (text wrapped).
  **0.4.1** runs the countdown in Python: raw mode once for the whole two
  minutes, input flushed at the start, `select()` on a non-blocking
  descriptor against a monotonic deadline, Ctrl-C a key like any other, a
  console under 40 columns set to 80x25. Tested on a pseudo-terminal (no
  key, a key, an early key, 12 columns, Ctrl-C), then **B passed**
  (`cancelled-untouched`: cancelled 47 s in, "CANCELLED. Nothing was
  erased." on screen, back in Windows, the prologue's outcome `stopped_at
  countdown`, both disks unchanged). A run with a stray keystroke later
  cancelled 0.3 s in. Same behaviour, not recorded as a row (a mistake in
  running the test, kept as a capture).
- **C, erase** (`erased-installed`, twice on 0.4.1 plus once on 0.4.0). The
  countdown ran its 120 s, and its end is `crossed_utc`. Both disks were
  cleared: EFI + `/boot` + `/` on the system disk and `/home` on the second.
  `outcome.json` schema-valid with credentials wiped. Fedora booted in about
  5 minutes, and the account's stored hash is the one made on Windows. In
  one C run the home disk carried the shape of the Aspire's 1 TB drive: GPT,
  one partition, LVM PV, VG `ubuntu-vg`, LV `ubuntu-lv`, ext4 (made from the
  installer's shell during the countdown). The installer found it and
  removed it.
- Also seen: R25's update gate fired for real on the rig (see R25).

**First physical run, the Aspire, 2026-09-26 (run 9): a FAILURE of the
one-click promise.** `[FAIL]` The launcher, both sentences, the password,
the countdown, the erase and the install all ran with nobody at the
keyboard. Fedora booted from the SSD, and the password chosen on Windows
signed in. But the machine came up at a **text** "fedora login:" prompt,
and the owner had to be told a command to reach the desktop. The owner's
ruling: "it's gotta be one click". A person must never meet a console.

Cause: the kickstart never asked for a graphical login, and an installer
run in text mode leaves the installed system at `multi-user.target`
(text-only startup). The rig could not see it: its first-boot marker
powered the guest off before a person would have looked.

Fixed the same night:

- kickstart generator 0.3.0 (`xconfig --startxonboot`, and `%post` sets
  `graphical.target`);
- `outcome.sh` 0.3.1 records `install.boot_target`;
- the outcome schema refuses a completed conversion that is not
  `graphical.target`;
- the rig verdict now requires the display manager (the graphical sign-in
  screen) running at first boot (`graphical_login`, `v9-erase.csv` line 8:
  y).

**Physical row written** (`v9-erase.csv` line 11, `fail`; evidence in
gitignored `rig/hyperv/artifacts/aspire-r27-2026-09-26-run9/`, 42 files
hash-checked against the stick):

- both drives matched by identity (SSD by serial, 1 TB by unique id);
- the KDE image read back at 25.0 MB/s; Wi-Fi and display passed;
- the countdown ran 120.0 s on a 240x67 console;
- the storage plan put EFI + `/boot` + `/` on the SSD and `/home` on the
  1 TB drive; the install took about 7 minutes;
- `grubenv` read `upg_fired=1` (the handoff fired, Secure Boot on);
- 0 disk I/O errors in `dmesg` at `%pre`;
- the owner's password signed in.

The outcome record fails today's schema (it names no `boot_target`), which
is the new rule catching this run. `anaconda-ks.cfg` confirms the cause:
Anaconda wrote `skipx` itself.

**Three more findings, owed:**

- **The installer's clock was 4 hours behind.** Windows keeps the hardware
  clock in local time, and the installer read it as UTC. Every Linux-side
  time in the run is shifted, so `crossed_utc` "19:48:11Z" was really about
  23:48Z, and the installed Fedora's clock is wrong until it reaches the
  network. The records must carry a correct time.
- **A stale "Windows Boot Manager" firmware entry** is left pointing at an
  erased disk (`efibootmgr-after.txt`). Fedora is first, so it is harmless,
  but an erase should remove it.
- **A `%pre` refusal shows Anaconda's raw traceback** (rig arm A). It needs
  a plain sentence. **Built 2026-09-27** (`verify.sh` 0.5.0, the owner's
  words): one plain screen, 60 s, then an unchanged restart. **Fired on
  the rig the same day** (`v9-erase.csv` line 14, arm A,
  `refused-before-countdown`, both disks unchanged): the screen showed the
  approved words, counted down, and the rig restarted into an untouched
  Windows; `report/refusal.json` on the stick holds the plain reason.

**Read back over SSH, 2026-09-27, the day after run 9** (read-only, no
root; capture in the gitignored
`rig/hyperv/artifacts/aspire-fedora-ssh-2026-09-27/`):

- **The 4-hour clock, confirmed on the installed system.** The journal's
  first boot is stamped 15:55 EDT for a run that crossed the commit line
  near 19:48 EDT. The time service then logged "System clock wrong by
  14400.686744 seconds" (exactly 4 h) and stepped the clock, about 20 hours
  after that first boot. Why so late is not known from this capture (no
  network at first is likely: run 9 carried no Wi-Fi). Since then the
  hardware clock holds UTC ("RTC in local TZ: no"; RTC read 20:43:28 when
  the G16 read 20:43:31 UTC). So a Windows reinstalled on this machine
  starts 4 h wrong the other way until it syncs.
- **The stale entries are gone, and nothing of ours removed them.** Run
  9's `efibootmgr-after.txt` listed "Windows Boot Manager" (Boot0003) and
  a firmware-made "Unknown Device" (Boot0000) pointing at the old Ubuntu
  disk's `\EFI\ubuntu\shimx64.efi`. A day later Boot0003 is gone and
  Boot0000 points at Fedora's `\EFI\fedora\shim.efi` on the new ESP.
  Insyde's firmware appears to prune and rebuild its own "Unknown Device"
  entries (they carry its `RC` marker), but that is a reading of one
  before-and-after, not a cause shown. Someone at the keyboard could also
  have changed them. Our own "upgrade_" entry (Boot0002, the stick) is
  still there, as on the rig: run 9 had no `settle-in` to remove it.
- **SMART, read with the owner's permission (`sudo smartctl -a`):** the
  SSD's 187 Reported_Uncorrect is **748, the same as run 8's scan on
  2026-09-26**. The erase, the install and a day of use added none. The
  rest is unchanged from 2026-09-13 as well: 5 Retired_Block_Count 7, 196
  Reallocated_Event_Count 7, 184 End-to-End_Error 639, 199 UDMA_CRC 0, 195
  Hardware_ECC_Recovered 59.5 million. New in this reading: the drive's
  own error log counts 10,055 ATA errors, 250 Read_Retry_Count is 252.7
  million, and it has 8,858 power-on hours. Its overall self-assessment
  still says PASSED, the vendor's thresholds being as lax as R18 records.
  The 1 TB HDD is clean: 0 reallocated, 0 pending, no errors logged,
  5,190 hours. One reading, not a trend: the flash is still failing (R18),
  it just failed no further this week.
- **0 disk-error lines** in about 20 hours of the kernel log (64,849
  lines); `sda`/`sdb` serials match the job. The boot target is now
  `graphical.target` with SDDM running: changed by hand after run 9, so it
  does not count toward the one-click row.

**Seen and signed in on the rig (2026-09-27, `v9-erase.csv` lines 12-13).**
Both desktops reached their sign-in screen, took the password typed on the
rig's keyboard, and opened the desktop. That is the check the rig had
skipped before run 9 (its marker powered the guest off first).

Also added the same night, by the owner: the launcher **menu of what the
computer starts at** (KDE desktop, GNOME desktop, text console; rig lines
9-10 held), and **the sign-in named on screen** (the password screen and a
box before the restart say "user <name>"). Re-run owed: the Aspire from the
stick with the owner's choice, ending at that screen.

**Found the same night (2026-09-26): the keep-Windows launchers never asked
for a password.** `RUN-CONVERT.cmd` and its data-loss twin passed no hash.
So the job carried the verify-only placeholder, and an install would have
created an account whose password is `verify-only`, told to nobody. Job
writer 0.14.0 refuses the placeholder unless `-VerifyOnly` (`RUN-VERIFY.cmd`,
which installs nothing). All four install launchers now ask for the
password and name the account, before and after.

**Unspoofable residue (rule #5), for the Aspire's physical row:**

- Secure Boot on (the rig's is off). Run 9 installed with it on
  (`upg_fired=1`) and Fedora booted from the SSD; the re-run must show it
  again, ending at the desktop;
- a real keyboard on the countdown;
- the Aspire's real drives by their real identities;
- installing onto a drive with bad blocks (under R23: the install may fail
  after the commit line, and the stick can run it again);
- whether a person reads the countdown.

**Closes when.** The rig arms above (done 2026-09-26) and one physical row
on the Aspire: one click, the sentences, nobody at the keyboard, Fedora
signs in with the chosen password **at the graphical sign-in screen**. Run 9
(2026-09-26) did everything but that last part, so it is a `fail` row; the
re-run is owed.

## R28: settle-in runs on any Linux · high · open (decided 2026-09-27; built and passed on the rig the same day: Fedora KDE, GNOME, console)

**What.** `settle-in` must work on whatever Linux the person picks, not
only Fedora (`architecture.md`, "It runs on any Linux"). It is one
self-contained program that draws its own window and works through kernel
interfaces, fed by a small installer adapter per distribution through one
fixed handoff folder.

**If real.** What can go wrong:

- **The window does not appear.** A self-drawn window needs the display
  (Wayland or X11) and Mesa. On a machine with an unusual graphics setup it
  may fail to draw. Then the person meets no welcome screen, and on the
  keep-Windows path the file pull never starts. The program must fall back
  to a text screen it can always show, and say why.
- **The floor is wrong.** The claim that desktop distributions share
  systemd, NetworkManager and the kernel interfaces is an argument until
  each one is installed and checked. A distribution below the floor must be
  refused in plain words, never half set up.
- **Kernel interfaces instead of tools.** Writing the hardware clock and
  the firmware's boot entries directly is exactly what `hwclock` and
  `efibootmgr` do, done by us. A mistake in a firmware variable write can
  leave a machine that does not start. This is a writer (rule #4) and is
  reviewed and tested as one.
- **A new language.** The repo is PowerShell, bash and Python. A compiled
  program adds a build step, and a binary on the stick must match its
  source. The build must be reproducible and checked like the rest of the
  kit (`SHA256SUMS`).

- **The window needs a new enough C library.** Built on this project's
  build machine, the window program needs glibc 2.39 or newer (Fedora 42
  has 2.41). Older distributions (Debian 12 has 2.36) would not open it.
  Owed: build it against an older baseline. The core is fully static and
  has no such limit.

- **Anything that runs at sign-in can block the desktop.** Found on the
  rig (run 2): a display manager starts the desktop through a login shell
  that reads `/etc/profile.d`, with a terminal on standard input and no
  display yet. A console hook that asked a question there froze the
  sign-in on a black screen. The hook now runs only when logind says the
  session is a text console. Every sign-in hook must be checked for this
  on each distribution and display manager (V10).
- **The password prompt's words are technical.** Pressing the button
  shows the desktop's standard prompt, which names the command it will
  run ("…to run `/usr/local/libexec/upgrade_/settle-in
  remove-old-boot-entry` as the super user"). **Built 2026-09-27:** a
  polkit policy file with the owner's words, for that command only; the
  rig must show it on KDE, GNOME and the console.

- **Agreeing clocks do not prove local time in a virtual machine** (rig
  run 10, 2026-09-27: a right clock moved 7 h forward for seconds, until
  the network time service corrected it). The evidence rule assumed the
  installer's clock can only come from the hardware clock when no time
  service synchronized; a hypervisor can set it too. Fixed: in a virtual
  machine that case is left alone. On bare metal the rule stands, and the
  Aspire's physical run is its first real test. Residue: a Windows whose
  own clock is wrong gives `settle-in` a wrong offset and a wrong base; the
  correction then reproduces Windows' time, and the network corrects it.
- **GNOME does not give the window focus** (rig run 10; logged "not given
  focus by the desktop"). Its welcome tour stays on top; the window waits
  in the overview. KDE gives focus (run 9). Whether to do more on GNOME is
  the owner's call.

**Found on the rig (2026-09-27, run 1).** After the erase, the firmware
still lists the old "Windows Boot Manager" (pointing at the erased
partition) and also the prologue's own one-time entry, "upgrade_",
pointing at the stick. The button removes the first. **Built 2026-09-27
(the owner's call):** `settle-in` removes our own automatically at first
start, matched only by the BCD id the prologue recorded; rig next.
- **The welcome apps cover the window** (KDE's Welcome Center, GNOME's
  tour; rig runs 3 and 5). **Built 2026-09-27:** the window opens 5 s
  later and asks for focus. Whether each desktop honours it is logged and
  captured; a desktop that refuses leaves the window in the taskbar.

**Closes when** the same `settle-in` file, unchanged, runs its first
startup on Fedora KDE, Fedora GNOME and at least one non-Fedora
distribution, on the rig and then on a real machine, each leaving a capture
behind. VALIDATION V10.

## R29: The Microsoft third-party UEFI CA is off on Secured-core PCs · high · open (raised 2026-09-27 by research, unverified)

**What.** Our handoff boots Fedora's shim, which is signed by Microsoft's
*third-party* UEFI certificate (the "Microsoft UEFI CA"), not the one
Windows itself uses. Lenovo's guide for Secured-core PCs (including
Copilot+ PCs) says that certificate is **turned off by default** on those
machines. If so, the firmware refuses shim, and the one-time boot into the
stick never happens (`docs/research/device-feasibility.md`, source C33).

**If real.** On those machines the handoff fails. If the firmware falls back
to Windows, that is fail-safe but a dead end for the person, with no
explanation. If it stops at a firmware error screen, it breaks walk-away.
Newer Windows 11 laptops are the ones most likely to be Secured-core; the
stranded Windows 10 machines this project targets mostly predate it, so
the stakes are "a class of machines silently can't convert", not data
loss.

**Unverified.** Only Lenovo's document was read. Microsoft's own
Secured-core documentation, other vendors, and whether the setting can be
read from Windows before the handoff are all open.

**Closes when** a primary source (Microsoft) confirms or denies the default,
and the scanner either reads the firmware's trust setting and refuses with
plain words ("turn on 'Allow Microsoft 3rd Party UEFI CA' in your firmware
settings"), or a real Secured-core machine shows the handoff works. Every
contact leaves a capture (rule #5).

## R30: The way back to Windows · high · open (decided 2026-09-27; the licence harvest built the same day, job writer 0.16.0)

**What.** After an erase, or after a reclaim, there is no Windows to roll
back to. Decided (2026-09-27, the owner): there is still a way back, a new
and empty Windows, offered by its own program on the Linux side
(`architecture.md`, "The way back to Windows"). First a guided stick, later
a walk-away reinstall. `evaluate` harvests which Windows it was and how it
was activated (`harvest.windows_license`) while Windows still exists.

**If real.** What can go wrong:

- **Activation does not come back.** Unactivated Windows works, but it
  nags and locks the personal settings, and the person may believe we broke
  their licence. Why it might not come back is so far only argument:
  a digital licence is tied to the hardware and to the edition (a Pro
  install on a Home licence does not activate); some licences are linked to
  a Microsoft account; a changed part (the Aspire gets a new drive) might
  count as new hardware. Only real reinstalls answer this.
- **Windows 11 refuses the hardware.** Most computers this project is for
  cannot run Windows 11. For them the way back is Windows 10, whose free
  security updates ended in October 2025 (the paid extension for home
  users ends in October 2026). Not a failure of ours, but the screen must
  say it, never hide it.
- **Microsoft stops offering Windows 10.** Then the way back for those
  computers closes, and the screen must say that too.
- **The installer's largest file is over 4 GB.** Firmware starts from
  FAT32 sticks, and FAT32 holds no file over 4 GB. Windows 11's
  `install.wim` is larger. Splitting it (for example with `wimlib`) is the
  known route; whether it installs cleanly, Secure Boot on, is unproven.
- **The stick writer runs on Linux.** Writing a Windows stick is a disk
  write. It carries R16's bar: the one USB stick the person named, never
  an internal drive, never the kit stick.
- **A licence key on our stick or in our files.** Never harvested: a
  firmware key stays in the firmware (Windows' installer reads it there),
  and a typed key is the person's to keep (R13). The self-test proves a key
  handed to the harvest never reaches `job.json`.
- **The walk-away reinstall, when built, is an erase.** It deletes Linux
  and `/home`, so its severity is critical: its own typed sentence, a
  countdown as the commit line, and rule #4. Not built.

**Decided (2026-09-27, the owner).**

- The way back exists on every path, and says its cost first.
- Its own program in the app menu, not the first-start window.
- Guided stick first; the walk-away reinstall later, after the current
  spine work.
- The harvest records facts, never keys. A failed read is recorded with its
  reason and is not a refusal: the failure costs a less informed way back,
  never data, so rule #1 does not demand a stop.
- The erase launchers say it before the sentence is typed (the words are a
  draft, awaiting the owner's approval; `architecture.md`, "Erase and
  install"). They are in both erase launchers, so the Aspire's next run
  shows them.

**Built (2026-09-27): the guided stick's read-only half** (`settle-in
go-back`; `architecture.md`, "The way back to Windows"). Two facts from
primary sources that day: Microsoft still offers the Windows 10 22H2 and
Windows 11 installers to a Linux browser, and prints a SHA-256 for each
language on the same page; its own stick instructions split `install.wim`
into `install.swm` for FAT32. The program refuses any file not in that
table. Its stick list is R16's rules on Linux (USB, calls itself
removable, not a hard drive, no system mount, swap or holder, not the
upgrade_ stick, a unique serial, big enough; the writer re-finds the stick
by serial, exact size and a typed model name). Ten logic cases and one
recording (the Aspire's two internal drives, both refused). Not a row: no
stick has been written.

**Built (2026-09-27): the harvest** (job writer 0.16.0,
`harvest.windows_license`; eight self-test cases, including a value shaped
like a product key that must never reach the job; five new schema
refusals). Read once by hand on the G16, unelevated, in 1 s: Windows 11
Pro (build 26200), activated, `OEM:DM`, a key in the firmware. The same
read confirmed the registry quirk: ProductName says "Windows 10 Pro" on
that Windows 11 machine. Not a row: the rig and the Aspire's next job are
the first recorded reads.

**The Aspire is the first data point, and it starts with a gap.** Run 9
(2026-09-26) erased its Windows 11 Home, and what its licence was (the
channel, whether the firmware holds a key) was never recorded: run 9's
reports do not hold it. That gap is why the harvest exists. **Half of it
was recovered from Linux (2026-09-27, over SSH, no root):** the firmware
has an ACPI `MSDM` table (85 bytes), the table that carries a Windows key
from the maker. Its contents were not read (R13). So the Aspire has a
firmware key; which edition it is for, only Windows' installer will say. The owner
reinstalls Windows by hand with Microsoft's installer before the follow-up
run, and that run's job records whether activation came back. One machine,
by hand, not the guided tool.

**Closes when** the guided stick has put back an activated Windows on
real machines of at least two licence kinds (a firmware key, and a digital
licence without one), each run leaving its harvest from before and after
(rule #5), and a machine that cannot run Windows 11 has been shown the
Windows 10 path with its warning. The walk-away reinstall closes
separately, on the rig and then on a real machine, like V9. VALIDATION
V11.
