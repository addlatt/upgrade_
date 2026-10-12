# Known risks

This project has one asset: people can trust what it tells them. A scanner
that says "probably fine" and is wrong is worse than no scanner at all,
because the person believed it and lost their data.

So this file keeps the things we have not yet proven in plain sight, even
when that is uncomfortable. Each risk says what it is, what would really
happen if it is real, what has been decided about it, what would close it,
and where it stands today. Nothing here is closed by a good argument. Only
evidence closes a risk: a primary source (the original document, such as a
vendor datasheet or the kernel's own code) or a real machine (CLAUDE.md,
rule #2). The story of how each one got where it is lives in `docs/log/`,
one dated entry per event; a risk links to its entries and keeps none of
the narrative itself. Where everything stands at a glance is
`docs/STATUS.md`.

Severity, the second-last word of every heading:

- **critical**: can give a confidently wrong answer on the very thing the
  tool exists to do, or can destroy someone's data
- **high**: wrong advice, or a part that fails silently
- **medium**: worse output, or a defect people would notice and report
- **low**: fragile inside, likely to become a bug later

State, the last word of every heading (the same five words as STATUS.md):

| State | Bar | Means |
|---|---|---|
| `planned` | `[#...]` | designed, nothing built |
| `built` | `[##..]` | code exists, never run against anything |
| `held-on-rig` | `[###.]` | passed on the rig (VMs, spoofed hardware); plumbing only |
| `held-on-machine` | `[####]` | passed on a real machine, at least one row |
| `closed` | `[####]` | the closing clause is met; the row that met it is named |

`[FAIL]` is a flag, not a state: a real failure that is still in the record.
`held-on-machine` on one machine is one data point, and the risk says so
when the matrix is still owed. Between two states, a risk carries the lower
one (CLAUDE.md, rule #1).

A few words used all through this file: **the rig** is our test bench of
virtual machines (Hyper-V and QEMU); a pass there proves the moving parts
connect, and is not a real machine. **`pass-plumbing`** is the result word
for that, and never closes a real-hardware question (CLAUDE.md rule #5).
**A row** is one line in a results file under `docs/validation-results/`;
"line N" counts as an editor shows it, with line 1 the header. **The
Aspire** is the Acer Aspire A515-51G, the laptop most physical rows come
from; **the G16** is the ASUS ROG Zephyrus G16, the development machine.

# Open

## R1: VMD detection has never fired · critical · held-on-rig

**What.** Some Intel machines run their disk through Intel RST (Rapid
Storage Technology) or VMD (Volume Management Device), storage modes that
can hide the disk from a Linux installer. The scanner's flagship check (the
README's "single most common false 'Linux won't install'") is meant to
catch this. `Test-UpgStorageMode` uses three signals: the PCI ID list in
`data/devices.ps1`, matched to the kernel's `vmd.c` table; the `iaStorVD`
service (the VMD generation of the RST driver); and an Intel controller
declaring PCI RAID class `CC_0104`. RST software on an AHCI controller warns (R7).

**If real.** A false GREEN on the highest-stakes check. The person backs up,
wipes, boots the installer, and it shows no disks at all: the exact failure
the tool promised to prevent, with Windows already gone. The first ID list
had three wrong IDs and missed the whole 2015-2020 pre-VMD RST family.

**Decided (2026-09-15, the owner):** two full runs of the both-modes flow
on the Aspire is enough of an owner's time; the RAID row there is skipped
unless a one-minute F2 look finds the SATA Mode option.

**Closes when.** The ID list is checked against a primary source (Intel
datasheets, the kernel's `drivers/pci/controller/vmd.c` ID table, or
linux-hardware.org probes from RST-enabled machines), **and** the check
fires correctly on at least one machine with RST enabled.

**Where it stands.** `[###.]` held-on-rig. The desk half is done (IDs
checked against `vmd.c` and pci.ids, 2026-08-22). The RED path has fired
only on a spoofed `8086:9a0b` on the QEMU rig
(`corpus/vm-qemu-q35-vmd-spoof-9a0b.json`): plumbing, never evidence
(rule #5). On real Intel RST silicon only the AHCI direction exists: the
Aspire, `v5-controller-mode.csv` line 2, `warn-rst-on-ahci`, plus lines 7
to 10 from the one-click both-modes flow (`RUN-STORAGE-MODE.cmd`), whose
two runs never reached the firmware setup (Acer's InsydeH2O ignores
boot-to-setup). Owed: one RST or VMD machine scanned to `[FAIL]`, RED. Log:
[desk check](log/2026-08.md#2026-08-22-vmd-ids-checked-against-primary-sources-r1),
[spoof](log/2026-08.md#2026-08-26-the-level-3-vmd-spoof-fired-on-the-qemu-rig-r1-v5),
[AHCI row](log/2026-08.md#2026-08-27-the-negative-direction-on-real-intel-rst-silicon-r1-r7-v5),
[one click](log/2026-09.md#2026-09-13-the-one-click-both-modes-harness-built-and-fired-on-the-rig-r1-v5-r24),
[Aspire runs](log/2026-09.md#2026-09-15-two-physical-runs-of-the-both-modes-flow-on-the-aspire-r1-v5).

## R2: Two-machine test corpus · high · built

**What.** Everything tested end to end was tested on two laptops: the ASUS
ROG Zephyrus G16 (Ryzen AI 9 HX 370, RTX 4060, MediaTek MT7925), where the
scanner was built, and the Acer Aspire A515-51G, which has run the
conversion. Every `fail` path is synthetic: run only by `-SelfTest` with
made-up check objects. Not yet tested on real hardware: Broadcom Wi-Fi
refusal, ARM refusal, free-space refusal, MBR partition-limit warning, the
BitLocker-enabled path, cloud-only placeholder detection (R8), and every
vendor quirk.

**If real.** Unknown. That is the problem: for most of the code we have no
evidence either way.

**Closes when.** Reports exist from a spread of machines: at least one Intel
laptop, one with Broadcom Wi-Fi, one pre-2015 machine, one with BitLocker on,
one Surface. This is the best single argument for shipping the scanner early
and asking for reports.

**Where it stands.** `[##..]` built. The scanner has `-DumpMachine` (a
hardware-only capture), and `-SelfTest` replays every curated capture in
`evaluate/windows/corpus/` through the pure detection checks, so a machine
met once stays tested forever (rule #5). The corpus holds the G16, the
Aspire (the one Intel laptop) and one synthetic VMD spoof. The spread above
does not exist, so the mechanism counts as built, not held.
[log](log/2026-08.md#2026-08-22-the-capture-mechanism-and-corpus-replay-r2)

## R3: Distro kernel table is unverified and stale · high · planned

**What.** `data/distros.ps1` marks Fedora (6.14) and Pop!_OS (6.9) with
`Approx=$true`, meaning nobody checked them against release notes. Ubuntu
26.04 LTS is missing entirely. This table produces the report's headline
claim: the scanner tells people "RULED OUT for shipping an older kernel:
Linux Mint, Ubuntu LTS, Pop!_OS" with total confidence, from data marked as
a guess.

**If real.** If Ubuntu 26.04 LTS ships a new enough kernel, we steer people
away from the most popular, best-supported option for no reason. The
recommendation then falls back to openSUSE Tumbleweed and Arch, which the
same table rates as unsuitable for newcomers.

**Closes when.** Every entry is confirmed against the distribution's own
release notes, `Approx` is cleared, `$script:UpgDistroTableVerified` is
updated, and Ubuntu 26.04 is added or explicitly left out with a reason.

**Where it stands.** `[#...]` planned. Nothing done: the table still reads
`UpgDistroTableVerified = '2026-08-16'`, the two `Approx` entries stand, and
Ubuntu 26.04 is still missing. No log entry yet.

## R4: Scanner and converter contradict each other · medium · built

**What.** The scanner's free-space check once said "Not enough free space
to install Linux **alongside** Windows", with 25/60 GB dual-boot thresholds
(`Test-UpgDisk`), while the converter listed dual-boot as a non-goal and
replaced Windows entirely (first through an external-drive design since
removed, now through the USB-only paths). The module split puts the scanner
inside `evaluate`, which owns all refusals; that fixed who owns the gate,
not what it says.

**If real.** The scanner answers a question the product no longer asks. A
person passes the free-space check, starts the converter, and is refused for
a reason the scanner never mentioned.

**Decided (2026-08-19, the owner):** the scanner keeps two personalities in
one codebase. Standalone, it is a general advisory tool that recommends
across distributions. Embedded, it is the converter's `evaluate` mode, with
the converter's own gates. The free-space check survives with a new meaning
under the USB-only design: shrinkable space gates the safety-copy path, and
stick capacity gates the clean-slate path.

**Closes when.** Both sides reflect the decision in words and in numbers.
The scanner informs; `evaluate` gates. The clean-slate gate needs the
harvester's folder sizing to answer "does your data fit an N GB stick",
which the scanner alone cannot work out.

**Where it stands.** `[##..]` built. The words were fixed 2026-08-22:
`Test-UpgDisk` reports `Disk in use` (info) and `Room to keep Windows`
(from `Get-PartitionSupportedSize`), nobody is told to buy an external
drive, and low free space is no longer RED. That removed the dangerous
contradiction, which is why the severity came down to medium. The numbers
side is the folder map's stick-fit gate (harvester 0.3.0, job writer
0.10.0, 2026-09-26; R5, R6), measured on the G16 and the Aspire
(`harvest-folder-map.csv` lines 2 to 4) but with its refusal unfired on a
physical job.
[log](log/2026-08.md#2026-08-22-the-scanners-disk-wording-reworked-r4)

## R5: Single-user assumption · high · built

**What.** `Harvest-UpgradeState.ps1` collects only the current user's
folders, browser profiles and account details.

**If real.** A family computer with three accounts moves one and silently
leaves the other two behind. The conversion reports success. The other two
people lose everything, on a machine that no longer has Windows on it. This
is a data-loss bug aimed straight at the "anybody can do this" audience,
who are the most likely to share a machine.

**Closes when.** The harvester lists all local profiles, sizes them, and
either moves all of them or refuses multi-user machines outright. Refusing
is an acceptable v1 answer. Silently moving one is not.

**Where it stands.** `[##..]` built (harvester 0.3.0, job writer 0.10.0,
2026-09-26). The folder map lists every other profile on the machine
(local, domain and Microsoft Entra; `Win32_UserProfile`, not special). The
job writer refuses a clean-slate job when any exist and names them; a
keep-Windows job is still written and says plainly that their files are
not in it, and `settle-in`'s reclaim will have to refuse on the same fact.
It also refuses an elevated run under another account, comparing the
elevated SID with the desktop owner's. Measured on the G16 (0 other
profiles) and the Aspire (1), `harvest-folder-map.csv` lines 2 to 4; the
refusal itself has not fired on a real machine. Owed: whether a Windows
leftover such as `defaultuser100000` (found on the Aspire, 2026-09-26)
should count; see R26's measurement.
[log](log/2026-09.md#2026-09-26-other-profiles-in-the-folder-map-and-the-wrong-account-refusal-r5)

## R6: Truncated sizing undersizes the backup · medium · built

**What.** `Get-HarvestFolderStats` stops at 250,000 files and sets
`Truncated=$true`. The backup estimate is then too low. A sibling found
while fixing it: `Get-ChildItem -Recurse -ErrorAction SilentlyContinue`
silently skips a sub-folder Windows will not list, and the prologue's
staging copies with the same call, so those files would also be left off
the stick.

**If real.** The path gate and the gap report are worked out from a number
that is too small. A machine is steered onto the clean-slate path with files
that do not really fit the stick. That is found out at staging, or worse,
trusted.

**Closes when.** The Phase A UI treats `Truncated` as a hard blocker rather
than a note, or sizing is made exact for the folders that feed the estimate.

**Where it stands.** `[##..]` built (job writer 0.10.0, 2026-09-26). The
job writer refuses any folder that hit the cap, and any folder the
harvester counts as `Unreadable` (with the first paths). Checked on real
Windows PowerShell 5.1 (the G16): a recursive listing does not follow
junctions or directory symlinks, and does walk OneDrive's cloud
directories. Self-tests use a real junction and a real folder with a deny
rule. On the G16 and the Aspire the unreadable count was 0 in all six
folders (`harvest-folder-map.csv` lines 2 to 4), so neither refusal has
fired for real.
[log](log/2026-09.md#2026-09-26-the-truncation-refusal-and-the-unreadable-folder-sibling-r6)

## R7: Broadcom vendor fallback over-refuses · medium · planned

**What.** The vendor fallback returns `fail` for *every* Broadcom device it
does not recognise (`Get-UpgWifiVendorFallback`, `14e4`).

**If real.** Some Broadcom parts work well enough with `b43` or `brcmfmac`.
We tell those people their machine needs a new card. Over-refusal is the
safer way to be wrong, but "refuse by default" is meant to mean *refuse when
we don't know*, not *claim a failure we haven't shown*.

**Closes when.** The fallback tells "known bad" apart from "unknown
Broadcom, bring a USB Ethernet adapter just in case", and common working
Broadcom IDs are added to the exact-match table.

**Where it stands.** `[#...]` planned. Nothing built, and no Broadcom
machine has been scanned (R2). The same shape of guard, warn instead of
fail when the evidence is incomplete, is what R1 uses for an RST driver on
an AHCI controller, and it has one real row (`v5-controller-mode.csv` line
2, `warn-rst-on-ahci`). No log entry yet.

## R8: Cloud-only detection positive path untested · medium · built

**What.** OneDrive can keep a file "online-only": on disk there is just a
placeholder (a stub with the name and size, but no contents). Detection
checks `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` and `FILE_ATTRIBUTE_OFFLINE`.
On the G16 and the Aspire it finds 0, and separate checks confirm there are
none, so the code has run and never found anything for real.

**If real.** OneDrive placeholder files copy as empty. The person's photos
seem to move across and are 0 bytes on the other side, found out long after
Windows is gone. `settle-in` pulls files by reading the mounted NTFS *from
Linux*, which has no OneDrive client, so a stub cannot be filled at pull
time, only copied empty.

**Decided (2026-09-26, the owner):** online-only files are not downloaded;
`settle-in` reconnects OneDrive. Their bytes already live in OneDrive, so
leaving them there loses nothing. Job writer 0.11.0 records
`cloud_files.result = left-in-cloud` and the count per folder, and says at
job time that they stay in OneDrive. The loss this risk names was only ever
copying the 0-byte stub as if it were the file.

**Closes when.** `Test-Materialize.ps1 -OneDrive` passes against a signed-in
OneDrive with Files On-Demand: the client's own stub and download behaviour,
its response to `attrib +P`, and a fetch stalled by the network, which no
test provider can stand in for (rule #5). The harness uploads a few MB to
the account and is only ever run on a machine the person owns.

**Where it stands.** `[##..]` built. The harvester's `-Materialize` (pin,
read through, three-fact verification, refuse on any failure) is
`pass-plumbing` against a real Cloud Files API test provider on the rig and
the G16 (`v8-materialize.csv` lines 4 and 5, 2026-09-08). It stays built
and unused by the launchers. The residue moved to `settle-in`, none of it
built: how a placeholder looks through ntfs-3g (never read), whether a
work-or-school OneDrive (the Aspire's is a university's) lets a Linux
client sign in, and the clean-slate staging skipping stubs instead of
copying them. Log:
[design](log/2026-08.md#2026-08-22-materialize-or-refuse-and-the-detection-arm-tested-r8),
[plumbing](log/2026-09.md#2026-09-08-materialization-built-and-its-plumbing-fired-r8-v8),
[in the job](log/2026-09.md#2026-09-26-online-only-files-recorded-in-the-job-and-left-in-the-cloud-r8).

## R9: No CI; dist can drift from source · medium · planned

**What.** `dist/upgrade-scan.ps1` is committed so people can download one
file. Nothing makes sure it matches `data/` and `evaluate/`. The self-test
never runs automatically. (CI, continuous integration, means tests that run
by themselves on every change.)

**If real.** A contributor edits `data/devices.ps1`, forgets `./build.sh`,
and the published scanner runs old data while the source looks correct.

**Closes when.** A GitHub Action runs `./build.sh`, fails if `dist/`
changes, and runs `-SelfTest` on every PR.

**Where it stands.** `[#...]` planned. No workflow exists; `./build.sh`,
`./port-check.sh` and `./docs-check.sh` are run by hand before a pull
request, and nothing enforces it. No log entry yet.

## R10: NVIDIA detection matches rendered display text · low · planned

**What.** `Get-UpgRecommendation` decides whether NVIDIA is present with
`$_.Detail -match 'NVIDIA'` across all checks. It matches formatted output
rather than structured data.

**If real.** Not wrong today. But `NVIDIA High Definition Audio` exists as a
device on the test machine. The moment any check prints device names into
`Detail`, distro recommendations change silently.

**Closes when.** Checks carry a structured vendor field and the
recommendation reads that instead of display text.

**Where it stands.** `[#...]` planned. Nothing built. No log entry yet.

## R11: Secure Boot check verifies nothing · low · planned

**What.** The check reports `ok` when Secure Boot is on and lists distros
known to work. It never confirms that this firmware will accept the *chosen*
distribution's shim (the small signed loader that lets Linux boot with
Secure Boot on).

**If real.** A GREEN Secure Boot line on a machine whose firmware refuses
the kit's boot files; the person finds out at the first Linux boot.

**Closes when.** The converter checks the target's signed shim against the
machine before committing, or the check's wording drops the implied
promise.

**Where it stands.** `[#...]` planned, on this risk's own words. Related
and built elsewhere: R34's release check (scanner 0.5.0, job writer 0.17.0)
judges whether the machine's Secure Boot still accepts a release's boot
files and refuses one that cannot start, held on the Aspire 2026-10-04.
Whether that meets this clause, or the check's wording still promises more
than it verifies, is for the owner to say. No log entry yet.

## R12: Code signing and antivirus · critical · planned

**What.** The finished converter runs as administrator, reads BitLocker
recovery keys, exports Wi-Fi passwords in plain text, images a disk, and
rewrites boot settings. In behaviour, that is an exact match for an
infostealer (malware that steals passwords) followed by ransomware. The
prologue also registers a SYSTEM startup task and an HKLM `RunOnce` entry
to resume with nobody signed in: textbook signs of malware that wants to
survive a restart. That exposure is R24's, with its evidence there.

**If real.** Unsigned, SmartScreen shows "Windows protected your PC" and
Defender may quarantine it outright. The target person (non-technical,
careful, warned their whole life about exactly this) stops there for good.
No amount of interface work makes up for it.

**Closes when.** An OV or EV code-signing certificate is obtained (it needs
a legal entity and a few hundred dollars a year), releases are signed and
reproducible, and the binary is sent to Microsoft's malware-analysis portal
for allowlisting. Start early: reputation builds with elapsed time and
number of installs, so the certificate is worth having before there is
anything to sign.

**Where it stands.** `[#...]` planned. No certificate, no signed release,
nothing sent to Microsoft. Reproducible builds, the half that does not need
a legal entity, are R14's. V6 is the calendar.
[log](log/2026-09.md#2026-09-13-the-prologues-startup-task-adds-to-the-antivirus-exposure-r12-r24)

## R13: The USB becomes a credential store · medium · built

**What.** `evaluate` writes secrets to the stick so the Linux side can use
them: Wi-Fi passwords in plain text (to recreate connections) and, under
the keep-Windows default, the **BitLocker recovery key** (so `settle-in` can
unlock and read the Windows partition). On the keep-Windows path the key is
needed later, by `settle-in`, for the file pull, so the stick's credential
area can only be wiped after that pull finishes, not at the end of cutover
(`architecture.md`, Contracts).

**If real.** Anyone who picks up the stick has the person's home and work
Wi-Fi, and worse, the key to their still-intact encrypted Windows disk.

**Decided (2026-09-26, the owner):** Wi-Fi passwords are harvested and
applied by `settle-in` on first startup (`architecture.md`, "The clock,
Wi-Fi and the old boot entry"). To shorten the time they sit on the stick,
the installer's last step moves them from the stick into a root-only folder
on the installed system and deletes them from the stick; `settle-in` applies
them and deletes that copy. Credentials on the stick then live from the
harvest to the end of the install (minutes, with the stick in the machine)
rather than until someone remembers to wipe it.

**Closes when.** Restricted file permissions on write, the credential wipe
at the correct (later) moment, and plain words to the person when they
choose what to do are built and verified, not just designed.

**Where it stands.** `[##..]` built, for the Wi-Fi half only. The
installer's move-and-delete step exists (`upgrade_/linux/outcome.sh`
records `moved` and `removed_from_stick` in `outcome.json`), and the
Aspire's run 11 (2026-10-04) connected to Wi-Fi by itself, but no row in
this risk's record names the removal, so it is not held. The BitLocker
key's wipe after the pull and the permissions on write wait on `settle-in`'s
file pull, which is not built.
[log](log/2026-08.md#2026-08-22-the-credential-wipe-moves-to-the-end-of-settle-ins-pull-r13)

## R14: Supply chain · medium · held-on-machine

**What.** A tool that runs as SYSTEM and repartitions disks is a valuable
target for attackers. A release is a build, a signature and the credential
that makes the signature; each is a place to tamper.

**If real.** Whoever tampers with a release binary owns every machine that
runs it.

**Decided (2026-10-08, the cut-over):** the Rust programs on the stick are
built reproducibly. One pinned toolchain (`rust-toolchain.toml`: rustc
1.93.1; zig 0.13.0 and cargo-zigbuild 0.23.4 for the Windows link),
`--locked`, symbols stripped, one codegen unit, source paths remapped. The
kit's manifest carries each program's sha256 and the toolchain line, and
`rebuild-check.sh` rebuilds from a fresh clone and compares, one row per
program in `docs/validation-results/r14-rebuild.csv`.

**Closes when.** Releases are reproducible, signed and checksummed, and the
release process does not depend on a single unprotected credential.

**Where it stands.** `[####]` held-on-machine for the reproducible half:
at commit `4b21dc3` all five programs came out the same bytes from a fresh
clone (`r14-rebuild.csv` lines 22 to 26, `same` = `y`, 2026-10-09), after
four runs that each found one more thing leaking into the bytes (the PE
link timestamp, a stale relink, path dependencies hashed by absolute path).
Signing and the release credential are not started; the certificate is
R12's.
[log](log/2026-10.md#2026-10-09-the-rebuild-checks-five-runs-r14)

## R15: The one-time UEFI boot handoff may not fire on other vendors' firmware · critical · held-on-machine

**What.** Walking away rests entirely on one command,
`bcdedit /set {fwbootmgr} bootsequence`, booting the stick exactly once. It
asks the firmware (the machine's built-in startup software, UEFI) to start
from the stick on the next boot only. Vendor firmware is creative about
removable-media entries: some ignore `bootsequence` for USB devices, some
re-scan and lose the entry.

**If real.** Harmless but total. The machine boots Windows and the person
concludes the tool did nothing. The product's core mechanism silently
doesn't exist on some share of hardware, and we don't know how big that
share is.

**Closes when.** The spine spike (build order step 0) passes in a VM and on
physical machines from at least three vendors.

**Where it stands.** `[####]` held-on-machine, one vendor. The Acer Aspire
A515-51G (InsydeH2O V1.21, Secure Boot on, BitLocker off) ran the one-click
flow (`RUN-TEST.cmd`, harness 0.3.0): the firmware booted a real USB stick
once through signed shim and GRUB, no key pressed, and came back to
Windows. `v0-handoff.csv` line 11, `fired-once` (2026-09-08), and again
line 12 (2026-09-12). The rig holds the fail-safe halves: `NoFile` ->
`ignored` on QEMU (line 4) and `SecureBootUnsigned` -> `ignored` under real
Secure Boot on Hyper-V (line 5). The Hyper-V BitLocker rows fired with no
recovery prompt even without suspension (lines 6 and 7), so the prologue
suspends as the cautious default, not because the handoff needs it. Owed:
Dell, Lenovo and HP; the fail-safe rows on real firmware; a physical
BitLocker row (whether any vendor firmware measures the attempted one-shot
into a sealed PCR). Log:
[QEMU leg](log/2026-08.md#2026-08-23-the-handoffs-vm-leg-fired-on-the-qemu-rig-r15-v0),
[Hyper-V leg](log/2026-08.md#2026-08-30-the-handoffs-hyper-v-leg-fired-rows-3-5-6-r15-v0),
[harness 0.2.0](log/2026-09.md#2026-09-07-handoff-harness-020-before-the-first-physical-row-r15-v0),
[one click](log/2026-09.md#2026-09-07-handoff-harness-030-the-one-click-flow-vm-leg-fired-r15-v0),
[the Aspire](log/2026-09.md#2026-09-08-the-first-physical-machine-fired-the-handoff-r15-v0-r21).

## R16: Stick authoring can write the wrong device · critical · held-on-machine

**What.** `evaluate` writes the live image with raw `\\.\PhysicalDrive`
writes, straight onto the device, and the person may have other USB
devices plugged in. `Write-UpgradeStick.ps1` chooses the target by a pure
function over every attached disk. A disk is chosen only if it is on the
USB bus and not HDD/SSD media (an enclosure is somebody's backup), is not
the system or boot disk, is the one disk carrying the `-Target` unique id,
is the size the person was shown, is online and writable, and the person
types its label (never a bare "yes"). Disk numbers are not accepted. Right
before `Clear-Disk` the target is found again by unique id from a fresh
list, and every file written is read back against `SHA256SUMS`.

**If real.** The tool destroys someone's data *before* the commit line.
That is the one failure the whole architecture exists to prevent, done by
the part that promised to be safe.

**Closes when.** The physical write row: several sticks **and a USB hard
drive attached at once**, `-Write` pointed at one stick. That one is
erased, written and verified, and every other device is untouched. A VM
cannot run it (no USB emulation; every VHDX is refused for its bus first).
That row is exactly the rule-#5 residue.

**Where it stands.** `[####]` held-on-machine, one device at a time. The
refusal matrix fired read-only on the G16 and the rig 2026-09-08 (23
self-test cases; `r16-stick-writer.csv` lines 2 to 7: the stick chosen
only at its real size, the system NVMe refused on five counts, every VHDX
refused for its bus). First physical write 2026-09-13 from the G16: one
8 GB General UDisk written and verified (lines 11 and 12); writer 0.1.1
learned that `Clear-Disk` leaves a real stick MBR-initialized where a VHDX
comes back RAW. One finding for `job.json`: a stick's `UniqueId` carries
the host name on the end and its `SerialNumber` can be empty. Owed: the
several-devices write. Log:
[built](log/2026-09.md#2026-09-08-the-stick-writer-built-and-the-refusal-matrix-fired-r16),
[first write](log/2026-09.md#2026-09-13-first-physical-stick-write-r16).

## R17: Counterfeit or failing flash as the sole data carrier · high · held-on-machine

**What.** On the clean-slate path the stick is, for a while, the only copy
of the person's files. Counterfeit sticks lie about their size and silently
throw away writes. Cheap flash fails without warning. Only the clean-slate
path (opt-in, or the full-disk fallback) puts user data on the stick; on
the default keep-Windows path the files never leave the internal disk, so
fewer people are exposed than when this was written.

**If real.** Files checked as "staged" do not exist, found out after the
wipe. The impact stays high.

**Closes when.** The cutover's read-back checksum check (which runs before
the commit line, while Windows still exists) is built as a hard gate (done)
and shown to catch a known-counterfeit stick.

**Where it stands.** `[####]` held-on-machine for the gate; the counterfeit
half untried. `upgrade_/linux/verify.sh` (the kickstart's `%pre`) reads the
chosen desktop image back against the stick's own `SHA256SUMS` before
anything is decided; in install mode a mismatch refuses (exit 23). It also
records the stick's read speed, the honest basis for the time estimate. On
the Aspire the 2.6 GB KDE image read back byte-identical at 22.6 MB/s
(`v1-live-boot.csv` line 5, `pass-plumbing`, 2026-09-12). The same stick
dropped off the G16's bus twice that morning under a steady 5 GB write
(Windows event 51): a loose connection, and exactly how a failing stick
looks. Owed: a known-counterfeit stick, and the same read-back over staged
files. Log:
[gate](log/2026-09.md#2026-09-09-the-read-back-gate-exists-r17-v1),
[read-back](log/2026-09.md#2026-09-12-first-physical-read-back-on-the-aspire-r17-v1).

## R18: Windows shrink headroom is unmeasured · high · held-on-machine

**What.** To keep Windows we shrink its partition. Files NTFS cannot move
(the MFT, System Restore's storage, the pagefile, hiberfil, the change
journal) cap the shrink far short of the free space: glued books on a shelf.
The scanner measures that floor by two read-only paths, elevated only; the
prologue re-measures after each rung of its ladder (hibernation and the
pagefile off with one restart, restore points and the journal with consent).

**If real.** Keep-Windows is offered to machines that cannot deliver it; the
prologue stops late, after the person confirmed, which kills walking away.

- **Decided (2026-09-08, the owner):** `evaluate` never repairs; the prologue
  clears a dirty flag as reversible prep, behind four guardrails, then forks.
- **Decided (2026-09-13, the owner):** a bad-block event on C:'s disk is RED,
  no override; volume health reads `Get-Volume` and the Chkdsk events.
- **Decided (2026-09-17, the owner):** Windows' own "repair queued" statement
  (`Get-Volume`, NTFS event 98) is a fact beside the dirty bit, same consent.
- **Decided (2026-09-20, the owner):** an event 98 is history once a check
  completed after it; a floor pinned by hiberfil or pagefile keeps the fork.
- **Decided (2026-09-20, the owner):** the Aspire keeps its dying SSD as the
  bad-conditions machine; its rows never stand in for a healthy drive.
- **Decided (2026-09-20, the owner):** restore points in the way are deleted,
  with consent (`fork.restore_points_consented`).
- **Decided (2026-09-22, the owner):** the change journal in the way is
  deleted, with consent (`fork.usn_journal_consented`), and created again.

**Closes when.** The gate uses the shrinkable number, measured on fragmented
real disks with the mitigations applied, and V4's Pass line (the share of
elevated machines that can) is answered by more machines than one.

**Where it stands.** `[####]` on one machine, and the answer was no: the
Aspire's old Windows, runs 4 to 8 (`r18-prologue.csv` lines 8 to 12, all
stopped), best 9.5 of 25 GB, three runs naming three different last unmovable
files; keep-Windows was refused there, correctly. The fresh Windows 11 the way
back installed shrank with room to spare (line 16, 2026-10-10; line 18,
2026-10-11, 238.2 to 213.2 GB, both `pass-plumbing`). Owed: V4's rate from
more machines; the pagefile read back; a documented way to move `$Mft::$BITMAP`
([log](log/2026-09.md#2026-09-26-the-aspires-eighth-run-the-first-physical-job-with-the-folder-map-row-12-r18-r25)).

## R19: cryptsetup BITLK read is a new trust dependency · medium · held-on-rig

**What.** On the keep-Windows path `settle-in` reads the kept BitLocker
volume from Linux through `cryptsetup` BITLK, unlocked with the harvested
recovery key, and copies the person's files into Linux. Reframed 2026-08-22:
the read happens with the person present, after the new Linux works, with
Windows intact as the backup. Downgraded high to medium then.

**If real.** Edge cases (key protector types, XTS variants, used-space-only
encryption) stop the copy, which is recoverable: Windows is still there. The
real danger is reading wrong data, which then checksums as what was read.

**Decided (2026-09-01, the owner):** `settle-in` reads the kept volume
through ntfs-3g (FUSE, an ordinary program), never the in-kernel `ntfs3`,
which crashed five runs out of five on Fedora 42's install kernel. Reopen
only with a kernel gate that is itself evidence.

**Closes when.** Read-and-copy is checked against real BitLocker volumes
across Windows 10 and 11 defaults, both XTS-AES key sizes and used-space-only
encryption, with checksums made on the Windows side at harvest time so the
Linux side catches read corruption, not only copy corruption.

**Where it stands.** `[###.]` `v3-bitlk-read.csv` lines 2 to 16
(2026-09-01/02, the rig): three BitLocker variants read 2,850 of 2,850 corpus
files byte-identical through ntfs-3g; `ntfs3` rows read `guest-crashed` and
`mismatch` (lines 6, 8 to 10). Design inputs found: zero-byte system files
show as FIFOs (`lstat` first), app-execution aliases are reparse points, the
shrunk-size warning is normal, and the harvest must hash last. Residue: real
disks, Windows 11's BitLocker, 4Kn sectors, half-encrypted states. Owed:
`settle-in`'s file pull, not built
([log](log/2026-09.md#2026-09-01-the-bitlk-reads-vm-leg-ntfs-3g-passes-ntfs3-crashes-r19-v3)).

## R20: Browser profile porting is assumed, not verified · medium · planned

**What.** The migration table promises that Chrome and Edge bookmarks,
history and extensions carry over by copying the profile folder. But cookies
and several other parts of the profile are encrypted with DPAPI (Windows'
per-user encryption), like the passwords. A version gap between the Windows
and Linux builds can make the browser reset or refuse the profile, and
extension state does not reliably survive a copy.

**If real.** "Your stuff silently didn't arrive": the project's worst kind of
failure, in the feature most people check first.

**Closes when.** Real Windows to Linux profile moves are checked for each
browser and version pair, and `evaluate`'s claims are narrowed to what the
evidence supports.

**Where it stands.** `[#...]` nothing checked. The harvester lists the
browsers in the folder map; no profile move has been tried on any machine.
Owed: one move per browser and version pair, and the narrowed claim.

## R21: Installing alongside a shrunk Windows may not leave Windows bootable · critical · held-on-machine

**What.** The default path installs Linux into the freed space and must leave
the shrunk Windows bootable: Windows is the rollback and the source of the
files. With Secure Boot on that means the Windows ESP reused without a
format, shim and GRUB fitting an ESP of often 100 MB, `bootmgfw.efi` kept,
GRUB listing Windows, and the firmware starting Linux with nobody there.

**If real.** A conversion whose kept Windows will not boot: no rollback, no
file source, and the person was told they had both. Trust-ending.

**Decided (2026-08-30, the owner):** shim stays in `EFI/Boot/bootx64.efi`
while Windows is kept; the prologue snapshots the ESP and the `Boot####` set;
`%post` re-creates the Windows entry, puts Linux first, checks `bootmgfw.efi`
and `grub.cfg`; `evaluate` wants 32 MiB free on the ESP. Re-taking: R22.

**Decided (2026-10-10, the owner):** shim in Windows' slot on firmware that
ignores operating-system entries (Acer's): `bootmgfw.efi` moves aside as
`bootmgfw-kept.efi`, shim takes its name, GRUB chainloads the kept name;
applied only on evidence, reversible from the snapshot, re-applied by a guard
task after servicing. **Amended (2026-10-11):** GRUB's "Windows (kept)" entry
records the choice (kickstart 0.9.0) and a set flag stands the detector down.
**Decided (2026-10-12, the owner):** the rollback runs from the Linux side
(`settle-in rollback`): the order through efivarfs, the files from the stick's
snapshot, a marker the guard honours; the Windows-side tool stays as plumbing.

**Closes when.** Alongside installs are proven on real machines from several
vendors, Secure Boot on: Windows boots from the menu, GRUB lists it, the ESP
had room, and `bootmgfw.efi` is intact (its bytes, on slot firmware). Firmware
that cannot is steered to clean slate before committing.

**Where it stands.** `[####]` one machine, the product's own way
(`v2-install.csv` line 15, the Aspire, 2026-10-11, `pass`); the rig held the
return, servicing and the rollback (lines 12, 13; `r21-rollback.csv` line 5).
The Aspire's rollback (`r21-rollback.csv` line 6, `windows-not-direct`): files
back, guard gone, but a `bcdedit` order change does not survive a restart on
this firmware (`efibootmgr`'s does), and kickstart 0.9.0 saved GRUB's flag on
`/boot`, unreadable from Windows (0.10.0 puts it on the ESP; unproven). Owed:
the Linux-side rollback's rows, 0.10.0 on the rig, an update under the guard, other vendors ([log](log/2026-10.md#2026-10-12-the-first-rollback-on-a-real-machine-r21-v1b)).

## R22: Windows servicing re-takes the firmware boot order · medium · held-on-rig

**What.** On the keep-Windows path Windows stays installed, and Windows
Update's boot-file servicing registers the Windows Boot Manager entry again,
first in `BootOrder`. Seen on the QEMU rig (2026-08-27): one Windows session
that applied an update at shutdown was enough, and the next power-on ran
Windows with no GRUB menu. It can happen at any update for as long as Windows
is kept.

**If real** (it is; the question is how often on vendor firmware). The person
restarted into Windows once, an update ran, and the machine "went back to
Windows". Linux and the pulled files look gone. Nothing is lost, but the
person this project exists for cannot know that. Medium.

**Decided (2026-08-30, the owner):** `settle-in` installs a boot-time unit
that puts the Linux entry first again after a Windows session moved it; the
welcome screen names the firmware's boot-menu key; reclaim removes both.

**Decided (2026-10-10, the owner):** on firmware that ignores the Linux
entry (the Acer finding, R21), the answer is instead a `guard` startup task
in the kept Windows that re-applies shim in Windows' slot when servicing has
put Windows' own file back. An update then costs one Windows boot, not Linux.

**Closes when.** The unit is built, and a Windows update on a real machine is
followed by a Linux boot with no action from the person; and the physical
matrix says which firmware ignores the re-assertion.

**Where it stands.** `[###.]` the guard re-applied the arrangement on the rig
after the bench put Windows' file back over the slot (`v2-install.csv` line
13, 2026-10-10, history 0 to 1). On the Aspire it ran at every Windows start
of 2026-10-10 and 2026-10-11 with nothing to re-apply (line 15).
`settle-in`'s order unit is not built. Owed: a real Windows update under the
guard on the Aspire (the trigger, not only the response), the unit, the
matrix ([log](log/2026-10.md#2026-10-11-the-detector-fires-on-the-aspire-r36-the-product-way-r21-r22-r25-r36-v1b)).

## R23: The acknowledged-data-loss path exists · high · held-on-machine

**What.** Rule #1 has no override for a RED verdict. On 2026-09-13, after
the Aspire's SSD was diagnosed as failing (R18), the owner asked for a way to
go ahead anyway, and one was built, narrowly: a separate launcher
(`RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd`) on which the person types, word for
word, *"I confirm that I understand the risks and could lose data"*. The job
writer then accepts a RED verdict only if every failing check is `Disk
health` or `Volume health`, and records `risk_acknowledgement` in `job.json`.
The prologue wants the same sentence and lifts exactly those two refusals.
Every screen and record says DATA LOSS ACCEPTED. The schemas refuse a
paraphrase, an empty override list, `identity` as an override, RED without
the block, and a repair on a non-Healthy disk without it. It never lifts an
identity mismatch, a moved or non-USB stick, legacy BIOS, an unknown
BitLocker state, RST/VMD, a failed image read-back, a failed ESP snapshot or
a schema-invalid job: their failure is "cannot work" or "wrong machine".

**If real.** The exception is how the first destroyed-photos incident
happens: the sentence pasted from a forum, or typed by the person whose
files are on the line because a tool called them overcautious. The
safeguards are the sentence itself (no short form, no flag), the separate
launcher, the loud record, and this entry. The path is for machines whose
files are already copied off; the launcher says so three times.

**Decided (2026-09-13, the owner):** build it, exactly that narrow, and it
does not widen.

**Closes when.** Never. It is a standing cost. What can change is its width,
and the rule is that it does not widen.

**Where it stands.** `[####]` in use on the Aspire since 2026-09-20, lifting
`disk-health` only since run 4: `r18-prologue.csv` lines 7 to 12, 15 and 16;
`v9-erase.csv` lines 11, 22, 30 to 32. Nothing has been lifted beyond the two
refusals. Its runs are logged under R18 and R27.

## R24: The walk-away resume: a SYSTEM startup task and a RunOnce entry · high · held-on-machine

**What.** The prologue restarts Windows at least once before the handoff
(the disk check, the pagefile re-measure) and must carry on with nobody at
the keyboard; the fork is chosen in advance in `job.json`. Until 2026-09-13
the resume was an elevated logon task, and every row that had fired had
someone signed in, so the walk-away half had never been tested.

**If real.** Two stakes that are not R18's. Defender or SmartScreen scores a
SYSTEM `AtStartup` task from an unsigned script plus an HKLM `RunOnce` value
as "survive a restart": if it removes them the conversion stalls at the
sign-in screen with Windows intact (a stall, not a loss; `-Resume` by hand).
And a SYSTEM task from a script under `ProgramData` hands the machine to any
user who can replace the script: `Protect-StateDir` locks the folder to
SYSTEM and Administrators and refuses to register the task if any Users
write permission survives, and the resume records the permissions read back.

**Decided (2026-09-13, the owner):** the resume runs as `NT AUTHORITY\SYSTEM`
at startup (`AtStartup`, `ServiceAccount`, `StartWhenAvailable`), polls for
the stick by volume id, records who ran it and whether a session existed,
and queues anything a person should read as a one-shot `RunOnce` notice.
Holding the person's Windows password for an automatic sign-in was refused.

**Closes when.** Probe rows from three more vendors, one BitLocker-on row,
one Fast Startup row, and no Defender detection across them. Or a signed
release (R12), after which the antivirus half no longer matters.

**Where it stands.** `[####]` `walkaway-probe.csv` line 3 (the Aspire,
2026-09-13, `resumed-unattended`: SYSTEM in session 0, 38 s after boot) and
line 5 (2026-10-10, the Rust prologue); on the product path the Aspire's runs
3 to 8 (`r18-prologue.csv` lines 7 to 12) each resumed as SYSTEM with nobody
signed in, run 6 through two Windows Update restarts (R25). Rig: line 6, 472 s
to the first Linux boot. Residue: BitLocker with a PIN (the promise does not
apply; `evaluate` should say so), TPM-only machines at the restart, Fast
Startup, managed devices (scanner 0.3.0 reads the policy). Owed: the vendor
rows, and something on screen during an unattended resume
([log](log/2026-09.md#2026-09-13-the-walk-away-resume-as-system-rig-row-6-and-the-aspires-probe-r18-r24)).

## R25: Windows Update restarts the machine during the prologue · high · held-on-machine

**What.** The prologue restarts Windows before the commit line and relies on
Windows restarting only when asked. On the Aspire's run 6 (2026-09-23) its
restart let a waiting update (KB5129195) finish, and Windows then restarted
twice more by itself, once 3 s into the resumed re-measure. A machine being
converted is an ordinary Windows until the commit line.

**If real** (it is; the question is when it bites). Before the shrink it costs
a re-measure. After it, an update restart can change what was measured. With
the one-time boot armed, an update restart could use it up: the firmware
boots the stick while Windows expected to come back and finish updating, and
what Windows does then is not known. Treated as the worst case, a broken
safety net before the line: high.

**Decided (2026-09-26, the owner, "get the update out of the way"):** refuse
by default. The prologue reads three pending-restart markers before anything
changes and again right before arming; a waiting update gets a restart of
the prologue's own (up to three, 10 minutes each) and the run stops if it
does not clear; a waiting update right before the arm stops the run and
grows C: back. Both launchers say the computer may restart first.

**Closes when.** A physical row in which a waiting update exists at CONVERT
and the prologue neither arms nor shrinks until it has cleared, and a rig
row that injects the pending state.

**Where it stands.** `[####]` the Aspire's run 11 (2026-10-04, `v9-erase.csv`
line 30): a real update waiting (`WU RebootRequired` true), one restart as
SYSTEM, nothing waiting, then armed. And its keep-Windows conversion of
2026-10-11 (`r18-prologue.csv` line 18, `pass-plumbing`): a fresh Windows
with an update restart waiting; the prologue restarted first, resumed as
SYSTEM, then shrank and armed. The rig read a real pending state on
2026-09-26 and 2026-10-08 (line 13). Not yet shown: the CBS markers on
Windows 11, and the injected rig row ([log](log/2026-10.md#2026-10-11-the-detector-fires-on-the-aspire-r36-the-product-way-r21-r22-r25-r36-v1b)).

## R26: The offer to discard Windows after a failed shrink · critical · planned

**What.** When keep-Windows cannot fit and the person chose *ask me then* at
CONVERT, the prologue stops as it does today. Then, at the next sign-in, it
offers to delete Windows and keep only the listed folders, staged to the
stick (`architecture.md`, "When Windows cannot be kept"). Prompted by the
Aspire (R18): a disk that cannot free 25 GB after every mitigation.

**If real.** Each is a trust-ending way to lose files: a file outside the
folder map (a `C:\Projects`, a second profile, mail stores) is gone; the
stick is the only copy for a while and cheap flash lies (R17); consent given
in frustration, after something failed; stale numbers days after the stop;
programs and licences that live only in Windows; OneDrive online-only files
copied as stubs (R8).

**Decided (2026-09-26, the owner):** stop is the default at every step; the
offer only on the failure branch, to a person who chose *ask me then*; no
offer when the files do not fit the stick; the consent is its own typed
sentence, separate from R23's, lifting no refusal; a yes re-runs the normal
chain with a new job carrying the sentence and the numbers shown; the wipe
stays in the live session behind the checksum read-back and the human check.
Online-only files are not downloaded: they stay in OneDrive.

**Closes when.** On the rig: a failed shrink, the offer, the typed yes,
staging read back, the wipe install, every staged file restored with matching
checksums, and the "does not fit" and "stick drops" paths refusing before the
wipe. Then one physical row on a machine whose owner chose to lose Windows,
plus evidence of the folder map's coverage on real machines.

**Where it stands.** `[#...]` designed, not built. The harvest half exists
(job writer 0.10.0, `harvest.stick_fit`; `harvest-folder-map.csv`): the
Aspire's six folders hold 16.6 GB in 35,351 files, one of 5.9 GB, so on the
8 GB FAT32 stick the offer would not be made; 70 GB sits outside the map (a
Docker disk, a WSL Ubuntu disk with the person's Linux files). Waits on:
staging moved to the exFAT partition, the live-session check, `settle-in`'s
pull, and a first physical install row (rule #4)
([log](log/2026-09.md#2026-09-26-the-harvest-half-built-and-the-aspires-folders-measured-r26)).

## R27: The one-click erase and install · critical · held-on-machine

**What.** A launcher that, after one typed sentence and a password, erases
every internal drive and installs Fedora with nothing kept
(`architecture.md`, "Erase and install"). The first path whose commit line
destroys everything on the machine; a 2-minute countdown in the installer is
that line. The owner chose it as the first destructive path to prove end to
end: the mechanism before the migration.

**If real.** The wrong drive is erased (every drive is named by serial,
unique id and size, checked in Windows and again in the installer); the
countdown does not show, or does not cancel; the sentence is typed unread; a
failing drive fails the install after the line; a wrong password hash makes
an account nobody can open.

**Decided (2026-09-26, the owner):** the system on the drive holding C:,
`/home` on the second internal drive; a 2-minute countdown where any key
cancels and silence goes ahead; the sentence "I confirm that everything on
this computer will be deleted and nothing will be kept" on its own launcher,
with R23's also typed on a RED machine; carrying files across is stage 2.

**Decided (2026-09-27, the owner):** the launcher asks what the computer
starts at (KDE, GNOME or the text console) and names the sign-in on screen.

**Closes when.** The rig arms (refuse, cancel, erase) and one physical row:
one click, the sentences, nobody at the keyboard, Fedora signing in with the
chosen password at the graphical sign-in screen. Met 2026-10-04 (run 11).

**Where it stands.** `[####]` `v9-erase.csv` lines 30 and 31 (the Aspire,
run 11, 2026-10-04: `erased-installed` with the stick left in, then
`not-installed-again`) and line 32 (2026-10-06, `cancelled-untouched`, a key
during the countdown, Windows back untouched). Two physical `fail` rows stay
in the record: line 11 (run 9, a text login) and line 22 (run 10, installed
twice, R35). Rig arms from line 2 on, with the Rust prologue since
2026-10-08 (lines 33 to 36). Kept open for: other firmware, a healthy drive, a
plain-words screen when an install fails after the line, the installer hang
of arm C5 (once), and the computer kept awake (R33)
([log](log/2026-10.md#2026-10-04-the-aspires-run-11-the-first-clean-conversion-r27-r35-v9)).

## R28: settle-in runs on any Linux · high · held-on-machine

**What.** `settle-in` must work on whatever Linux the person picks, not only
Fedora (`architecture.md`, "It runs on any Linux"): one self-contained
program that draws its own window and works through kernel interfaces, fed
by a small installer adapter per distribution through one handoff folder.

**If real.** The window does not appear, so no welcome and no file pull; the
floor (systemd, NetworkManager, the kernel interfaces) is wrong for some
distribution, which must be refused in plain words; writing the hardware
clock and firmware variables directly is a writer (rule #4); a binary on the
stick must match its source; the window needs glibc 2.39 (Debian 12 has
2.36); a sign-in hook can freeze the desktop; a wrong Windows clock gives a
wrong offset, which the network then corrects.

**Decided (2026-09-27, the owner):** `settle-in` removes the conversion's own
firmware entry at first start, matched only by the BCD id the prologue
recorded; the old Windows entry is a button, with the owner's words in a
polkit policy for that one command; the window opens 5 s after sign-in and
asks for focus; GNOME's refusal of focus is left as it is.

**Closes when.** The same `settle-in` file, unchanged, runs its first startup
on Fedora KDE, Fedora GNOME and at least one non-Fedora distribution, on the
rig and then on a real machine, each leaving a capture (V10).

**Where it stands.** `[####]` `settle-in-first-start.csv` line 20 (the
Aspire, run 11, 2026-10-04, Fedora 44 KDE, `pass`): the clock corrected, the
saved Wi-Fi connected by itself 7 s after NetworkManager started, SSH carried
keys only, the own entry removed, the window in front. Lines 18 and 19 (run
10, GNOME) fail on Wi-Fi only (R35). Rig passes from line 4 on. Findings not
fixed: the old-entry button was never offered on real firmware (nothing named
Windows was left), a stale "upgrade_ go back to Windows" entry nothing
removes, the window opening again at the second sign-in (not decided). Owed:
GNOME's physical pass, a non-Fedora distribution, an older glibc baseline
([log](log/2026-10.md#2026-10-04-settle-ins-first-physical-pass-run-11-r28-v10)).

## R29: The Microsoft third-party UEFI CA is off on Secured-core PCs · high · planned

**What.** The handoff boots Fedora's shim, signed by Microsoft's third-party
UEFI certificate (the "Microsoft UEFI CA"), not the one Windows uses.
Lenovo's guide for Secured-core PCs (Copilot+ PCs among them) says that
certificate is turned off by default on those machines. If so, the firmware
refuses shim and the one-time boot into the stick never happens
(`docs/research/device-feasibility.md`, source C33).

**If real.** On those machines the handoff fails. Falling back to Windows is
fail-safe but a dead end with no explanation; stopping at a firmware error
screen breaks walk-away. The stranded Windows 10 machines this project
targets mostly predate Secured-core, so the stakes are "a class of machines
silently can't convert", not data loss.

**Closes when.** A primary source (Microsoft) confirms or denies the default,
and the scanner either reads the firmware's trust setting and refuses with
plain words ("turn on 'Allow Microsoft 3rd Party UEFI CA' in your firmware
settings"), or a real Secured-core machine shows the handoff works. Every
contact leaves a capture (rule #5).

**Where it stands.** `[#...]` raised 2026-09-27 by research; only Lenovo's
document was read. Nothing built. Owed: Microsoft's own documentation, other
vendors, whether the setting can be read from Windows before the handoff,
and one Secured-core machine.

## R30: The way back to Windows · high · held-on-rig

**What.** After an erase, or after a reclaim, there is no Windows to roll
back to. A new and empty Windows is offered by its own program on the Linux
side (`architecture.md`, "The way back to Windows"): a guided stick first,
the walk-away reinstall later (R33). `evaluate` harvests which Windows it was
and how it was activated (`harvest.windows_license`), never a key.

**If real.** Activation does not come back (edition, account-linked licence,
changed hardware) and the person believes we broke it; Windows 11 refuses the
hardware, so the way back is Windows 10, whose free updates ended in October
2025; Microsoft stops offering Windows 10; `install.wim` is over FAT32's 4 GB;
the stick writer is a disk write (R16's bar); a key must never reach our
files (R13); the walk-away reinstall is an erase (critical, R33).

**Decided (2026-09-27, the owner):** a way back on every path, and it says
its cost first; its own program in the app menu, not the first-start window;
guided stick first; the harvest records facts, never keys, and a failed read
is recorded, not a refusal; the erase launchers say it before the sentence
is typed (words a draft, awaiting the owner's approval).

**Closes when.** The guided stick has put back an activated Windows on real
machines of two licence kinds (a firmware key; a digital licence without
one), each run leaving its harvest from before and after, and a machine that
cannot run Windows 11 has been shown the Windows 10 path with its warning.
The walk-away reinstall closes separately (R33). V11.

**Where it stands.** `[###.]` `v11-way-back.csv` line 3 (2026-09-27,
`pass-plumbing`): a stick from Microsoft's Windows 10 22H2 ISO, `install.wim`
split by wimlib, Windows Setup installed in a Gen 2 VM with Secure Boot on.
The licence harvest (job writer 0.16.0) was read once by hand on the G16, not
a row. The Aspire's firmware carries a maker's key (`MSDM`); its Windows 11
Home came back activated with a digital licence on 2026-10-03, through R33's
path, not the guided stick. Owed: the guided stick on a real machine, Windows
11's ISO, a digital licence without a firmware key, the words approved
([log](log/2026-10.md#2026-10-03-the-first-after-reinstall-licence-data-point-r30)).

## R31: The window in front of the scripts · medium · built

**What.** `UPGRADE.exe` (`upgrade_/windows/window/`) replaces the black
console with a window. It runs the kit's flows with the launchers' arguments
and shows their progress; before the restart it registers a one-shot sign-in
task that opens it again to show the result (`architecture.md`, "Stack").
Since the port (R32) the launchers are flows inside it.

**If real.** The window says something the scripts did not: a softer word, a
forward button after a refusal, "you can still cancel" at the wrong moment
(rules #1 and #3 in a new place); it cannot draw (egui needs OpenGL, and a
basic display driver may give no context); it does not come back after the
restart; closing it mid-run; antivirus, since an unsigned `.exe` that
elevates and registers a sign-in task is R12's pattern. Once it fronts
convert and erase its words sit on the commit line and this risk is critical.

**Decided (2026-09-27, the owner):** a Rust window calling the scripts from
the stick, the verify flow first; it must reopen after the restart. The
`.cmd` launchers stay on the stick as the fallback until the window has its
rows.

**Closes when.** V12's rows: the rig, then the Aspire, then a machine on the
basic display driver.

**Where it stands.** `[##..]` built; no row has passed. `v12-window.csv`
lines 2 and 3 (the rig, 2026-09-28 and 2026-10-08) read `stopped-before-arm`:
the window could not open with OpenGL in the guest ("egui_glow requires
opengl 2.0+"), tried wgpu, and stopped at step 2. On the Aspire the window
launched the Rust prologue on 2026-10-10 (`r18-prologue.csv` lines 15 and
16, "from UPGRADE.exe --accepting-data-loss"), but no V12 row records that
run or its reopening after the restart. Owed: a rig row that reaches the
arm, the Aspire's row, the basic-display machine, and signing.

## R32: The port from PowerShell to Rust · critical · held-on-machine

**What.** The Windows side (about 10,600 lines of PowerShell: scanner,
harvester, job writer, stick writer, kickstart generator, prologue, handoff
and rollback) is rewritten in Rust, so one language runs the conversion from
the window to the disk (`architecture.md`, "Stack"). The rows the scripts
earned do not prove new code (rule #2).

**If real.** A port decides differently and nobody sees it: a verdict one
step softer, a refusal that no longer fires (in the prologue, data loss, so
critical); Windows' tools still speak text, so every parser carries the same
traps; old proof cannot all be replayed; the `.exe` must be rebuilt byte for
byte from source (R14).

**Decided (2026-09-27, the owner):** five steps, safest first, each script in
use until the parity ledger (V13) has a Rust pass beside every line it earned;
builds reproducible; raw tool output kept. **Decided (2026-10-04, the
owner):** the whole port built on the branch `rust-port`, the cut-over after
one success of the PowerShell process on `main`, the rig re-run before any
machine. **Decided (2026-10-11, the owner):** the PowerShell leaves the
product now, one Rust product: the scripts go, the ledger lines they still
hold stay `owed` to the Rust, and the goldens recorded from them are frozen
(`FROZEN-GOLDENS.sha256`, `port-check.sh`); the Rust is the reference. Three
phases in `RUST-PORT.md`, "What comes next".

**Closes when.** Every line of `port-parity.csv` reads `pass` (self-tests
and corpus as `cargo test`, rig rows re-run by the same harness, physical
rows re-run on the machine), no PowerShell on the stick or in `data/`, and a
release rebuilt byte for byte by someone else.

**Where it stands.** `[####]` the whole Windows side in Rust, the cut-over on
the Aspire (`v2-install.csv` line 15, `pass`; `r18-prologue.csv` lines 18 and
19); phase 1 of the leaving done 2026-10-11 (the ten scripts, their recorders,
the launchers, `build.sh` and `dist/` gone; the port check Rust-only; the kit
built); ledger 657 `pass`, 45 `owed`. Owed: the unforced rig row on the fixed
kit (the rig's guest channel is down), phase 2 (the storage-mode harness, the
diag tools, the stick writer), phase 3 (the data tables), the 45 lines, the
rebuild by someone else
([log](log/2026-10.md#2026-10-11-the-powershell-leaves-the-product-decided-phase-1-done)).

## R33: The walk-away way back to Windows · critical · held-on-machine

**What.** "Go back to Windows", 100% managed (`architecture.md`, "The way
back to Windows", stage 2). From the installed Linux: download Windows from
Microsoft's catalog, build a stick, restart into it once; a gate of ours on
the stick matches the drives and counts down 2 minutes; then Windows Setup
erases both internal drives and installs Windows with nobody at the
keyboard. R27 in reverse, so it carries R27's bar and rule #4.

**If real.** The wrong drive (WinPE's disk numbers differ, so the gate
matches by serial or world-wide name and exact size); a download that is not
Microsoft's (plain HTTP; size and SHA-1 against the catalog, whose signature
is not checked yet); the countdown does not cancel; Setup waits at a screen
with Linux already erased; the firmware ignores `BootNext` (safe) or keeps
it (Setup twice); Windows 11 refuses the processor; a passwordless account.

**Decided (2026-09-29, the owner):** Microsoft's catalog; both drives erased,
the second left empty; the commit line in the gate on the stick, before Setup.

**Decided (2026-10-02, the owner):** the way back carries Wi-Fi, as every OS
switch does. **Decided (2026-10-04, the owner):** it carries the time and
remote access (SSH, keys only) too. **Decided (2026-10-05, the owner):** the
product keeps the computer awake while it works, on both sides; backlog.

**Closes when.** Rig arms refuse, cancel and erase pass with Secure Boot on,
nobody at the keyboard, ending at the Windows sign-in; then one real machine
(the Aspire, under R23) with a physical cancel before the erase. V11, step 4.

**Where it stands.** `[####]` `v11-walkaway.csv` line 2 (2026-10-03, `fail`
on four defects, all fixed that day: the start through the firmware's own USB
entry, the password forced at first sign-in, Wi-Fi carried, progress shown),
line 3 (2026-10-04, `windows-reached`, nobody at the keyboard, activated),
line 4 (2026-10-05, `cancelled-untouched`; gate 0.3.0 had lost its writes
silently, 0.3.1 reads every write back), line 5 (2026-10-11,
`windows-reached`, driven over ssh on a second stick, the carried Wi-Fi up
30 min after the restart). Owed: a cancel started by the program's own
restart; never-twice on real firmware; the sleep inhibitor; the program
findable from the desktop and a no-serial stick refused (the owner's call);
the stale entry each Prepare leaves ([log](log/2026-10.md#2026-10-11-the-way-back-run-3-driven-over-ssh-r33-v11)).

## R34: Secure Boot revokes the kit's boot files · critical · held-on-machine

**What.** Secure Boot keeps a revocation level for Linux boot programs (SBAT,
a UEFI variable shim reads, one `component,generation` line each). A boot
file whose own `.sbat` names a lower generation is refused. Windows raises
the level through Windows Update, and so does a newer shim; it only goes up.
Found on the Aspire's run 10 (2026-10-03): the new Windows 11 had applied
`2025051000` (`grub,5`), the kit's Fedora 42 GRUB was `grub,3`, shim refused
it, and the firmware fell back to Windows.

**If real.** The stick will not start: safe, but it reads as a handoff that
did not fire. Far worse, the installed system will not start, because the
erase installs the same old GRUB: a machine with nothing on it that boots.
Everything that can refuse must refuse before the line (rule #3).

**Decided (2026-10-03, the owner):** the release is not fixed in the kit.
`evaluate` reads what the machine accepts and the kit is built for a release
that meets it, from `data/releases.ps1`, whose facts are measured from each
release's own files by `data/tools/measure-release.py`, never typed (V14).

**Closes when.** The kit carries boot files that meet the level on the
machines in the record, the installed system's files are checked too, and
the Aspire boots the stick and the installed system with Secure Boot on. Met
2026-10-04 (run 10, kit ee74318; held again in run 11). Kept open because the
level moves.

**Where it stands.** `[####]` scanner 0.4.0's *Secure Boot revocations*
check fired over SSH on the Aspire (Fedora 42 files FAIL, Fedora 44 OK); the
kit moved to Fedora 44 (`make-kit.sh --release`; rig arm C 2026-10-03, the
Secure Boot half on the `UPGRIGSB` VM); runs 10 and 11 started the stick and
the installed Fedora 44 with Secure Boot on. Still open: shim's list already
holds `2025112400` (`grub,6`), which Fedora 44's GRUB is not, and the check
catches that without fixing it; Fedora 44's release shim is signed under the
2011 authority only; the Aspire cannot take Microsoft's 2023 keys. Owed: the
next release when the level moves, and other machines
([log](log/2026-10.md#2026-10-04-the-secure-boot-clause-met-on-the-aspire-runs-10-and-11-r34-v14)).

## R35: The stick installs again after the install · critical · held-on-machine

**What.** The stick starts the installer whenever it carries the marker file
`upgrade_/boot-install`. The Windows side removed it at every stop; nothing
on the Linux side ever did. That was safe only while the firmware started the
new system after the install, and not the stick. On the Aspire's run 10
(2026-10-04) the firmware started the stick again, with Fedora first in its
order; the stick erased and installed a second time and began a third
countdown, which the owner's key stopped at 15.9 s.

**If real.** With nobody at the keyboard it never stops: erase, install,
restart, for ever. A second commit line nobody agreed to (rule #3). Run 10
cost no data (an erase), but the Wi-Fi password was gone with the first
install, the first install's records were overwritten, and a cancel's screen
said "Restarting into Windows" when no Windows was left.

**Closes when.** Arm D passes on the rig, and the Aspire finishes an install
with the stick left in and comes up in Fedora once. Met 2026-10-04 (run 11).
Kept open for the list below.

**Where it stands.** `[####]` fixed 2026-10-04: `verify.sh` 0.6.0 writes
`upgrade_/converted` and removes `boot-install` at the line (or refuses and
erases nothing); `grub.cfg` hands a converted stick over to the installed
system; prologue 0.12.0 clears `converted` only when arming a new job behind
its typed sentences; rig arm D (`not-installed-again`). `v9-erase.csv` lines
30 and 31 (the Aspire, run 11): the firmware started the stick again after
the install and it handed over to Fedora, once more after a restart with the
stick in, partition tables unchanged; rig D rows at lines 24, 27, 29 and 34.
Still open: other firmware; a failed install after the line has no
plain-words screen; a cancelled stick on USB-first firmware may loop into
"returning to Windows"; why this firmware started the stick with Fedora
first is unknown
([log](log/2026-10.md#2026-10-04-the-aspires-run-11-the-first-clean-conversion-r27-r35-v9)).

## R36: The kept Windows' clock runs hours wrong after every Linux boot · medium · held-on-machine

**What.** Windows keeps the hardware clock in local time. Linux keeps it in
UTC, and `settle-in` stores it as UTC on first startup (`architecture.md`,
"The clock"), which is right when Windows is gone. On the keep-Windows path
it is not gone: Windows reads the UTC as local time, so after every Linux
boot the kept Windows runs fast by the zone's whole offset. Seen on the
Aspire 2026-10-10: 7 h fast in Pacific time, `RealTimeIsUniversal` absent,
the Windows time service stopped, and nothing on the Linux side moving it
back.

**If real** (it is). Every person on the default path who restarts into the
kept Windows finds its clock hours wrong: websites, sign-in codes, mail
order and the stamps on files edited there. Nothing is lost, but a safety
net that looks broken is not trusted. Medium.

**Decided (2026-10-10, the owner):** Linux adapts. The kept Windows is not
touched (rule #4), and reclaim returns the machine to UTC later anyway. The
other way, `RealTimeIsUniversal` written into Windows by the prologue, was
not taken. Built the same day: the kickstart (0.7.0; the Rust generator
follows it) writes `timezone <zone>` without `--utc` when the harvest says
the Windows clock is local, so `/etc/adjtime` is LOCAL from the first boot;
`settle-in`'s clock step on keep-Windows (`left-local`) never writes the
hardware clock; the rig's bench records the host's clock beside the guest's
and `v2-verdict` says `windows-clock-wrong` past five minutes.

**Closes when.** The Aspire does a Linux boot followed by a Windows boot
with Windows' clock within a minute of the host's, read the way it was read
here, on an install made with kickstart 0.7.0 or a `settle-in` first start
that wrote LOCAL; and the rig repeats the two boots on the keep-Windows arm.

**Where it stands.** `[####]` held on the Aspire both ways. By hand on
2026-10-10 (`timedatectl set-local-rtc 1`; Windows +3 s and +3 s against the
host after two Linux boots; `v2-install.csv` line 14, `pass`). On the product
path on 2026-10-11 (kickstart 0.8.0, `settle-in` `left-local`; +1 s, +2 s and
+1 s after each Linux boot, nobody touching the clock; line 15, `pass`).
Windows had also corrected its own clock within four hours on 2026-10-10, so
the symptom lasts hours, not for good. Owed: the rig's two boots on the
keep-Windows arm ([log](log/2026-10.md#2026-10-10-the-kept-windows-clock-by-hand-on-the-aspire-r36-v1b)).

# Closed

## F1: `break` inside `ForEach-Object` terminated the whole script · high · closed

**What.** `Get-HarvestFolderStats` limited work on large folders with a
45-second stopwatch and `break`. With no loop around it, PowerShell unwinds
past the function and ends the entire script, silently, with exit code 0.
**If real.** No error, no `state.json`, for anyone with a large Documents
or Pictures folder (the test machine's largest holds 447 files).
**Closes when.** The pipeline stops correctly and a test holds it there.
**Where it stands.** Closed: `Select-Object -First` replaced the `break`,
locked in by a harvester `-SelfTest` case (2026-08-22).
[log](log/2026-08.md#2026-08-22-the-four-fixed-defects-locked-in-by-self-tests-f1-f2-f3-f4)

## F2: `$args` collision reported zero Wi-Fi networks · high · closed

**What.** A local variable named `$args` inside a function hid PowerShell's
built-in `$args`. Passing it on sent nothing to `netsh`, so no profiles.
**If real.** The harvester cheerfully reported 0 networks on a machine with
14, and the person arrives in Linux with no Wi-Fi.
**Closes when.** The call reaches `netsh` with its arguments.
**Where it stands.** Closed: renamed to `$netshArgs` (2026-08-22). No direct
test (the bug sat on the live side of the parse seam), but the seam keeps
the parsing, where a silent empty result would hide, under test.
[log](log/2026-08.md#2026-08-22-the-four-fixed-defects-locked-in-by-self-tests-f1-f2-f3-f4)

## F3: UTF-8 SSIDs mangled · medium · closed

**What.** `Get-Content -Raw` reads as ANSI on PowerShell 5.1, turning
`<name>'s iPhone` into `<name>â€™s iPhone`.
**If real.** Any network name with a curly quote, accent or emoji made a
NetworkManager profile that never connects, looking like a broken Wi-Fi
driver rather than a text-encoding bug.
**Closes when.** The export is read with its declared encoding.
**Where it stands.** Closed: switched to `XmlDocument.Load()`, which honours
the declared encoding; harvester `-SelfTest` case (2026-08-22).
[log](log/2026-08.md#2026-08-22-the-four-fixed-defects-locked-in-by-self-tests-f1-f2-f3-f4)

## F4: Visual Studio rule could never match · low · closed

**What.** `(?<!Microsoft )Visual Studio (?!Code)` excluded "Microsoft Visual
Studio Community 2022": the very product the rule was written to flag, and
how nearly every real install is named.
**If real.** The app-risk database never warns about Visual Studio.
**Closes when.** The rule matches the real product name and a test holds it.
**Where it stands.** Closed: corrected to `Visual Studio (?!Code)`, locked in
by the scanner `-SelfTest` case "Microsoft Visual Studio Community 2022 must
match" (2026-08-22).
[log](log/2026-08.md#2026-08-22-the-four-fixed-defects-locked-in-by-self-tests-f1-f2-f3-f4)
