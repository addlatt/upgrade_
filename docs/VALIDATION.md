# Validation gates

This is the plan for proving things before we build more on top of them.
`RISKS.md` lists what might be wrong. This file is the ordered plan for
finding out. The same rule applies to both: nothing here closes because the
argument sounds good. Only evidence closes it.

A **gate** is one question that has to be answered "yes" before the work
that depends on it goes ahead. Gates are ordered by what dies if the answer
is no, weighted by how little evidence exists today. Each gate states three
things:

- **the experiment** (what we run),
- **the pass criterion** (what counts as yes),
- **the fallback** (what we build instead if it fails), because "we'll deal
  with it" is not a plan.

Gates are grouped into the same **tiers** used in `CLAUDE.md` and
`RISKS.md`, so the three agree:

- **Tier 1, no product if these fail:** V0, V1, V1b
- **Tier 2, a core promise breaks (recoverable):** V4, V3, V2, and V9
  (the one-click erase and install, the current front since 2026-09-26)
- **Tier 3, silent data loss:** V8
- **Tier 4, kills adoption, not the mechanism:** V5, V6, V7

The V-numbers are fixed names, not a priority order. Read the tier, not the
number. **The meta-rule: nothing gets built on top of a gate it depends on
until that gate is proven.** The dependency map is at the bottom.

**Method note (2026-08-22): spoof everything spoofable.** Each gate splits
in two:

1. **The part we can test without the real thing.** Detection logic fed
   made-up objects, replays of recorded real machines (`-DumpMachine` →
   `evaluate/windows/corpus/`), and fake devices inside virtual machines.
   This part gets automated tests that run on every `-SelfTest`.
2. **The residue.** What only a real machine or a primary source can
   settle. The residue is what a gate's **Pass** line means.

A green simulation narrows a gate. It never closes one, because the
simulation is built from our own model of the hardware, and that model is
exactly what the gate is there to question. CLAUDE.md rule #5 has the full
statement.

A few words used all through this file:

- **The rig** is our test bench of virtual machines: QEMU with OVMF (a free
  UEFI firmware) under `rig/vm/`, and Hyper-V under `rig/hyperv/`.
- **`pass-plumbing`** means the pipes all connect on the rig. It is not a
  pass on real hardware.
- **Row N** of a CSV in `docs/validation-results/` is one recorded run. Rows
  are written by scripts, never by hand.
- **SB** is Secure Boot, the firmware feature that only starts signed boot
  code.

### Where each gate stands

```text
[####]  real machine   [###.]  rig   [##..]  built, untried
[#...]  planned        [....]  not started   [FAIL]  failed for real
```

| Gate | State | In one line |
|---|---|---|
| V0 boot handoff | `[####]` 1 vendor | Acer fired once, SB on. Dell, Lenovo, HP and the real-firmware fail-safe rows owed |
| V1 unattended install | live boot `[####]`, install `[###.]` | physical live boot SB on (row 4); the physical install is owed |
| V1b alongside install | `[###.]` | rig passes, SB off; physical SB-on install needs a machine other than the Aspire |
| V2 amp firmware | `[....]` | experiment on the G16 not run |
| V3 BitLocker read | `[###.]` | all three configs byte-identical via ntfs-3g; real disks owed |
| V4 disks shrink | `[####]` 1 disk | the Aspire's answer is no (best 9.5 of 25 GB); the population count needs ~20 elevated reports |
| V9 erase and install | rig `[###.]`, real `[####]` 2026-10-04 (run 11: `erased-installed` with the stick left in, then `not-installed-again`); runs 9 and 10 `[FAIL]`, fixed | run 9 came up at a text login; run 10 installed twice (R35). Run 11, kit aba09ec, is the first clean conversion (R27). The clean physical cancel row is still owed (Part A was skipped in run 11; two rig key presses went unread on 2026-09-29, cause not found) |
| V11 way back to Windows | harvest `[##..]`, guided stick `[###.]`, walk-away rig `[###.]`, real `[FAIL]` (2026-10-03: the Aspire reached Windows 11, activated; fail on four defects, `v11-walkaway.csv` line 2; R33) | the four fixed 2026-10-03 (start through the firmware's USB entry, password forced at the first sign-in, Wi-Fi carried, progress shown), the password and the first-sign-in hook proven on the rig; owed on the Aspire: Wi-Fi online at first start, the program's own start, a real cancel |
| V8 OneDrive placeholders | `[###.]` | cfapi provider `pass-plumbing` on the rig and the G16; signed-in OneDrive owed |
| V5 VMD detection | plumbing `[###.]`, AHCI row `[####]` | the RST/VMD row on real hardware is owed |
| V6 code signing | `[....]` | calendar-bound, not started |
| V7 scanner generalizes | `[....]` | needs the public release and reports |
| V12 the window | verify flow: stop path `[###.]`, restart and reopen `[##..]` | rig line 2: draws via wgpu, shows the job writer's refusal, leaves nothing; the rig has no USB, so the reopen after the restart needs the Aspire |
| V13 the Rust port | steps 1 and 2 `[##..]`, steps 3 to 5 `[#...]` | 2026-10-04: the schema library and the scanner's judging half are built; 406 ledger lines `pass`, 20 `owed` (rig and physical rows); nothing switched over |

---

# Tier 1: no product if these fail

## V0: The boot handoff fires · kills: walk-away itself · RISKS R15

**What this gate is about.** The whole walk-away promise rests on one
command: `bcdedit /set {fwbootmgr} bootsequence`. `bcdedit` is Windows' tool
for editing its boot settings. That command asks the firmware to boot the
USB stick exactly once, on the next restart only. It has to work on firmware
from vendors who have never heard of us. Evidence before 2026-09-08: zero
machines.

**VM leg fired (2026-08-23).** On the QEMU+OVMF rig (`rig/vm/`), the
baseline SB-off run records `fired-once`: one-time boot, the payload ran,
Windows came back with no keypress, and the one-shot entry cleared itself.
Two bugs were fixed on the way, and both would have hit the physical test
too (see RISKS R15):

- the payload's `fs0:` marker went to the wrong place;
- the harness counted its own test entry as a reorder.

**Hyper-V Gen 2 leg fired (2026-08-30).** On `rig/hyperv/` (real Secure
Boot, a virtual TPM, the stick as a SCSI VHDX):

- **Row 3:** Secure Boot refused the unsigned payload. Result `ignored`,
  and the fail-safe held.
- **Row 5:** BitLocker on, suspension armed, `fired-once`, no recovery
  prompt.
- **Row 6:** `NoSuspend`, `fired-once` with **no recovery prompt**. The
  finding we registered in advance is recorded word for word: the one-shot
  resets before Windows boots, so the PCRs (the TPM's boot measurements
  that BitLocker's key is sealed against) are unchanged at unseal time. See
  R15.
- **Row 4** means nothing on Hyper-V. Its Windows-only Secure Boot database
  refuses shim (the small signed loader Linux uses to boot under Secure
  Boot) at the firmware.

Remaining after that: the physical vendor matrix. That covers the
removable-USB clause no VM has, and whether any vendor's firmware measures
the one-shot into a sealed PCR. A VM pass narrows V0. It does not close it.

**PHYSICAL LEG OPENED: the first real machine fired (2026-09-08).** Acer
Aspire A515-51G, firmware V1.21, **Secure Boot on**, BitLocker off, Windows
11 Home 22631.

- The signed shim payload booted once and recorded itself into `grubenv`.
- The one-shot cleared itself and the boot order was intact.
- Windows came back **with no keypress**: `fired-once`, `keypress_free=y`.
- It ran through the one-click `-Auto` flow end to end. `RUN-TEST.cmd`
  needed one double-click and one UAC consent, then the machine did the
  rest, including the return check that classified the result and cleaned
  up after the reboot.

The row is in `v0-handoff.csv`, `mode=auto`. This is the project's **first
evidence from physical hardware** on any gate, and it reaches the
removable-USB clause no VM could. It does not close V0. One vendor is not a
matrix, and the fail-safe rows (`NoFile`, `SecureBootUnsigned`) have still
never run on real firmware.

**Experiment.** Week one in a VM: OVMF UEFI, a Windows guest, a stick image
attached as USB, the four bcdedit commands, reboot. Then the physical
matrix: the ASUS G16 plus borrowed machines from at least three other
vendors (Dell, Lenovo and HP are the population). For each machine:

- Does the stick boot exactly once?
- After a deliberate failure in the live environment, does the machine boot
  Windows normally with no user action?
- **From 2026-09-13, the walk-away resume** (RISKS R24): does the SYSTEM
  startup task fire with nobody signed in, and see the stick?
  `RUN-PROBE.cmd` does this read-only with one restart, and writes one
  `walkaway-probe.csv` row per vendor, in the same half-hour visit.
- **From 2026-09-13, the storage-mode visit** (V5): `RUN-STORAGE-MODE.cmd`
  on any Intel machine whose setup has a SATA Mode option. One click, the
  setup screen twice, a Safe Mode sign-in twice, and it fills that vendor's
  `v5-controller-mode.csv` rows in the same visit.

**Pass.** It fires on all tested firmware, or fails *safe* (Windows boots)
on the ones where it doesn't. The failure must be detectable, so the tool
can say so instead of silently doing nothing.

**If it fails.** The fallback is one manual step: "when it restarts, press
the key we show you." Walk-away drops from perfect to press-one-key. We
survive that, but the interface, the docs and the marketing all change.
That is why this is V0.

**How to run.** The harness is `upgrade_/windows/Test-Handoff.ps1` (arm,
reboot, check, with `-FailMode` for the deliberate-failure paths). Build the
stick per `upgrade_/windows/handoff-payload/README.md`. Evidence lands in
`docs/validation-results/v0-handoff.csv`.

**Physical leg: readiness decided (2026-09-07).** V0 is the one gate ready
for a live run on a real machine:

- The mechanism can be undone: one BCD entry (BCD is Windows' boot
  configuration store), exported first, and removed by `-Check` whatever
  the outcome.
- No disk is written.
- The harness has fired on two VM firmwares (six rows, including the
  fail-safe and BitLocker rows).

What goes on the stick is built and checked by `./make-kit.sh`
(`dist/kit/stick-shell`, `dist/kit/stick-shim`; the run-book is
`README-STICK.txt` on the stick).

Harness 0.2.0 owed three things before a physical machine, and has them:

1. It reads BitLocker state through `manage-bde` when the PowerShell module
   is missing (Home editions).
2. It refuses to arm on an unknown state, or on BitLocker on without
   suspension.
3. The shim payload records itself through `grubenv`, so the
   Secure-Boot-on row is classified by the harness, not by someone watching.

Harness **0.3.0 adds the one-click (fully managed) flow**, `RUN-TEST.cmd`:
one double-click, one UAC "Yes", then `-Arm -Auto` registers an elevated
logon task. After the reboot that task runs the return check itself,
classifies, cleans up, asks one popup question (it times out to `unknown`)
and writes the row to the stick. The 0.2.0 baseline, the self-recording
shim payload and the 0.3.0 auto flow all fired on the QEMU rig (three
`fired-once` rows, SB off, plumbing only).

A physical row is copied word for word from the stick's CSV, and every
physical run leaves a `-DumpMachine` capture for `evaluate/windows/corpus/`.
First target: the Acer Aspire A515-51G.

## V1: Unattended install completes, Secure Boot on · kills: the conversion

The second half of the spine, and the base both paths share. A
custom-composed live stick (Fedora's signed shim, GRUB and kernel,
untouched) boots with Secure Boot on. Then a **kickstart** (Fedora's
answer file, which tells the installer what to do) drives **Anaconda**
(Fedora's installer) to a login screen with zero human input.

**Experiment.** The same spike as V0: they are one build-order item. A VM
with Secure Boot enforcing, then the physical matrix. This gate covers the
base install to a login screen. The concerns specific to installing next to
Windows are V1b.

**Reversible half fired on the rig (2026-09-08).** The vertical's first
live boot: `rig/hyperv/v1.sh run`, row 2 of
`docs/validation-results/v1-live-boot.csv`, `pass-plumbing`. Step by step:

1. The V0 harness armed the one-shot exactly as `RUN-TEST.cmd` does.
2. The stick (the kit plus `upgrade_/{job.json,ks.cfg,boot-verify}`) booted
   shim → GRUB. GRUB recorded the marker and started the **unmodified**
   Fedora 42 installer kernel and stage2 from the stick, with `inst.ks=`
   and `upg.mode=verify`.
3. `%pre` (the kickstart's script that runs before anything is installed)
   ran `upgrade_/linux/verify.sh`. It matched the job's disk by unique id
   (`scsi-3600224802f9d…` → `/dev/sda`) and exact size, saw the display
   driving 1024×768, and found the Windows ESP on `sda1`. (The **ESP** is
   the small EFI System Partition that holds every OS's boot files.)
4. It wrote the keep-windows storage `%include`, put `verify.json` on the
   stick and rebooted.
5. Windows came back on its own, and the return check wrote its row.

Nothing was installed and the internal disk was untouched. Row 1 is the same
run's first attempt, `verify-incomplete` (a bug in the report writer,
fixed).

Secure Boot was **off** (Hyper-V's template refuses shim, the usual rig
clause). The chain is the simpler shape named under "If it fails" below,
not a composed image. So the "custom-composed live stick" half of this gate
is now the *desktop squashfs* (a compressed read-only disk image of the
desktop) that the kickstart's `liveimg` points at. It has been **on the
stick since 2026-09-09** (row 3, `pass-plumbing`):

- Fedora's own Workstation and KDE live `squashfs.img`, unmodified;
- checked against Fedora's published ISO hashes when fetched
  (`rig/vm/fetch-desktops.sh`);
- shipped by `make-kit.sh` (the kit is 5.3 GB);
- named with `--checksum=` in the kickstart;
- read back byte for byte against the stick's manifest by `%pre` in the
  live session, before anything is decided.

What this gate still lacks is the install itself (the destructive half),
and Secure Boot on.

**First physical row, Secure Boot ON (2026-09-12).** Acer Aspire A515-51G
(InsydeH2O V1.21, now Windows 11 Home 22631), the one-click `RUN-VERIFY.cmd`
from the stick: scanner → job writer (`New-Job.ps1`) → kickstart →
handoff. Row 4 of `v1-live-boot.csv`, `pass-plumbing` with
`secureboot=on`:

- `fired-once`, no keypress.
- The unmodified Fedora installer booted through shim, with the firmware's
  Secure Boot variable reading 1 inside the live session.
- Identity matched **by serial**. Windows reports an ATA disk's unique id
  as padded model+serial text, not hex. The verifier's hex match correctly
  found nothing, and the serial vote found `ata-HFS256G39TND-N210A_…`, with
  the size exact. This machine also has a second 1 TB disk with an Ubuntu
  install, which the check had to not pick.
- Display 1920×1080 on HDMI.
- **Wi-Fi checked for the first time:** `wlp3s0` scanned 28 networks.
- The 2.6 GB KDE image read back byte-identical at **22.6 MB/s**. That is
  the honest number for this stick's time estimate.
- Windows came back by itself.

The job writer forced **clean-slate**. This C: still carries the NTFS dirty
flag (RISKS R18, unchanged since 2026-09-08), so shrink cannot be measured
and keep-Windows cannot be offered. The ESP (100 MiB, 46 MB free) and disk
health would have allowed it. Nothing installed.

**Pass.** Hands-off from power-on to login, Secure Boot still on.

**If it fails.** A known simpler shape exists: ship the *unmodified* Fedora
ISO on one partition, and the kickstart on a second volume labelled
`OEMDRV`, which Anaconda picks up by itself. Less control, much less image
work, the same signed chain. If custom composition fights us, fall back to
that rather than fighting. (2026-09-08: this is the shape the vertical uses,
with `inst.ks=` on the stick itself instead of an OEMDRV volume.)

## V1b: Installing alongside a shrunk Windows leaves Windows bootable · kills: the default path's safety net · RISKS R21

**Why it has its own gate.** The default keep-Windows path installs Linux
into space freed from Windows, and **must leave the shrunk Windows fully
bootable**. Windows is both the way back and the source of the person's
files. This is harder than the wipe install, and since the redesign it is
the common case, not a "variant" of V1. It earns its own Tier 1 gate.

**The converter's own install ran it (2026-09-10).** Not a bench kickstart
this time, but the product's own:

- `New-Kickstart.ps1` from a schema-valid job;
- `%pre` `verify.sh`: identity, image read-back, a snapshot of the ESP to
  the stick, the storage include;
- `liveimg` from the stick's KDE squashfs into the space behind the shrunk
  C:, reusing the 100 MiB Windows ESP without reformatting it;
- `%post` (the kickstart's script after the install) `outcome.sh`: the R21
  checklist and `outcome.json`.

Row 3 of `docs/validation-results/v2-install.csv`, `pass-plumbing`:

- the install took about four minutes;
- `bootmgfw.efi` (Windows' boot loader) was byte-identical before, after
  and across four boot cycles;
- no Microsoft boot file changed; the ESP gained 17 files / 19 MB;
- the firmware kept the Windows Boot Manager entry, Fedora was first in
  BootOrder, and GRUB listed Windows;
- **shim sits in the fallback slot, with Windows' copy in the 144-file
  snapshot on the stick.** That is the decided design, so this is the
  `pass-plumbing` the V1b vocabulary reserves for "once the converter's own
  install step is what runs";
- Windows booted through GRUB twice and Fedora twice, with markers on the
  stick.

Rows 1-2 are the same run failing at the outcome writer, then the schema
refusing the result, which is what the schema is for. The two bugs: a shell
boolean passed into Python, and the kept partition looked up by filesystem
name (a BitLocker volume says `BitLocker`, not `ntfs`).

Secure Boot was off (the Hyper-V template clause). The physical SB-on
install remains the residue, and it needs a machine other than the Aspire.
Keep-Windows is refused there (runs 5-7, RISKS R18), and the Aspire keeps
its dying drive by decision (2026-09-20).

**2026-09-12:** the same install ran once more as the tail of the
prologue's own run (`v2-install.csv` row 4, `pass-plumbing`;
`r18-prologue.csv` row 4). That is the conversion end to end from the one
typed word: disk check → shrink → handoff → install → cycles. The restore
half of the snapshot also fired (`r21-rollback.csv` row 1).

**VM leg fired (2026-08-27).** On the QEMU+OVMF rig (`rig/vm/v1b.sh`, SB
off, the only mode this host can run): C: shrunk by 32 GiB, Fedora 42
kickstarted into the gap reusing the Windows ESP unformatted. All five
checks held:

- the ESP had room (+6.2 MB, on a 260 MiB ESP);
- `bootmgfw.efi` stayed byte-identical throughout;
- Windows was reached through the GRUB menu three times (its own
  `BootCurrent` named Fedora's entry);
- Linux booted five times;
- every cycle was a fresh QEMU.

It is recorded as `fallback-loader-replaced`, not a bare pass. The install
overwrote Windows' `EFI/Boot/bootx64.efi` with shim. The run also caught
Windows taking the boot order back after a servicing pass, and the firmware
dropping OS boot entries. Five design inputs in all, detailed in RISKS R21.
Row: `docs/validation-results/v1b-alongside.csv`. Remaining: the
Secure-Boot-on chainload (the MOK experiment on a UEFI-CA-template Hyper-V
guest), and the physical vendor matrix. A VM pass narrows V1b. It does not
close it.

**Hyper-V leg fired: the ~100 MiB ESP row (2026-08-31).** On `rig/hyperv/`
(Hyper-V UEFI v4.1, SB off, because the guest's Windows-only Secure Boot
template refuses shim). Windows Setup's default 100 MiB ESP took the same
6.2 MB install with 63 MiB still free. All five checks held, and the result
is `fallback-loader-replaced` again (shim replaced `EFI/Boot/bootx64.efi` on
a second firmware). New evidence:

- BitLocker (TPM-sealed, XtsAes128) survived the whole flow. Suspend once
  before the installer; protection resumes by itself and re-seals against
  the GRUB path; the next chainloaded boot unseals silently. No recovery
  prompt anywhere.
- Hyper-V's UEFI *kept* the Windows `Boot####` entry, where OVMF had
  deleted them all.

Findings 1, 4 and 5 reproduced; RISKS R21 has the detail. Row 2 in
`v1b-alongside.csv`.

**Hyper-V leg: the Secure-Boot-on chainload fired (2026-08-31).**
"Chainload" means GRUB handing over to Windows' own boot loader. A second
guest on the UEFI-CA template, Fedora installed alongside under SB
enforcing. The Windows Production PCA (taken from `bootmgfw.efi`'s own
signature) was enrolled into shim's MokList (MOK is shim's own list of
extra trusted keys).

- **Negative:** PCA not enrolled → GRUB's chainload refused
  (`bad shim signature`).
- **Positive:** PCA enrolled → Windows boots to the desktop, SB enforcing
  confirmed from both Fedora and Windows.

This proves the chainload *verification* only. It does not prove a both-CA
database (Hyper-V can't express one), nor the vendor matrix. Record:
`validation-results/v1b-mok-chainload-2026-08-31.md`; RISKS R21.

**Decided (2026-08-30)** from those findings (RISKS R21 has the list):

- shim keeps the fallback slot, and rollback restores Windows' copy from a
  snapshot the prologue takes;
- checking the boot chain after the install is a cutover step, with results
  in `outcome.json`;
- os-prober (the tool that finds other OSes for GRUB's menu) is set
  explicitly;
- `evaluate` gates on ≥ 32 MiB free on the ESP;
- the boot-order takeover is its own risk (R22), with a settle-in unit that
  re-asserts the order.

**Owed code, landed 2026-08-30:** the scanner's ESP check, "Boot partition
(ESP)". It needs free space ≥ 32 MiB and the Windows Boot Manager entry
pointing at the mounted ESP. Elevated only, like the shrink query, behind a
collect/judge seam, with six self-test cases (see RISKS R21 item 4). The
bench row turns `pass-plumbing` only once the converter's own install step
runs on it.

**Experiment.** Install alongside on real machines from several vendors,
Secure Boot on: `--onpart` into the freed space, **reuse the existing
Windows ESP without reformatting it**, add shim + GRUB, run `os-prober`.
Check each of these:

- the ~100 MB Windows-made ESP had room for the added entries;
- `bootmgfw.efi` is untouched;
- Windows still boots from the GRUB menu;
- Linux boots;
- both survive a few power cycles.

**Pass.** After the install, *both* systems boot from the menu, Secure Boot
still on, on every machine in the matrix.

**If it fails.** Machines whose firmware or ESP can't take the alongside
install are steered to **clean slate**, and `evaluate` says so before
anything is committed. Clean slate never shares an ESP: it wipes and lays
down a fresh layout. The default simply doesn't apply to those machines.
The product still converts them, without the safety net.

**How to run.** The bench is `rig/vm/v1b.sh` (run-book in
`rig/vm/README.md`):

- offline disk inspections before and after (`v1b-inspect.py`);
- the shrink inside the guest (`rig/vm/guest/v1b-shrink.ps1`);
- the kickstart (`rig/vm/v1b-ks.cfg`), loaded automatically from an OEMDRV
  volume;
- boot markers written by each OS;
- `v1b.sh verdict`, which turns all of it into the CSV row.

On a physical machine the same pieces apply, with the machine's own disk in
place of the qcow2. The inspector then needs a block-device reader instead
of `qemu-img dd`.

# Tier 2: a core promise breaks (recoverable, but the default is broken)

Three gates here. All are failures we can recover from, but each breaks a
core promise:

- **V4:** do disks shrink enough? This decides whether the default path even
  applies.
- **V3:** the BITLK read that delivers files on the default path.
- **V2:** firmware that makes the speakers work.

They are kept in V-number order below.

## V2: Extracted amp firmware makes speakers work · kills: the artifact pipeline

The docs say `evaluate` must extract vendor firmware *now* because it
"cannot be added later". That assumes extraction works at all: that the
right files ("blobs") can be pulled from the Windows driver store
automatically, and that the Linux kernel accepts them. Evidence today: the
community does this by hand. Nobody has shown it automated end to end.

**Experiment.** On the test G16 itself, which has the exact CS35L56 amp the
pipeline was designed for. Extract from its driver store, install Fedora by
hand, put the firmware in place, and play a sound through the *speakers*.
This checks extract → carry → inject on the hardware class that motivated
it.

**Pass.** Speakers you can hear, using only firmware taken from that
machine's own Windows.

**If it fails.** The promise narrows. We rely on what upstream
`linux-firmware` covers, and the scanner tells owners of 2023+ laptops the
truth about their speakers instead of promising them. The "first
impression is working hardware" claim gets a hardware-generation asterisk.

## V3: cryptsetup BITLK reads · kills: the default path's file delivery · RISKS R19

BitLocker is on by default on most machines this project targets. The
keep-Windows path (now the default) delivers the person's files by reading
the kept Windows partition in `settle-in`, through cryptsetup's BITLK
support. (cryptsetup is Linux's disk-encryption tool; BITLK is its BitLocker
reader.) The redesign made a failure here *recoverable*: the person is
present, Linux is verified, and Windows is intact as a backup (see R19). But
if it's flaky, the default experience is broken for everyone on modern
BitLocker machines.

**Experiment.** On the bench, all in VMs:

1. Windows 11 with BitLocker defaults (XTS-AES-128, used-space-only).
2. Hash every file from inside Windows.
3. Attach the disk to Linux, unlock it with the recovery key, hash again,
   compare.
4. Repeat for XTS-AES-256 and full-disk encryption.

Thousands of files, byte-identical or it fails. Test from an *installed*
Fedora (where `settle-in` runs), not only the live environment.

**Pass.** Identical hashes across all three configurations.

**If it fails.** Either decrypt in Windows first (`manage-bde -off`: adds
hours, works), or BitLocker machines get clean-slate only. Both are
survivable, both are worse, and either one changes the intent-capture UI
(the screens that ask what the person wants). So we need the answer before
that UI exists.

**VM leg fired (2026-09-01).** On the Hyper-V rig (`rig/hyperv/v3.sh`,
run-book in `rig/hyperv/README.md`). Read from the V1b guest's *installed*
Fedora 42, against its own encrypted-then-shrunk Windows 10 C:.

- The recovery-password unlock works with every key-entry form cryptsetup
  offers.
- A planted set of 2,850 files, plus everything under `C:\Users`, read back
  **byte-identical through ntfs-3g** (the long-standing user-space NTFS
  driver).
- The kernel's `ntfs3` driver on the F42 install kernel (6.14.0-63)
  **crashed (oopsed) in five runs out of five** and wedged the guest twice.
  On the current F42 kernel (6.19.14-108) it reads cleanly.

Decided (2026-09-01): `settle-in` uses ntfs-3g. See RISKS R19 for the full
list of findings: the size-mismatch warning every shrunk volume produces,
the FIFO trap, app-exec aliases, and the churn from running apps that fakes
mismatches. Rows: `docs/validation-results/v3-bitlk-read.csv` (one per
driver, per kernel, per config).

All three configs fired:

- XTS-AES-128 used-space-only, on the guest's own C:;
- XTS-AES-256 used-space-only, and XTS-AES-128 "full", both built in the
  product's order (encrypt, then shrink) on copies of the pristine disk, and
  read from the same installed Fedora as data disks.

For all three, on the current kernel, the planted files were 2,850/2,850
byte-identical under every driver. Every `Users` file was identical except
what Windows itself rewrote after the hash. (The XTS-256 run's first rows
say `mismatch` for one 330-byte crypt32 URL-cache metadata entry that both
Linux drivers read identically. The classifier learned that cache, and the
later rows for the same run supersede them. Both stay in the CSV.)

Remaining, and no VM row closes it: real disks, Windows 11's BitLocker,
physical machines, and "full-disk" as a real disk experiences it (the rig's
thin VHDX never had its free space rewritten).

**How to run.** `rig/hyperv/v3.sh run <config>`, on a guest with the
Fedora-side run hook installed (`guest/v3-bootstrap.sh`, once, from the
console). It:

1. builds the OEMDRV transport carrying the reader and the recovery
   password;
2. boots Windows through GRUB, plants and hashes (`guest/v3-plant.ps1`),
   with a full shutdown;
3. boots Fedora, unlocks, mounts and hashes again (`guest/v3-read.sh`,
   three driver passes);
4. `v3-verdict.py` writes the rows.

Other configs are built with `guest/v3-encrypt.ps1` on a copy of the
pristine disk in a throwaway VM (`v3.sh mkvm`), then read as a data disk.

## V4: Real disks can actually shrink · kills: whether the default even applies · RISKS R18

Keep-Windows is the **default**, and it needs shrinkable space of at least
~20 GB plus the person's data. Files that can't be moved (the MFT, the VSS
store) often cap a shrink far below the free space. (The MFT is NTFS's
master file table. VSS is Windows' shadow-copy service, which System
Restore uses.) If most real machines can't shrink enough, the default
rarely applies. The design would then lean almost entirely on clean slate
and big sticks. That changes what stick size we tell people to buy, and how
often anyone gets the safety net at all.

**Experiment.** Add the shrinkable-space query (the same one Disk
Management uses) to the scanner and ship it. Every scanner report then
measures the population for free. Judge after ~20 real reports.

*Done (2026-08-22):* the query is in `Test-UpgDisk` as `Room to keep
Windows`. A caveat found on real hardware: `Get-PartitionSupportedSize`
needs Administrator (RISKS R18), so only elevated runs carry a number.
Filter the JSON corpus on `RanAsAdmin=true` before judging, and note that
the sample will lean toward people willing to elevate.

*Measurement hardened (2026-09-08):* the first physical machine (Acer
Aspire A515-51G) returned "could not measure" on two elevated scans. The
scanner had been throwing away the reason and printing a guessed cause
(Fast Startup) that the rig contradicts (see RISKS R18). Now the scanner:

- keeps Windows' own error text;
- refuses to name a cause;
- tries a second read-only path (`diskpart shrink querymax`, through VDS)
  when the Storage API path refuses;
- labels the source in the report.

So V4's population count needs the `ShrinkSource` field read next to the
number. `storage-api` and `diskpart` measure the same thing through
different services and should agree. Any machine where they disagree is a
finding in its own right. A machine whose C: carries the dirty flag reports
no number at all until the prologue's disk-check step (decided 2026-09-08,
R18) has run. Count it separately, not as "cannot shrink".

*The prologue's step (2026-09-12):* the disk check, the re-measure by both
paths and the shrink are product code now (`Invoke-Prologue.ps1`, RISKS
R18). The **prologue** is the part of `upgrade_` that runs on Windows
before the handoff. It has a rig bench that injects the flag
(`rig/hyperv/prologue.sh`) and one evidence file,
`docs/validation-results/r18-prologue.csv`. Row 4 was `pass-plumbing` the
same day: flag → full boot-time check → 57.8 GB by both paths → 25 GB freed
→ install. Its `remeasured_gb` / `diskpart_gb` pair is the same two-path
measurement V4 counts, taken *after* the check, at the moment it matters.

*The Acer Aspire's real flag answered (2026-09-13, row 5,
`stopped-volume-check`):* it is not a shrink-headroom case at all. It is a
failing SSD (RISKS R18 has the diagnosis). So the first physical
measurement after a real check still waits for a machine with a healthy
drive. What that row did prove: the refusal path works on real hardware,
and `HealthStatus` and the scan cmdlet's return string are not evidence.
The guardrails were rebuilt the same day on the event log, SMART (the
drive's own health counters) and the volume's own status.

*The walk-away resume (2026-09-13):* rows 1-5 all had someone signed in
when the resume fired (the rig signs in automatically; on the Aspire the
person signed in). So prologue 0.3.0 resumes as SYSTEM at startup, and the
bench now switches the guest's autologon **off** before the flow
(`prologue.sh autologon off`). A row counts as walk-away only when
`state.Resumes` shows SYSTEM, session 0 and no explorer. Otherwise the
verdict script writes `resume-attended`.

- **Row 6** (2026-09-13, `pass-plumbing`) is the first such row: both
  resumes SYSTEM in session 0 with autologon off, 472 s from the check
  restart to the first Linux boot, nobody signed in.
- **Its physical residue** (a real USB stick at a real firmware's boot, a
  real sign-in screen) has its own read-only probe. `RUN-PROBE.cmd` on the
  kit (`Invoke-Prologue.ps1 -Probe`) registers the same SYSTEM startup
  task and restarts once. The task writes the row to
  `upgrade_/walkaway-probe.csv` on the stick, which is copied word for word
  into `docs/validation-results/walkaway-probe.csv` (rig rows come from
  `prologue.sh probe`). It is what a borrowed machine can run in a
  half-hour visit. The Aspire's dying drive stops the conversion at the
  disk gate, so the probe is the row it can still give.
- **Rig row 1 (2026-09-13):** `resumed-unattended`. SYSTEM, session 0,
  12 s after boot, stick after 4 s, notice queued, task removed, no user
  session.
- **Row 2, the first physical row (2026-09-13, Acer Aspire A515-51G,
  InsydeH2O V1.21, Windows 11 Home 22631, Secure Boot on, a real USB
  stick): `resumed-unattended`.** SYSTEM, session 0, no explorer, 38 s
  after boot, the stick seen 5 s later, notice queued, task removed, copied
  word for word from the stick's CSV.

The rest of the matrix is V0's (above). The stakes and the residue are
R24's.

### The Aspire, run by run

The Aspire's drive is dying, so every run below is on one bad disk. It is
one data point, not a rate.

**2026-09-17, the Aspire again (R18):** its dirty bit was now clear but the
repair was still queued. The Storage API answered 0 GB shrinkable, no
error, 33.6 GB free, and the job writer forced clean slate. Decided the
same day: a queued repair makes the number "unmeasured" in the job, and is
a second trigger for the prologue's check. The launchers log every step to
the stick. The physical keep-Windows row is still owed.

**2026-09-20 (R18), three runs and a read-only diagnostic:**

- The launcher had been exiting after every job (an unescaped `)` in cmd).
  Fixed, it ran all five steps and gave the first physical row of the
  acknowledged path, `stopped-volume-check` (`r18-prologue.csv` row 7). The
  scheduled check left no proof it ran, and the prologue refused to
  measure.
- The diagnostic (over SSH, reading Windows' own records) showed the check
  should never have been asked for. The full check had already run on
  09-15, and the 0 GB was `hiberfil.sys` on the last cluster (Defrag event
  259), about 42 GB once it is off.
- Built the same day: an event 98 counts only if no completed check comes
  after it. And a small cold number pinned by hibernation, page or swap
  files is keep-windows, pending the prologue's own re-measure.

For V4 this was the first real measurement of *why* a cold number is small,
and the cause was the mitigable kind. Owed: the rerun, which would be the
first time the prologue's mitigation, shrink and handoff run on real
hardware.

**2026-09-20 evening, the rerun: `stopped-shrink`** (`r18-prologue.csv`
row 8). The mitigation ran on real hardware for the first time. The answer
is V4's first mitigated number from a physical disk: **48.5 GB free, 7.2 GB
shrinkable.** Behind `hiberfil.sys` sat System Restore's shadow-copy
storage, which nothing on the ladder moves. (The **ladder** is the
prologue's list of steps that free space, tried in order.)

- **Corrected:** the "about 42 GB" above was read from Defrag 259's shrink
  *target*. That target does not say what removing the named file frees.
  Only re-measuring does.
- "The cause was the mitigable kind" was half true: the first cause was,
  the second was not.
- One dying machine is a data point, not the fraction this section asks
  for. But it is a no, and shadow storage is now a named candidate for why
  real disks fall short.
- Also exposed (R18): a stop after the mitigation left hibernation and the
  pagefile off (a bug; fixed in 0.5.1, proven in run 6). And a sign-in
  during the re-measure lowered the second path's number.

The shrink and the handoff have still not run on real hardware.

**2026-09-22, run 5: `stopped-confirm`** (`r18-prologue.csv` row 9). The
same disk, two days later, measured cold at 3.2 GB. This time it was pinned
by NTFS's change journal (`$UsnJrnl`), which had been allocated since run 4
in the tail that run 4's mitigation had emptied.

- For V4, a second lesson about cold numbers: they are not stable between
  runs, and the first unmovable file on a real disk changes with ordinary
  use. That is why the decision belongs to the prologue's re-measure right
  before the shrink, not to the job writer's reading at scan time.
- The run also exposed that the job writer turned a person's `stop` into a
  clean-slate job when the cold number was not mitigable (RISKS R18, fifth
  run). Fixed the same day in job writer 0.8.0: under `stop`, a small cold
  number is now keep-windows for the prologue to re-measure. It held on the
  real machine in runs 6 and 7.
- No new mitigated number: the mitigation never ran.

**Decided and built the same day:** the change journal joins the ladder
(deleted with consent, created again afterwards; prologue 0.8.0). What it
frees is the next row's question.

**2026-09-23, run 6: `stopped-shrink`** (row 10).

- The journal rung works and buys little (8.4 -> 9.5 GB).
- Behind it sits System Restore's storage. The restore-point deletion ran
  but deleted nothing, cause not recorded (R18; the reporting is fixed in
  0.9.0, unfired).
- This disk's best measured number is 9.5 GB of 25.
- The stop restored everything it touched, read back after restarts. 0.5.1
  proven.
- New: a pending Windows update turned the prologue's one restart into
  three (RISKS R25).

Built 2026-09-26 in prologue 0.9.0: the update gate (R25), and a
restore-point step that records what Windows answered and tries its WMI
objects if vssadmin deletes nothing.

**2026-09-26, run 7: `stopped-shrink`** (row 11) at 7.2 GB, pinned by
NTFS's own `$Mft::$BITMAP`, which nothing on the ladder moves. Runs 5-7 on
one disk named three different last files (the journal, System Restore's
storage, the MFT's bitmap), each one moving with ordinary use. The ladder
bought at most 1.1 GB. This disk's answer for V4 is no: one used, failing
disk, a data point, not a rate. It is the case the discard offer (R26) is
designed for.

**Pass.** A meaningful fraction (say, a third) of *elevated* scanned
machines could fit Linux plus their data in shrinkable space.

**If it fails.** Keep-Windows becomes the lucky path rather than the
default. Messaging, stick-size advice and the intent UI shift their weight
toward clean slate.

**Designed (2026-09-26), not built:** the concrete form of that shift is
the offer to discard Windows after a failed shrink (`architecture.md`,
"When Windows cannot be kept"; RISKS R26). It is:

- asked only of a person who chose *ask me then*;
- shown with the real numbers;
- not offered when the files do not fit the stick;
- confirmed by a typed sentence;
- with the wipe still behind the live-session checks.

Its validation: rig rows for the yes path (staged, read back, wiped, every
file restored with matching checksums) and for each refusal (files too big,
a stick that drops, 0 folders). Then one physical row on a machine whose
owner chose to lose Windows. It gets built after the harvest and a first
physical install row.

**The harvest half landed 2026-09-26** (folder map + stick fit in the job).
First measurement, the Aspire: 16.6 GB in the six folders, one file over
4 GB. So no offer on today's 8 GB FAT32 stick
(`validation-results/harvest-folder-map.csv`, RISKS R26).

## V9: One-click erase and install (R27)

The owner's first end-to-end destructive target: erase every internal
drive, install Fedora, keep nothing (`architecture.md`, "Erase and
install"). Design decided 2026-09-26.

**Rig: passed 2026-09-26** (`validation-results/v9-erase.csv`):

- **A** refused before the countdown.
- **B** cancelled with both disks untouched. A first B attempt froze, and
  was fixed in `verify.sh` 0.4.1.
- **C** erased and installed, with `/home` on the second disk and the
  chosen password, including once over an Ubuntu-style LVM.
- Each of the person's three start choices held (KDE, GNOME, console).
- **A again, 2026-09-27 (line 14), with `verify.sh` 0.5.0:** the refusal is
  now one plain screen in the owner's words instead of Anaconda's
  traceback; 60 s later the rig restarted into an untouched Windows.

**Seen and signed in (2026-09-27, lines 12-13):** with the rig's power-off
marker removed, the GDM and SDDM sign-in screens appeared. The account's
password was typed on the rig's keyboard, and the GNOME desktop and the KDE
Plasma desktop (Welcome Center, taskbar) came up. Screenshots are in the
gitignored `rig/hyperv/artifacts/v9/{gnome,kde}-seen/`.

**Physical: the Aspire's run 9 (line 11) FAILED the one-click promise.**
Everything ran unattended and the password signed in, but it came up at a
text login (fixed; RISKS R27). A physical re-run that ends at the chosen
screen is owed.

**Physical: the Aspire's run 10 (2026-10-04) FAILED too, on a new fault.**
Kit ee74318, Fedora 44, Secure Boot on. It ended at the chosen screen
(GNOME's sign-in), with the handoff, the erase and the install all
unattended. But the firmware started the stick again after the install,
and the stick installed a second time (RISKS R35). Fixed the same day and
proven on the rig as a fourth arm:

4. After arm 3, the stick first in the boot order and left in: the stick
   hands over to the installed Fedora, no countdown, both disks unchanged
   (`not-installed-again`).

The physical row then owed: one install with the stick left in, Fedora up
once, Wi-Fi carried.

**Physical: the Aspire's run 11 (2026-10-04) PASSED. The first clean
conversion.** Kit aba09ec (re-proven on the rig first: C7, D3), Fedora 44,
KDE, Secure Boot on, the stick left in throughout. Three rows, each written
by its verdict script from the machine's own records:

- `v9-erase.csv`, `erased-installed`: one install. The firmware started
  the stick again after it, and the stick handed over to Fedora.
- `v9-erase.csv`, `not-installed-again`: the owner restarted once with the
  stick in. The stick started again and handed over again.
- `settle-in-first-start.csv`, `pass`: the clock corrected, Wi-Fi connected
  by itself, remote access carried (V10).

Still owed after run 11: the clean physical cancel (arm 2 on a real
machine; it was skipped by accident), other vendors' firmware, and the
old-entry button on real firmware (nothing was left for it to remove).

**Method.** Rig first, on a copy of the rig's Windows disk plus a blank
second disk:

1. A job naming a disk that is not attached: `%pre` refuses before any
   countdown.
2. The countdown with a key pressed: Windows comes back, both disks
   byte-unchanged at the partition table.
3. The countdown left alone: both disks cleared, Fedora on the first,
   `/home` on the second, `outcome.json` with the commit line crossed at the
   countdown's end, and the account's password signs in.

Then the Aspire (system on the failing SSD under R23, `/home` on the 1 TB
drive).

**Pass.** All three rig arms, then the physical row: one click, the two
typed answers, nobody at the keyboard afterwards, Fedora signs in.

## V10: settle-in runs on any Linux (R28)

Decided 2026-09-27 (the owner): `settle-in` is one self-contained program,
fed by a per-distribution installer adapter through one handoff folder.
Built the same day (Rust core 0.1.0, egui window). `[###.]` rig, Fedora
KDE, GNOME and the text console. Rows in `validation-results/settle-in-first-start.csv`,
written by `rig/hyperv/settle-in-verdict.py`, never by hand.

**Rig, 2026-09-27 (the erase arm of V9, with three made-up Wi-Fi networks
planted in the job, `v9-job.py --spoof-wifi`; plumbing only):**

- **Run 1 (line 2) `[FAIL]` (the harness).** settle-in itself did its
  job: it ran before the time service and NetworkManager, left the clock
  alone for the right reason (Hyper-V's hardware clock holds UTC), set up
  2 of 3 networks, listed the enterprise one, and deleted its copy of the
  passwords; the stick was clear. The capture was broken (a wrong `nmcli`
  field, and a loop variable that overwrote the boot marker's user).
- **Run 2 (line 3) `[FAIL]`, a one-click failure.** The first KDE sign-in
  stayed black: the console hook in `/etc/profile.d` ran inside SDDM's
  session start and waited for an answer nobody could type. Fixed (the
  hook runs only on a real text console; the window marks itself shown
  only when closed). In the second session, by hand: the window drew the
  approved words, and the button removed exactly the stale "Windows Boot
  Manager" (Boot0004) through the password prompt. The next boot started
  Fedora (Boot0005) into the graphical sign-in. NetworkManager loaded
  both networks with their names exact, including one written as raw
  bytes.
- **Run 3 (line 4) `pass-plumbing`, KDE.** The fix held: the first
  sign-in reached the desktop and the window opened by itself. The button,
  clicked with the mouse, removed the stale entry (Boot0008 on this
  install) and the next boot started Fedora. After Close and a restart,
  the second sign-in showed no window. NetworkManager's own reading of the
  files matched the job field by field (hidden, auto-connect, security,
  mode 600). KDE's Welcome Center opens on top of the window (a finding).

- **Run 4 (line 5) `[FAIL]`, the text console (the harness).** The
  verdict found no capture: the rig's boot marker waited 120 s for a
  desktop sign-in manager that the console path never starts, and the
  restarts cut it off before it wrote. The screenshots
  (`artifacts/v9/C4-settle/console-*.png`, gitignored) show the path
  working: at the first console sign-in the approved words, the question,
  "yes", the password at the text prompt, "Removed."; the next boot
  started Fedora; the second sign-in showed nothing. Fixed: the marker
  waits for a display manager only when the system starts at the desktop.
  The console no longer prints the window's button line above the
  question.

- **Run 5 (line 6) `pass-plumbing`, GNOME.** The first sign-in reached
  the desktop and the window opened by itself, under GNOME's welcome tour
  in the overview. Clicked, the button removed the stale entry through
  GNOME's password prompt; the next boot started Fedora; the second
  sign-in showed no window. NetworkManager's reading matched field by
  field. The row's "0 of 0" sessions is the verdict not knowing GDM's log
  lines yet (the log shows GNOME Shell started after the sign-in); both
  the capture and the verdict read GDM's lines now. GNOME gives the window
  only minimal controls and a generic icon.

- **Run 6 (line 7) `pass-plumbing`, the text console.** With the boot
  marker fixed, every column came from the evidence: the approved words at
  the first console sign-in (no button line), "yes", the password at the
  text prompt, the stale entry removed, the next boot at the console as
  chosen, and nothing shown at the second sign-in.

- **Run 7 (line 8) `pass-plumbing`, KDE: the window check is automatic
  now.** The window writes a line to the system log on its first drawn
  frame ("settle-in-window: showing the summary") and when the person
  closes it; the next boot captures the previous boot's sign-ins, and the
  verdict's `window` column fails a desktop install whose first sign-in
  did not show it. Here it showed 3.6 s after the sign-in started.

- **Run 9 (line 9) `pass-plumbing`, KDE, with the owner's answers of
  2026-09-27:** the window opened 5 s after sign-in in front of KDE's
  Welcome Center, and KDE gave it focus (logged); the password prompt
  showed the owner's words; `settle-in` removed the conversion's own
  "upgrade_" entry at first start by its recorded id; the button removed
  the stale Windows Boot Manager.
- **Run 10 (line 10) `[FAIL]`, GNOME: settle-in moved a right clock 7 h
  forward.** At the end of the install the installer's clock and the
  hardware clock agreed and no time service had synchronized. On bare
  metal that means the installer copied the local-time hardware clock
  (the Aspire's case). On Hyper-V the host's time sync set the installer's
  clock, and the virtual hardware clock holds UTC (the same quirk that
  runs this rig's Windows 7 h fast), so the -7 h correction was wrong. The
  network time service put it back within seconds; runs 1-9 had escaped
  only because it synchronized during the install. **Fixed the same day:**
  in a virtual machine (the CPU's hypervisor flag) that case is left
  alone, and the verdict now fails any correction that disagrees with a
  synchronized clock (it caught this run). Also in run 10: GNOME did not
  give the window focus (its welcome tour stays on top; logged), the
  password prompt showed the owner's words on GNOME too, and the own-entry
  removal and the button passed. The verdict missed GDM's sign-in line
  (its format carries a process number); fixed.

- **Run 11 (line 11) `[FAIL]` on the button only, GNOME, after the fix:**
  the clock was left alone for the new reason ("this is a virtual
  machine…"), the window column read GDM's sign-in ("shown in the first
  sign-in; not given focus by the desktop"), and our own entry was
  removed. The fail is the harness: the longer clock line moved the button
  15 px down and the rig's click landed on the text above it, so nothing
  was pressed. The button passed on GNOME in run 10.

- **Run 12 (line 12) `pass-plumbing`, GNOME, the re-run of run 11:** the
  button, clicked where it was, removed the stale Windows Boot Manager
  through GNOME's prompt in the owner's words; the next boot started
  Fedora; our own entry was removed at first start; the window was shown
  at the first sign-in (GNOME again did not give it focus); NetworkManager
  read both networks field by field. The installer's time service
  synchronized this time, so the clock was "not needed" (run 11 showed the
  virtual-machine case). Built in a clean worktree at `0725967`, because
  another session had uncommitted settle-in changes in the main tree.

**Residue the rig cannot close.** Hyper-V's hardware clock already holds
UTC, so on the rig the clock step is correctly "not needed" and the
correction itself (the Aspire's 4-hour case) is proven only by the tests.
Real Wi-Fi, a real firmware's boot entries and a non-Fedora distribution
are still owed.

**Method.** Build the program once. On the rig, install Fedora KDE and
Fedora GNOME through the converter, and at least one non-Fedora
distribution by hand with a hand-placed handoff folder. On each, check:

1. the program starts at first boot, before the network;
2. its window appears (or its text screen, when the console was chosen);
3. the clock, Wi-Fi and boot-entry steps give the same results as on
   Fedora;
4. a distribution missing a floor piece (for example without
   NetworkManager) gets a plain refusal, not a half setup.

Then the same on a real machine.

**Pass.** The same file, byte for byte, passes 1-4 on all three.

**V10 addition, remote access (decided 2026-10-04, the owner).** SSH is
carried only if Windows had it on, public keys only. `[###.]` on the rig
(settle-in-first-start.csv, rig run 17): with a spoofed "Windows had it on"
key, the new Fedora 44 accepted that key over SSH from the host, refused a
password (the server offered only public key), ran sshd enabled, and kept
`~/.ssh` 700 and `authorized_keys` 600 owned by the person with the
`ssh_home_t` label. **Physical, 2026-10-04 (the Aspire's run 10): carried.** Carried again in run 11 the same day, on KDE, inside a passing row.
The new Fedora came up with SSH on and the G16's key signed in, with no
tester step. That key then gathered the run's evidence.

## V11: The way back to Windows (R30)

Decided 2026-09-27 (the owner): after an erase or a reclaim, a program on
the Linux side offers a new, empty Windows, and says what that costs first
(`architecture.md`, "The way back to Windows"). `[#...]` planned.

**Method.** In order, each step on the rig before a real machine:

1. **The harvest** (`harvest.windows_license`). The self-test feeds it
   made-up licence facts (activated or not, each channel, a firmware key
   present or not, a failed read) and proves a key handed to it never
   reaches `job.json`. Then the rig's Windows, then a real one.
   **Built 2026-09-27** (job writer 0.16.0): the self-test passes; read
   once by hand on the G16 (Windows 11 Pro, activated, `OEM:DM`, a key in
   the firmware), not yet a row.
2. **The Aspire's hand reinstall** (2026-09-27, before its follow-up
   run). Windows put back by the owner with Microsoft's installer; the
   follow-up run's job records whether activation came back. The licence
   before the erase was never recorded (R30), so this row has only an
   "after".
3. **The guided stick.** From an installed Linux: download Microsoft's
   installer, check it, write it to the named stick (refusing every other
   drive), start from it with Secure Boot on, install, and read the
   activation state on the new Windows.
   **Built 2026-09-27** (`settle-in go-back`; the window's `--go-back`).
   The rig leg first: `settle-in go-back write --image` writes a new file
   (never a disk) from the rig's own Windows 10 ISO (Microsoft's English
   x64 hash), and `rig/hyperv/v11-stick.ps1` boots that file as a disk in
   a new Generation 2 VM, Secure Boot on (Microsoft's Windows template),
   offline, with an empty 64 GB target. That proves the stick's layout
   and the wimlib split (plumbing); only a real stick in a real machine
   closes the step.
   **Rig leg fired 2026-09-27** (`v11-way-back.csv`): line 2 `fail` (the
   writer stopped itself at its last check, on a wimlib option; fixed),
   line 3 `pass-plumbing`: 905 files and two `install.swm` parts read
   back, the 11 editions the same as the original, Windows Setup started
   with Secure Boot on, installed from the split image and restarted into
   Windows' first screens. Two findings changed the done words: Setup lists
   the stick (`WINSETUP`) beside the computer's drives, and it asks for an
   edition, so the words now say never delete `WINSETUP` and name the
   edition from the harvest.
4. **The walk-away reinstall** (RISKS R33; decided 2026-09-29, the
   owner: 100% managed, now). Refuse, cancel and erase arms, as in V9. The
   rig starts from a Fedora the V9 rig installed (two disks), Secure Boot
   on. Refuse: a job naming a drive that is not there; the gate must
   refuse before any countdown, both disks unchanged, and Linux start
   again. Cancel: a key during the gate's countdown; Linux starts again,
   untouched. Erase: nobody at the keyboard from the restart to the
   Windows sign-in; both disks rewritten (Windows on the system disk, one
   empty NTFS volume on the second), the gate's record on the stick. Then
   the Aspire: a physical cancel first, then the erase.
   **Added 2026-10-04 (the owner): the time and remote access.** The erase
   arm also passes only when the new Windows shows the right time in the
   right zone before it reaches any network (the gate's record names what
   it read and wrote; Windows' Kernel-General record shows no later
   jump), and, where Linux had SSH on, when its key signs in and a password
   does not. And a fourth arm, as V9 has: the stick left in and started
   again after the line never counts down again and hands over to Windows.
   All three ran on the rig 2026-10-04 (`[###.]`, RISKS R33); the Aspire is
   owed.
   **Added 2026-10-02 (the owner): Wi-Fi.** The erase arm also passes only
   when the new Windows is on the network the Linux side knew, at its
   first start, with no Wi-Fi password left on the stick (spoofed networks
   on the rig, as `settle-in`'s rows do; a real network on the Aspire).

**Pass.** Step 3 puts back an activated Windows on at least two real
machines with different licence kinds (a firmware key; a digital licence
without one), each with its harvest from before and after, and a machine
that cannot run Windows 11 is shown the Windows 10 path and its warning.

# Tier 3: silent data loss (the trust-ending class)

## V8: OneDrive placeholders are materialized at evaluate · kills: file integrity on the default path · RISKS R8

On the default path, files are pulled from the mounted Windows partition
*by Linux*, which has no OneDrive client. OneDrive's "free up space" leaves
a **placeholder** on disk: a stub that looks like the file, with the real
bytes in the cloud. A placeholder not forced local beforehand copies over
as **0 bytes**. The person's photos arrive empty, and they find out later.
`evaluate` must *materialize* them (force the download while Windows is
still running), not just detect them, because no later stage can.

**Decided (2026-09-26, the owner): no download.** Online-only files stay in
OneDrive, where their bytes already are. The job records them
(`cloud_files.result = left-in-cloud`) and `settle-in` reconnects OneDrive.
The danger left is copying a stub as if it were the file, and that is now
`settle-in`'s to refuse (RISKS R8). The materializer below stays built,
unused by the launchers.

**Experiment.** On a machine with OneDrive "free up space" files present:
confirm `evaluate` detects them, forces them local, and that they carry real
bytes on the NTFS partition afterwards. Confirm the pinned/unpinned
attribute bits (`0x00080000` / `0x00100000`) don't need to be part of the
detection.

**cfapi leg fired (2026-09-08).** `evaluate/windows/Test-Materialize.ps1` is
a sync provider on Windows' Cloud Files API (cfapi, the interface OneDrive
itself uses). It:

1. creates real dehydrated placeholders with known bytes;
2. refuses to serve one;
3. runs the harvester's `-Materialize` seam in a separate process;
4. hashes the NTFS bytes afterwards.

`pass-plumbing` on the rig (Win10 19045) and the G16 (Win11 26200). Rows in
`docs/validation-results/v8-materialize.csv`. The pinned/unpinned bits are
confirmed not to be part of detection. Remaining: `-OneDrive` against a
signed-in client (the residue).

**Pass.** No cloud-only stub survives into the pulled data as a 0-byte
file.

**If it fails.** `evaluate` refuses machines with placeholders it cannot
materialize, rather than silently copying empties. Refuse-by-default
applies: better to turn someone away than to lose their photos.

# Tier 4: kills adoption, not the mechanism

## V5: VMD detection fires on real RST hardware · kills: scanner trust · RISKS R1

**Intel RST / VMD** is a storage setting on many Intel laptops. With it on,
the Linux installer can't see the SSD at all. The scanner's check for it is
the flagship check, and it has never matched anything on a real machine.
The project's only asset is that its report can be trusted, and this is the
report's highest-stakes line.

**Experiment.** An afternoon: compare the ID list with the kernel's
`drivers/pci/controller/vmd.c` table and linux-hardware.org probes. Then one
machine: any 11th-gen or newer Intel Dell or Lenovo laptop with RST on
(they ship that way). The scanner must say FAIL. Switch it to AHCI (the
standard mode) and the scanner must say OK.

**Desk half done (2026-08-22).** The ID list was reconciled against
`vmd.c` (mainline master) and pci.ids:

- Three bogus IDs removed. `7ec0` was a USB controller, a false-RED
  landmine on Core Ultra 200 machines. `2010` and `e0b0` aren't Intel
  devices.
- Five kernel IDs added (`28c0, 4c3d, b60b, b06f, b07f`).
- `09ab` kept, with an Intel citation (article 000088762).
- The `^iaStorV` service regex was replaced. It missed the whole pre-VMD
  RST family (iaStorA/iaStorAC/iaStorAVC, the Skylake-Comet Lake remap
  generation). It is now three signals: the kernel ID list, the `iaStorVD`
  service, and PCI RAID class code `CC_0104` from CompatibleID (format
  checked live on the G16).
- Six detection-level self-test cases feed made-up PnP entries through the
  real check. All pass.

Full evidence trail in RISKS R1.

**Level-3 spoof done (2026-08-26).** The full Windows PnP → WMI → scanner
pipeline now fires on simulated hardware. A patched QEMU `pci-testdev`
(`rig/vm/`) presents PCI `8086:9a0b`, class `0104`. Windows lists it as an
unknown RAID Controller (CompatibleIDs `PCI\CC_010400` / `PCI\CC_0104`).
Both the source scanner and the built `dist/` return the check
`[FAIL] Storage controller mode` with the detail `Intel RST / VMD active`,
verdict RED. The capture was taken hardware-only and kept
as the synthetic corpus regression
`evaluate/windows/corpus/vm-qemu-q35-vmd-spoof-9a0b.json`.

This closes the **plumbing**: listing, parsing and verdict all work end to
end. But it is built from our own model of the IDs, so per CLAUDE.md rule #5
it narrows V5 without closing it. See RISKS R1 for the full trail.

**The evidence file and the one-click visit (2026-09-13).** Rows live in
`validation-results/v5-controller-mode.csv`, written by
`rig/v5-verdict.py` from a run's JSON report and capture, never by hand.
The `result` column cross-checks the mode asked for against the PCI class
code the controller declared (vocabulary in the results README).

- **Row 1 is real:** the Acer Aspire A515-51G in AHCI mode, `8086:9d03`,
  class `0106`, **iaStorAC bound** → `warn-rst-on-ahci`. That is the R7
  guard on real silicon, not `[OK]`. On that machine the negative direction
  is `warn` by design.
- **The positive direction is one click:** `RUN-STORAGE-MODE.cmd`
  (`evaluate/windows/Test-StorageMode.ps1`). It scans, arms a Safe Mode
  boot through a copied boot entry booted once, and restarts straight into
  the firmware setup for the person to change SATA Mode. It scans again as
  SYSTEM with nobody signed in, asks for the mode back, scans a third time
  and cleans up. The person's part is the vendor's setup screen twice and a
  Safe Mode sign-in twice.
- **Fired on the Hyper-V rig** (`rig/hyperv/prologue.sh storage-mode`): the
  copy boots Safe Mode exactly once, the RunOnce restarts it at sign-in, the
  SYSTEM resume scans 7 s after the normal boot, and the cleanup reads back
  clean. Rows 2-3, `no-intel-controller` / `flow_result=mode-unchanged`,
  plumbing only. RISKS R1 has the run-by-run findings, including that Task
  Scheduler will not run the task in Safe Mode, and that `bcdedit /copy`
  puts the copy on the boot menu.

**Physical runs (2026-09-15, the Aspire, rows 6-9).** The one-click flow
fired twice on real firmware: Safe Mode through the copied entry, the
sign-in marker, the SYSTEM resume 21-27 s after boot, cleanup. Both runs
ended `mode-unchanged`, because the setup screen was never reached. The
InsydeH2O V1.21 firmware ignores the boot-to-setup request (it refused it
once with error 203, and accepted-and-ignored it once), and F2 was not
caught in time. The RAID row is still owed. RISKS R1 has the run-by-run
record and the 2026-09-15 decision to stop, unless a one-minute F2 look
finds SATA Mode on this machine.

**Pass.** Both directions on at least one physical machine, with the list
reconciled against the kernel's. The reconciliation and the level-3
plumbing are done. **What remains is the physical machine.**

- The Aspire can give the **pre-VMD RST clause** (signal 3, class `0104`,
  `8086:282a`), if its setup exposes SATA Mode.
- **VMD proper** (signals 1-2) still takes an 11th-gen or newer Intel
  laptop with RST on. Its first scan is the FAIL row as shipped, then OK
  (or `warn-rst-on-ahci`) after switching to AHCI. The same half-hour visit
  as V0's.
- The G16 (AMD, standard NVMe) cannot exercise the positive path, and
  neither can the spoof or the rig. The synthetic capture is the residue's
  regression test, not a substitute for it.

**If it fails.** Fix the list and run it again. This one has no fallback
because it has no excuse: it's cheap.

## V6: A signed binary can earn Defender's tolerance · kills: distribution · RISKS R12

Not a code question, a calendar question. Microsoft's SmartScreen (the
"Windows protected your PC" screen) builds its trust in a signed program
over elapsed time. So this validation *is* the fix.

**Experiment.** Start now:

1. a legal entity and an OV (organization-validated) certificate;
2. sign something trivial (the scanner wrapped in an exe is perfect);
3. submit it to Microsoft's malware-analysis portal;
4. distribute it modestly, and measure SmartScreen's behaviour monthly.

By the time the converter exists, we'll know whether signed + submitted +
aged is enough, or whether an EV certificate or store distribution is
needed.

**Pass.** The signed test binary downloads and runs on a stock machine
without SmartScreen stepping in.

**If it fails.** An EV (extended validation) certificate, Microsoft Store
packaging, or distribution through repair-event channels, with humans who
can click past warnings. All slower, all workable, all better known a year
early.

## V7: The scanner generalizes beyond one laptop · kills: the knowledge-base model · RISKS R2

Every check was verified on one ASUS G16. The community-table model only
works if reports from unfamiliar machines mostly confirm the tables.

**Experiment.** Ship the scanner publicly (it's ready apart from the R4
wording rework) and ask for reports: at least one Intel laptop, one
Broadcom machine, one pre-2015 machine, one BitLocker-on machine, one
Surface. This also feeds V4 for free.

**Pass.** Reports arrive and the verdicts survive contact, or the failures
are table gaps (one-line fixes) rather than logic failures.

## V12: The window in front of the scripts · kills: the non-technical front door · RISKS R31

Decided 2026-09-27 (the owner): `UPGRADE.exe`, a Rust window that runs the
kit's scripts from the stick and opens again after the restart.
`[##..]` built, untried.

**Built 2026-09-27 (window 0.1.0, verify flow only).** The flow logic has
19 tests on Linux: every call matches `RUN-VERIFY.cmd` argument for
argument, RED and a missing verdict stop, refusals are read in the
scripts' words, and the result screen reads `verify.json`,
`refusal.json` and the return check's row. On the G16 it starts
(`--version`) and draws its screens (`--preview`, which runs nothing), and
Windows' task scheduler accepts its sign-in task (registered once without
administrator rights under a test name, then deleted). None of that is
the flow on a machine.

**Rig run 1 (2026-09-27, window 0.1.1; `v12-window.csv` line 2,
`stopped-before-arm`).** On the Hyper-V guest (basic display adapter,
OpenGL 1.1 only) the window could not open with OpenGL, logged that to the
stick, started again with wgpu and drew. Start was pressed from the guest's
keyboard. The scan ran, then the job writer refused: "the stick is on bus
'SAS', not USB". The window showed that sentence, said nothing was
changed, armed nothing and left nothing behind (no task, no state, no
`bootsequence`). That refusal is right (R16: the stick must be USB), and it
means **the rig cannot take the window past the job writer**: Hyper-V has
no USB, which is why `v1.sh` has always written the rig's job on the host.
The restart, the reopen at sign-in and the result screen need the Aspire
(step 2). Found and fixed in the bench on the way: a window started by a
task does not get the keyboard focus (`v12.sh` now brings it to the
front), and the bench must read only the window's own log lines.
Before this, 0.1.0 on the same guest could not draw at all and wrote
nothing to the stick (screenshots only, no row); 0.1.1 fixed both.

**Method.** In order:

1. **The rig.** The verify flow from the window on the Hyper-V guest,
   Secure Boot on: scan, job, kickstart, arm, the live boot, the return,
   the window opening at the next sign-in and showing the result. Plus the
   stops: a kit with a file missing, a RED scan (a recorded RED machine or
   an injected one), closing the window before the arm.
2. **The Aspire.** The same flow on real firmware, as a physical row.
3. **A machine on the basic display driver** (or a VM without 3D): the
   window cannot draw, and the message box points to the `.cmd`.

**Pass.** Steps 1 and 2 end with the window showing the same result the
return check recorded, with nothing armed or registered left behind; step
3 shows the fallback, not a blank screen or silence.

---

## V14: The kit's Linux release starts where it is used · kills: the one click, and after an erase, the machine · RISKS R34

Decided 2026-10-03 (the owner): the release is chosen from what the
machine accepts (`architecture.md`, "It chooses the Linux release").
`[##..]` built, partly fired.

| Step | What | Status |
|---|---|---|
| 1 | The scanner reads the SBAT level and judges the stick's boot files | `[####]` fired on the Aspire 2026-10-03 over SSH: Fedora 42 files FAIL (`grub,3 < grub,5`), Fedora 44 files OK |
| 2 | Release facts measured from the files (`measure-release.py`) | `[###.]` Fedora 44 measured and committed (47bc8cf); Fedora 42 owed (a record only) |
| 3 | The scanner judges every release, including the db's authorities; the job writer refuses an unstartable one | `[##..]` self-tests only |
| 4 | `make-kit.sh --release fedora-44`: the kit carries Fedora 44, every file checked against the table | `[###.]` built and used on the rig; both new scanner checks OK inside the rig guest from the stick |
| 5 | The rig: V9 arm C and a settle-in run on the Fedora 44 kit | `[###.]` arm C erased-installed (Secure Boot off: Hyper-V cannot trust both authorities); the stick's chain and the installed system booted with Secure Boot on under the third-party CA on a second VM; Plasma Setup found and fixed (C2, settle-in run 16) |
| 6 | The Aspire: the scan says Fedora 44 starts; the stick boots; the installed system boots, Secure Boot on | `[####]` 2026-10-04, run 10: all three held (the run itself is a `fail` row for R35, not for this). Held again in run 11 the same day (kit aba09ec), a passing run |

**Residue that only a real machine closes:** what a firmware actually
holds (the level and the db) and whether it refuses exactly as shim's rule
says. The rig's Hyper-V firmware has its own level and keys; a pass there
is plumbing.

## V13: The port from PowerShell to Rust · kills: trust in every row the Windows side earned · RISKS R32

Decided 2026-09-27 (the owner): Rust becomes the conversion's one
language, piece by piece (`architecture.md`, "Stack"). The roadmap is
`docs/RUST-PORT.md`. Steps 1 and 2 `[##..]` built (2026-10-04), with the
replay half of step 3; the rest `[#...]` planned.

**Decided (2026-10-04, the owner):** the whole port is built now on the
branch `rust-port`, and the project cuts over to Rust after one success of
the PowerShell process on `main`. This gate's order at the cut-over: every
`selftest`, `corpus` and differential line `pass` first (on the branch),
then the `rig` lines re-run with the Rust build, then the `physical` ones.
A line is never marked `pass` because its PowerShell row passed.

**Where it stands (2026-10-04).** `./port-check.sh` runs the whole check:
the two PowerShell self-tests and `schemas/check.py`, that the recorded
files are fresh, that every self-test case has a Rust case under the same
name, and the Rust tests.

| Piece | Ledger lines | `pass` | `owed` |
|---|---|---|---|
| `schemas/check.py` (step 1) | 103 | 103 | 0 |
| `upgrade-scan.ps1` | 118 | 108 | 10 (`v5-controller-mode.csv` lines 2 to 10, `v1-live-boot.csv` line 4: the storage-mode rows need the rig's both-modes harness run with the Rust build, and the Aspire's RST driver is gone with its fresh Windows). Its own rows: `v13-rust-scanner.csv` |
| `Harvest-UpgradeState.ps1` | 49 | 47 | 2 (`v8-materialize.csv`; `harvest-folder-map.csv`) |
| `New-Job.ps1` (judging half) | 130 | 126 | 4 (the rig and physical rows that ran its jobs) |
| `New-Kickstart.ps1` (step 4, first piece) | 26 | 22 | 4 (the rig and physical rows whose installs ran its kickstart: `v1-live-boot.csv`, `v2-install.csv`, `v9-erase.csv`) |
| `Read-Password.ps1` (2026-10-07) | 15 | 12 | 3 (`v9-erase.csv` lines 11, 22, 30: the Aspire's runs where the person typed the password through it; one physical run with `upgrade-job password` re-earns them) |

**2026-10-07, the cut-over sessions** (`docs/CUTOVER-PROMPT.md`): the
scanner is a product command (`upgrade-scan scan --json --out`, proven on
the G16 against `upgrade-scan.ps1 -Json -OutDir` in the same minute,
elevated and not: JSON SAME field for field, text byte-identical;
`v13-rust-scanner.csv` line 7), and the password hasher is ported (34
recorded calls, the specification's vectors). Two differences kept on
purpose, both stricter, in `docs/RUST-PORT.md`.

The scanner's rig and physical rows are not all found yet. Before step 3
starts, every results file is read again for rows the scanner had a part
in, and each gets its `owed` line.

**The parity ledger.** `docs/validation-results/port-parity.csv` lists
every piece of evidence a PowerShell piece has earned, one line each, and
the Rust test that must replace it:

| Column | Meaning |
|---|---|
| `piece` | the PowerShell file being ported, e.g. `upgrade-scan.ps1` |
| `evidence` | what it passed: a self-test case name, a corpus file, or a results file and line (`r18-prologue.csv:6`) |
| `kind` | `selftest`, `corpus`, `rig` or `physical` |
| `rust_test` | the `cargo test` name, or the harness and row that re-earned it |
| `result` | `owed`, `pass` or `fail` |
| `date` | when `result` last changed |

A piece's lines are written in full **before** its port starts, so the
ledger says what must be matched, not what happened to be tested. A
PowerShell piece is retired only when all its lines read `pass`.

**Method, by kind:**

1. **`selftest` and `corpus`:** port the case as a `cargo test`, fed the
   same made-up object or the same recording. Same verdict, same fields.
2. **Differential:** while both exist, run PowerShell and Rust on the same
   input (the corpus, a rig guest, a captured tool output) and require the
   same result, word for word where it is a refusal.
3. **`rig`:** re-run the same `rig/hyperv/*.sh` harness with the Rust build;
   it appends its row as today.
4. **`physical`:** where the run kept the raw output of the tools it called,
   a replay test closes the decision; the effect on the firmware or the
   disk still takes one re-run on the machine. One Aspire run can close
   many lines.

**Pass.** Per piece: every ledger line `pass`, and the release rebuilt
byte for byte by a second person from the tagged source (R14).

---

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

# Dependency map: what is blocked on what

| Waiting on | Blocked work |
|---|---|
| V0 + V1 + V1b (the spine spike) | everything in `upgrade_/` and `settle-in/`; the live image; the kickstart generator beyond a stub |
| V1b specifically | the default keep-Windows cutover (alongside install); if it fails on a machine, that machine is clean-slate-only |
| V2 | the artifact-extraction pipeline's scope (build order step 2) |
| V3 | the intent-capture UI's path logic; the settle-in file pull |
| V4 | stick-size guidance; intent UI weighting (ship scanner change now) |
| V8 | the settle-in file pull's integrity guarantee; `settle-in` never copying a stub as the file (online-only files stay in OneDrive, decided 2026-09-26) |
| V11 | the "Go back to Windows" program (walk-away since 2026-09-29); until it passes, the launchers' line promises only what is proven |
| V14 | every kit build (its release must be one the target machines can start); the Aspire's run 10 re-run (done 2026-10-04, run 11) |
| V13 | retiring each PowerShell piece; the cut-over to Rust (decided 2026-10-04: after one success of the PowerShell process on `main`) |
| V5 | nothing: do it this week regardless |
| V6 | nothing: start the clock now; blocks only the eventual release |
| V7 | table confidence; multi-distro ambitions |

**Decided (2026-09-08):** the dependency map is now carried out as a single
front-to-back, one-click **vertical**. The reversible half comes first
(schemas → V8 materialization → stick writer → live image → hardware check →
back to Windows), the destructive half second. It is built on the Hyper-V
rig, then on a physical machine we own, never a borrowed one. Borrowed
vendors get half-hour read-only visits that fill a whole column of the
matrix. The reasoning and the split live in `architecture.md`, "Build
order".

V5 and V6 start immediately, because they cost an afternoon and a calendar
respectively. V0 + V1 + V1b are the spine spike: one VM build-order item,
now including the alongside install that keeps Windows bootable. V3 and V8
are bench tests that can run in parallel. V4 and V7 ride the scanner's
public release.
