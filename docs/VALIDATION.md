# Validation gates

This is the plan for proving things before we build more on top of them.
`RISKS.md` lists what might be wrong. This file is the ordered plan for
finding out. The same rule applies to both: nothing here closes because the
argument sounds good. Only evidence closes it.

A **gate** is one question that has to be answered "yes" before the work
that depends on it goes ahead. Gates are ordered by what dies if the answer
is no, weighted by how little evidence exists today. Each gate states, in
the same five parts: why it has its own gate, what passes (the checklist,
in the words its CSV uses), how it is run (the rig arm, the real machine,
the scripts by name, and the residue only a real machine can prove), what
we build instead if it fails, and where it stands (one paragraph, the
latest row, what is owed, a link to the log).

Gates are grouped into the same **tiers** used in `CLAUDE.md` and
`RISKS.md`, so the three agree:

- **Tier 1, no product if these fail:** V0, V1, V1b
- **Tier 2, a core promise breaks (recoverable):** V4, V3, V2, and the
  three built since: V9 (the one-click erase and install, the current front
  since 2026-09-26), V10 (settle-in) and V11 (the way back to Windows)
- **Tier 3, silent data loss:** V8
- **Tier 4, kills adoption, not the mechanism:** V5, V6, V7, and V12, V13,
  V14

The V-numbers are fixed names, not a priority order. Read the tier, not the
number. **The meta-rule: nothing gets built on top of a gate it depends on
until that gate is proven.** The order of work and what each step waits on
live in `ROADMAP.md`; where every gate stands today, in one table, lives in
`STATUS.md`; the runs themselves are in `log/`.

**Method note (2026-08-22): spoof everything spoofable.** Each gate splits
in two:

1. **The part we can test without the real thing.** Detection logic fed
   made-up objects, replays of recorded real machines (`-DumpMachine` →
   `evaluate/windows/corpus/`), and fake devices inside virtual machines.
   This part gets automated tests that run on every `-SelfTest`.
2. **The residue.** What only a real machine or a primary source can
   settle. The residue is what a gate's "What passes" means.

A green simulation narrows a gate. It never closes one, because the
simulation is built from our own model of the hardware, and that model is
exactly what the gate is there to question. CLAUDE.md rule #5 has the full
statement.

A few words used all through this file:

- **The rig** is our test bench of virtual machines: QEMU with OVMF (a free
  UEFI firmware) under `rig/vm/`, and Hyper-V under `rig/hyperv/`.
- **`pass-plumbing`** means the pipes all connect on the rig. It is not a
  pass on real hardware.
- **Line N** of a CSV in `docs/validation-results/` is one recorded run, as
  an editor numbers it (line 1 is the header). Rows are written by scripts,
  never by hand. Each file's columns and result words are explained in
  `validation-results/README.md`.
- **SB** is Secure Boot, the firmware feature that only starts signed boot
  code.
- The state word at the end of each heading, and its bar, come from the
  docs skill: `planned` `[#...]`, `built` `[##..]`, `held-on-rig` `[###.]`,
  `held-on-machine` `[####]`, `closed` `[####]`; `[FAIL]` flags a real
  failure that stays in the record.

---

# Tier 1: no product if these fail

## V0: The boot handoff fires · kills: walk-away itself · RISKS R15 · held-on-machine

**Why it has its own gate.** The whole walk-away promise rests on one
command: `bcdedit /set {fwbootmgr} bootsequence`. `bcdedit` is Windows'
tool for editing its boot settings; that command asks the firmware to boot
the USB stick exactly once, on the next restart only. It has to work on
firmware from vendors who have never heard of us.

**What passes.** A `v0-handoff.csv` row reads `fired-once` with
`keypress_free=y`: the stick booted once, the one-shot cleared itself, the
boot order was intact, and Windows came back with nobody at the keyboard.
A deliberate failure (`-FailMode`: no file, an unsigned payload) must fail
*safe*: Windows boots and the row says so (`ignored`), because a failure
the tool cannot detect is one it cannot report. The gate passes when it
fires on every tested firmware, or fails safe on the ones where it does
not. One vendor is not the matrix: Acer is in; Dell, Lenovo and HP are the
population still owed. Vocabulary: `validation-results/README.md`.

**How it is run.** The harness is `upgrade_/windows/Test-Handoff.ps1`
(arm, reboot, check; `-FailMode` for the fail-safe paths) and, since the
port, `upgrade-prologue verify-arm`. The one-click flow is `RUN-TEST.cmd`:
one double-click, one UAC "Yes", then `-Arm -Auto` registers an elevated
logon task that runs the return check after the reboot, classifies, cleans
up and writes the row to the stick. The stick is built by `./make-kit.sh`
(`dist/kit/stick-shell`, `dist/kit/stick-shim`; run-book `README-STICK.txt`
on the stick). The mechanism can be undone: one BCD entry, exported first,
removed by `-Check` whatever the outcome; no disk is written. Rig first
(`rig/vm/`, QEMU and OVMF, Secure Boot off; `rig/hyperv/`, Gen 2, Secure
Boot on with a virtual TPM for the BitLocker rows), then real machines. A
borrowed machine's half-hour visit also runs `RUN-PROBE.cmd` (the
walk-away resume, R24, one `walkaway-probe.csv` row) and, on an Intel
machine with a SATA Mode option, `RUN-STORAGE-MODE.cmd` (V5). Every
physical run leaves a `-DumpMachine` capture for `evaluate/windows/corpus/`
and its row is copied word for word from the stick's CSV. Residue only a
real machine proves: the removable-USB clause no VM has, and whether a
vendor's firmware measures the one-shot into a sealed PCR.

**If it fails.** One manual step: "when it restarts, press the key we show
you." Walk-away drops from perfect to press-one-key, and the interface, the
docs and the marketing all change. That is why this is V0.

**Where it stands.** `[####]` held-on-machine, one vendor. The Acer Aspire
A515-51G fired once with Secure Boot on, signed payload, no key pressed,
through the one-click `-Auto` flow (`v0-handoff.csv` line 11,
`fired-once`, 2026-09-08; again line 12, 2026-09-12). The rig's latest row
is line 15 (`fired-once`, 2026-10-09). Owed: Dell, Lenovo and HP, and the
fail-safe rows (`NoFile`, `SecureBootUnsigned`) on real firmware
([log](log/2026-09.md#2026-09-08-the-first-real-machine-fired-the-handoff-v0-r15)).

## V1: Unattended install completes, Secure Boot on · kills: the conversion · held-on-machine

**Why it has its own gate.** The second half of the spine, and the base
both paths share: a stick carrying Fedora's signed shim, GRUB and kernel,
untouched, boots with Secure Boot on, and a **kickstart** (Fedora's answer
file) drives **Anaconda** (Fedora's installer) to a sign-in screen with no
human input. What is specific to installing next to Windows is V1b.

**What passes.** Hands-off from power-on to the sign-in screen, Secure
Boot still on. Two halves, each with its own file:

- the reversible half, `v1-live-boot.csv`: the handoff fires, the stick
  boots shim then GRUB, the **unmodified** Fedora installer starts with
  `inst.ks=` and `upg.mode=verify`, `%pre` (the kickstart's script before
  anything is installed) runs `upgrade_/linux/verify.sh`, which matches
  the job's disk by unique id or serial and exact size, checks the display
  and Wi-Fi, finds the Windows ESP (the small EFI System Partition that
  holds every OS's boot files), reads the chosen desktop image back byte
  for byte against the stick's manifest (R17's gate), writes `verify.json`
  to the stick, and Windows comes back by itself with nothing installed:
  `pass-plumbing`;
- the destructive half: an install from that chain that reaches the chosen
  sign-in screen with nobody at the keyboard. Its rows are V9's
  (`v9-erase.csv`, `erased-installed`) and V1b's (`v2-install.csv`).

Vocabulary: `validation-results/README.md`.

**How it is run.** `rig/hyperv/v1.sh run` on the Hyper-V rig (Secure Boot
off: Hyper-V's template refuses shim, the usual rig clause). On a real
machine, `RUN-VERIFY.cmd` from the stick: scanner, job writer, kickstart
(`upgrade_/windows/New-Kickstart.ps1`, now the Rust `upgrade_/kickstart`),
handoff, the live session, the return. The stick carries Fedora's own
Workstation and KDE live `squashfs.img`, unmodified, checked against
Fedora's published ISO hashes when fetched (`rig/vm/fetch-desktops.sh`),
shipped by `make-kit.sh` and named with `--checksum=` in the kickstart.
Residue a real machine proves: Secure Boot on through shim on that
vendor's firmware, a real stick's read speed (22.6 MB/s on the Aspire's,
the honest number for its time estimate), and the machine's own disk
identity (Windows reports an ATA disk's id as padded model and serial
text, not hex; the serial vote found it).

**If it fails.** A known simpler shape exists: the *unmodified* Fedora ISO
on one partition and the kickstart on a second volume labelled `OEMDRV`,
which Anaconda picks up by itself. Since 2026-09-08 the vertical uses that
shape, with `inst.ks=` on the stick itself instead of an OEMDRV volume.

**Where it stands.** `[####]` held-on-machine. Live boot: the Acer Aspire
A515-51G, Secure Boot on, Wi-Fi checked (28 networks), `v1-live-boot.csv`
line 5, `pass-plumbing` (2026-09-12; the rig's latest is line 6,
2026-10-09). Install: the Aspire reached its KDE sign-in unattended with
Secure Boot on, `v9-erase.csv` line 30, `erased-installed` (2026-10-04),
and beside Windows, `v2-install.csv` line 11, `pass-plumbing`
(2026-10-10). Owed: other vendors' firmware
([log](log/2026-09.md#2026-09-12-the-first-physical-live-boot-row-secure-boot-on-v1)).

## V1b: Installing alongside a shrunk Windows leaves Windows bootable · kills: the default path's safety net · RISKS R21 · held-on-machine

**Why it has its own gate.** The default keep-Windows path installs Linux
into space freed from Windows and **must leave the shrunk Windows fully
bootable**: Windows is both the way back and the source of the person's
files. This is harder than the wipe install, and it is the common case.

**What passes.** After the install, *both* systems boot, Secure Boot still
on, on every machine in the matrix. A `v2-install.csv` row (the
converter's own install) reads `pass-plumbing` only when the R21 checklist
holds: the Windows-made ESP (often 100 MiB) had room; `bootmgfw.efi`
(Windows' boot loader) is byte-identical before, after and across the boot
cycles; no Microsoft boot file changed; the firmware kept the Windows Boot
Manager entry; GRUB lists Windows; Windows and Linux are each reached
twice with no key pressed; and `fallback_loader` says what sits in the
ESP's fallback slot (`shim-in-windows-slot` on firmware that ignores
operating-system entries, R21). The bench kickstart's file is
`v1b-alongside.csv` (`fallback-loader-replaced` is its honest word for
shim overwriting `EFI/Boot/bootx64.efi`). The rollback's file is
`r21-rollback.csv`: Windows first again, its loader restored from the
snapshot `%pre` took. Vocabulary: `validation-results/README.md`.

**How it is run.** The bench is `rig/vm/v1b.sh` (QEMU; run-book in
`rig/vm/README.md`; `v1b-inspect.py` offline before and after, the shrink
by `rig/vm/guest/v1b-shrink.ps1`, the kickstart `rig/vm/v1b-ks.cfg` from
an OEMDRV volume, boot markers from each OS, `v1b.sh verdict`). The
product path is `rig/hyperv/v2.sh` and `rig/hyperv/prologue.sh`: the
prologue shrinks, `%pre` `verify.sh` snapshots the ESP to the stick,
`liveimg` installs the stick's squashfs into the freed space reusing the
Windows ESP unformatted, `%post` `outcome.sh` runs the checklist and writes
`outcome.json`; `v2-verdict.py` writes the row, `Invoke-Rollback.ps1`
(`ROLLBACK.cmd`) the rollback's. The Secure-Boot-on chainload is proven on
a UEFI-CA Hyper-V guest with Windows' PCA enrolled in shim's MokList
([log](log/2026-08.md#2026-08-31-secure-boot-on-chainload-via-mok-on-hyper-v-v1b-r21)). A real machine has
no bench marker: its evidence comes from `rig/hyperv/physical/` (the
collectors' boot lines, the offline inspector on an image of the drive's
first MiB and its ESP), judged by `v2-verdict.py --physical`. **Decided
(2026-10-04, the owner):** keep-Windows runs on the Aspire itself under
R23 (option C); a pass there is evidence for the mechanism with Secure
Boot on, not the healthy-drive row. **Decided (2026-10-10, the owner):**
shim in Windows' slot is proven on the rig first, then the Aspire.
Residue: a vendor's firmware keeping or dropping entries, Secure Boot on,
a real servicing pass.

**If it fails.** Machines whose firmware or ESP cannot take the alongside
install are steered to clean slate, and `evaluate` says so before anything
is committed. The product still converts them, without the safety net.

**Where it stands.** `[####]` held-on-machine, one machine with a dying
drive (R23). The Aspire, 2026-10-11: the return applied shim in Windows'
slot from its own evidence, then Fedora and Windows were each reached
with nobody at the keyboard, Secure Boot on, the clock right
(`v2-install.csv` line 15, `pass`). Line 11 (2026-10-10) was the same by
hand. The rig held the return, the guard surviving a servicing pass and
the rollback (lines 12 and 13, `r18-prologue.csv` line 17,
`r21-rollback.csv` line 5, all `pass-plumbing`). Owed: a real Windows
update under the guard (R22), the rollback on this machine, the
healthy-drive row on another machine
([log](log/2026-10.md#2026-10-11-the-detector-fires-on-the-aspire-r36-the-product-way-r21-r22-r25-r36-v1b)).

# Tier 2: a core promise breaks (recoverable, but the default is broken)

Failures we can recover from, but each breaks a core promise: V4 (do disks
shrink enough? this decides whether the default path even applies), V3
(the BITLK read that delivers files on the default path), V2 (firmware
that makes the speakers work), then V9, V10 and V11, the three gates of the
one-click erase, the first start and the way back. In V-number order.

## V2: Extracted amp firmware makes speakers work · kills: the artifact pipeline · planned

**Why it has its own gate.** The docs say `evaluate` must extract vendor
firmware *now* because it "cannot be added later". That assumes extraction
works at all: that the right files ("blobs") can be pulled from the
Windows driver store automatically, and that the Linux kernel accepts
them. The community does this by hand; nobody has shown it automated end
to end.

**What passes.** Speakers you can hear, using only firmware taken from
that machine's own Windows.

**How it is run.** On the G16 itself, which has the exact CS35L56 amp the
pipeline was designed for: extract from its driver store, install Fedora
by hand, put the firmware in place, play a sound through the *speakers*.
That checks extract, carry and inject on the hardware class that motivated
it. Nothing here can be spoofed; the whole gate is residue.

**If it fails.** The promise narrows to what upstream `linux-firmware`
covers, and the scanner tells owners of 2023+ laptops the truth about
their speakers instead of promising them. The "first impression is working
hardware" claim gets a hardware-generation asterisk.

**Where it stands.** `[#...]` planned. The experiment on the G16 has not
run: no row, no CSV, no log entry.

## V3: cryptsetup BITLK reads · kills: the default path's file delivery · RISKS R19 · held-on-rig

**Why it has its own gate.** BitLocker is on by default on most machines
this project targets. The keep-Windows path delivers the person's files by
reading the kept Windows partition in `settle-in`, unlocked through
cryptsetup's BITLK support (cryptsetup is Linux's disk-encryption tool;
BITLK is its BitLocker reader). A failure is recoverable (Windows is
intact, the person is present), but if the read is flaky the default is
broken for everyone on a modern BitLocker machine.

**What passes.** Identical hashes across all three configurations:
XTS-AES-128 used-space-only (the default), XTS-AES-256 used-space-only,
and XTS-AES-128 full. Thousands of files, hashed inside Windows and again
from an *installed* Fedora (where `settle-in` runs), byte-identical or the
row fails. One `v3-bitlk-read.csv` row per driver, per kernel, per config;
`pass-plumbing` on the rig. Reads go through ntfs-3g (the user-space NTFS
driver): the kernel's `ntfs3` oopsed five runs out of five on the F42
install kernel (R19). Vocabulary: `validation-results/README.md`.

**How it is run.** `rig/hyperv/v3.sh run <config>` (run-book in
`rig/hyperv/README.md`), on a guest with the Fedora-side hook installed
once (`guest/v3-bootstrap.sh`). It builds the OEMDRV transport carrying
the reader and the recovery password; boots Windows through GRUB, plants
and hashes (`guest/v3-plant.ps1`), with a full shutdown; boots Fedora,
unlocks, mounts and hashes again (`guest/v3-read.sh`, three driver
passes); `v3-verdict.py` writes the rows. Other configs are built with
`guest/v3-encrypt.ps1` on a copy of the pristine disk in a throwaway VM
(`v3.sh mkvm`), in the product's order (encrypt, then shrink), and read as
data disks. Residue no VM row closes: real disks, Windows 11's BitLocker,
physical machines, and "full-disk" as a real disk experiences it (the
rig's thin VHDX never had its free space rewritten).

**If it fails.** Either decrypt in Windows first (`manage-bde -off`: adds
hours, works), or BitLocker machines get clean-slate only. Both are
survivable, both are worse, and either changes the intent-capture screens.

**Where it stands.** `[###.]` held-on-rig. All three configurations read
2,850 of 2,850 planted files byte-identical under every driver on the
current kernel (`v3-bitlk-read.csv` lines 2 to 4 and 11 to 16,
`pass-plumbing`, latest 2026-09-02). Lines 6 and 7 (`guest-crashed`,
`incomplete`) are the install kernel's `ntfs3`; lines 8 to 10 (`mismatch`)
are one crypt32 cache entry the classifier then learned; all stay. Owed: a
real disk, and the file pull in `settle-in`, which is not built
([log](log/2026-09.md#2026-09-01-the-v3-bitlk-read-fired-on-the-rig-v3-r19)).

## V4: Real disks can actually shrink · kills: whether the default even applies · RISKS R18 · held-on-machine

**Why it has its own gate.** Keep-Windows is the default and needs about
20 GB of shrinkable space plus the person's data. Files that cannot be
moved (the MFT, NTFS's master file table; the VSS store System Restore
uses) often cap a shrink far below the free space. If most real machines
cannot shrink enough, the default rarely applies, and the design leans on
clean slate and big sticks.

**What passes.** A meaningful fraction (say, a third) of *elevated*
scanned machines could fit Linux plus their data in shrinkable space. Two
measurements count: the scanner's cold number (`Room to keep Windows`,
elevated only, with its `ShrinkSource`: `storage-api` and `diskpart` should
agree, and a disagreement is a finding in its own right), and the
prologue's re-measure by both paths after the disk check and the ladder,
at the moment it matters (`r18-prologue.csv`, `remeasured_gb` and
`diskpart_gb`). A C: carrying the dirty flag or a queued repair reports no
number until the prologue's check has run; count it separately, not as
"cannot shrink". Never predict a shrink number: only a re-measure says.
A row counts as walk-away only when `state.Resumes` shows SYSTEM, session
0 and no explorer; otherwise the verdict writes `resume-attended`.
Vocabulary: `validation-results/README.md`.

**How it is run.** The scanner ships the query and every elevated report
measures the population for free; judge after about 20 real reports,
filtered on `RanAsAdmin=true` (the sample leans toward people willing to
elevate). The prologue (`Invoke-Prologue.ps1`, now `upgrade-prologue`;
`RUN-CONVERT.cmd`, and `RUN-CONVERT-ACCEPTING-DATA-LOSS.cmd` under R23)
runs the disk check, the ladder (hibernation and pagefile off with one
restart, restore points and the change journal with consent) and the
re-measure. `rig/hyperv/prologue.sh` injects the dirty flag, and
`prologue.sh autologon off` makes the resume walk-away; `prologue.sh
probe` and `RUN-PROBE.cmd` (`-Probe`, read-only, one restart) write
`walkaway-probe.csv`. `evaluate` never repairs (R18). Residue: a healthy
real disk after a real check; the Aspire's is dying, so its rows measure
one bad disk.

**If it fails.** Keep-Windows becomes the lucky path rather than the
default. Messaging, stick-size advice and the intent screens shift toward
clean slate; the concrete form is the offer to discard Windows (R26,
designed, not built).

**Where it stands.** `[####]` held-on-machine, one disk. The Aspire's
answer is no: runs 4 to 8 stopped before anything irreversible, best 9.5
of 25 GB, three different last unmovable files (`r18-prologue.csv` lines 8
to 12, `stopped-shrink` but for line 9, `stopped-confirm`). The fresh
Windows 11 shrank with room to spare: lines 16 and 18 (2026-10-10 and
2026-10-11, `pass-plumbing`, the Rust prologue; 238.2 to 213.2 GB). The
resume fired unattended on real firmware (`walkaway-probe.csv` lines 3 and
5, `resumed-unattended`). Owed: the population count (about 20 elevated
reports) and a healthy drive
([log](log/2026-09.md#2026-09-26-the-aspires-run-7-stopped-shrink-v4-r18-r26)).

## V9: One-click erase and install · RISKS R27 · held-on-machine

**Why it has its own gate.** The owner's first end-to-end destructive
target (decided 2026-09-26): erase every internal drive, install Fedora,
keep nothing (`architecture.md`, "Erase and install"). Its commit line is
a 2-minute cancellable countdown in the installer, and it has its own
launcher and typed sentence.

**What passes.** Four arms in `v9-erase.csv`, one row each, written by
`rig/hyperv/v9-verdict.py` from the arm's own evidence, never by hand:

- A, refuse: a job naming a disk that is not attached; `%pre` refuses
  before any countdown, on one plain screen in the owner's words, and the
  machine restarts into an untouched Windows: `refused-before-countdown`;
- B, cancel: a key during the countdown; Windows comes back, both disks
  byte-unchanged at the partition table: `cancelled-untouched`;
- C, erase: the countdown left alone; both disks cleared, Fedora on the
  first, `/home` on the second, `outcome.json` with the commit line
  crossed at the countdown's end, the account's password signs in, and the
  chosen screen (KDE, GNOME or the text console, `graphical_login`) comes
  up: `erased-installed`;
- D, not installed again (R35): the stick left in and first in the boot
  order after C hands over to the installed Fedora, no countdown, both
  disks unchanged: `not-installed-again`.

`fail` is kept, never removed. The gate passes when all four hold on the
rig, then on a real machine: one click, the two typed answers, nobody at
the keyboard afterwards, Fedora signs in at the chosen screen. Vocabulary:
`validation-results/README.md`.

**How it is run.** `rig/hyperv/v9.sh` on a copy of the rig's Windows disk
plus a blank second disk (arm A is armed by the V0 harness), with the
installer's `verify.sh` (the countdown; the `converted` and `boot-install`
markers on the stick) and `outcome.sh`. Then the Aspire: the system on the
failing SSD under R23, `/home` on the 1 TB drive, the rows written by the
same verdict script from the machine's own records (`disks_unchanged`
there compares partition tables only). Residue: a vendor's firmware
starting the stick again after the install, and the sign-in screen on
real graphics.

**Where it stands.** `[####]` held-on-machine, one vendor. The Aspire, kit
aba09ec, Fedora 44, KDE, Secure Boot on, the stick left in throughout:
`v9-erase.csv` line 30, `erased-installed`, and line 31,
`not-installed-again` (2026-10-04, run 11, the first clean conversion);
line 32, `cancelled-untouched` (2026-10-06). Runs 9 and 10 are `[FAIL]`
rows that stay (lines 11 and 22, `fail`: a text login; a second install,
R35). The rig's latest is line 36 (`refused-before-countdown`,
2026-10-09). Owed: other vendors' firmware, and the old-entry button on
real firmware (nothing was left for it to remove)
([log](log/2026-10.md#2026-10-04-the-aspires-run-11-passed-the-first-clean-conversion-v9-v10-r27-r35)).

## V10: settle-in runs on any Linux · RISKS R28 · held-on-machine

**Why it has its own gate.** `settle-in` is one self-contained program,
fed by a per-distribution installer adapter through one handoff folder
(decided 2026-09-27, the owner). It is the first thing the person sees on
the new system, and it must do the same on every desktop and every
distribution it claims.

**What passes.** The same file, byte for byte, on Fedora KDE, Fedora GNOME
and the text console, and at least one non-Fedora distribution: (1) the
program starts at first boot, before the network; (2) its window appears
(or its text screen, when the console was chosen), logged as shown at the
first sign-in and not at the second; (3) the clock, Wi-Fi and boot-entry
steps give the same results: the clock corrected only when the installer's
clock disagrees with a synchronized one (never in a virtual machine),
every network from the job loaded by NetworkManager field by field and the
passwords' copy deleted, the conversion's own "upgrade_" entry removed by
its recorded id, the stale Windows Boot Manager removed by the button
through the desktop's password prompt; (4) a distribution missing a floor
piece (for example NetworkManager) gets a plain refusal, not a half setup.
SSH is carried only if Windows had it on, public keys only: the key signs
in, a password is refused, sshd is enabled, `~/.ssh` is 700 and
`authorized_keys` 600 (decided 2026-10-04, the owner). Rows:
`settle-in-first-start.csv`, `pass` or `pass-plumbing`, written by
`rig/hyperv/settle-in-verdict.py`, never by hand; a `fail` stays.
Vocabulary: `validation-results/README.md`.

**How it is run.** On the rig, the erase arm of V9 with three made-up
Wi-Fi networks planted in the job (`v9-job.py --spoof-wifi`) and a spoofed
"Windows had it on" SSH key; the rig's boot marker captures the sign-ins,
and the window writes its first drawn frame and its close to the system
log. A non-Fedora distribution is installed by hand with a hand-placed
handoff folder. Then a real machine, its evidence gathered over the
carried SSH. Residue the rig cannot close: Hyper-V's hardware clock
already holds UTC, so the correction itself is proven only on bare metal;
real Wi-Fi; a real firmware's boot entries; a non-Fedora distribution.

**Where it stands.** `[####]` held-on-machine, one machine, Fedora only.
The Aspire's run 11: the clock corrected, Wi-Fi connected by itself, SSH
carried, KDE (`settle-in-first-start.csv` line 20, `pass`, 2026-10-04;
lines 18 and 19 are run 10's `fail`, R35). The rig holds KDE, GNOME and
the console (latest line 17, `pass`, 2026-10-04). Owed: a non-Fedora
distribution, the old-entry button on real firmware, the file pull (not
built)
([log](log/2026-10.md#2026-10-04-the-aspires-run-11-passed-the-first-clean-conversion-v9-v10-r27-r35)).

## V11: The way back to Windows · RISKS R30 · held-on-machine

**Why it has its own gate.** There is always a way back to Windows, a new
and empty one, and it says its cost first (decided 2026-09-27, the owner;
`architecture.md`, "The way back to Windows"). It is an erase, so rule #4
applies, and it is 100% managed (decided 2026-09-29, the owner; R33).

**What passes.** An activated Windows put back on at least two real
machines with different licence kinds (a firmware key; a digital licence
without one), each with its harvest from before and after, and a machine
that cannot run Windows 11 shown the Windows 10 path and its warning. Four
steps, each on the rig before a real machine:

1. the harvest (`harvest.windows_license`): the self-test feeds it made-up
   licence facts and proves a key handed to it never reaches `job.json`;
2. the Aspire's hand reinstall: only an "after", since the licence before
   the erase was never recorded (R30);
3. the guided stick (`settle-in go-back`): Microsoft's installer
   downloaded, checked against its published hash, written to the named
   stick and no other drive, started with Secure Boot on, installed, the
   activation state read on the new Windows (`v11-way-back.csv`);
4. the walk-away reinstall (R33), four arms as V9 has, in
   `v11-walkaway.csv`: refuse before any countdown, both disks unchanged,
   Linux starting again; cancel on a key, untouched
   (`cancelled-untouched`); erase with nobody at the keyboard from the
   restart to the Windows sign-in, both disks rewritten, the gate's record
   on the stick (`windows-reached`); the stick left in never counts down
   again. The erase arm also passes only when the new Windows shows the
   right time in the right zone before any network, is on the network the
   Linux side knew at its first start with no Wi-Fi password left on the
   stick (added 2026-10-02, the owner), and, where Linux had SSH on, its
   key signs in and a password does not (added 2026-10-04, the owner).
   Anything less is `fail`, every reason listed.

Vocabulary: `validation-results/README.md`.

**How it is run.** The rig: `settle-in go-back write --image` writes a
file, never a disk, and `rig/hyperv/v11-stick.ps1` boots it in a new
Generation 2 VM, Secure Boot on, offline, with an empty 64 GB target; the
walk-away arms start from a Fedora the V9 rig installed, two disks,
spoofed networks. The Aspire: a real stick, a physical cancel first, then
the erase, the rows written by `rig/hyperv/v11w-verdict.py` from the
records copied off the stick. Residue: activation on a real licence, a
real network at first start, a real firmware's USB entry.

**Where it stands.** `[####]` held-on-machine, one machine, one licence
kind. The Aspire: `v11-walkaway.csv` line 2, `fail` (2026-10-03: Windows
11 reached and activated by its digital licence, four defects, all fixed
that day); line 3, `windows-reached` (2026-10-04); line 4,
`cancelled-untouched` (2026-10-06); line 5, `windows-reached` (2026-10-11,
driven over ssh, a second stick). The guided stick: `v11-way-back.csv`
line 3, `pass-plumbing` (rig, 2026-09-27). Owed: a second machine with a
firmware key, the Windows 10 path on a machine that cannot run Windows 11,
the licence harvest as a row ([log](log/2026-10.md#2026-10-11-the-way-back-run-3-driven-over-ssh-r33-v11)).

# Tier 3: silent data loss (the trust-ending class)

## V8: OneDrive placeholders are materialized at evaluate · kills: file integrity on the default path · RISKS R8 · held-on-rig

**Why it has its own gate.** Files are pulled from the Windows partition
*by Linux*, which has no OneDrive client. OneDrive's "free up space" leaves
a **placeholder** on disk: a stub that looks like the file, with the real
bytes in the cloud. A stub copied as if it were the file arrives as 0
bytes, and the person finds out later. This is the trust-ending class.

**Decided (2026-09-26, the owner): no download.** Online-only files stay
in OneDrive, where their bytes already are. The job records them
(`cloud_files.result = left-in-cloud`) and `settle-in` reconnects OneDrive.
The danger left is copying a stub as if it were the file, and that is
`settle-in`'s to refuse (R8). The materializer stays built, unused by the
launchers.

**What passes.** No cloud-only stub survives into the pulled data as a
0-byte file. For the materializer: a `v8-materialize.csv` row reads
`pass-plumbing` when every placeholder was detected, pinned, read through,
and the NTFS bytes afterwards hash to the known content, with a refusal on
any failure; the pinned and unpinned attribute bits (`0x00080000` /
`0x00100000`) are not part of detection. For `settle-in` (the path now in
use): it refuses to copy a stub as if it were the file (R8); that check is
not yet written. Vocabulary: `validation-results/README.md`.

**How it is run.** `evaluate/windows/Test-Materialize.ps1` is a real sync
provider on Windows' Cloud Files API (cfapi, the interface OneDrive itself
uses): it creates dehydrated placeholders with known bytes, refuses to
serve one, runs the harvester's `-Materialize` seam in a separate process,
and hashes the NTFS bytes afterwards. It runs on the rig and on any Windows
machine. Residue: `-OneDrive` against a signed-in client, and `settle-in`'s
refusal against a real OneDrive folder; neither has run.

**If it fails.** `evaluate` refuses machines with placeholders it cannot
handle, rather than silently copying empties. Refuse by default: better to
turn someone away than to lose their photos.

**Where it stands.** `[###.]` held-on-rig. The cfapi provider:
`v8-materialize.csv` lines 4 and 5, `pass-plumbing` (2026-09-08, the rig
on Windows 10 19045 and the G16 on Windows 11 26200; lines 2 and 3,
`setup-failed`, stay). The G16 row is the same synthetic provider, so it
is plumbing, not a real-hardware pass. Owed: a signed-in OneDrive, and
`settle-in`'s refusal, which is not built
([log](log/2026-09.md#2026-09-08-the-cfapi-leg-fired-v8-r8)).

# Tier 4: kills adoption, not the mechanism

## V5: VMD detection fires on real RST hardware · kills: scanner trust · RISKS R1 · held-on-machine

**Why it has its own gate.** Intel RST / VMD is a storage setting on many
Intel laptops; with it on, the Linux installer cannot see the SSD at all.
The scanner's check for it is the flagship check and the report's
highest-stakes line, and the project's only asset is that the report can
be trusted.

**What passes.** Both directions on at least one physical machine, with
the ID list reconciled against the kernel's `drivers/pci/controller/vmd.c`.
Rows in `v5-controller-mode.csv`, written by `rig/v5-verdict.py` from a
run's JSON report and capture, never by hand; the `result` column
cross-checks the mode asked for against the PCI class code the controller
declared. RST or VMD on must give `[FAIL] Storage controller mode`,
verdict RED; AHCI must give `[OK]`, or `warn-rst-on-ahci` when Intel's RST
driver is still bound on an AHCI-class controller (the R7 guard). The
check has three signals: the kernel ID list, the `iaStorVD` service, and
PCI RAID class code `CC_0104` from CompatibleID. Vocabulary:
`validation-results/README.md`.

**How it is run.** Desk: the ID list against `vmd.c` and
linux-hardware.org probes. Logic: six detection-level self-test cases feed
made-up PnP entries through the real check. Spoof: a patched QEMU `pci-testdev`
(`rig/vm/`) presents PCI `8086:9a0b`, class `0104`, and the capture
`evaluate/windows/corpus/vm-qemu-q35-vmd-spoof-9a0b.json` is the
regression; it closes the plumbing (listing, parsing, verdict), never the
clause. Real machine: `RUN-STORAGE-MODE.cmd`
(`evaluate/windows/Test-StorageMode.ps1`) scans, arms a Safe Mode boot
through a copied boot entry booted once, restarts into the firmware setup
for the person to change SATA Mode, scans again as SYSTEM with nobody
signed in, asks for the mode back, scans a third time and cleans up; the
rig runs it as `rig/hyperv/prologue.sh storage-mode`. Residue: the Aspire
can give the pre-VMD RST clause (class `0104`, `8086:282a`) if its setup
exposes SATA Mode; VMD proper (signals 1 and 2) takes an 11th-gen or newer
Intel laptop with RST on, in V0's half-hour visit. The G16 (AMD, standard
NVMe) cannot exercise the positive path, and neither can the spoof.

**If it fails.** Fix the list and run it again. This one has no fallback
because it has no excuse: it is cheap.

**Where it stands.** `[####]` held-on-machine, the negative direction
only. The Aspire in AHCI mode, `8086:9d03`, class `0106`, iaStorAC bound:
`v5-controller-mode.csv` line 2, `warn-rst-on-ahci` (2026-09-13). The
one-click visit fired twice on its firmware (lines 7 to 10, 2026-09-15:
`warn-rst-on-ahci` and `mode-mismatch`) but never reached the setup
screen, because InsydeH2O V1.21 ignores boot-to-setup. The rig's rows are
lines 3 to 6, `no-intel-controller`. Owed: the RST or VMD row on real
hardware
([log](log/2026-09.md#2026-09-15-the-storage-mode-visit-on-the-aspire-rows-6-to-9-v5-r1)).

## V6: A signed binary can earn Defender's tolerance · kills: distribution · RISKS R12 · planned

**Why it has its own gate.** Not a code question, a calendar question.
Microsoft's SmartScreen (the "Windows protected your PC" screen) builds
its trust in a signed program over elapsed time, so this validation *is*
the fix, and it has to start early.

**What passes.** The signed test binary downloads and runs on a stock
machine without SmartScreen stepping in.

**How it is run.** A legal entity and an OV (organization-validated)
certificate; sign something trivial (the scanner wrapped in an exe);
submit it to Microsoft's malware-analysis portal; distribute it modestly
and measure SmartScreen's behaviour monthly. By the time the converter
ships we know whether signed, submitted and aged is enough. Nothing here
can be spoofed.

**If it fails.** An EV (extended validation) certificate, Microsoft Store
packaging, or distribution through repair-event channels, with humans who
can click past warnings. All slower, all workable, all better known a year
early.

**Where it stands.** `[#...]` planned. The clock has not started: no
entity, no certificate, no row, no log entry.

## V7: The scanner generalizes beyond one laptop · kills: the knowledge-base model · RISKS R2 · planned

**Why it has its own gate.** Every check was verified on one ASUS G16, and
since then one Acer Aspire. The community-table model only works if
reports from unfamiliar machines mostly confirm the tables.

**What passes.** Reports arrive and the verdicts survive contact, or the
failures are table gaps (one-line fixes) rather than logic failures.

**How it is run.** Ship the scanner publicly and ask for reports: at least
one Intel laptop, one Broadcom machine, one pre-2015 machine, one
BitLocker-on machine, one Surface. Every report that comes with a
`-DumpMachine` capture joins `evaluate/windows/corpus/` and is replayed on
every self-test run after that. This also feeds V4 for free.

**Where it stands.** `[#...]` planned. The scanner has not shipped
publicly; the corpus holds the G16, the Aspire and the rig's VMD spoof.
No row, no log entry.

## V12: The window in front of the scripts · kills: the non-technical front door · RISKS R31 · held-on-rig

**Why it has its own gate.** `UPGRADE.exe` is a Rust window that runs the
kit's flows from the stick and opens again after the restart (decided
2026-09-27, the owner). It is the only thing a non-technical person sees,
so a blank screen or a silent exit is a failed conversion before it
starts.

**What passes.** Three steps, in order. (1) The rig: the verify flow from
the window on the Hyper-V guest: scan, job, kickstart, arm, the live boot,
the return, the window opening at the next sign-in and showing the
result; plus the stops: a kit with a file missing, a RED scan (a recorded
RED machine or an injected one), closing the window before the arm. (2)
The Aspire: the same flow on real firmware, as a physical row. (3) A
machine on the basic display driver (or a VM without 3D): the window
cannot draw, and the message box points to the `.cmd`. Steps 1 and 2 pass
when the window shows the same result the return check recorded, with
nothing armed or registered left behind (no task, no state, no
`bootsequence`); step 3 shows the fallback, not a blank screen or silence.
Rows: `v12-window.csv`; `stopped-before-arm` is a right stop at a refusal,
not a pass. Vocabulary: `validation-results/README.md`.

**How it is run.** `rig/hyperv/v12.sh` drives the window on the guest (it
brings the window to the front, since a window started by a task does not
get the keyboard focus, and reads only the window's own log lines); the
flow logic has its tests on Linux (every call matches the launcher
argument for argument); `--version` and `--preview` run on the G16 without
touching anything. Residue: the rig has no USB, so the job writer rightly
refuses the rig's SCSI stick (R16) and the rig can never take the window
past the job writer; the restart, the reopen at sign-in and the result
screen need the Aspire.

**Where it stands.** `[###.]` held-on-rig, the stop path only. The window
drew via wgpu on the guest (OpenGL 1.1 only), ran the scan, showed the job
writer's refusal in its words and left nothing behind: `v12-window.csv`
line 2, `stopped-before-arm` (2026-09-28, window 0.1.1), and line 3, the
same over the Rust programs (2026-10-08). The Aspire's run of 2026-10-10
started from `UPGRADE.exe --accepting-data-loss` (`r18-prologue.csv` line
16's harness column), but no `v12-window.csv` row records the reopen or
the result screen. Owed: steps 2 and 3
([log](log/2026-09.md#2026-09-27-the-windows-rig-run-1-v12-r31-r16)).

## V14: The kit's Linux release starts where it is used · kills: the one click, and after an erase, the machine · RISKS R34 · held-on-machine

**Why it has its own gate.** Windows 11 raised the Aspire's firmware SBAT
level (Secure Boot's list of boot programs too old to trust) and the kit's
Fedora 42 GRUB was refused. A release that cannot start is a stick that
does nothing, or after an erase a machine that boots nothing. **Decided
(2026-10-03, the owner):** the release is chosen from what the machine
accepts (`architecture.md`, "It chooses the Linux release").

**What passes.** Six steps. (1) The scanner reads the firmware's SBAT
level and judges the stick's boot files. (2) Each release's boot facts in
`data/releases.ps1` are measured from its files by
`data/tools/measure-release.py`, never typed. (3) The scanner judges every
release, including the Secure Boot db's authorities, and the job writer
refuses one that cannot start. (4) `make-kit.sh --release <name>` builds a
kit whose every file is checked against the table. (5) The rig: V9's arm C
and a settle-in run on that kit. (6) A real machine: the scan says the
release starts, the stick boots, the installed system boots, Secure Boot
on. The gate passes when a machine that refuses one release is given one
it accepts and reaches its desktop with Secure Boot on. The rows are V9's
and V10's; the scanner's judgement has its self-tests.

**How it is run.** Steps 1 to 4 are self-tests and a build
(`port-check.sh` checks the recorded tables). Step 5 is `rig/hyperv/v9.sh`
on the Fedora 44 kit (Secure Boot off on the main guest, since Hyper-V
cannot trust both authorities; the chain and the installed system also
booted with Secure Boot on under the third-party CA on a second VM). Step
6 is the Aspire over SSH (the scan) and its V9 run. Residue only a real
machine closes: what a firmware actually holds (the level and the db) and
whether it refuses exactly as shim's rule says; the rig's firmware has its
own level and keys, so a pass there is plumbing.

**Where it stands.** `[####]` held-on-machine, one firmware. The Aspire's
scan over SSH said Fedora 42's files FAIL (`grub,3 < grub,5`) and Fedora
44's OK (2026-10-03); the Fedora 44 kit booted and installed with Secure
Boot on in run 10 (`v9-erase.csv` line 22, a `fail` row for R35, not for
this) and run 11 (line 30, `erased-installed`, 2026-10-04). Fedora 44 is
measured and committed (47bc8cf); Fedora 42's record is owed. Owed: a
second firmware, and a machine that refuses the kit's release on the day
([log](log/2026-10.md#2026-10-03-the-kits-release-chosen-from-what-the-machine-accepts-step-by-step-v14-r34)).

## V13: The port from PowerShell to Rust · kills: trust in every row the Windows side earned · RISKS R32 · held-on-machine

**Why it has its own gate.** Rust becomes the conversion's one language,
piece by piece (decided 2026-09-27, the owner; `architecture.md`, "Stack";
the roadmap is `docs/RUST-PORT.md`). Every row the PowerShell earned says
nothing about the Rust: a line is never marked `pass` because its
PowerShell row passed.

**What passes.** Per piece: every line of the parity ledger
`validation-results/port-parity.csv` reads `pass`, and the release is
rebuilt byte for byte by a second person from the tagged source (R14,
`r14-rebuild.csv`). A piece's lines are written in full *before* its port
starts, so the ledger says what must be matched, not what happened to be
tested; a PowerShell piece is retired only when all its lines read `pass`.

| Column | Meaning |
|---|---|
| `piece` | the PowerShell file being ported, e.g. `upgrade-scan.ps1` |
| `evidence` | what it passed: a self-test case name, a corpus file, or a results file and line (`r18-prologue.csv:6`) |
| `kind` | `selftest`, `corpus`, `rig` or `physical` |
| `rust_test` | the `cargo test` name, or the harness and row that re-earned it |
| `result` | `owed`, `pass` or `fail` |
| `date` | when `result` last changed |

**How it is run.** By kind. `selftest` and `corpus`: the case ported as a
`cargo test`, fed the same made-up object or the same recording, same
verdict, same fields. Differential: while both exist, PowerShell and Rust
run on the same input (the corpus, a rig guest, a captured tool output, or
a real machine in the same minute: `v13-rust-scanner.csv`,
`v13-rust-job.csv`, `v13-rust-prologue.csv`) and must agree, word for word
where it is a refusal. `rig`: the same `rig/hyperv/*.sh` harness with the
Rust build (`PROLOGUE=rust rig/hyperv/prologue.sh run`), appending its row
as today. `physical`: where the run kept the raw output of the tools it
called, a replay test closes the decision; the effect on the firmware or
the disk still takes one re-run on the machine. `./port-check.sh` runs the
whole check: both PowerShell self-tests, `schemas/check.py`, the recorded
files fresh, every self-test case with a Rust case under the same name,
and the Rust tests. **Decided (2026-10-04, the owner):** the whole port is
built on the branch `rust-port`, and the project cuts over to Rust after
one success of the PowerShell process on `main`; the order is every
`selftest`, `corpus` and differential line `pass` first, then the `rig`
lines re-run with the Rust build, then the `physical` ones.

**Where it stands.** `[####]` held-on-machine: 657 ledger lines `pass`,
45 `owed`, none `fail` (counted 2026-10-11). The Rust scanner's reads and
report matched the PowerShell's on the G16 and the Aspire in the same
minute (`v13-rust-scanner.csv` lines 4 and 5, `SAME`); the Rust prologue
ran the Aspire's keep-Windows conversions (`v2-install.csv` lines 11 and
15; `r18-prologue.csv` line 18, the whole path on 2026-10-11, update
restart, resume, shrink, handoff, return) and its resume
(`walkaway-probe.csv` line 5). Fresh-clone rebuilds match
(`r14-rebuild.csv` lines 22 to 26, `same` = `y`). Owed: the 45 lines (the
rig's erase arms and the Aspire's rows for each piece), and the
second-person rebuild ([log](log/2026-10.md#2026-10-11-the-detector-fires-on-the-aspire-r36-the-product-way-r21-r22-r25-r36-v1b)).

# Lesser gates (validate when their component is built)

Real, but they degrade rather than kill, or only touch the fallback path:

- **Counterfeit stick is caught** (R17): buy a known-fake stick, and confirm
  the read-back check fails it before the commit line. *Fallback path only
  now: data rides the stick only on clean slate.*
- **Browser profile transplant matrix** (R20): real Windows→Linux moves per
  browser and version, before `evaluate` promises anything.
- **Both desktops fit the stick:** build the dual-squashfs image, weigh it,
  and set the minimum stick size from the number, not a guess.
- **Windows reinstall fallback is real:** check the digital-licence
  reactivation claim once, on the G16, so the clean-slate consent screen
  tells the truth.
