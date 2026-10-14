# upgrade_ architecture

How the thing works, and the dated decisions behind it. Where each piece stands:
[STATUS.md](STATUS.md). What is unproven: [RISKS.md](RISKS.md). What comes next:
[ROADMAP.md](ROADMAP.md). The project has three modules:

```
   evaluate            upgrade_             settle-in
   ---------           ---------            ---------
   read + decide       do the thing         confirm + hand over
   Windows             Windows -> Linux     Linux
   reversible          contains the         after the fact
                       commit line
```

The modules are split by **commitment**, not by operating system: "can this step
still be undone?" An earlier draft split them by OS (Windows / live USB / first
boot); that put reversible and irreversible work in the same phase and hid the
one thing the person cares about, whether they can still change their mind. **On
the name:** the middle module's directory has the project's name on purpose,
because the conversion *is* the product. In prose, "the converter" is the module
and "the project" the whole.

---

## The commit line

Exactly one moment in a conversion cannot be undone: **the first destructive
write** (the first time something on the disk is overwritten or deleted). A
one-way door. Which moment that is depends on the path the person chose when
`evaluate` captured their intent:

- **Clean-slate path** (files are copied to the stick first): the disk wipe,
  inside the cutover. Everything before it only adds things; if anything fails
  before the wipe, the machine boots Windows again.
- **Safety-copy path** (files stay on the internal disk): the **reclaim**,
  meaning deleting the Windows partition, in `settle-in`, only after the checks
  pass and the person clearly says yes. Until reclaim, Windows is there and
  boots; rolling back means restoring a boot entry, not a disk image.
- **Erase path** (nothing kept): the end of a two-minute countdown on the stick
  ("Erase and install", below).

Two rules follow. They are not negotiable:

1. **The screen must say "you can still cancel" right up to that exact moment**,
   and must stop saying it the instant the line is crossed.
2. **Everything that could refuse must refuse before the line:** identity,
   checksum, capacity and hardware refusals; on the clean-slate path, a
   live-session hardware check with a person present too, since there is no
   rollback beyond it.

---

## Module 1: evaluate

**Runs in Windows. Read-only. Can be run again and again. Never changes
anything.** Its output is a complete, validated **job spec** (`job.json`,
versioned) plus an `artifacts/` directory; the test for "done" is that
`upgrade_` can run it start to finish with no further human input. Where it
stands: see [STATUS.md](STATUS.md).

### It is the last moment Windows exists

Some things the finished Linux system needs exist *only* inside the Windows
install. Laptops from 2023 onward with Cirrus "smart amplifiers" (speaker chips
that need their own firmware) need firmware files ("blobs") that ship inside the
Windows driver package; wipe first, and the speakers are silent with no way to
fix it. So `evaluate` pulls out files (artifacts), not just facts:

| Artifact | Why it can't wait |
|---|---|
| Vendor audio firmware (Cirrus CS35L41/56, TI TAS2781) from the driver store | Gone with Windows; the fix for silent speakers |
| BitLocker recovery key | Can't be recovered later; unlocks the data volume on the safety-copy path, and avoids a lockout mid-conversion |
| Wi-Fi profiles and PSKs (the network passwords) | Needed to reconnect; the person may not know their own password |
| Browser profiles | Bookmarks, history, extensions, Firefox passwords |
| OEM firmware/ACPI quirks (maker-specific hardware settings) | Shipped by the maker, Windows-only |

```text
evaluate extracts  ->  the USB carries  ->  upgrade_ injects before first boot
```

### It captures intent, not just state

If any decision is missing, the converter has to stop and ask a person, and
walk-away is dead. So `evaluate` also collects what the computer starts at (KDE
desktop, GNOME desktop or the text console), the account name, and the password
(hashed straight away to SHA-512 crypt, a one-way scrambled form that Linux can
check but nobody can read back).

**The conversion path is not a coin flip.** Keeping Windows aside is the safer
net, so it is the **default whenever the disk has room for it**. Clean slate
(wiping Windows and copying files to the stick) appears only when the person
clearly wants Windows gone, or as the forced fallback when the disk is too full
to keep both; it needs the person present for a two-minute hardware check in the
live session, because it has no rollback. Keep-Windows is true walk-away, with
the files coming over later.

**Amended (2026-09-22, RISKS R18):** the forced fallback happens only when the
person's pre-chosen fork allows it (`fork.if_cannot_keep = clean-slate`). Under
`stop`, the job writer never writes a clean-slate job: if the disk and the ESP
allow keeping Windows, the job is keep-windows and the prologue's own re-measure
decides; if not, there is no job, and the person is told why before CONVERT. The
prologue refuses a stale forced clean-slate job under `stop` the same way, and
refuses clean slate whenever the job lists no folders or nothing was staged. A
"stop" is never turned into a wipe.

### It harvests what only Windows can give

- **The folder map.** *Where* the person's files really live, looked up through
  Windows' known-folder APIs (the official way to ask "where is this person's
  Documents folder?"), so a OneDrive redirect is followed, not guessed.
  `settle-in` reads it off the stick (`harvest.folders`; `harvest.stick_fit`
  says whether they fit the stick). The job writer refuses when the map could be
  wrong: the elevated account is not the one on screen; a folder hit the file
  cap or holds sub-folders Windows would not list (RISKS R6); or, on the clean
  slate, the folders do not fit the stick or other profiles exist (R5).
- **Which files are only in the cloud.** OneDrive's "free up space" placeholders
  are 0-byte stubs: a name with no contents. Linux has no OneDrive client to
  fill them, so read from the mounted NTFS later they copy over *empty* (RISKS
  R8). **Decided (2026-09-26, the owner's call):** they are not downloaded.
  `evaluate` counts them per folder and records `cloud_files.result =
  left-in-cloud`; `settle-in` skips them and reconnects OneDrive instead. (The
  harvester's `-Materialize`, which downloads them, stays built and unused.)
- **Which Windows this was, and how it was activated** (decided 2026-09-27, the
  owner; RISKS R30), for the way back. `harvest.windows_license` records the
  edition (Home or Pro), 10 or 11 (from the build number: Windows 11 still calls
  itself "Windows 10" in the registry), whether it was activated, the licence
  channel (`OEM:DM`, a key the maker put in the firmware, or `Retail`), and
  whether the firmware holds a product key. **Never a key itself.** A firmware
  key stays in the firmware, where Windows' installer finds it again; a typed
  key is the person's to keep, and the stick is not a credential store (RISKS
  R13). A read that fails is recorded with its reason, not refused.
- **The software inventory.** Every installed desktop program (the registry's
  Apps & features entries, minus updates and hidden components) and every Store
  app, names and versions only, as `harvest.software`, for `settle-in`'s "you
  had these" list with the Linux or web equivalent named for each. Personal
  data: it travels only in `job.json` on the stick, never in a report.
- **The clock, the Wi-Fi networks and SSH:** Module 3 and "What migrates".

### It writes the stick

`evaluate` makes the USB stick itself: the live image plus an exFAT staging
partition (a separate section of the stick for copied files). The person never
meets an ISO or a burning tool (an ISO is also published for technical users and
repair events). Raw disk writes are the one place this tool could destroy data
*before* the commit line, so picking the device is defensive: removable-bus
devices only; size and volume label confirmed with the person; refuse if
anything is unclear. The rules are a pure function with a read-only `-Plan`, and
the write path finds the device again by its unique id before the first
destructive call (RISKS R16).

### It refuses

Every refusal lives in `evaluate`, the only module that can refuse for free:

- **RED scanner verdict.** No override flag, ever.
- **Neither path fits:** the data is too big for the stick AND there is too
  little shrinkable space to keep it in place. The refusal is a **gap report**:
  how many GB to free (largest folders listed) or what stick size would change
  the answer, so running `evaluate` again moves toward a yes.
- **More than one user account** (RISKS R5): moving one and silently leaving the
  rest behind is a data-loss bug. The harvest schema is shaped per user, so
  multi-user support is an extension, not a rewrite.
- **Folder sizing was cut short**, so the staging estimate is untrusted (R6).
- **RST/VMD active** (Intel storage modes that hide the disk from Linux); cannot
  be automated, see "The handoff".
- **The shared ESP cannot take the alongside install** (keep-Windows path only;
  decided 2026-08-30, RISKS R21). The ESP is the EFI System Partition, the small
  FAT partition the firmware boots from. The gate is **at least 32 MiB free on
  the ESP** (the rig measured Fedora's footprint at 6.2 MB), and the ESP must be
  the FAT volume the firmware's *Windows Boot Manager* entry points at. A
  machine that fails it is steered to clean slate, never installed alongside.
- **No Linux release the kit carries can start here** (RISKS R34; next section).

**One exception, narrow and dated (2026-09-13, RISKS R23):** a person who has
already copied their files off a machine the scanner refused for its *drive*
(bad blocks, SMART media errors, a volume needing a full repair) may type a
fixed sentence on a separate launcher and go ahead. It lifts those two refusals
and no other, is recorded in `job.json` and `outcome.json`, and turns every
later screen red (CLAUDE.md rule #1).

### It chooses the Linux release from what the machine accepts (decided 2026-10-03, the owner)

Secure Boot keeps a list of boot programs too old to trust (the SBAT level).
Windows raises it by itself, so a fixed release in the kit breaks as soon as
Windows moves the level (RISKS R34). So the release is a choice:

```text
 the machine (scanner)              the releases (data/releases.json)      the kit (make-kit.sh)
 ---------------------              ---------------------------------      ---------------------
 Secure Boot on / off               one entry per release, MEASURED        --release ID: fetches
 SBAT level (registry +        ->   from its own files by              ->  that release's files,
   firmware SbatLevelRT)            data/tools/measure-release.py:          refuses any byte that
 the authorities in the db          each boot file's SBAT data, a           differs from the entry,
   (2011, 2023)                     shim's built-in levels, who signed      writes release.json and
                                    it; every file pinned by sha256         the installed boot facts
```

The scanner judges every release in the table (check *Linux releases*): each
boot file, on the stick and in the installed system, against the level (the
strictest of every source, plus the release's own shims' levels), and each shim
against the authorities in the firmware's db; and checks the actual files on the
stick (check *Secure Boot revocations*). Both are RED when nothing can start;
the R23 sentence lifts neither. The job writer reads `release.json` on the stick
and refuses a release the scan did not find startable (no, unknown, or not
judged). A new release or distribution is a table entry, measured from its
files, never typed; one without a Microsoft-signed shim (Arch, so Omarchy,
today) is still worth an entry, and the scanner then says plainly that it cannot
start with Secure Boot on. Installing it also needs installer automation of its
own (today only Fedora's kickstart exists) and a rig run.

---

## Module 2: upgrade_

**Starts in Windows, finishes in Linux. Contains the commit line.** Where it
stands: see [STATUS.md](STATUS.md).

### Two paths, one stick

There is no external drive anywhere in this design. Everything travels on the
USB stick, or never moves at all:

|  | keep Windows (default) | clean slate (opt-in / fallback) |
|---|---|---|
| User files | never leave the internal disk; pulled in `settle-in` from the untouched Windows partition | copied to the stick's exFAT partition, restored during cutover |
| Qualifies when | shrinkable space >= Linux (~20 GB) + headroom | data + artifacts fit the stick |
| Windows afterwards | intact until `settle-in` reclaims it | gone at the wipe |
| Rollback | full, until reclaim | none: the wipe is the commit line |
| Walk-away | total | after a 2-minute human check in the live session |

**Keep-Windows is the default** wherever the disk fits it, because a working
Windows to fall back on is the better safety net. On that path **no user data is
read out of Windows during the destructive part of the conversion at all:**
cutover installs Linux into the freed space and touches nothing of the person's;
the files come across afterwards, in `settle-in`, with Windows still whole. That
moves the encrypted-read risk (RISKS R19) out of its old corner: unattended, the
only copy, pre-wipe.

### When Windows cannot be kept: the offer to discard it

**Decided (2026-09-26, the owner's call): designed; not built.** RISKS R26.

A real disk can fail to free the 25 GB Linux needs even after every mitigation
(RISKS R18), leaving the person two bad outcomes: stop, or a wipe chosen before
anyone knew the number. So the fork gets a third value, and the wipe is asked
for only once the answer is known:

1. **At CONVERT, the fork is asked plainly:** if Windows cannot be kept, *stop*
   (the default) or *ask me then* (`fork.if_cannot_keep = ask` in `job.json`).
   `clean-slate` stays, for a person who wants Windows gone from the start.
   Nothing ever wipes on a pre-chosen answer made without the number.
2. **When the shrink cannot fit, the prologue stops exactly as it does today**
   (mitigations put back, C: as it was, the resume task removed, a stopped
   `outcome.json`), and registers the question for the next sign-in instead of
   the plain stop notice (the resume, in session 0, cannot show a window).
3. **The question is a window with the real numbers,** measured again when it
   opens: what the drive could free and what Linux needs; the folders that would
   be kept, each with its size (online-only OneDrive files listed as staying in
   OneDrive; RISKS R8); the stick's free space; and what is lost, in plain
   words: every program, Windows' settings, every file outside those folders.
4. **If the files do not fit on the stick, there is no offer,** only the gap, in
   GB, and "a stick of at least N GB would".
5. **Consent is a typed sentence, word for word**, like R23's and separate from
   it: *"I confirm that Windows will be deleted and only the listed folders will
   be kept"*. "Keep Windows" is the default button, and closing the window means
   "keep Windows". On a RED machine the R23 sentence is still typed as well;
   neither sentence stands in for the other or widens it (CLAUDE.md rule #1).
6. **A "yes" wipes nothing by itself.** It runs the normal chain again, elevated
   (one UAC consent): a new job (`intent.path = clean-slate`, `path_reason =
   user-chose-clean-slate`) carrying the typed sentence and the numbers shown;
   the kickstart; the prologue, which re-validates, measures the folders again,
   refuses if they no longer fit, stages them with per-file checksums at a
   measured speed, refuses on 0 folders or 0 files, and arms the handoff only if
   no Windows update is waiting (RISKS R25).
7. **The wipe is where it always was:** in the live session, after the identity
   check, the staged checksums read back (RISKS R17), the hardware checks, and
   the two-minute human check. "You can still cancel" is on screen until that
   moment (rule #3); cancelling boots back into an untouched Windows.

Stop stays the default at every step, and the extra prompt exists only on the
branch where keep-Windows failed. The build waits for the harvest and a first
physical install row (ROADMAP.md).

### Stage 1 - prologue (runs in Windows, reversible)

The prologue (`Invoke-Prologue.ps1`, behind the one-click `RUN-CONVERT.cmd`:
scanner -> job writer -> kickstart -> the typed word -> the prologue) is
everything the converter does in Windows before it hands the machine to the
stick. Live reads are kept apart from decisions (collect/judge seams) so the
decisions can be self-tested.

1. **Re-validate `job.json` against the live machine.** If anything changed
   since `evaluate` ran, it stops here.

   **1b. Clear the volume flag, if `evaluate` found one** (decided 2026-09-08,
   RISKS R18). Windows refuses to measure or shrink a C: that carries NTFS's
   "dirty flag" (a mark that says the volume needs a check) or a repair Windows
   has queued. `evaluate` never repairs; it records the flag and the fork the
   person chose in advance. Here, as reversible prep with its own restart:

   ```text
   read-only online scan confirms the flag
     -> refuse if the physical disk is not `Healthy`
     -> Windows' spot-fix, or a full `chkdsk /f` only when the scan
        logged real errors
     -> restart
     -> re-measure shrinkable space
     -> take the fork the user chose in `evaluate`
        (keep Windows if it fits; else clean slate, or stop)
   ```

   The check's real outcome goes into `outcome.json`. Every restart in this
   stage resumes as SYSTEM with **nobody signed in**; anything a person should
   read is queued for the next sign-in (decided 2026-09-13; "The walk-away
   resume", R24).
2. **Keep Windows (default): shrink C: with `Resize-Partition`,** Microsoft's
   own code path, the most-tested NTFS resize there is, and it works with
   BitLocker still on. Only artifacts are copied to the stick. Shrinkable space
   is measured by two independent read-only paths; the request is exactly what
   Linux needs (`linux_min_gb` plus the harvested bytes x 1.2), never the
   maximum, and the kept Windows must keep 8 GB free (decided 2026-09-12). When
   the cold measurement does not fit, a ladder of reversible mitigations runs,
   all behind the typed CONVERT:
   - **Hibernation is switched off on every keep-Windows conversion** (decided
     2026-09-12; the kept volume must be mountable later). **The pagefile is
     turned off only when the measurement does not fit**, with one restart to
     re-measure. **Any stop puts both back as the prologue found them** (0.5.1,
     2026-09-20; recorded before they are touched). The return after an install
     brings the pagefile back; hibernation stays off.
   - **Restore points that pin the shrink are deleted** (0.6.0, decided
     2026-09-20) when Windows names their storage as the last unmovable file and
     the job carries consent (`fork.restore_points_consented`, asked in plain
     words on the launcher): the one pre-commit act a stop cannot undo (R18).
   - **NTFS's change journal that pins the shrink is deleted** (0.8.0, decided
     2026-09-22) on the same terms (`\$Extend\$UsnJrnl:$J` named as the last
     unmovable file; `fork.usn_journal_consented`). At most once per boot, then
     one re-measure; it is created again right after the shrink and at every
     stop. What does not come back is the record of changes it held: search and
     sync programs look through the files again.
   - **A Windows update waiting for a restart is let finish first** (0.9.0,
     decided 2026-09-26, RISKS R25). Checked before anything changes and again
     before the shrink; the prologue restarts itself (at most three times) to
     let it finish, and never arms the boot to the stick while one waits.

   A stop after the shrink grows C: back, re-enables BitLocker, removes the boot
   entry and scrubs the stick's credentials (decided 2026-09-12).

   **Clean slate (opt-in / fallback):** copy user files and artifacts to the
   stick's exFAT partition with per-file checksums; 0 folders or 0 files is a
   refusal. The write speed is measured and shown as a **computed** time
   estimate: cheap flash at 20 MB/s is not a ten-minute job, and the person must
   learn that before walking away. Windows reads its own BitLocker volume, so
   encryption never enters this path.
3. **Hard confirmation:** type the word, not a checkbox.
4. **Suspend BitLocker, write the one-time boot entry, restart.** Decided
   (2026-09-07, from the V0 harness): if the BitLocker state cannot be
   determined (through the PowerShell module, or `manage-bde` on editions
   without it), the prologue **refuses** rather than guess whether the return
   boot will stop at a recovery-key prompt.

Still fully reversible; even the shrink can be undone by growing the partition
back. The prologue's record (`upgrade_/prologue.json`) is what `%post`'s
`outcome.sh` carries into `outcome.json` as the `prologue` block.

#### The handoff

`upgrade_` creates a one-time UEFI boot entry, so the person never touches a
firmware menu. `bcdedit` is Windows' built-in tool for editing boot entries:

```
bcdedit /copy {bootmgr} /d "upgrade_"        -> returns {guid}
bcdedit /set {guid} device partition=<usb>:
bcdedit /set {guid} path \EFI\BOOT\BOOTX64.EFI
bcdedit /set {fwbootmgr} bootsequence {guid}  -> next boot only
```

`bootsequence` applies to the next boot only, so any failure leaves the machine
booting Windows normally (RISKS R15). **Never assume the firmware starts the
right thing after an install** (RISKS R35): the stick itself decides what it
does when started again (`upgrade_/converted`, above).

**BitLocker must be suspended first** (`manage-bde -protectors -disable C:
-rebootcount 1`): changing the boot configuration breaks the TPM's measurement
of the boot and triggers a recovery-key prompt, on a machine nobody is sitting
at. **RST/VMD cannot be automated:** no portable API exists, the settings live
at maker-specific places in UEFI setup variables, and writing them blindly
bricks machines. `evaluate` hard-refuses, for good.

### Stage 2 - cutover (runs in the live environment)

After the machine has booted from the stick:

5. Check the machine's identity against `job.json` again: same disk serial, same
   firmware mode. A mismatch means the USB was moved to another computer:
   **abort.** Do not partition a stranger's laptop.
6. Check every staged checksum by reading it back from the stick. This is also
   the fake-flash test (RISKS R17): a stick that lied about its capacity fails
   here, while Windows still exists.
7. Automatic hardware checks, before anything is destroyed: Wi-Fi connects, the
   display runs at native resolution, amp firmware loads.

**Keep Windows** (the default) is unattended, and does nothing destructive:

8. Create partitions in the freed space. The Windows partition and the existing
   ESP are never reformatted. Fedora's bootloader is added alongside
   `bootmgfw.efi` (Windows' boot loader), which keeps rollback a boot-menu entry
   rather than a restore. **Before touching the ESP, snapshot `EFI/Boot` and
   `EFI/Microsoft` (every file, with checksums) and the firmware's `Boot####`
   entries to the stick.** Decided 2026-08-30 (RISKS R21): "added alongside"
   still replaces one file. Fedora's shim (the small signed loader that lets
   Linux boot with Secure Boot on) overwrites the fallback loader
   `EFI/Boot/bootx64.efi`, **kept on purpose** while Windows is kept: with shim
   in the fallback slot, a firmware that loses its NVRAM entries still reaches
   GRUB and both systems; with Windows' copy there, Windows only. The snapshot
   lets rollback put Windows' copy back.
9. Install via kickstart (the answer file that lets Fedora's installer run with
   nobody at the keyboard) with `--onpart`, touching the new partitions only;
   `/boot/efi` is the existing ESP with `--noformat`. os-prober (which finds
   other operating systems for the boot menu) is **switched on explicitly**
   (`GRUB_DISABLE_OS_PROBER=false`), whatever the distro's default, so Windows
   is in the menu because we put it there.
10. Inject artifacts (vendor firmware, drivers) into the installed system
    *before* first boot, so the first impression is working hardware.
11. **Check the boot chain, then** write `outcome.json` and logs to the stick
    and reboot into Linux. The check (decided 2026-08-30, RISKS R21) is a
    checklist, not a hope: a *Windows Boot Manager* firmware entry pointing at
    `\EFI\Microsoft\Boot\bootmgfw.efi` on the shared ESP (re-created with
    `efibootmgr` if the firmware dropped it); the Linux entry first in
    `BootOrder`; `bootmgfw.efi` matching the step-8 snapshot; `grub.cfg` listing
    the Windows entry. Each result is recorded in `outcome.json`; nothing here
    is destructive, so a failed check does not abort but is written down for
    `settle-in` to show. GRUB's Windows entry is the installer's own, not
    os-prober's: it chainloads `bootmgfw-kept.efi` if present, else
    `bootmgfw.efi`.
12. **On firmware that ignores operating-system entries, shim goes into Windows'
    slot** (decided 2026-10-10, the owner; RISKS R21). Some firmware (Acer's
    InsydeH2O) boots only from its own priority list, where the one
    operating-system entry is "Windows Boot Manager", so the machine starts
    Windows with no menu. The prologue's return on the kept Windows is the
    detector: the install completed, Fedora was left first, and yet Windows
    booted with nobody pressing a key. On that evidence it moves `bootmgfw.efi`
    one name aside (`bootmgfw-kept.efi`, same folder, so Windows' boot manager
    still finds its BCD), copies Fedora's signed shim to `bootmgfw.efi`,
    registers a `guard` startup task in the kept Windows (Windows' servicing
    puts its own file back; the guard re-applies the arrangement, so an update
    costs one Windows boot), and restarts once. Firmware that honours the entry
    never gets this; the rollback and the way back undo it.
    **Amended (2026-10-11):** "with nobody pressing a key" is GRUB's word: the
    "Windows (kept)" entry writes `upg_windows_chosen=1` into GRUB's environment
    block on the ESP before it chainloads (kickstart 0.9.0); the return reads
    and clears it, and a set flag means nothing is arranged (RISKS R21).

**The person's files are not touched here.** No NTFS read, no BitLocker unlock,
no copy. They come across in `settle-in`, with Windows still there as a complete
fallback; the commit line waits there, at reclaim.

**Clean slate** (opt-in, or forced when the disk is too full to keep both) is
the only path that wipes, so the person is present, by design:

8. The live desktop asks for two minutes: speakers (*not headphones*), display,
   Wi-Fi. A human ear is the only test for a smart amp, and this is the last
   moment the system can refuse for free.

> **=== COMMIT LINE (clean slate) ===** everything below destroys something

9. Wipe, partition, install via kickstart with the chosen desktop.
10. Inject artifacts into the installed system before first boot.
11. Restore files, Wi-Fi (as NetworkManager keyfiles) and browser profiles.
12. Wipe the credential part of the stick, but **leave the staged files**: until
    the person says otherwise it is their only backup, and `settle-in` says so.
13. Write `outcome.json` and logs to the stick (the only telemetry; it stays
    there unless the person chooses to send it). Reboot.

**A note on a reversal.** An earlier revision rejected any single-disk design,
because an unattended resize-and-move loses both the original and the copy in a
power cut. The keep-Windows path is not that design: the resize is Windows' own
shrink, the files are *copied* later, not moved, the original NTFS data is never
changed, and nothing is deleted until the person says yes at reclaim.

### Rollback is a mode of upgrade_, not a fourth module

On the keep-Windows path, rollback means restoring the Windows boot entry. It
takes seconds, and can be reached from the stick and from `settle-in`.
Concretely (decided 2026-08-30): put the Windows Boot Manager entry first in
`BootOrder` (re-creating it if missing); restore Windows' own
`EFI/Boot/bootx64.efi` from the step-8 snapshot; leave the Linux partitions and
`EFI/fedora` in place until the person asks for the space back. A boot-order
change, not a deletion. The Windows-side tool (`Invoke-Rollback.ps1`,
`ROLLBACK.cmd` on the kit stick, run from the kept Windows) reads the job, the
outcome and the snapshot; refuses on a moved stick or a path that kept no
Windows; restores `EFI/Boot/bootx64.efi` from the snapshot with its checksum
checked (the shim copy is saved to the stick); puts `{bootmgr}` first with
`bcdedit`; and records `upgrade_/rollback.json`. The Linux-side twin
(`efibootmgr`) belongs to `settle-in`. Owning rollback here is deliberate:
recovery paths nobody owns get discovered by the person whose laptop is already
a brick. The clean-slate path has no rollback, which is exactly why it has a
human gate before its commit line; the honest fallback there is Microsoft's own
install media (the digital licence reactivates on the same hardware) plus the
stick's files.

### Design constraints: offline, and one stick, honestly sized



The live image carries Fedora as a squashfs (a compressed, read-only disk image)
on the USB, both desktops, so the desktop choice costs a question, not a
download. It does **not** install over the network: the machines most likely to
need converting are exactly the ones with Broadcom Wi-Fi, or a card too new for
the shipped kernel. The images are the unmodified `LiveOS/squashfs.img` out of
Fedora's own Workstation and KDE live ISOs, installed with kickstart `liveimg
--checksum=`; the installer boot files come from the netinst ISO, also
unmodified. There is no image of our own making.



`evaluate` works out what the stick must hold (live image, artifacts, and on the
clean-slate path the staged files) and refuses a stick that cannot hold it, with
the gap report saying what would fit. `harvest.stick_fit` sizes the folders the
way the prologue's staging lays them down: each file rounded up to the volume's
clusters (the smallest block the file system hands out), plus the directories
and the manifest, or the prologue's own bytes x 1.02 if larger, plus 64 MB. A
FAT32 volume never fits a file over 4 GB. A clean-slate job must fit (the schema
refuses one that does not); a keep-Windows job records the answer for the
discard offer. The gate measures the volume actually used (today the FAT32 kit
volume, not the exFAT `UPGDATA` partition; ROADMAP.md). On that path the stick
is, briefly, the only copy of the person's files, so the read-back check in step
6 is a hard gate, not a warning.

## Erase and install: the one-click fresh start

**Decided (2026-09-26, the owner's call).** RISKS R27, VALIDATION V9.

The first destructive path proven end to end is the simplest: **erase every
internal drive and install Fedora, keeping nothing.** It proves the mechanism
(one click, one consent, a machine that comes back as Linux) before anything is
carried across; carrying files is stage 2. It is a `clean-slate` job whose
reason is `user-chose-fresh-start`, and it differs from the discard offer (R26)
in one way that matters: the person said up front that nothing is to be kept.

```text
 Windows (nothing on disk changes)          installer on the stick
 --------------------------------           ----------------------
 launcher: typed sentence, password,  -->   %pre: find each drive by
 desktop menu; job names the drives         identity + exact size,
 prologue: re-validate, let update          read image back (R17)
 finish (R25), arm one-time boot,                 |
 restart                                    2:00 countdown on screen
                                            any key -> back to Windows
                                                  |
                                     ===== COMMIT LINE (reaches 0) =====
                                                  |
                                            erase, install Fedora,
                                            %post writes outcome.json,
                                            reboot into Fedora
```

1. **Its own launcher, its own sentence.** `RUN-ERASE-AND-INSTALL.cmd` (for a
   RED machine, `RUN-ERASE-AND-INSTALL-ACCEPTING-DATA-LOSS.cmd`, which also asks
   for the R23 sentence; neither stands in for the other). The consent is typed
   word for word, first: *"I confirm that everything on this computer will be
   deleted and nothing will be kept"*. Then the Linux password (twice, hidden);
   the job holds only its SHA-512 crypt hash. **The person is told the account's
   name** (decided 2026-09-26, the owner): "Your Fedora account: <name>" on the
   password screen, and before the restart "Your Fedora sign-in: user <name>,
   password the one you just chose". The name comes from the job writer, so
   screen and job cannot disagree. **The person picks what the computer starts
   at** (decided 2026-09-26, the owner, after the Aspire's run 9 reached a text
   console nobody chose): KDE Plasma, GNOME, or text console (KDE, started at
   the console), as `intent.desktop` + `intent.start_at`; the kickstart follows
   it, and the outcome schema refuses a conversion that starts anywhere else.
2. **The job names every drive it will erase:** the internal drives (not USB) by
   serial, unique id and size in `erase_consent.disks`, each with a role: the
   drive holding C: is `system`; a second internal drive is `home` (so a failing
   system drive can be reinstalled without losing what is on the other). More
   than two internal drives, a `home` drive that is not Healthy, or a drive that
   cannot be identified is a refusal. (Decided 2026-09-26 on the Aspire: system
   on the SSD, `/home` on the 1 TB drive.)
3. **The prologue changes nothing on the disks:** it re-validates the job (both
   drives by identity), lets a waiting Windows update finish (RISKS R25), arms
   the one-time boot to the stick and restarts. Windows still boots if anything
   stops before the countdown ends.
4. **The installer checks, then counts down.** `%pre` (`verify.sh`, which runs
   inside the installer before it does anything) finds each listed drive by
   identity and exact size and refuses on any mismatch (a stick moved to another
   computer never erases it); reads the desktop image back against its checksum
   (RISKS R17); then shows a **two-minute countdown: "Erasing both drives in
   1:59 - press any key to cancel and restart into Windows."** Any key cancels:
   the report goes to the stick and the machine restarts into an untouched
   Windows. The moment it reaches zero is **the commit line**: `verify.sh`
   writes the time to the stick, writes `upgrade_/converted` and removes
   `upgrade_/boot-install` (a converted stick only ever hands over to the
   installed system, never installs again; RISKS R35), and the installer erases
   and installs. **Every refusal before the countdown is a plain screen**
   (decided 2026-09-27, the owner; `verify.sh` 0.5.0), not Anaconda's raw
   traceback, for 60 s (any key: now), then the computer restarts unchanged with
   `report/refusal.json` on the stick:

   ```text
   NOTHING WAS CHANGED ON THIS COMPUTER.

   The installer stopped before touching anything, because <reason>.

   It restarts in 60 seconds, exactly as it was before.
   Press any key to restart now.

   The details are saved on the USB stick, in upgrade_\report.
   ```

   The eight reasons, approved verbatim: this USB stick was prepared for a
   different computer, or one of its drives has changed since · the installer
   could not work out where to put Linux · Windows' startup files were not where
   the preparation found them · the copy of Linux on this USB stick is damaged ·
   a safety copy of Windows' startup files could not be made · this version
   cannot yet put your files back on an erased computer · the last-chance
   countdown could not be shown, and nothing is erased without it · this USB
   stick was prepared by a different version of this tool.
5. **After it:** Anaconda (Fedora's installer) clears both drives and installs
   Fedora (EFI, `/boot` and `/` on the system drive, `/home` on the home drive);
   `%post` writes `outcome.json` (`commit_line.crossed = true`, the countdown's
   end as `crossed_utc`, `act = wipe`) to the stick, and the machine reboots
   into Fedora with the person's account.

It carries no files, browsers or programs (stage 2) and keeps no Windows to fall
back on: the countdown is the last exit. Going back later means a new, empty
Windows ("The way back to Windows"), and the launcher says so before the
sentence is typed (draft words, awaiting the owner's approval):

```text
  If you change your mind later, you can put Windows back, but it will be
  a new, empty Windows: nothing on this computer today comes back. On many
  older computers that means Windows 10, which no longer gets free
  security updates.
```

---

## Module 3: settle-in

**Runs on Linux at first boot. Starts by itself, once.** It looks like a welcome
screen. It is a safety gate: it does not teach Linux, install applications, run
a tour or check in later. Checking the hardware and bringing the files home are
the two reasons it exists; the pull belongs here only because it is the first
safe moment (Linux proven, Windows intact). Where it stands: see
[STATUS.md](STATUS.md).

1. **Check the hardware.** Speakers (*not headphones*), Wi-Fi, display and
   brightness, suspend/resume. A silent smart amp is the most common complaint
   after an install, and you can't hear it with headphones in.
2. **Bring the files home** (keep-Windows path). Safe here because the hardware
   is proven and Windows is still whole as a complete backup. Following the
   folder map from `job.json`, `settle-in` mounts the shrunk-aside Windows
   partition, unlocking BitLocker with the harvested recovery key (`cryptsetup`
   BITLK, Linux's reader for BitLocker volumes); mounts it read-only through
   **ntfs-3g**, not the kernel `ntfs3` driver (decided 2026-09-01: the install
   kernel's `ntfs3` crashed on the rig; RISKS R19); and copies Documents,
   Pictures and the rest, plus browser profiles, checking checksums as it goes.
   It expects cryptsetup's size-mismatch warning on every shrunk volume, and
   copies only regular files (Windows' empty SYSTEM files show up as FIFOs and
   would hang a plain open). **It never copies a OneDrive placeholder** (decided
   2026-09-26, RISKS R8): it recognises each one on the mounted NTFS, skips it,
   checks its count against the job's, refuses the pull if they disagree, and
   offers to sign in to OneDrive where those files still are. The person is
   present, so a stubborn unlock or a read error is *asked about*, not guessed
   at; Windows is intact, so a failure here loses nothing. (On the clean-slate
   path the files were restored during cutover, so this step is skipped.)
3. **Confirm the data arrived:** counts and checksums against `outcome.json`.
4. **Hand off:** where files went, what replaced what, and what is gone and is
   not coming back. On the clean-slate path, also: **keep the stick,** their
   only backup until they say otherwise.
5. **Decide** (keep-Windows path): keep, or roll back to Windows (a boot-menu
   restore). Only after the person confirms things work does it offer the
   **reclaim**: delete the Windows partition and grow into the space, stated
   plainly as the one irreversible act on this path, offered exactly once and
   never nagged; saying no leaves a `reclaim` command behind for later.
6. **Exit.**

**And one standing job, for as long as Windows is kept** (decided 2026-08-30,
RISKS R22). On every Linux boot, a unit (a small background service) `settle-in`
leaves behind checks the firmware's `BootOrder` and puts the Linux entry back
first if a Windows session moved it (the rig showed Windows re-registering its
own entry *and* taking first place after one servicing pass). The welcome screen
says: "If Windows ever starts on its own after an update, hold [the boot-menu
key we detected] and pick Fedora once; it will stay put after that." Reclaim
removes the unit along with Windows.

### It runs on any Linux (decided 2026-09-27, the owner)

Fedora is only the first Linux this project installs, so nothing in `settle-in`
may assume Fedora. The owner's words: this is the platform on which any device
and any distribution is supported in an organized way (RISKS R28, VALIDATION
V10). No window toolkit is on every distribution, and the new system is offline
at first startup; what every desktop Linux shares is the kernel. So:

```text
 per distribution (small)        the same everywhere (one program we ship)
 ------------------------        ------------------------------------------
 installer adapter               settle-in
   Fedora: kickstart %post  -->    reads one fixed folder (job, secrets)
   later: Ubuntu, Debian ...       works through kernel interfaces
   (the only part that knows       draws its own window, or asks in text
    the distribution's installer)  when the person chose the console
```

1. **One handoff folder, the same on every distribution.** Each distribution's
   installer adapter does two things only: put the job and the secrets in one
   fixed root-only place on the installed system, and switch on `settle-in`'s
   start-at-boot service. Fedora's adapter is the kickstart the converter
   writes. `settle-in` never asks which distribution it is on.
2. **One self-contained program,** a single compiled file carried on the stick
   and copied over by the adapter. It works through the kernel's own interfaces
   (the hardware clock through `/dev/rtc0`, firmware boot entries through
   `/sys/firmware/efi/efivars`) and draws its own window, needing only the
   display and Mesa; the same file asks its questions as text when the person
   chose the console. **Language: Rust (decided 2026-09-27, the owner).** The
   compiler refuses whole classes of mistakes before the program runs, and its
   window libraries load the display libraries only when they are there; the
   cost (a harder read for newcomers, more packages to review) is met by keeping
   dependencies to a short, named list. **The window: egui (decided 2026-09-27,
   the owner).** A fully static program cannot load the system's display
   libraries while it runs, so the window is a second, small program beside the
   static core. It opens at the first sign-in through the standard autostart
   folder (`/etc/xdg/autostart`, which GNOME, KDE and the others all read),
   shows what the first startup did, and ends with the old-boot-entry button,
   which asks for the password through polkit (the standard administrator prompt
   on every desktop). On the console, the same content comes as text. **The
   window's words (approved 2026-09-27, the owner), verbatim.** They live in one
   place, `settle-in summary`, which both the window and the console print:

   ```text
   <the system's own name> is ready
   The clock
     Set. Windows kept the computer's clock in local time; it now keeps
     the standard time Linux uses. Nothing to do.
     (or) Not changed: <reason>. It will set itself once you are online.
   Wi-Fi
     These networks connect by themselves:
       <names>
     Not set up (join it from the network menu if you need it):
       <name> - <reason>
     Their passwords are no longer on the USB stick or in this setup
     program.
     (or) Windows had no saved Wi-Fi networks.
   The old Windows startup entry
     Windows is gone, but the computer's startup menu still lists
     "Windows Boot Manager". Choosing it would do nothing.
     [ Remove the old Windows startup entry ]
     You will be asked for your password.
   [ Close ]
   ```

   Also approved (2026-09-27, the owner), verbatim: a network Windows did not
   connect to by itself ("These networks are set up; connect to them from the
   network menu:"); no NetworkManager ("Not set up: <reason>."); after the
   button ("Removed." / "Not removed: <reason>"); the console's question
   ("Remove the old Windows startup entry now? Type yes and press Enter
   (anything else skips):"); and the password prompt, through a polkit policy
   file that applies only to the button's command
   (`org.upgrade.settle-in.policy`): "Removing the old Windows startup entry
   changes this computer's startup settings. Type your password to allow it."
   **The window asks to come first** (decided 2026-09-27, the owner): KDE's
   Welcome Center and GNOME's tour open at the same sign-in, so it opens 5 s
   after sign-in, asks the desktop for focus, then for attention as the
   fallback, and logs what it got ("in front" or "not given focus"). **Our own
   firmware entry goes at first start** (decided 2026-09-27, the owner): the
   one-time "upgrade_" entry the prologue made is left behind by an erase, and
   `settle-in` removes it, found only by the exact BCD id the prologue recorded
   (`outcome.prologue.handoff.entry_guid`, carried as "BCDOBJECT={id}").
3. **A floor it checks, and refuses below:** UEFI, systemd (what starts services
   at boot), NetworkManager (the Wi-Fi manager nearly every desktop distribution
   uses; its connection files are the same format everywhere) and a display.
   Anything missing gets a plain sentence, never a guess: a distribution with
   another Wi-Fi manager has its networks listed, not set up. Each supported
   distribution is a row in `data/distros.json`, backed by a real install.

What stays Fedora-specific, on purpose: the installer adapter (kickstart,
`%pre`, `%post`), which lives in `upgrade_`, not here.

### The clock, Wi-Fi and the old boot entry (decided 2026-09-26, the owner)

One idea: **what only Windows knows is harvested on the Windows side;
`settle-in` applies it on first startup without asking; anything the person
might want to keep is a button, never automatic.**

- **The clock.** `evaluate` harvests `harvest.clock`: the Windows time zone and
  its IANA name (the standard name Linux uses, like `Europe/London`), whether
  the hardware clock holds local time (Windows' default: `RealTimeIsUniversal`
  absent or 0), and the UTC offset at harvest. On first startup `settle-in`
  reads the hardware clock as the local time it is, converts it to UTC, sets the
  system clock, and stores the hardware clock as UTC from then on. It uses the
  offset Windows last ran with, not the zone's rules at first startup (corrected
  2026-09-27): the hardware clock holds whatever offset Windows last wrote, and
  a first startup after a daylight-saving change would otherwise be off by an
  hour. The zone's rules (the installed system's tzdata) are a cross-check; if
  they disagree with Windows, the clock is left alone. **It decides from
  evidence, never assumes:** the installer adapter records the installer's
  clock, the hardware clock, and whether a time service had synchronized
  (`cutover.clock`), because a synchronized installer may already have rewritten
  the hardware clock as UTC, and correcting it again would move a right clock by
  hours. Readings that match neither story, an unknown sync state, or a time
  service that already ran this startup leave the clock alone for the time
  service to correct once online. It marks the attempt before touching anything,
  so an interrupted run never corrects twice. Records the installer wrote with
  the shifted clock are corrected by the same offset in `settle-in`'s report,
  never silently rewritten. **Decided (2026-10-10, the owner): on the
  keep-Windows path, Linux adapts.** Windows still reads the hardware clock as
  local time, so a UTC clock would leave the kept Windows hours fast after every
  Linux boot (RISKS R36). The kickstart writes `timezone` with no `--utc`
  (0.7.0), so the installed system keeps the hardware clock in local time, and
  `settle-in` never writes that clock there (`left-local`; it only makes
  `/etc/adjtime` say LOCAL and sets the system clock once if nothing has). The
  erase path keeps the rule above; reclaim may switch to UTC.
- **Wi-Fi.** `evaluate` exports the saved Wi-Fi profiles with their passwords
  into `artifacts/credentials/wifi/` on the stick, one file per network with a
  password; `job.json` lists the networks (`harvest.wifi.profiles`) with only
  each file's path (RISKS R13: secrets are files, not fields). **Not with
  `netsh`:** `netsh wlan export` shortens file names, so two names that start
  alike silently become one file. The job writer reads each network by name from
  Windows' Native Wifi API (the interface `netsh` itself uses), counts them
  against the profile files Windows stores on disk, and refuses if the counts
  differ. WPA3 in "transition mode" (the router also accepts WPA2) is set up as
  WPA2-personal. The export runs only after every other check passed, so a
  refused job never leaves passwords on the stick. Every install launcher says
  so first, in the owner's approved words (2026-09-27), verbatim: *"Your saved
  Wi-Fi networks and their passwords are copied onto this stick, so Fedora can
  connect to them on its own. They are removed from the stick at the end of the
  install and from Fedora once it has set them up."* The nochroot `%post` moves
  them to a root-only directory on the installed system and deletes them from
  the stick; on first startup `settle-in` creates the NetworkManager connections
  (WPA/WPA2/WPA3 personal and open; enterprise networks listed, not guessed) and
  deletes its copy. **Every stop removes them from the stick too** (decided
  2026-09-27, the owner): a prologue stop, a cancelled countdown, a return to
  Windows, an abort, or a launcher that fails after the job was written; a
  re-run exports again.
- **The old boot entry.** At the end of `settle-in`, on the erase and
  clean-slate paths only, a button: "Remove the old Windows startup entry". It
  removes firmware entries that point at a Windows boot loader on a disk that no
  longer has one, and never while a Windows ESP still exists (keep-Windows keeps
  its entry by design, RISKS R21, R22).



## The way back to Windows (decided 2026-09-27, the owner)

On the keep-Windows path the way back already exists: step 5 offers "keep, or
roll back" until the person says yes to the reclaim. On the erase path, and
after a reclaim, there is no Windows left. **Decided (2026-09-27, the owner):
there is still a way back, and it is honest about what it costs.** "You can
always go back" is what gets a nervous person to try Linux at all, and someone
who could not make a Windows stick is who this is for (R30, V11).

- **It is its own program, "Go back to Windows", in the app menu,** for as long
  as the person wants it; not in the first-start window, where a way out on the
  first day sends the wrong message. It ships with `settle-in`.
- **It says the cost first.** Going back means a new, empty Windows; nothing on
  the Linux side comes along unless the person copies it off first. **The
  catch:** most of these computers cannot run Windows 11, so going back means
  Windows 10, whose free security updates ended in October 2025 (the paid
  extension for home users ends in October 2026). Their computer, their choice;
  the screen says it plainly.
- **It shows what Windows told us before it was erased**
  (`harvest.windows_license`, in the installed system's root-only copy of
  `job.json`): the edition, 10 or 11, whether it was activated and how. What
  that means for activation after a reinstall is said only where a primary
  source says it (RISKS R30). **Which Windows to offer:** 11 if Windows 11 ran
  here before, or if Linux can see TPM 2.0 and UEFI (Microsoft's installer then
  checks the processor); otherwise 10, with the October 2025 / October 2026
  warning. The 32-bit Windows 10 file and the wrong Windows are refused.

**Stage 1, a guided stick** (`settle-in go-back`): the person downloads
Microsoft's own installer in their own browser (we never redistribute it); the
program checks it against Microsoft's published SHA-256 table
(`settle-in/data/windows-media.json`, copied by
`settle-in/tools/refresh-windows-media.py`), writes it to a USB stick the person
names (found again by serial, exact size and the typed model name; the stick
writer's refusals, RISKS R16), splits `sources\install.wim` into `install.swm`
parts under FAT32's 4 GB with `wimlib-imagex` (static, GPL-3.0, built from a
pinned source and carried on the kit), reads every file back, and shows the
steps in plain words. Nothing changes until the person starts Windows'
installer. **Decided (2026-10-12, the owner): only stage 2 stays** (R30).

**Stage 2, a walk-away reinstall. Decided (2026-09-29, the owner): the way back
is 100% managed, and it comes now.** An unattended Windows install with its own
sentence and a countdown as its commit line: the erase path in reverse. It
deletes Linux and `/home`, so (rule #4) it is reviewed hardest. Three answers:

- **Windows comes from Microsoft's catalog, not a browser** (reversing the
  2026-09-27 choice). Microsoft's own Media Creation Tool reads a signed
  catalog, `products.cab` (via `go.microsoft.com/fwlink`, served by
  `download.microsoft.com` over HTTPS), listing every Windows 10 22H2 and
  Windows 11 file by edition and language with a direct link, size and SHA-1
  (read 2026-09-29: Windows 11 is 24H2, build 26100.4349; Windows 10 is
  19045.3803). The file is an `.esd` (Windows' compressed image format), not an
  ISO; its host, `dl.delivery.mp.microsoft.com`, refuses HTTPS (seen
  2026-09-29), so it comes over plain HTTP and is kept only if its size and
  SHA-1 match the catalog fetched over HTTPS (RISKS R33).
- **Both internal drives are erased:** Windows on the system drive, the second
  wiped and left as one empty NTFS drive, so nothing from Linux is left behind.
  The same shape as the forward erase, said first.
- **The commit line is on the stick, before Windows Setup:** a small program of
  ours (the gate) runs first inside WinPE (the small Windows that runs Setup),
  built with the same Rust toolchain as `UPGRADE.exe`.

```text
Linux, the "Go back to Windows" program            (nothing changed yet)
  cost first -> which Windows -> both drives named -> the typed sentence
  -> choose the stick (R16's rules) -> download from Microsoft's catalog
  -> check size + SHA-1 -> build the stick from the .esd -> read it back
  -> start from the stick once (BootNext: the firmware's own USB entry
     when it has one, else our own) -> restart
The stick, WinPE: the gate                         (nothing changed yet)
  wait for the stick to appear (up to 90 s) -> find each drive by serial
  or world-wide name and exact size, or refuse (plain screen,
  restart into Linux) -> 2-minute any-key countdown (a key: restart into
  Linux, untouched)
  ---- countdown ends: the commit line ----
  record the crossing on the stick -> write the answer file naming the
  drives it found -> Windows Setup, unattended: wipe both drives,
  install -> the account is created and signs in once by itself: the
  Wi-Fi profiles are added (and leave the stick), the password is marked
  "must change", it signs out -> the Windows sign-in asks for a new password
```

- **The stick is built from the `.esd`** with `wimlib-imagex`: its first image
  is Setup's files, its second and third make `sources\boot.wim`, and the one
  edition this computer had goes into `install.wim`, split into `install.swm`
  parts under FAT32's 4 GB. `boot.wim` is changed in one way: it starts the gate
  instead of Setup. **The job travels on the stick** (`upgrade_\go-back.json`):
  each drive by serial, world-wide name, size and model, the sentence, the
  edition, the account name, and the Wi-Fi networks by name (never a password).
  The gate refuses a job it cannot match to this computer, reads every write
  back, and refuses to count down or cross without a record.
- **No password rides on the stick.** The answer file creates a local account
  with the person's Linux name and no password; Windows signs it in once by
  itself, and its first-sign-in commands mark it "must change" and sign out, so
  Windows asks for a new password the first time. Not `SetupComplete.cmd`:
  Microsoft disables it on computers with a maker's key (RISKS R33). **No
  product key is written anywhere:** Windows Setup reads a firmware key itself
  (RISKS R13). A computer without one may stop at Setup's key page, which would
  break walk-away (RISKS R33).
- **Decided (2026-10-02, the owner): Wi-Fi comes along, both ways, as a given**
  (the rules: "What migrates"). NetworkManager's saved connections (WPA2/WPA3
  personal and open; enterprise, WEP and keyring passwords listed, not carried)
  become Windows Wi-Fi profiles on the stick. The stick's script adds them at
  the first automatic sign-in (a hook that runs on every computer, unlike
  `SetupComplete.cmd`), after starting Windows' Wi-Fi service, then deletes
  them; the gate and "Do not go back" delete them at every stop. The screen says
  so before the sentence is typed (words to be approved).

---

## Contracts between modules

The modules talk to each other only through files on the stick:

| File | Written by | Read by |
|---|---|---|
| `job.json` + `artifacts/` | evaluate | upgrade_, **settle-in** |
| `outcome.json` + logs | upgrade_ | settle-in |
| `/var/lib/upgrade_/` on the installed system | the installer adapter (Fedora: `outcome.sh`) | settle-in |

`settle-in` reads `job.json` too: on the keep-Windows path it needs the folder
map and the BitLocker recovery key to pull the files. So `job.json` outlives
cutover, and the recovery key can only be scrubbed **after** the pull. **The
handoff folder** (`outcome.sh` 0.4.0): at the end of the install the adapter
copies `job.json` and `outcome.json` into `/var/lib/upgrade_/` on the installed
system, root-only (folders 0700, files 0600), laid out like the stick's
`upgrade_/` so the job's relative paths resolve the same. The Wi-Fi password
files go to `artifacts/credentials/wifi/` there, each checked by SHA-256, then
leave the stick. `outcome.json` records what moved (`credentials.wifi`) and the
clocks at the end of the install (`cutover.clock`). This is the one place
`settle-in` reads, on every distribution, so the stick can already be gone at
first boot. Both schemas are versioned; a module that meets a version it does
not understand must refuse, not guess.

---

## Interface and privilege model

### One elevation, no stored credentials

It ships as an `.exe` whose manifest declares `requestedExecutionLevel =
requireAdministrator`. Windows asks once, at launch, and every privileged
operation runs inside that process. Measured on a stock Windows 11 machine
(Administrators group, unelevated token, `ConsentPromptBehaviorAdmin = 5`, the
default): elevation is **a single Yes click, not a password prompt**, for the
vast majority of home users.

**Techniques we will not use:** `runas /savecred`, scheduled tasks with "run
with highest privileges", COM elevation moniker abuse. All are known UAC-bypass
tricks that Defender targets by name; they would get the tool flagged *and*
really weaken the machine, for nothing the manifest doesn't already give.
**Amended (2026-09-13):** that rule is about *getting* elevation without the
consent click. The prologue does register one elevated scheduled task, from a
run that already holds UAC-consented elevation, only to survive its own restart:
one-shot, removed by every exit path, run from a directory locked to SYSTEM and
Administrators before the task exists, so it grants nothing beyond that Yes.

**The walk-away resume (decided 2026-09-13).** The prologue's restarts (the disk
check, the pagefile re-measure) come back to Windows before the handoff, and
everything after the typed word must run with nobody at the keyboard. So the
resume runs as SYSTEM at startup, before and without a sign-in; finds the stick
by volume id (polled, because USB shows up late at boot); and queues anything a
person should read as a one-shot `RunOnce` notice at their next sign-in, since
session 0 (where system services run) has no screen. **What we will not do
instead:** take the person's Windows password to automate the sign-in.
Microsoft-account holders often sign in with a PIN and may not know it, Windows
11 accounts can be passwordless, autologon stores the secret on a disk that
`settle-in` later mounts from Linux, and a third-party tool asking for a
Microsoft password looks exactly like phishing (`shutdown /g`, Automatic Restart
Sign-On, is a courtesy to add later). **We never ask for a Microsoft account
password**; it is not needed, since `manage-bde -protectors -get C:` returns the
recovery key with local admin alone. **The only credential we create** is the
new Linux account password, typed once and hashed straight away to SHA-512
crypt; only the hash reaches the USB.

**Prompt budget for the whole conversion: four.** One UAC consent click; one
confirmation that the stick about to be written is the right device; one
password the person chooses; one polkit prompt (Linux's admin-permission box) in
`settle-in` at reclaim. The SYSTEM resume adds none; the clean-slate path's
two-minute hardware check is a gate, not a prompt; the discard offer adds one
typed sentence and one UAC consent, only on its branch.

### Stack

**Decided (2026-09-27, the owner): a Rust window, `UPGRADE.exe`, in front of the
scripts on the stick.** It replaces the 2026-09-13 plan (WPF hosted in Windows
PowerShell 5.1 now, C# WPF on .NET Framework 4.8 later). One `.exe` at the
stick's root: it **double-clicks**, asks for administrator access once, and
needs nothing installed (not WebView2, which is present on Windows 11 but not
guaranteed on Windows 10). It draws with egui, the same library as `settle-in`'s
window, so the person sees one kind of window before and after the switch. **It
directs the work; it does not redo it:** it runs the kit's four programs
(`upgrade-scan`, `upgrade-job`, `upgrade-harvest`, `upgrade-prologue`) as child
processes with fixed arguments, shows their progress in plain words, and stops
where they stop. It adds exactly two things of its own: it stops on a RED scan before the job writer
runs (more cautious than before, never less), and before the restart it
registers a one-shot sign-in task that opens it again, so the person sees the
result without looking for it; a restart the window cannot follow is not
started. It cannot show anything before a sign-in (the walk-away resume runs as
SYSTEM with no screen). If its graphics cannot draw on a machine, it says so in
a plain Windows message box and names the programs on the stick, which a
helper can run from a command prompt.
Built from WSL for `x86_64-pc-windows-gnu`, linked by zig through
`cargo-zigbuild` (`upgrade_/windows/window/build.sh`); its logic is tested on
Linux, and `make-kit.sh` checks it starts on Windows. Its words are drafts until
the owner approves (RISKS R31, VALIDATION V12); Rust changes nothing about code
signing (R12).

**Decided (2026-09-27, the owner): Rust becomes the conversion's one language,**
from the window down to the code that touches the disk. The PowerShell side is
ported piece by piece, safest first (RISKS R32, VALIDATION V13): the `job.json`
/ `outcome.json` types, as one library shared with `settle-in`; the scanner's
judging half, fed the same corpus recordings; the read-only collectors; the job
writer, the stick writer and the kickstart generator; the prologue, the handoff
and the rollback, last. **Decided (2026-10-04, the owner):** all five steps are
built on the branch `rust-port`, beside the scripts, and the project cuts over
to Rust after one success of the PowerShell process on `main`. (The 2026-09-27
plan had steps 3 to 5 wait for V0's three more vendors and V9's physical
re-run.) The cut-over happened on 2026-10-10. **Decided (2026-10-11, the
owner):** the PowerShell leaves the product now, in three phases
(`docs/RUST-PORT.md`; RISKS R32). Builds are reproducible (RISKS R14).

### Code signing is the gating item

By behaviour alone (elevates, reads BitLocker keys, exports Wi-Fi passwords,
writes raw USB devices, resizes partitions, rewrites boot configuration) the
finished tool is an exact match for an infostealer followed by ransomware.
Unsigned, Defender may quarantine it, and the target user stops there for good.
It needs an OV/EV certificate, a legal entity, and a reputation that only builds
up with time (RISKS R12), so it starts before there is anything to sign.

---

## What migrates, and what silently doesn't

**Decided (2026-10-02, the owner): every OS switch carries Wi-Fi, as part of the
one click.** Any direction, any source, any target: Windows to Linux today,
Linux back to Windows, and every pair added later. The modules are named for
their roles, not their OS: `evaluate` runs on the OS being left and harvests the
saved networks and their passwords; `settle-in` runs on the OS being arrived in
and adds them before anyone signs in, so the new system is online from its first
start. The rules are the same in every direction: the passwords travel on the
stick only for the trip, are deleted from it once added (and at every stop),
never go into a record, and the screen says so before the decision is typed; a
network the target cannot take (an enterprise sign-in) is listed, not guessed. A
new direction is not finished until it does this.

| Item | Ports? | Notes |
|---|---|---|
| Documents, Pictures, Desktop, Downloads, Videos, Music | yes | Found via the known-folder APIs, so OneDrive redirection is handled. |
| Wi-Fi networks + passwords | yes | Windows' Native Wifi API -> NetworkManager keyfiles (Module 3). |
| Remote access (SSH) | yes, only if Windows had it on | **Decided (2026-10-04, the owner):** carried as part of the same rule. `evaluate` reads whether Windows' OpenSSH server starts by itself and copies only the PUBLIC keys it allows (the person's `authorized_keys` and the administrators' file; a key with options in front is not carried). `settle-in` puts them in the new account, turns password sign-in over SSH off, and turns the SSH server on. A Windows without it: nothing is turned on (`harvest.ssh`). The window's line is a draft for the owner. |
| The time, on the way back to Windows | yes | **Decided (2026-10-04, the owner).** Linux keeps the hardware clock in UTC and Windows reads it as local time. `prepare` records the zone (Windows' name for it, and its offset now); the gate, after the commit line, writes that zone's wall time to the hardware clock, and the answer file names the zone. The forward direction is `harvest.clock` and settle-in's first start. |
| Remote access (SSH), on the way back to Windows | yes, only if Linux had it on | **Decided (2026-10-04, the owner).** Public keys only. Windows' SSH server is a download, so a task installs it once the network is up; password sign-in off. |
| Enterprise / 802.1X Wi-Fi | **no** | Certificates and sign-in methods don't map cleanly. Flag and skip. |
| Firefox profile | yes | Bookmarks, history, extensions **and saved passwords**: the NSS key database works on any platform. |
| Chrome/Edge bookmarks, history, extensions | yes | The profile directory ports. |
| Chrome/Edge **saved passwords** | **no** | Encrypted with DPAPI (Windows' own key store); no Linux equivalent. Must be said in `evaluate`, not discovered in `settle-in`. |
| OneDrive cloud-only files | **needs care** | Decided 2026-09-26: not downloaded. The job records them as left in the cloud, `settle-in` reconnects OneDrive, and nothing may copy a stub as if it were the file. RISKS R8. |
| Installed applications | no | Out of scope for v1; the inventory names equivalents (Module 1). |
| Windows settings, Outlook data, licences | no | Say so plainly in the handoff sheet. |

## Non-goals (v1)

- Any distribution other than Fedora, **in v1.** The seams are cut for more (the
  distribution lives in `job.json`, the kickstart generator is per target,
  nothing outside `upgrade_/linux/` knows what is installed); multi-distro is an
  intended open-source future, not a v1 promise.
- Dual-boot as a product. The keep-Windows path holds Windows *for a while*, as
  a rollback and a file source that both end at reclaim; it is not a supported
  two-OS machine, and `settle-in` nudges toward reclaim once the files are
  across and the system is confirmed.
- Migrating Windows applications.
- Machines `evaluate` flags RED. No override, ever.

---

## Platform scope: beyond Windows PCs (decided 2026-09-27, the owner)

Smart TVs, and devices beyond Windows PCs in general, are on the long-term plan.
`docs/research/device-feasibility.md` scores every device family on five doors
(a Linux build, firmware that boots something else, a way in from the old
system, drivers, a way back), using only doors the maker opens on purpose;
research, not evidence (rule #2). Headlines as of 2026-09-27: Intel Macs without
T2 look like the best next source; every smart TV checked is blocked (app
developer modes only), so the TV story today is a converted laptop plugged into
the TV. The step-by-step path for each feasible family is in
`docs/research/future-paths.md`. Nothing here changes v1's scope.

## Platform scope: Apple hardware

Three cases, three answers; the deciding reason is not the one people expect.

| Mac | Answer |
|---|---|
| Apple Silicon (M1 onward) | Long term: in scope by wrapping Asahi's installer, never replacing it (amended 2026-09-27). |
| Intel with T2 (2018-2020) | Out of scope for v1. |
| Intel without T2 (pre-2018) | Not v1; revisit after the Windows path ships. |

**Apple Silicon: not because Apple blocks it.** These Macs boot other operating
systems as a **designed feature**. From the Asahi Linux project:

> "Apple allows booting unsigned/custom kernels on Apple Silicon Macs without a
> jailbreak! This isn't a hack or an omission, but an actual feature that Apple
> built into these devices. That means that, unlike iOS devices, Apple does not
> intend to lock down what OS you can use on Macs (though they probably won't
> help with the development)."
> (<https://asahilinux.org/about/>)

Two things clash with this project's promise: **you cannot fully replace the
OS** (a 2.5 GB "stub" macOS partition with Apple's bootloader, firmware and
recovery image must stay, by the platform's design; full macOS is *recommended*
today only because firmware updates cannot yet be applied from Linux), and
**walk-away is impossible** (changing boot policy needs 1TR, One True Recovery:
holding the power button and typing an admin password). The deciding reason is
neither: **Asahi already does this, and does it better than we would. Decision:
never in scope. Refer people to Asahi,** by name, in the scanner. **Amended
(2026-09-27, the owner): every Mac, eventually.** Long term, upgrade_ **wraps
Asahi**: upgrade_'s scan and refusals, the harvest into `job.json`, and
`settle-in` on first start, with Asahi's own installer doing the partitioning,
boot policy and stub in the middle. Fedora Asahi Remix is Fedora, and
`settle-in` runs on any Linux (RISKS R28), so the back half carries over. The
1TR step is done by a person, so this path is never walk-away, and says so up
front (`docs/research/future-paths.md`, Path F). Not v1.

**Intel Macs with T2 (2018-2020): a hard maybe.** The T2 chip owns the SSD
controller, keyboard, trackpad, audio and camera; Linux needs out-of-tree
drivers (t2linux's `apple-bce`), Secure Boot must be turned off in recovery, and
audio has been partial. A project rather than an install. Not v1.

**Intel Macs without T2 (pre-2018): the genuinely interesting case.** Ordinary
x86 machines with conventional firmware, among the easiest Linux targets there
are; the main pain is Broadcom Wi-Fi, which `data/devices/wifi.json` already covers.
macOS Tahoe is the final release supporting Intel Macs, and only four models
(the 2020 iMac, the 2019 16-inch MacBook Pro, the 2020 four-port 13-inch MacBook
Pro, the 2019 Mac Pro); every other Intel Mac is *already* out of support, this
project's founding argument with a different logo on the lid. Supporting them is
a second, parallel implementation, not a port: `evaluate/macos/`
(`system_profiler`, `ioreg`, `diskutil` and `security` replace WMI, the
registry, `netsh` and `manage-bde`), no `bcdedit` equivalent for the one-time
boot entry (`bless` and Startup Disk need their own design), FileVault instead
of BitLocker with a different key-escrow model. Shared: the philosophy, the risk
register, `data/`, and almost no code. **Decision: not v1.** Revisit after the
Windows path ships and has real-world conversion data behind it.

---

## Layout

The tree itself is in CLAUDE.md ("Layout"), its one home. One directory per
module (`evaluate/`, `upgrade_/`, `settle-in/`), each split by platform below
that (`windows/`, `linux/`), with `data/` and `schemas/` at the top level. Three
decisions worth stating, because they will be questioned. **Module first,
platform second:** modules cross platforms (`upgrade_` starts in Windows and
finishes in Linux); a platform-first tree would scatter one module across two
roots. **`data/` is top-level, not inside `evaluate/`:** it is where most
contributions land, a newcomer must find it in seconds, and it is truly shared
(`upgrade_` needs the firmware mapping to inject, `settle-in` the quirk data).
**`schemas/` is not merged into `data/`:** `data/` changes constantly through
drive-by PRs; `schemas/` are the contracts between modules and change rarely,
under careful review.

## Build order

The order itself (what is built next, and what each step waits on) is in
[ROADMAP.md](ROADMAP.md). The decisions that shape it live here.

**Decided (2026-09-08): build a vertical, then the matrix.** Every gate tests
one component on its own, but the failures that end a product like this live
*between* components: the folder map the Linux side cannot parse, the checksum
taken before OneDrive finished materializing, the stick that checks out on the
machine that made it and not on the target. So the build runs as one
**front-to-back, one-click vertical** (a thin slice through every stage), split
at the commit line: **the reversible half first** (plug in, one double-click,
one UAC consent, walk away, come back to a verdict with Windows untouched),
**the destructive half second** (shrink, the ESP snapshot and restore, the
alongside install, the boot-chain check, the `settle-in` pull, the reclaim
offer). The stick writer is deliberately the first real writer, because it
writes to a stick and not the internal disk. **No component that writes to the
internal disk is built until the spine it depends on is proven on real
hardware** (CLAUDE.md rule #4).

**Where it is built.** First the Hyper-V rig: disposable, restorable from a VHDX
(a virtual disk file) in a minute, where a first vertical gets broken and
restarted twenty times. Then a machine we own and can image. **A borrowed
machine is never the trailblazer:** one attempt, no baseline image, someone's
data. A borrowed Dell/Lenovo/HP is a 30-minute read-only visit that fills one
column of the matrix (scan, harvest, author, handoff, live boot, hardware
check), and on a recent Intel machine the first real firing of the RST/VMD check
(V5). Once the vertical is one script writing an evidence row per stage, the
matrix of vendors x stages builds itself from such visits.

**Decided (2026-09-08): `evaluate` never repairs.** The first physical machine's
C: carried NTFS's dirty flag, and Windows refused to measure the shrink until a
disk check ran (RISKS R18). `evaluate` detects the flag and captures the
person's fork; the prologue clears it as reversible prep (stage 1, step 1b) with
four guardrails, re-measures, and branches on the real number. It is the first
step that changes the internal disk, so it sits behind intent capture and before
the commit line, not in a preflight check.
