# upgrade_ architecture

The project has three modules:

```
   evaluate            upgrade_             settle-in
   ---------           ---------            ---------
   read + decide       do the thing         confirm + hand over
   Windows             Windows -> Linux     Linux
   reversible          contains the         after the fact
                       commit line
```

The modules are split by **commitment**, not by operating system. In other
words, the question that divides them is "can this step still be undone?"

An earlier draft split them by OS (Windows / live USB / first boot). That
put reversible and irreversible work inside the same phase. It was the
wrong seam, because it hid the one thing the user actually cares about:
whether they can still change their mind.

**On the name.** The middle module's directory has the same name as the
project on purpose, because the conversion *is* the product. In prose we
always say "the converter" for the module and "the project" for the whole.
That way no sentence ever has to tell `upgrade_` apart from `upgrade_`.

---

## The commit line

Exactly one moment in a conversion cannot be undone: **the first
destructive write** (the first time something on the disk is overwritten
or deleted). Think of it as a one-way door. Which moment that is depends on
the path the person chose when `evaluate` captured their intent:

- **Clean-slate path** (files are copied to the stick first): the commit
  line is the disk wipe, inside the cutover. Everything before it (copying
  files to the stick, checksums, the restart into the live environment)
  only adds things. If anything fails before the wipe, the machine simply
  boots Windows again.
- **Safety-copy path** (files stay on the internal disk): the commit line
  is the **reclaim**, meaning deleting the Windows partition. That happens
  in `settle-in`, only after the checks pass and the user clearly says yes.
  Until reclaim, Windows is physically there and still boots. Rolling back
  means restoring a boot entry, not restoring a disk image.

Two rules follow. They are not negotiable:

1. **The screen must say "you can still cancel" right up to that exact
   moment**, and must stop saying it the instant the line is crossed.
   Someone watching a progress bar deserves to know which side of the line
   they are on.
2. **Everything that could refuse must refuse before the line.** That
   means identity checks, checksum checks, capacity checks and hardware
   refusals. On the clean-slate path it also means a live-session hardware
   check with a person present, because there is no rollback on the other
   side.

---

## Module 1: evaluate

**Runs in Windows. Read-only. Can be run again and again. Never changes
anything.**

### It is the last moment Windows exists

This duty is easy to miss, and impossible to add later.

Some things the finished Linux system needs exist *only* inside the Windows
install. Once Windows is gone, they are gone too. The clearest example is
vendor audio firmware. On laptops from 2023 onward with Cirrus "smart
amplifiers" (speaker chips that need their own firmware), Linux can drive
the speakers, but it needs firmware files ("blobs") that ship inside the
Windows driver package. Wipe first, and the speakers are silent with no way
to fix it on the machine.

So `evaluate` pulls out files (artifacts), not just facts:

| Artifact | Why it can't wait |
|---|---|
| Vendor audio firmware (Cirrus CS35L41/56, TI TAS2781) from the driver store | Gone with Windows; the fix for silent speakers |
| BitLocker recovery key | Can't be recovered later; unlocks the data volume on the safety-copy path, and avoids a lockout mid-conversion |
| Wi-Fi profiles and PSKs (the network passwords) | Needed to reconnect; the user may not know their own password |
| Browser profiles | Bookmarks, history, extensions, Firefox passwords |
| OEM firmware/ACPI quirks (maker-specific hardware settings) | Shipped by the maker, Windows-only |

This turns "if we have to, we inject it from the USB at runtime" from a
backup plan into a designed pipeline:

```text
evaluate extracts  ->  the USB carries  ->  upgrade_ injects before first boot
```

### It captures intent, not just state

If any decision is missing, the converter has to stop and ask a person, and
walk-away is dead. So `evaluate` also collects:

- what the computer starts at (KDE desktop, GNOME desktop or the text
  console; the launcher menu since 2026-09-26),
- the account name,
- the password (hashed straight away to SHA-512 crypt, a one-way scrambled
  form that Linux can check but nobody can read back).

**The conversion path is not a coin flip.** Keeping Windows aside as a
fallback is plainly the safer net, so it is the **default whenever the disk
has room for it**. Clean slate (wiping Windows and copying files to the
stick) is not offered as an equal choice. It appears only when the user
clearly wants Windows gone, or as the forced fallback when the disk is too
full to keep both. Most users never make this choice at all.

**Amended (2026-09-22, RISKS R18, the Aspire's fifth run):** the forced
fallback happens only when the person's pre-chosen fork allows it
(`fork.if_cannot_keep = clean-slate`). Under `stop`, the job writer never
writes a clean-slate job:

- If the disk and the ESP allow keeping Windows, the job is keep-windows,
  and the prologue's own re-measure decides. It stops if Linux still does
  not fit.
- If they do not, there is no job, and the person is told why before
  CONVERT.

The prologue refuses a stale forced clean-slate job under `stop` the same
way. It also refuses clean slate whenever the job lists no folders or
nothing was staged. A "stop" is never turned into a wipe.

One consequence the screen must still state plainly: clean slate needs the
user to be present for a two-minute hardware check in the live session
before anything is destroyed, because it has no rollback. The default
keep-Windows path is true walk-away. The files come over later, in
`settle-in`, from the untouched Windows partition (see Module 3).

### It harvests what only Windows can give

Two things `settle-in` will need cannot be got once the machine has booted
Linux. So `evaluate` captures them while Windows is alive. This is the same
"last moment Windows exists" duty as pulling out artifacts:

- **The folder map.** *Where* the user's files really live. Documents,
  Pictures and the rest are looked up through Windows' known-folder APIs
  (the official way to ask "where is this person's Documents folder?"), so
  a OneDrive redirect is followed, not guessed. `settle-in` reads this map
  off the stick to know what to pull. A naive Linux-side copy of `\Users\`
  would miss redirected folders entirely.
- **Which files are only in the cloud.** OneDrive's "free up space"
  placeholders are 0-byte stubs on disk: a name with no contents, like an
  empty envelope. Linux has no OneDrive client to fill them. Read from the
  mounted NTFS later, they copy over *empty* (RISKS R8). **Decided
  (2026-09-26, the owner's call):** they are not downloaded. Their bytes
  already live in OneDrive. `evaluate` counts them per folder and records
  `cloud_files.result = left-in-cloud`, and `settle-in` skips them and
  reconnects OneDrive instead. (The harvester's `-Materialize`, which
  downloads and verifies them, stays built and unused.)
- **Which Windows this was, and how it was activated** (decided
  2026-09-27, the owner; RISKS R30). The way back to Windows (`settle-in`,
  "The way back to Windows") needs to know this, and only Windows can say.
  `harvest.windows_license` records the edition (Home or Pro), whether it
  was Windows 10 or 11 (from the build number: Windows 11 still calls
  itself "Windows 10" in the registry), whether it was activated, the
  licence channel (for example `OEM:DM`, a key the maker put in the
  firmware, or `Retail`), and whether the firmware holds a product key.
  **Never a key itself.** A firmware key stays in the firmware, where
  Windows' installer finds it again. A key the person typed is theirs to
  keep, and the stick is not a credential store (R13). A read that fails
  is recorded with its reason, not refused: its cost is a less informed
  way back, never lost data. Built 2026-09-27 (job writer 0.16.0).

**The software inventory (2026-09-13).** The job writer records every
installed desktop program (the registry's Apps & features entries, minus
updates and hidden components) and every Store app the person installed.
It keeps names and versions only, as `harvest.software`. It is for
`settle-in`: a "you had these" list, with the Linux or web equivalent named
for each. That is the missing piece of the move for someone who does not
know what LibreOffice is. It is personal data, so it travels only in
`job.json` on the stick and never in a report that leaves the machine.

### It writes the stick

`evaluate` makes the USB stick itself: the live image plus an exFAT staging
partition (a separate section of the stick for copied files). The user
never meets an ISO or a burning tool. (An ISO is also published for
technical users and repair events.)

Raw disk writes are the one place this tool could destroy data *before*
the commit line, so picking the device is defensive:

- removable-bus devices only,
- size and volume label confirmed with the user,
- refuse if anything is unclear.

See RISKS R16. (`evaluate/windows/Write-UpgradeStick.ps1`, 2026-09-08: the
rules as a pure function, a read-only `-Plan`, and a write path that finds
the device again by its unique id before the first destructive call. The
physical several-devices write row is still owed.)

### It refuses

Every refusal lives in `evaluate`, because it is the only module that can
refuse for free:

- **RED scanner verdict.** No override flag, ever.
- **Neither path fits:** the user's data is too big for the stick AND there
  is too little shrinkable space to keep it in place. The refusal is a
  **gap report**, not a door slammed shut. It says exactly how many GB to
  free (largest folders listed) or what stick size would change the answer,
  so running `evaluate` again moves toward a yes.
- **More than one user account** (see RISKS R5). Moving one and silently
  leaving the rest behind is a data-loss bug. Refused in v1. The harvest
  schema is shaped per user from the start, so multi-user support is an
  extension, not a rewrite.
- **Folder sizing was cut short**, so the staging estimate can't be
  trusted (RISKS R6).
- **RST/VMD active.** (Intel storage modes that hide the disk from Linux.)
  Cannot be automated; see below.
- **The shared ESP cannot take the alongside install** (keep-Windows path
  only; decided 2026-08-30, RISKS R21). The ESP is the EFI System
  Partition, the small FAT partition the firmware boots from. The rig
  measured Fedora's footprint on it at 6.2 MB. The gate is **≥ 32 MiB free
  on the ESP**, and the ESP must be the FAT volume the firmware's *Windows
  Boot Manager* entry points at. A machine that fails this is steered to
  clean slate, never quietly installed alongside. (Scanner check landed
  2026-08-30 as "Boot partition (ESP)"; see RISKS R21 item 4.)

**One exception, narrow and dated (2026-09-13, RISKS R23):** a person who
has already copied their files off a machine the scanner refused for its
*drive* (bad blocks, SMART media errors, a volume needing a full repair)
may type a fixed sentence on a separate launcher and go ahead. It lifts
those two refusals and no other. It is recorded in `job.json` and
`outcome.json`, and it turns every later screen red. CLAUDE.md rule #1
carries the amendment.

### Output

A complete, validated **job spec** (`job.json`, versioned) plus an
`artifacts/` directory. The test for "done": `upgrade_` can run it start to
finish with no further human input.

### Current status

Largely built. `evaluate/windows/upgrade-scan.ps1` (reads the machine,
refuses) and `evaluate/windows/Harvest-UpgradeState.ps1` (state and intent
scaffolding) exist and are tested.

- The `job.json` / `outcome.json` contracts exist as JSON Schema in
  `schemas/` (2026-09-08; `schemas/check.py` holds the examples and the
  documents each schema must refuse).
- Since 2026-09-12 the job writer (`evaluate/windows/New-Job.ps1`) writes
  `job.json`, and the prologue and `outcome.sh` write `outcome.json`.
- **The folder map is in the job (2026-09-26, job writer 0.10.0, harvester
  0.3.0).** The job writer runs the harvester (`-FolderMapOut`, its own
  process). It writes the six known folders into `harvest.folders`, and
  whether they fit the stick into `harvest.stick_fit`. It refuses when the
  map could be wrong:
  - the elevated account is not the one signed in on the screen (UAC with
    another account's password reads the wrong person's folders),
  - a folder hit the file cap or holds sub-folders Windows would not list
    (R6),
  - OneDrive online-only files were found and not made local (R8),
  - or, on the clean slate, the folders do not fit the stick or other
    people's profiles exist (R5).
- **Not yet built:** artifact extraction, the intent-capture UI, multi-user
  migration, Wi-Fi and browsers into the job.
- **OneDrive online-only files are not downloaded** (decided 2026-09-26,
  the owner). Job writer 0.11.0 records them as
  `cloud_files.result = left-in-cloud`, and `settle-in` reconnects OneDrive.
- **Materialization of cloud placeholders exists**
  (`Harvest-UpgradeState.ps1 -Materialize`, 2026-09-08). "Materialize"
  means forcing the real file down from the cloud. Its plumbing is proven
  against Windows' own cloud files filter (RISKS R8). Since the
  no-download decision it stays built and unused by the launchers.

---

## Module 2: upgrade_

**Starts in Windows, finishes in Linux. Contains the commit line.**

### Two paths, one stick

There is no external drive anywhere in this design. Everything travels on
the USB stick, or never moves at all:

|  | keep Windows (default) | clean slate (opt-in / fallback) |
|---|---|---|
| User files | never leave the internal disk; pulled in `settle-in` from the untouched Windows partition | copied to the stick's exFAT partition, restored during cutover |
| Qualifies when | shrinkable space ≥ Linux (~20 GB) + headroom | data + artifacts fit the stick |
| Windows afterwards | intact until `settle-in` reclaims it | gone at the wipe |
| Rollback | full, until reclaim | none: the wipe is the commit line |
| Walk-away | total | after a 2-minute human check in the live session |

**Keep-Windows is the default** wherever the disk fits it, because a
working Windows to fall back on is the better safety net. Clean slate is
used only when the user wants Windows gone, or forced only when the disk
cannot keep both. When neither fits, `evaluate` refuses with a gap report.

That default has a deep consequence. On the keep-Windows path **no user
data is read out of Windows during the destructive part of the conversion
at all.** Cutover installs Linux into the freed space and touches nothing
of the user's. The files come across afterwards, in `settle-in`, with
Windows still whole. That is what moves the encrypted-read risk (RISKS
R19) out of the corner it used to sit in: unattended, the only copy, before
a wipe.

### When Windows cannot be kept: the offer to discard it

**Designed (2026-09-26, the owner's call), not built.** RISKS R26.

The Aspire's runs 4-7 (RISKS R18) showed a real disk that cannot free the
25 GB Linux needs, even after every mitigation. The person was then left
with two bad outcomes: stop, or a wipe they had to choose before anyone
knew the number. So the fork gets a third value, and the wipe is asked for
only once the answer is known:

1. **At CONVERT, the fork is asked plainly:** if Windows cannot be kept,
   *stop* (the default) or *ask me then*. `fork.if_cannot_keep = ask` is
   recorded in `job.json`. The existing `clean-slate` value stays, for a
   person who wants Windows gone from the start. Nothing ever wipes on a
   pre-chosen answer made without the number.
2. **When the shrink cannot fit, the prologue stops exactly as it does
   today.** The mitigations are put back, C: is as it was, the resume task
   is removed, and a stopped `outcome.json` is written. Windows is a normal
   Windows while the person decides, whether that takes a minute or a
   week. The only difference: the stop registers the question for the next
   sign-in, instead of the plain stop notice. (The resume runs in session 0
   and cannot show a window; see "The walk-away resume" below.)
3. **The question is a window with the real numbers,** measured again when
   it opens, not copied from the stop:
   - what the drive could free, and what Linux needs;
   - the folders that would be kept, each with its size, from the folder
     map (`harvest.folders`; OneDrive's online-only files are listed as
     staying in OneDrive, not kept on the stick; R8);
   - the stick's free space.

   It names what is lost, in plain words: every installed program,
   Windows' settings, and every file outside the listed folders.
4. **If the files do not fit on the stick, there is no offer.** Only the
   gap, in GB, and "a stick of at least N GB would". This is the same
   honest sizing as "one stick, honestly sized" below. A stick is the whole
   kit.
5. **Consent is a typed sentence, word for word**, like R23's and separate
   from it: *"I confirm that Windows will be deleted and only the listed
   folders will be kept"*. "Keep Windows" is the default button. Closing
   the window means "keep Windows". On a RED machine the R23 sentence is
   still typed as well. Neither sentence stands in for the other, and
   neither widens the other (CLAUDE.md rule #1).
6. **A "yes" wipes nothing by itself.** It runs the normal chain again,
   elevated (one UAC consent):
   - the job writer writes a new job (`intent.path = clean-slate`,
     `path_reason = user-chose-clean-slate`) carrying the typed sentence
     and the numbers the person was shown;
   - the kickstart generator follows;
   - the prologue starts on that job: re-validate, measure the folders
     again, refuse if they no longer fit, stage them with per-file
     checksums at a measured speed, refuse on 0 folders or 0 files
     (0.7.0), and arm the handoff only if no Windows update is waiting
     (R25).
7. **The wipe is where it always was:** in the live session, after the
   identity check, the staged checksums read back from the stick (R17),
   the hardware checks, and the two-minute human check. "You can still
   cancel" is on screen until that moment (rule #3). Cancelling there
   boots back into an untouched Windows. After it, the files are restored
   from the stick during cutover, and `settle-in` shows what came across.

What this does not change: stop stays the default at every step, the
walk-away path (keep Windows) is untouched, and the extra prompt exists
only on the branch where keep-Windows failed.

**Build order.** Every piece this depends on is either unbuilt or unproven:
the folder map in the job (Build order, the harvest), staging at real size,
the live-session human check, the cutover restore from the stick, and
`settle-in`. And the wipe is the most destructive writer in the project
(CLAUDE.md rule #4, "no component that writes to a disk gets built until
the spine it depends on is proven on real hardware"). So the build waits
for the harvest and for a first physical install row (V1/V1b). The design
is written down now so that the pieces built before it are built to fit it.

### Erase and install: the one-click fresh start

**Decided (2026-09-26, the owner's call); built and passed on the rig the
same day (all three arms, `v9-erase.csv`); physical row owed.** RISKS R27,
VALIDATION V9.

The first destructive path to be proven end to end is the simplest one:
**erase every internal drive and install Fedora, keeping nothing.** The
owner's goal is to prove the mechanism first (one click, one consent, a
machine that comes back as Linux) before carrying anything across.
Carrying files (the folder map, staging, `settle-in`'s pull) is stage 2.
This path neither needs it nor pretends to do it.

It is a `clean-slate` job whose reason is `user-chose-fresh-start`. It
differs from the discard offer (R26) in one way that matters: the person
said, before anything started, that nothing is to be kept.

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

1. **Its own launcher, its own sentence.** `RUN-ERASE-AND-INSTALL.cmd`
   (and, for a RED machine, `RUN-ERASE-AND-INSTALL-ACCEPTING-DATA-LOSS.cmd`,
   which also asks for the R23 sentence; neither sentence stands in for the
   other). The consent is typed word for word, before anything else
   happens: *"I confirm that everything on this computer will be deleted
   and nothing will be kept"*. The launcher then asks for the Linux
   password (typed twice, hidden). The job holds only its SHA-512 crypt
   hash.
   **The person is told the account's name** (decided 2026-09-26, the
   owner). The password screen says "Your Fedora account: <name>". Before
   the restart, a box repeats "Your Fedora sign-in: user <name>, password
   the one you just chose". The name comes from the job writer, so the
   screen and the job cannot disagree.
1b. **The person picks what the computer starts at** (decided 2026-09-26,
   after the Aspire's run 9 reached a text console nobody chose). Every
   install launcher has a menu: KDE Plasma desktop, GNOME desktop, or text
   console (for people who know Linux commands; the KDE edition is
   installed and starts at the console). The choice travels as
   `intent.desktop` + `intent.start_at`. The kickstart follows it, and the
   outcome schema refuses a completed conversion that starts anywhere
   else. Rig: all three held (`v9-erase.csv` lines 9-10 plus line 8).
2. **The job names every drive it will erase.** The job writer lists the
   machine's internal drives (not USB) by serial, unique id and size in
   `erase_consent.disks`, each with a role:
   - the drive holding C: is `system` (Fedora's boot files and system);
   - a second internal drive is `home` (the person's home folder, so a
     failing system drive can be reinstalled without losing what is on the
     other).

   More than two internal drives, a `home` drive that is not Healthy, or a
   drive that cannot be identified is a refusal. (Decided 2026-09-26 on
   the Aspire: system on the SSD, `/home` on the 1 TB drive.)
3. **The prologue changes nothing on the disks.** No disk check, no
   shrink, no staging. It re-validates the job (both drives by identity),
   lets a waiting Windows update finish (R25), arms the one-time boot to
   the stick and restarts. Windows is untouched and still boots if
   anything stops before the countdown ends.
4. **The installer checks, then counts down.** `%pre` (`verify.sh`, the
   script that runs inside the installer before it does anything):
   - finds each listed drive by identity and exact size, and refuses on
     any mismatch (a stick moved to another computer never erases it);
   - reads the desktop image back against its checksum (R17);
   - then shows a **two-minute countdown on screen: "Erasing both drives
     in 1:59 - press any key to cancel and restart into Windows."**

   Any key cancels: the report goes to the stick and the machine restarts
   into an untouched Windows (the handoff was one-time). If nobody touches
   it, the moment it reaches zero is **the commit line**. `verify.sh`
   writes the time to the stick and the installer erases and installs.
   (Rule #3: "you can still cancel" is on screen until that exact moment,
   and nothing on the internal drives has been written before it.)
   **Every refusal before the countdown is a plain screen** (decided
   2026-09-27, the owner; `verify.sh` 0.5.0). Before, Anaconda showed its
   raw Python traceback. Now, for 60 s (any key: now), then the computer
   restarts unchanged, with `report/refusal.json` on the stick:

   ```text
   NOTHING WAS CHANGED ON THIS COMPUTER.

   The installer stopped before touching anything, because <reason>.

   It restarts in 60 seconds, exactly as it was before.
   Press any key to restart now.

   The details are saved on the USB stick, in upgrade_\report.
   ```

   The eight reasons, approved verbatim: this USB stick was prepared for a
   different computer, or one of its drives has changed since · the
   installer could not work out where to put Linux · Windows' startup
   files were not where the preparation found them · the copy of Linux on
   this USB stick is damaged · a safety copy of Windows' startup files
   could not be made · this version cannot yet put your files back on an
   erased computer · the last-chance countdown could not be shown, and
   nothing is erased without it · this USB stick was prepared by a
   different version of this tool.
5. **After it:** Anaconda (Fedora's installer) clears both drives and
   installs Fedora (EFI, `/boot` and `/` on the system drive, `/home` on
   the home drive). `%post` (the script that runs after the install)
   writes `outcome.json` (`commit_line.crossed = true`, the countdown's
   end as `crossed_utc`, `act = wipe`) to the stick, and the machine
   reboots into Fedora with the person's account. Walk-away holds the
   whole way: one click, two typed answers, then nobody needs to be there.

What it does not do: carry files, Wi-Fi, browsers or programs (stage 2),
or keep Windows as a fallback (there is none after the commit line; the
countdown is the last exit). Going back later means a new, empty Windows
(`settle-in`, "The way back to Windows"), and the launcher says so before
the sentence is typed (draft words, awaiting the owner's approval):

```text
  If you change your mind later, you can put Windows back, but it will be
  a new, empty Windows: nothing on this computer today comes back. On many
  older computers that means Windows 10, which no longer gets free
  security updates.
```

The existing clean-slate path with staged
files still stops before arming, as before, because its restore is not
built.

### Stage 1 - prologue (runs in Windows, reversible)

The prologue is everything the converter does in Windows before it hands
the machine to the stick.

1. Re-validate `job.json` against the live machine. If anything changed
   since `evaluate` ran, it stops here.

   **1b. Clear the volume flag, if `evaluate` found one** (decided
   2026-09-08, RISKS R18). Windows refuses to measure or shrink a C: that
   carries NTFS's "dirty flag" (a mark that says the volume needs a
   check). This was seen on the first physical machine. `evaluate` never
   repairs. It records the flag and the fork the user chose in advance.
   Here, as reversible prep with its own restart:

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

   The check's real outcome goes into `outcome.json`. This is before the
   commit line, so "stop" leaves Windows as it was, plus a completed disk
   check. Every restart in this stage resumes as SYSTEM at startup with
   **nobody signed in**. Anything a person should read is queued for their
   next sign-in (decided 2026-09-13, "The walk-away resume" below, RISKS
   R24).
2. **Keep Windows (default):** turn off the pagefile and hibernation, then
   shrink C: with `Resize-Partition`. That is Microsoft's own code path,
   the most-tested NTFS resize there is, and it works with BitLocker still
   on. Only artifacts are copied to the stick. The user's files stay put
   and are pulled later, in `settle-in`.

   **Clean slate (opt-in / fallback):** copy user files and artifacts to
   the stick's exFAT partition with per-file checksums. The write speed is
   measured and shown as a **computed** time estimate. Cheap flash at
   20 MB/s is not a ten-minute job, and the user must learn that before
   walking away. Windows reads its own BitLocker volume, so encryption
   never enters this path.
3. Hard confirmation: type the word, not a checkbox.
4. Suspend BitLocker, write the one-time boot entry, restart.
   Decided (2026-09-07, from the V0 harness): if the BitLocker state
   cannot be determined (read through the PowerShell module, or
   `manage-bde` on editions without it), the prologue **refuses** here
   rather than guess whether the return boot will stop at a recovery-key
   prompt.

Still fully reversible. Nothing has been destroyed. Even the shrink can be
undone by growing the partition back.

#### The handoff

`upgrade_` creates a one-time UEFI boot entry, so the user never touches a
firmware menu: no maker-specific boot key, no hostile BIOS screens. `bcdedit`
is Windows' built-in tool for editing boot entries.

```
bcdedit /copy {bootmgr} /d "upgrade_"        -> returns {guid}
bcdedit /set {guid} device partition=<usb>:
bcdedit /set {guid} path \EFI\BOOT\BOOTX64.EFI
bcdedit /set {fwbootmgr} bootsequence {guid}  -> next boot only
```

`bootsequence` applies to the next boot only, so any failure leaves the
machine booting Windows normally. That property is what makes this safe to
try.

**BitLocker must be suspended first** (`manage-bde -protectors -disable C:
-rebootcount 1`). Changing the boot configuration breaks the TPM's
measurement of the boot and triggers a recovery-key prompt, on a machine
nobody is sitting at.

**RST/VMD cannot be automated.** No portable API exists. The settings live
at maker-specific places in UEFI setup variables, and writing them blindly
bricks machines. `evaluate` hard-refuses. This is permanent, not a v1
limitation.

### Stage 2 - cutover (runs in the live environment)

The cutover is what happens after the machine has booted from the stick.

5. Check the machine's identity against `job.json` again: same disk
   serial, same firmware mode. A mismatch means the USB was moved to
   another computer: **abort.** Do not partition a stranger's laptop.
6. Check every staged checksum by reading it back from the stick. This is
   also the fake-flash test (RISKS R17): a stick that lied about its
   capacity fails here, while Windows still exists.
7. Automatic hardware checks, before anything is destroyed: Wi-Fi
   connects, the display runs at native resolution, amp firmware loads.
   What the old design discovered in `settle-in` is now a refusal gate.

**Keep Windows** (the default) is unattended, and does nothing destructive
at all:

8. Create partitions in the freed space. The Windows partition and the
   existing ESP are never reformatted. Fedora's bootloader is added
   alongside `bootmgfw.efi` (Windows' boot loader), which is what keeps
   rollback a boot-menu entry rather than a restore.
   **Before touching the ESP, snapshot `EFI/Boot` and `EFI/Microsoft`
   (every file, with checksums) and the firmware's `Boot####` entries to
   the stick.**
   Decided 2026-08-30, after the rig showed (RISKS R21) that "added
   alongside" still replaces one file Windows put there: Fedora's shim (the
   small signed loader that lets Linux boot with Secure Boot on) overwrites
   the fallback loader `EFI/Boot/bootx64.efi`. That replacement is **kept
   on purpose** while Windows is kept. With shim in the fallback slot, a
   firmware that loses its NVRAM entries still reaches GRUB, and from there
   both systems (seen on the rig). With Windows' copy there, it would reach
   Windows only. The snapshot is what lets rollback put Windows' copy back.
9. Install via kickstart with `--onpart`, touching the new partitions only.
   (A kickstart is the answer file that lets Fedora's installer run with
   nobody at the keyboard.) `/boot/efi` is the existing ESP with
   `--noformat`. os-prober (the tool that finds other operating systems
   for the boot menu) is **switched on explicitly**
   (`GRUB_DISABLE_OS_PROBER=false`), whatever the distro's default. So
   Windows appears in the menu because we put it there, not by luck
   (Fedora 42 happened to have it on; a later release may not).
10. Inject artifacts (vendor firmware, drivers) into the installed system
    *before* first boot, so the first impression is working hardware.
11. **Check the boot chain, then** write `outcome.json` and logs to the
    stick and reboot into Linux. The check (decided 2026-08-30, RISKS R21)
    is a checklist, not a hope:
    - the firmware holds a *Windows Boot Manager* entry pointing at
      `\EFI\Microsoft\Boot\bootmgfw.efi` on the shared ESP (re-create it
      with `efibootmgr` if the firmware dropped it; the rig's firmware
      deleted every OS entry at the installer boot);
    - the Linux entry is first in `BootOrder`;
    - `bootmgfw.efi` matches the step-8 snapshot;
    - `grub.cfg` lists the Windows entry.

    Each result is recorded in `outcome.json`. Nothing here is
    destructive, so a check that cannot be satisfied does not abort. It is
    written down for `settle-in` to show, and Windows stays reachable from
    the GRUB menu either way.

**The user's files are not touched here.** No NTFS read, no BitLocker
unlock, no copy. Windows is left whole. The files come across in
`settle-in`, after the new system has proven itself and with Windows still
there as a complete fallback. That delay is the whole point: nothing
destructive happens on this path, and the commit line waits in
`settle-in`, at reclaim.

**Clean slate** (opt-in, or forced when the disk is too full to keep both)
is the only path that wipes, so the user is present, by design:

8. The live desktop asks for two minutes: speakers (*not headphones*),
   display, Wi-Fi. A human ear is the only test for a smart amp, and this
   is the last moment the system can refuse for free.

> **=== COMMIT LINE (clean slate) ===** everything below destroys something

9. Wipe, partition, install via kickstart with the chosen desktop.
10. Inject artifacts into the installed system before first boot.
11. Restore files from the stick, Wi-Fi profiles to NetworkManager
    keyfiles, browser profiles.
12. Wipe the credential part of the stick, but **leave the staged files**.
    Until the user says otherwise, the stick is their only backup, and
    `settle-in` tells them so.
13. Write `outcome.json` and logs to the stick. This is the only telemetry,
    and it stays there unless the user chooses to send it. Reboot.

**A note on a reversal.** An earlier revision rejected any single-disk
design. The reason given was the unattended resize-and-move, where a power
cut loses both the original and the copy. The keep-Windows path is not that
design:

- the resize is Windows' own shrink;
- the files are *copied* (in `settle-in`, later), not moved;
- the original NTFS data is never changed;
- and it is never deleted until the user says yes at reclaim.

### Rollback is a mode of upgrade_, not a fourth module

On the keep-Windows path, rollback means restoring the Windows boot entry.
It takes seconds, not hours, and can be reached from the stick and from
`settle-in`.

Concretely (decided 2026-08-30):

- put the Windows Boot Manager entry first in `BootOrder` (re-creating it
  if missing);
- restore Windows' own `EFI/Boot/bootx64.efi` from the step-8 snapshot;
- leave the Linux partitions and `EFI/fedora` in place until the user asks
  for the space back.

Rollback is a boot-order change, not a deletion.

**Built 2026-09-12, Windows side:** `upgrade_/windows/Invoke-Rollback.ps1`
(`ROLLBACK.cmd` on the kit stick, run from the kept Windows). It:

- reads the job, the outcome and the snapshot;
- refuses on a moved stick or a path that kept no Windows;
- restores `EFI/Boot/bootx64.efi` from the snapshot with its checksum
  checked (the shim copy is saved to the stick);
- puts `{bootmgr}` first with `bcdedit`;
- records `upgrade_/rollback.json`.

Rows are in `docs/validation-results/r21-rollback.csv`. The Linux-side twin
(`efibootmgr`, from `settle-in`) comes with `settle-in`.

Owning rollback here, rather than leaving it implied, is deliberate.
Recovery paths nobody owns get discovered by the person whose laptop is
already a brick. On the clean-slate path there is no rollback, which is
exactly why that path has a human gate before its commit line. The honest
fallback there is Microsoft's own install media (the digital licence
reactivates on the same hardware) plus the files still on the stick.

### Design constraint: offline

The live image carries Fedora as a squashfs (a compressed, read-only disk
image) on the USB. It carries both desktops, so the desktop choice costs a
question, not a download. It does **not** install over the network.

(Built 2026-09-09: the images are the unmodified `LiveOS/squashfs.img` out
of Fedora's own Workstation and KDE live ISOs, installed with kickstart
`liveimg --checksum=`. The stick's installer boot files come from the
netinst ISO, also unmodified. There is no image of our own making; the
desktop a person gets is exactly Fedora's.)

The machines most likely to need converting are exactly the ones with
Broadcom Wi-Fi, or a card too new for the shipped kernel. A network
installer fails hardest on the hardware we most expect to see. Going
offline costs stick space, and removes a whole class of mid-conversion
failure.

### Design constraint: one stick, honestly sized

`evaluate` works out what the stick must hold (live image, artifacts, and
on the clean-slate path the staged files). It refuses a stick that cannot
hold it, with the gap report saying what would fit.

**Built for the staged files (2026-09-26):** `harvest.stick_fit` sizes the
folders the way the prologue's staging lays them down on the stick volume
it stages to:

- each file rounded up to the volume's clusters (the smallest block the
  file system hands out),
- plus the directories and the manifest,
- or the prologue's own bytes × 1.02 if larger,
- plus 64 MB.

A FAT32 volume never fits a file over 4 GB. A clean-slate job must fit
(the schema refuses one that does not). A keep-Windows job records the
answer for the discard offer.

**Found while building it:** the prologue stages to the stick's FAT32 kit
volume, not to the exFAT `UPGDATA` partition the stick writer creates for
staging. The gate measures the volume actually used, so it is honest about
today's stick. Moving staging to exFAT is owed before the discard offer is
built.

On that path the stick is also, briefly, the only copy of the user's files.
That is why the read-back check in step 6 is a hard gate, not a warning.

### Current status

**The prologue exists as product code (2026-09-12):**
`upgrade_/windows/Invoke-Prologue.ps1`, grown from the V0 harness. It is
driven by the one-click `RUN-CONVERT.cmd` on the kit stick:

```text
scanner -> job writer -> kickstart -> the typed word -> the prologue
```

Steps 1, 1b, 2 (keep-Windows), 3 and 4 above are built with collect/judge
seams (live reads kept apart from the decisions, so the decisions can be
tested) and self-test cases (130 at 0.9.0, 2026-09-26).

On real hardware it has run seven times on the Aspire (RISKS R18): the disk
check, the pagefile rung, the stop that puts Windows back (proven), the
change-journal rung, and a correct refusal of keep-Windows on that disk.

The clean-slate half of step 2 stages files with checksums and a
measured-speed estimate, but then **stops** before the handoff. The live
session's human gate before the wipe is not built, and this code will not
arm an unattended wipe.

Decided (2026-09-12) while building it:

- Hibernation is switched off on every keep-Windows conversion (the kept
  volume must be mountable later).
- **The pagefile is turned off only when the cold measurement does not
  fit**, with one restart to re-measure. Otherwise the kept Windows stays a
  normal Windows.
- The shrink request is exactly what Linux needs (`linux_min_gb` plus the
  harvested bytes × 1.2), never the maximum, and the kept Windows must
  keep 8 GB free.
- A stop after the shrink grows C: back, re-enables BitLocker, removes the
  boot entry and scrubs the stick's credentials.

Added since:

- **Any stop puts hibernation and the pagefile back as the prologue found
  them** (0.5.1, 2026-09-20; they are recorded before they are touched.
  The Aspire's fourth run stopped with both left off.) The return after an
  install brings the pagefile back while hibernation stays off.
- **Restore points that pin the shrink are deleted** (0.6.0, decided
  2026-09-20) when Windows names their storage as the last unmovable file
  and the job carries the person's consent. The launcher's decision screen
  asks for it in plain words. It is the one pre-commit act a stop cannot
  undo (R18).
- **NTFS's change journal that pins the shrink is deleted** (0.8.0,
  decided 2026-09-22, the Aspire's fifth run) on the same terms: Windows
  names `\$Extend\$UsnJrnl:$J` as the last unmovable file, and the job
  carries the consent the same screen asks for. It happens at most once
  per boot, then one re-measure. Its sizes are read first, and it is
  created again right after the shrink and at every stop. What does not
  come back is the record of changes it held: search and sync programs
  look through the files again.
- **A Windows update waiting for a restart is let finish first** (0.9.0,
  decided 2026-09-26, RISKS R25). The prologue checks before it changes
  anything and again before the shrink. It restarts once itself (at most
  three times) to let the update finish, and carries on by itself. It
  refuses to arm the boot to the stick while an update waits.

The prologue's record (`upgrade_/prologue.json`) is what `%post`'s
`outcome.sh` carries into `outcome.json` as the `prologue` block. Rows:
`docs/validation-results/r18-prologue.csv`. The cutover (stage 2) is the
kickstart, `%pre` verifier and `%post` checklist proven in
`v2-install.csv`. Rollback's Windows side is built (`Invoke-Rollback.ps1`,
`ROLLBACK.cmd`, `r21-rollback.csv`); `settle-in` is not.

---

## Module 3: settle-in

**Runs on Linux at first boot. Starts by itself, once.**

It looks like a welcome screen. It is a safety gate.

1. **Check the hardware.** Speakers (*not headphones*), Wi-Fi, display and
   brightness, suspend/resume. A silent smart amp is the most common
   complaint after an install, and you can't see it if you only test with
   headphones plugged in.
2. **Bring the files home** (keep-Windows path). This is where the user's
   data arrives. It is safe to do it here rather than during cutover
   because the hardware is already proven, and Windows is still whole as a
   complete backup. Following the folder map from `job.json`, `settle-in`:
   - mounts the shrunk-aside Windows partition, unlocking BitLocker with
     the harvested recovery key (`cryptsetup` BITLK, Linux's reader for
     BitLocker volumes);
   - mounts it read-only through **ntfs-3g**, not the kernel `ntfs3`
     driver (decided 2026-09-01: the install kernel's `ntfs3` oopsed, that
     is crashed, on the rig; RISKS R19);
   - copies Documents, Pictures and the rest, plus Wi-Fi and browser
     profiles, into the new home, checking checksums as it goes.

   It expects cryptsetup's size-mismatch warning on every shrunk volume.
   It copies only regular files (Windows' empty SYSTEM files show up as
   FIFOs and would hang a plain open).

   **It never copies a OneDrive placeholder** (decided 2026-09-26, RISKS
   R8). The job counts them per folder. `settle-in` must recognise each one
   on the mounted NTFS, skip it, check its count against the job's, refuse
   the pull if they disagree, and then offer to sign in to OneDrive (a
   Linux client, or the web) where those files still are.

   Because the user is present, a stubborn unlock or a read error can be
   *asked about* rather than guessed at, which would not be possible on an
   unattended only copy. And because Windows is intact, a failure here
   loses nothing: the user restarts into Windows and tries again. (On the
   clean-slate path the files were already restored during cutover, so
   this step is skipped.)
3. **Confirm the data arrived.** Counts and checksums against
   `outcome.json`.
4. **Hand off.** Where files went, what replaced what, and what is gone
   and is not coming back. On the clean-slate path, also: **keep the
   stick.** It still holds their files, and it is their only backup until
   they say otherwise.
5. **Decide** (keep-Windows path). Keep, or roll back to Windows (a
   boot-menu restore, not an image restore). Only after the user confirms
   things work (the new system runs *and* their files came over) does it
   offer the **reclaim**: delete the Windows partition and grow into the
   space. It is stated plainly as the one irreversible act on this path.
   It is offered exactly once and never nagged. Saying no leaves a
   `reclaim` command behind for whenever they are ready.
6. **Exit.**

**And one standing job, for as long as Windows is kept** (decided
2026-08-30, RISKS R22). On every Linux boot, `settle-in` leaves behind a
unit (a small background service) that checks the firmware's `BootOrder`
and puts the Linux entry back first if a Windows session moved it. The rig
showed Windows re-registering its own firmware entry *and* taking first
place after a single servicing pass. After that, the machine boots straight
into Windows and Linux looks gone. The welcome screen says so in plain
words: "If Windows ever starts on its own after an update, hold [the
boot-menu key we detected] and pick Fedora once; it will stay put after
that." Reclaim removes the unit along with Windows.

### It runs on any Linux (decided 2026-09-27, the owner)

Fedora is only the first Linux this project installs. The person should be
able to pick any Linux later, so nothing in `settle-in` may assume Fedora.
The owner's words: this is the platform on which any device and any
distribution is supported in an organized way. RISKS R28, VALIDATION V10.

The problem: no window toolkit is on every distribution. GTK is not
guaranteed (Kubuntu's Firefox is a snap that brings its own copy), Qt is
not on GNOME distributions, and some minimal installs have no Python. And
the new system is offline at first startup: it cannot download what is
missing. What every desktop Linux does share is the kernel and a few
standard pieces.

So `settle-in` is built in three layers:

```text
 per distribution (small)        the same everywhere (one program we ship)
 ------------------------        ------------------------------------------
 installer adapter               settle-in
   Fedora: kickstart %post  -->    reads one fixed folder (job, secrets)
   later: Ubuntu, Debian ...       works through kernel interfaces
   (the only part that knows       draws its own window, or asks in text
    the distribution's installer)  when the person chose the console
```

1. **One handoff folder, the same on every distribution.** Each
   distribution's installer adapter does two things only: put the job and
   the secrets in one fixed place on the installed system (root-only), and
   switch on `settle-in`'s start-at-boot service. Fedora's adapter is the
   kickstart the converter already writes. `settle-in` reads that folder
   and never asks which distribution it is on.
2. **One self-contained program.** `settle-in` is a single compiled file,
   carried on the stick and copied over by the adapter. It needs nothing
   the distribution may or may not have: it draws its own window (needing
   only the display and Mesa, the graphics library every desktop has), and
   the same file asks its questions as text when the person chose the
   console. It does its work through the kernel's own interfaces where it
   can: the hardware clock through `/dev/rtc0` (no `hwclock`), firmware
   boot entries through `/sys/firmware/efi/efivars` (no `efibootmgr`).
   **Language: Rust (decided 2026-09-27, the owner).** The compiler
   refuses whole classes of mistakes before the program ever runs, and its
   window libraries load the display libraries only when they are there.
   The cost is a harder read for newcomers and more third-party packages to
   review, so dependencies are kept to a short, named list.
   **The window: egui (decided 2026-09-27, the owner).** A fully static
   program cannot load the system's display libraries while it runs, so
   the window is a second, small program beside the static core. It opens
   at the person's first sign-in through the standard autostart folder
   (`/etc/xdg/autostart`, which GNOME, KDE and the others all read), shows
   what the first startup did, and ends with the old-boot-entry button,
   which asks for the person's password through polkit (the standard
   administrator prompt on every desktop). When the person chose the
   console, the same content comes as text at their first console
   sign-in.

   **The window's words (approved 2026-09-27, the owner), verbatim.** They
   live in one place, `settle-in summary`, which both the window and the
   console print:

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

   Also approved (2026-09-27, the owner), verbatim: a network Windows did
   not connect to by itself ("These networks are set up; connect to them
   from the network menu:"); no NetworkManager ("Not set up: <reason>.");
   after the button ("Removed." / "Not removed: <reason>"); the console's
   question ("Remove the old Windows startup entry now? Type yes and press
   Enter (anything else skips):"); and the password prompt, through a
   polkit policy file that applies only to the button's command
   (`org.upgrade.settle-in.policy`): "Removing the old Windows startup
   entry changes this computer's startup settings. Type your password to
   allow it."

   **The window asks to come first** (decided 2026-09-27, the owner). KDE's
   Welcome Center and GNOME's tour open at the same sign-in. The window now
   opens 5 s after sign-in, asks the desktop for focus, and asks for
   attention as the fallback. A desktop may refuse, so the window logs what
   it got ("in front" or "not given focus"), and the rig records it.

   **Our own firmware entry goes at first start** (decided 2026-09-27, the
   owner). The one-time "upgrade_" entry the prologue made to reach the
   stick is left behind by an erase. `settle-in` removes it automatically,
   because nobody would want it. It is found only by the exact BCD id the
   prologue recorded (`outcome.prologue.handoff.entry_guid`, which the
   entry carries as "BCDOBJECT={id}"), with the button's order-first
   writer.
3. **A floor it checks, and refuses below.** On start it checks for UEFI,
   systemd (what starts services at boot), NetworkManager (the Wi-Fi
   manager nearly every desktop distribution uses; its connection files are
   the same format everywhere) and a display. Anything missing gets a plain
   sentence, never a guess: a distribution that uses another Wi-Fi manager
   has its networks listed, not set up. Each supported distribution becomes
   a row in `data/distros.ps1`, with evidence from a real install.

What stays Fedora-specific, on purpose: the installer adapter (kickstart,
`%pre`, `%post`), which lives in `upgrade_`, not here.

### The clock, Wi-Fi and the old boot entry (decided 2026-09-26, the owner)

Found on the Aspire's run 9: the installer's clock was 4 hours behind, and
a stale "Windows Boot Manager" entry was left in the firmware after the
erase.

The owner's decisions follow one idea: **what only Windows knows is
harvested on the Windows side; `settle-in` applies it on first startup
without asking; anything the person might want to keep is a button, never
automatic.**

- **The clock.** `evaluate` harvests Windows' clock facts into
  `harvest.clock`:
  - the Windows time zone and its IANA name (the standard name Linux uses,
    like `Europe/London`),
  - whether the hardware clock holds local time (Windows' default:
    `RealTimeIsUniversal` absent or 0),
  - the UTC offset at harvest.

  On first startup `settle-in` reads the hardware clock as the local time
  it is, converts it to UTC, sets the system clock, and stores the
  hardware clock as UTC from then on.
  **Corrected (2026-09-27, found while building it):** the conversion uses
  the offset Windows was using when it last ran (harvested), not the zone's
  rules at first startup. The hardware clock holds whatever offset Windows
  last wrote, and nothing moves it after Windows has gone. With the zone's
  rules, a first startup after a daylight-saving change would be off by an
  hour. The zone's rules (from the installed system's own tzdata) are a
  cross-check: if they disagree with Windows, or daylight saving changed
  between Windows' last run and the install, the clock is left alone.
  **It decides from evidence, never assumes.** The installer adapter
  records, at the end of the install, the installer's clock, the hardware
  clock, and whether a time service had synchronized (`cutover.clock`). A
  synchronized installer may already have rewritten the hardware clock as
  UTC; correcting it again would move a right clock by hours. Readings
  that match neither story, or an unknown sync state, leave the clock
  alone, and so does a time service that already ran this startup. Once
  online, the time service corrects a clock left alone. It marks the
  attempt before touching anything, so an interrupted run never corrects
  twice. This happens before anything time-sensitive runs,
  and before the network (whose time service corrects the rest). The
  records the installer wrote with the shifted clock are corrected by the
  same offset in `settle-in`'s report, never silently rewritten.
- **Wi-Fi.** `evaluate` exports the saved Wi-Fi profiles with their
  passwords into `artifacts/credentials/wifi/` on the stick, one file per
  network that has a password. `job.json` lists the networks
  (`harvest.wifi.profiles`) and, for each, only its file's path (RISKS
  R13: secrets are files, not fields).
  **Not with `netsh` (found 2026-09-27 on the G16).** `netsh wlan export`
  writes all profiles at once and shortens file names to fit the folder
  path. Two names that start alike get the same file, and the second
  silently overwrites the first: 14 networks became 13 files. So the job
  writer reads each network by name from Windows' Native Wifi API (the
  interface `netsh` itself uses) and counts them against the profile files
  Windows stores on disk. If the two counts differ, it refuses. On the G16
  both said 14, and 10 had a password. (The G16 also returned the
  passwords in clear without elevation; the job writer runs elevated
  anyway.) WPA3 in "transition mode" (the router also accepts WPA2; 9 of
  the G16's 10 WPA3 networks) is set up as WPA2-personal, which such a
  router accepts. The export runs only after every other check passed, so
  a refused job never leaves passwords on the stick. The launcher says plainly, before anything is
  written, that the Wi-Fi passwords are copied onto the stick. It uses the
  owner's approved words (2026-09-27), shown verbatim on every install
  launcher before the harvest runs: *"Your saved Wi-Fi networks and their
  passwords are copied onto this stick, so Fedora can connect to them on
  its own. They are removed from the stick at the end of the install and
  from Fedora once it has set them up."*
  At the end of the install, the nochroot `%post` moves them to a
  root-only directory on the installed system and deletes them from the
  stick. On first startup `settle-in` creates the NetworkManager
  connections and deletes its copy. It creates the ones Linux can join
  (WPA/WPA2/WPA3 personal and open); enterprise networks are listed, not
  guessed. The machine that ends up holding the passwords is the one that
  already knew them, and the stick stops carrying them within minutes.
  **Every stop removes them from the stick too** (decided 2026-09-27, the
  owner): a prologue stop, a cancelled countdown, a return to Windows, an
  abort, or a launcher that fails after the job was written. A re-run
  exports them again. The sentence is shown in READ THIS FIRST (on
  `RUN-CONVERT.cmd`, which has no such block, in the same place).
- **The old boot entry.** At the end of `settle-in`, on the erase and
  clean-slate paths only, there is a button: "Remove the old Windows
  startup entry". It removes firmware entries that point at a Windows boot
  loader on a disk that no longer has one. It never does this while a
  Windows ESP still exists (keep-Windows keeps its entry by design,
  R21/R22).

**Built (2026-09-27): the Windows side.** Job writer 0.15.0
(`harvest.clock`, `harvest.wifi`, 21 self-test cases, a live read on the
G16), prologue 0.11.0 (the stop rule), the sentence on the four install
launchers. `[##..]` built, not yet fired on the rig. Still to build: the
installer adapter's hand-over (`%post`), then `settle-in`'s first-startup
service (clock, Wi-Fi) on the rig, then the button with `settle-in`'s
window.

### The way back to Windows (decided 2026-09-27, the owner)

On the keep-Windows path the way back already exists: step 5 above offers
"keep, or roll back" until the person says yes to the reclaim. On the
erase path, and after a reclaim, there is no Windows left to roll back to.
**Decided: there is still a way back, and it is honest about what it
costs.** "You can always go back" is what gets a nervous person to try
Linux at all, and someone who could not make a Windows stick is exactly who
this project is for. If Linux does not suit them, they need the same help
getting back. RISKS R30, VALIDATION V11.

- **It is its own program, "Go back to Windows", in the app menu,** for as
  long as the person wants it. It is not in the first-start window: the
  first start is about settling in, and a way out on the first day sends
  the wrong message. It ships with `settle-in` (the same package, any
  Linux).
- **It says the cost first.** Going back means a new, empty Windows.
  Nothing on the Linux side comes along unless the person copies it off
  first. **The catch:** most of these computers cannot run Windows 11.
  That is why this project exists. For them, going back means Windows 10,
  whose free security updates ended in October 2025 (the paid extension
  for home users ends in October 2026). It is their computer and their
  choice; the screen says it plainly.
- **It shows what Windows told us before it was erased**
  (`harvest.windows_license`, kept in the installed system's root-only copy
  of `job.json`): the edition, 10 or 11, whether it was activated and how.
  What that means for activation after a reinstall is said only where a
  primary source says it (R30 names the unknowns).

It is built in two stages:

1. `[##..]` **A guided stick.** The person downloads Microsoft's own
   installer in their own browser, from Microsoft's page (we never
   redistribute it). The program checks the file, writes it to a USB
   stick the person names, and shows the steps in plain words (restart,
   the maker's boot key, remove the Linux partitions, install). It writes
   only to that stick, never to an internal drive, with the stick writer's
   refusals (R16) on the Linux side. Nothing on the computer changes until
   the person starts the Windows installer themselves.

   **Design (2026-09-27), from what Microsoft's pages show.** Read that
   day with a Linux browser's identity:
   - Microsoft still offers both: the Windows 10 page (edition "Windows 10
     (multi-edition ISO)", 22H2) and the Windows 11 page ("multi-edition ISO
     for x64 devices"). Its download link is made by the page's scripts
     after the person picks the edition and language, and lasts 24 hours.
     So the person downloads; the program does not drive Microsoft's page
     (that would lean on an interface nobody promised to keep).
   - Both pages print a SHA-256 for every language ("Verify your
     download"). `settle-in/tools/refresh-windows-media.py` copies those
     tables into `settle-in/data/windows-media.json` (114 rows). A file not
     in the table is refused, and so is the 32-bit Windows 10 file and the
     wrong Windows for this computer.
   - Which Windows to offer: 11 if Windows 11 ran here before, or if Linux
     can see TPM 2.0 and UEFI (Microsoft's installer then checks the
     processor); otherwise 10, with the October 2025 / October 2026 warning.
   - The 4 GB limit: Microsoft's own instructions for a stick
     (learn.microsoft.com, "Install Windows from a Flash Drive") say FAT32,
     copy everything but `sources\install.wim`, and split that into
     `install.swm` parts ("Windows Setup automatically installs from this
     file"). On Linux the split is `wimlib-imagex split`. Whether a
     wimlib-made split installs cleanly, Secure Boot on, is R30's open
     question.

   **Built (2026-09-27, the read-only half, `settle-in go-back`):**
   `screen` (the cost first, which Windows it was, which to download;
   draft words), `check` (the file against Microsoft's table),
   `downloads`, and `sticks` (every disk with the rules it breaks; the
   Aspire's two internal drives, recorded and replayed in a test, are both
   refused). Read on the Aspire under Fedora: it offers Windows 11 (TPM
   2.0, UEFI, Secure Boot on, a key in the firmware; i7-8550U).
   **The writer and the window, built the same day** (`go-back write`, as
   root through its own password prompt; the window's `--go-back`, in the
   app menu as "Go back to Windows"). The writer finds the stick again by
   serial, exact size and the typed model name, writes a DOS table and
   FAT32, copies, splits `install.wim` with `wimlib-imagex` (static,
   GPL-3.0, built from a pinned source and carried on the kit), and reads
   every file back. `[###.]` on the rig (`v11-way-back.csv` line 3):
   Windows 10 installed from it, Secure Boot on. A real stick in a real
   machine is still owed, and the window has not been seen on a desktop.
2. `[#...]` **Later, a walk-away reinstall.** An unattended Windows install
   (an answer file on the stick), with its own typed sentence and a
   countdown as its commit line: the erase path in reverse. It deletes
   Linux and `/home`, so under rule #4 it is built last and reviewed
   hardest. It does not jump ahead of the current work.

   **Decided (2026-09-29, the owner): the way back is 100% managed, and it
   comes now.** The owner wants to reinstall the Aspire's Windows with it,
   so it moves ahead of the other work. Three answers, all the same day:

   - **Windows comes from Microsoft's catalog, not a browser.** This
     reverses the 2026-09-27 choice. Microsoft's own Media Creation Tool
     reads a signed catalog, `products.cab`, reached through
     `go.microsoft.com/fwlink` and served by `download.microsoft.com` over
     HTTPS. It lists every Windows 10 22H2 and Windows 11 file by edition
     and language, with a direct link, its size and its SHA-1 (read
     2026-09-29: Windows 11 is 24H2, build 26100.4349; Windows 10 is
     19045.3803). The file is an `.esd` (Windows' compressed image format),
     not an ISO. Its host, `dl.delivery.mp.microsoft.com`, refuses HTTPS
     (seen 2026-09-29), so the file comes over plain HTTP and is kept only if
     its size and SHA-1 match the catalog fetched over HTTPS (RISKS R33).
   - **Both internal drives are erased.** Windows goes on the system drive.
     The second drive is wiped and left as one empty NTFS drive, so Windows
     shows it as an empty drive and nothing from Linux is left behind. The
     same shape as the forward erase, and the screen says it first.
   - **The commit line is on the stick, before Windows Setup.** A small
     program of ours (the gate) runs first inside Windows Setup's own
     environment (WinPE, the small Windows that runs Setup). It is built
     with the same Rust toolchain as `UPGRADE.exe`.

   **How it runs (design, 2026-09-29; nothing built):**

   ```text
   Linux, the "Go back to Windows" program            (nothing changed yet)
     cost first -> which Windows -> both drives named -> the typed sentence
     -> choose the stick (R16's rules) -> download from Microsoft's catalog
     -> check size + SHA-1 -> build the stick from the .esd -> read it back
     -> a one-time boot entry for the stick (BootNext) -> restart
   The stick, WinPE: the gate                         (nothing changed yet)
     find each drive by serial and exact size, or refuse (plain screen,
     restart into Linux) -> 2-minute any-key countdown (a key: restart into
     Linux, untouched)
     ---- countdown ends: the commit line ----
     record the crossing on the stick -> write the answer file naming the
     drives it found -> Windows Setup, unattended: wipe both drives,
     install -> first start creates the account -> the Windows sign-in
   ```

   - **The stick is built from the `.esd`** with `wimlib-imagex` (already on
     the kit): its first image is Setup's files (copied to the stick), its
     second and third make `sources\boot.wim`, and the one edition this
     computer had goes into `install.wim`, split into `install.swm` parts
     under FAT32's 4 GB (the split V11's rig leg already proved).
     `boot.wim` is changed in one way: it starts the gate instead of Setup.
   - **The job travels on the stick** (`upgrade_\go-back.json`): each drive
     by serial, size and model, the sentence, the edition, the account
     name. The gate refuses a job it cannot match to this computer.
   - **No password rides on the stick.** The answer file creates a local
     account with the person's Linux name and no password, marked "change
     at first sign-in", so Windows asks for a new password the first time.
     Unproven (R33).
   - **No product key is written anywhere.** Windows Setup reads a
     firmware key itself (R13). A computer without one may stop at Setup's
     key page, which would break walk-away: the rig has no firmware key,
     so it shows what happens (R33).

### Scope boundary

`settle-in` does not teach Linux, install applications, run a tour, or
check in later. Checking the hardware and bringing the files home are the
two reasons it exists. Turning it into a welcome experience would water
them down. The data pull belongs here only because this is the first
moment it can happen safely (Linux proven, Windows intact), not because
`settle-in` is a general migration tool.

### Current status

Nothing built.

---

## Contracts between modules

The modules talk to each other only through files on the stick:

| File | Written by | Read by |
|---|---|---|
| `job.json` + `artifacts/` | evaluate | upgrade_, **settle-in** |
| `outcome.json` + logs | upgrade_ | settle-in |
| `/var/lib/upgrade_/` on the installed system | the installer adapter (Fedora: `outcome.sh`) | settle-in |

`settle-in` reads `job.json` too, not just `outcome.json`. On the
keep-Windows path it needs the harvested folder map (and the BitLocker
recovery key) to pull the user's files from the mounted Windows partition.
The stick is still there at first boot, so this needs no new carrier. But
it does mean `job.json` outlives cutover, and the timing of the credential
wipe has to allow for that: the recovery key can only be scrubbed **after**
`settle-in` has finished the pull, not at the end of cutover.

**The handoff folder (built 2026-09-27, `outcome.sh` 0.4.0).** At the
end of the install the adapter copies `job.json` and `outcome.json` into
`/var/lib/upgrade_/` on the installed system, root-only (folders 0700,
files 0600), laid out like the stick's `upgrade_/` so the job's relative
paths resolve the same. The Wi-Fi password files go to
`artifacts/credentials/wifi/` there, each checked by SHA-256, and are then
removed from the stick. `outcome.json` records what moved
(`credentials.wifi`) and the clocks at the end of the install
(`cutover.clock`). This is the one place `settle-in` reads, on every
distribution, so the stick can already be gone at first boot. Tested
against a fake stick and system (both the normal case and a job naming a
file the stick lacks); not yet fired on the rig.

Both schemas are versioned. A USB written by one release will one day be
read by another. A module that meets a version it does not understand must
refuse, not guess.

---

## Interface and privilege model

### One elevation, no stored credentials

It ships as an `.exe` whose manifest declares `requestedExecutionLevel =
requireAdministrator`. Windows asks once, at launch, and every privileged
operation runs inside that process.

Measured on a stock Windows 11 machine: user in the Administrators group,
unelevated token, `ConsentPromptBehaviorAdmin = 5` (the default).
Elevation is **a single Yes click, not a password prompt**, for the vast
majority of home users.

**Techniques we will not use:** `runas /savecred`, scheduled tasks with
"run with highest privileges", COM elevation moniker abuse. All are known
UAC-bypass tricks that Defender targets by name. They would get the tool
flagged *and* really weaken the machine, for nothing the manifest doesn't
already give.

**Amended (2026-09-13):** that rule is about *getting* elevation without
the consent click. The prologue does register one elevated scheduled task.
But it does so from a run that already holds UAC-consented elevation, only
to survive its own restart. It is one-shot and removed by every exit path.
Since 2026-09-13 it runs as SYSTEM at startup (below, "the walk-away
resume"). The directory it runs from is locked to SYSTEM and Administrators
before the task exists, so it grants nothing to anyone who did not already
click Yes.

**The walk-away resume (decided 2026-09-13).** The prologue's restarts (the
disk check, the pagefile re-measure) come back to Windows before the
handoff. Everything after the typed word must run with nobody at the
keyboard. The fork is pre-chosen in `job.json`, so nothing waits on a
person. So the resume:

- runs as SYSTEM at startup, before and without a sign-in;
- finds the stick by volume id (polled, because USB shows up late at boot);
- queues anything a person should read as a one-shot `RunOnce` notice
  shown at their next sign-in, since session 0 (where system services run)
  has no screen.

**What we will not do instead:** take the person's Windows password to
automate the sign-in.

- Microsoft-account holders often sign in with a PIN and may not know the
  password.
- Windows 11 accounts can be passwordless.
- Autologon stores the secret on a disk that `settle-in` later mounts from
  Linux.
- A third-party tool asking for a Microsoft password looks exactly like
  phishing (below).

The plain alternative, `shutdown /g` (Automatic Restart Sign-On, which
Windows Update uses), is a courtesy to add later. The SYSTEM resume makes
it unnecessary for the mechanism.

**We never ask for a Microsoft account password.** A Microsoft-branded
password box looks exactly like phishing, and it is not needed:
`manage-bde -protectors -get C:` returns the recovery key with local admin
alone.

**The only credential we create** is the new Linux account password. It is
typed once and hashed straight away to SHA-512 crypt, and only the hash
reaches the USB.

**Prompt budget for the whole conversion: four.** (The SYSTEM resume adds
none: the restarts need no sign-in, and its notices are queued, not
asked.)

1. One UAC consent click.
2. One confirmation that the stick about to be written is the right device.
3. One password the user chooses.
4. One polkit prompt (Linux's admin-permission box) in `settle-in` at
   reclaim.

(The clean-slate path adds its two-minute live-session hardware check,
which is a gate, not a prompt. The offer to discard Windows after a failed
shrink, designed 2026-09-26, adds one typed sentence and one UAC consent,
and only on that branch; see "When Windows cannot be kept" above.)

### Stack

**Decided (2026-09-27, the owner): a Rust window, `UPGRADE.exe`, in front
of the scripts on the stick.** It replaces the 2026-09-13 plan (WPF hosted
in Windows PowerShell 5.1 now, C# WPF on .NET Framework 4.8 later).

- It is one `.exe` at the stick's root. It **double-clicks**, asks for
  administrator access once, and needs nothing installed.
- It draws with egui, the same library as `settle-in`'s window, so the
  person sees one kind of window before and after the switch.
- **It directs the work; it does not redo it.** It runs the kit's scripts
  as child `powershell.exe` processes with the same arguments the `.cmd`
  launchers use, shows their progress in plain words, and stops where they
  stop. The scripts stay the thing under test, and `data/*.ps1` stays the
  community's edit surface.
- It adds exactly two things of its own. It stops on a RED scan before the
  job writer runs (more cautious than before, never less). And before the
  restart it registers a one-shot sign-in task that opens it again, so the
  person sees the result without looking for it. That task has the shape of
  `Test-Handoff.ps1`'s return-check task, which fired on the Aspire. A
  restart the window cannot follow is not started.
- **What it cannot do:** show anything before a sign-in. The walk-away
  resume runs as SYSTEM with no screen (above); the window reopens at the
  next sign-in and shows where things stand.
- If its graphics cannot draw on a machine, it says so in a plain Windows
  message box and points to the `.cmd`, which stays on the stick.
- Built from WSL for `x86_64-pc-windows-gnu`, linked by zig through
  `cargo-zigbuild` (`upgrade_/windows/window/build.sh`); its logic is
  tested on Linux, and `make-kit.sh` checks it starts on Windows.

**First slice (2026-09-27): the verify flow only** (`RUN-VERIFY.cmd`:
nothing installed, nothing on the internal drive changed). The convert and
erase flows, with their typed words, come after it has earned its rows
(RISKS R31, VALIDATION V12). Its words are drafts until the owner approves
them. Rust does not change code signing (R12): the `.exe` still needs a
certificate and time.

Not WebView2: it is present on Windows 11, but not guaranteed on
Windows 10.

**Decided (2026-09-27, the owner): Rust becomes the conversion's one
language,** from the window down to the code that touches the disk. The
PowerShell side is ported piece by piece, safest first (RISKS R32,
VALIDATION V13):

1. the `job.json` / `outcome.json` types, as one library shared with
   `settle-in`;
2. the scanner's judging half, fed the same corpus recordings;
3. the read-only collectors (the scanner's reads, the harvester);
4. the job writer, the stick writer and the kickstart generator;
5. the prologue, the handoff and the rollback, last.

Steps 3 to 5 wait for V0's three more vendors and V9's physical re-run.
Until then, and for each piece until its lines in
`docs/validation-results/port-parity.csv` all read `pass`, the window
keeps calling that piece's script as described above, and the `.cmd`
launchers stay on the stick. So today the scripts are still the thing
under test. Builds are made reproducible, so the `.exe` on the stick can
be matched to the open source (R14).

### Code signing is the gating item

See RISKS R12. The finished tool elevates, reads BitLocker keys, exports
Wi-Fi passwords, writes raw USB devices, resizes partitions and rewrites
boot configuration. By behaviour alone, that is an exact match for an
infostealer followed by ransomware. Unsigned, Defender may quarantine it,
and the target user stops there for good. It needs an OV/EV certificate, a
legal entity, and a reputation that only builds up with time. Start before
there is anything to sign.

---

## What migrates, and what silently doesn't

| Item | Ports? | Notes |
|---|---|---|
| Documents, Pictures, Desktop, Downloads, Videos, Music | yes | Found via the known-folder APIs, so OneDrive redirection is handled. |
| Wi-Fi networks + passwords | yes | `netsh wlan export profile key=clear` -> NetworkManager keyfiles. |
| Enterprise / 802.1X Wi-Fi | **no** | Certificates and sign-in methods don't map cleanly. Flag and skip. |
| Firefox profile | yes | Bookmarks, history, extensions **and saved passwords**: the NSS key database works on any platform. |
| Chrome/Edge bookmarks, history, extensions | yes | The profile directory ports. |
| Chrome/Edge **saved passwords** | **no** | Encrypted with DPAPI (Windows' own key store); no Linux equivalent. They silently will not appear. Must be said in `evaluate`, not discovered in `settle-in`. |
| OneDrive cloud-only files | **needs care** | Placeholders copy as empty files, and no Linux-side reader can fill them. Decided 2026-09-26: they are not downloaded. The job records them as left in the cloud, `settle-in` reconnects OneDrive, and nothing may copy a stub as if it were the file. RISKS R8. |
| Installed applications | no | Out of scope for v1. |
| Windows settings, Outlook data, licences | no | Say so plainly in the handoff sheet. |

## Non-goals (v1)

- Any distribution other than Fedora, **in v1.** The seams are cut for
  more: the distribution lives in `job.json`, the kickstart generator is
  per target, and nothing outside `upgrade_/linux/` knows what is being
  installed. Multi-distro is an intended open-source future, not a v1
  promise.
- Dual-boot as a product. The default keep-Windows path holds Windows
  *for a while*, as a rollback and a file source that both end at reclaim.
  It is not a supported two-OS machine, and `settle-in` nudges toward
  reclaim once the files are across and the system is confirmed.
- Migrating Windows applications.
- Machines `evaluate` flags RED. No override, ever.

---

## Platform scope: beyond Windows PCs (decided 2026-09-27, the owner)

Smart TVs, and devices beyond Windows PCs in general, are on the long-term
plan. The research that starts it is `docs/research/device-feasibility.md`:
every device family scored on five doors (a Linux build, firmware that
boots something else, a way in from the old system, drivers, a way back),
using only doors the maker opens on purpose. It is research, not evidence
(rule #2). Headlines as of 2026-09-27: Intel Macs without T2 look like the
best next source; every smart TV checked is blocked (app developer modes
only), so the TV story today is a converted laptop plugged into the TV.
The step-by-step path for each feasible family (old Surfaces, x86
handhelds, Intel Macs, Snapdragon X, T2 Macs, and the TV choice) is in
`docs/research/future-paths.md`. Nothing here changes v1's scope.

## Platform scope: Apple hardware

There are three different cases with three different answers, and the
deciding reason is not the one people expect.

| Mac | Answer |
|---|---|
| Apple Silicon (M1 onward) | Long term: in scope by wrapping Asahi's installer, never replacing it (amended 2026-09-27). |
| Intel with T2 (2018-2020) | Out of scope for v1. |
| Intel without T2 (pre-2018) | Not v1; revisit after the Windows path ships. |

### Apple Silicon (M1 onward): out of scope, and not because Apple blocks it

It is worth being precise here, because the obvious assumption is wrong.
Apple Silicon Macs boot other operating systems as a **designed feature**.
From the Asahi Linux project's own description:

> "Apple allows booting unsigned/custom kernels on Apple Silicon Macs without a
> jailbreak! This isn't a hack or an omission, but an actual feature that Apple
> built into these devices. That means that, unlike iOS devices, Apple does not
> intend to lock down what OS you can use on Macs (though they probably won't
> help with the development)."
> (<https://asahilinux.org/about/>)

Two things do clash with this project's specific promise.

**You cannot fully replace the OS.** Apple firmware must stay on the disk:

> "2.5GB of this is used for the 'stub' macOS partition, that includes critical
> components such as Apple's bootloader and firmware, and a full copy of the
> macOS recovery image. This is required by the design of these platforms."

Note the detail: the required piece is the 2.5 GB stub, not a full macOS
install. Keeping full macOS is currently *recommended* because firmware
updates cannot yet be applied from Linux. Asahi expects that to change, and
at that point they will "be comfortable recommending Linux-only setups." So
this is a statement about today, not a permanent fact.

**Walk-away is impossible.** Changing boot policy needs 1TR (One True
Recovery): physically holding the power button and entering an admin
password. No software can automate that.

But the deciding reason is neither of those. **Asahi already does this, and
does it better than we would.** Their installer handles partitioning, boot
policy and the stub correctly, backed by the kernel and GPU work that makes
the machines usable. Building a rival path would hand users a worse version
of something that already exists and is actively maintained.

**Decision: never in scope. Refer people to Asahi.** That is the outcome
that serves the user, and the scanner should say so by name.

**Amended (2026-09-27, the owner): every Mac, eventually.** The reason
above still stands, so upgrade_ will not build its own Apple Silicon
installer. Instead, long term, it **wraps Asahi**: upgrade_'s scan and
refusals, the harvest into `job.json`, and `settle-in` on first start, with
Asahi's own installer doing the partitioning, boot policy and stub in the
middle. Fedora Asahi Remix is Fedora, and `settle-in` runs on any Linux
(R28), so the back half carries over. What stays true: the 1TR step is done
by a person, so this path is never walk-away, and it says so up front. The
path, and a map of every Mac era, is in `docs/research/future-paths.md`
(Path F). Not v1.

### Intel Macs with T2 (2018-2020): a hard maybe

The T2 chip owns the SSD controller, keyboard, trackpad, audio and camera.
Linux needs out-of-tree drivers (the `apple-bce` work from the t2linux
project). Secure Boot must be turned off through Startup Security Utility
in recovery. Audio support has historically been partial.

It can be done, but it is a project rather than an install, and "click
convert and walk away" is not an honest description of it. Out of scope
for v1.

### Intel Macs without T2 (pre-2018): the genuinely interesting case

These are ordinary x86 machines with conventional firmware, among the
easiest Linux targets there are. The main pain is Broadcom Wi-Fi, which
`data/devices.ps1` already covers, because it is the same chip family that
plagues PC laptops.

And the timing matches this project's founding argument exactly. macOS
Tahoe is the final release supporting Intel Macs, and it supports only four
models:

- the 2020 iMac,
- the 2019 16-inch MacBook Pro,
- the 2020 four-port 13-inch MacBook Pro,
- the 2019 Mac Pro.

Every other Intel Mac is *already* out of support.

That is the same situation that started this project (working machines
declared obsolete by a vendor's roadmap, with no security updates and no
upgrade path), with a different logo on the lid. Unlike Apple Silicon,
nobody is serving these users well right now.

### What supporting Intel Macs would actually cost

Not a port. A second, parallel implementation:

- **`evaluate/macos/`**: every way of collecting information differs.
  `system_profiler`, `ioreg`, `diskutil` and `security` replace WMI, the
  registry, `netsh` and `manage-bde`.
- **No `bcdedit` equivalent.** The one-time UEFI boot entry is what makes
  walk-away possible on Windows. macOS has `bless` and Startup Disk, which
  behave differently and need their own design.
- **FileVault instead of BitLocker**, with a different key-escrow model
  (a different way of storing the recovery key).

It would share the philosophy, the risk register and `data/`, and almost no
code.

**Decision: not v1.** Revisit pre-T2 Intel Macs after the Windows path
ships and has real-world conversion data behind it.

---

## Layout

```
data/            knowledge base - community PRs land here, expected to churn
  devices.ps1      hardware: Wi-Fi, GPU, audio, storage quirks
  distros.ps1      distribution kernel table
schemas/         job.json / outcome.json contracts - change rarely, review hard

evaluate/        module 1 - read, capture intent, refuse
  windows/
upgrade_/        module 2 - do the conversion; holds the commit line
  windows/         prologue (reversible): image, stage, boot handoff
  linux/           cutover (irreversible): partition, install, inject, restore
settle-in/       module 3 - verify, hand over, stop
  linux/

dist/            built single-file artifacts users download
docs/            architecture, risks
build.sh         inlines data/ into dist/
```

Three decisions worth stating, because they will be questioned:

**Module first, platform second.** Modules cross platforms: `evaluate` is
Windows-only, `upgrade_` starts in Windows and finishes in Linux, and
`settle-in` is Linux-only. A platform-first tree would scatter one module
across two roots and hide the pipeline. A flat module tree would mix
PowerShell and Python/bash, with different toolchains, in one directory.
`upgrade_/windows/` and `upgrade_/linux/` keep the mental model and
separate the toolchains.

**`data/` is top-level, not inside `evaluate/`.** It is where most
contributions land, and "adding a device is a one-line PR" only works if a
newcomer finds the file in seconds. It is also truly shared: `upgrade_`
needs the firmware mapping to inject, and `settle-in` needs the quirk data
to know what to check.

**`schemas/` is not merged into `data/`.** They change in opposite ways.
`data/` should change constantly through drive-by PRs. `schemas/` are the
contracts between modules and must change rarely and under careful review.
Different review bars deserve different directories, and a shared junk
drawer would blur them.

## Build order

0. **The spine spike, before anything else:** a hello-world conversion in
   a VM (`bcdedit` handoff -> live boot -> kickstart -> reboot into
   Fedora). No data, no artifacts, throwaway machines only. Walk-away rests
   entirely on the handoff working (RISKS R15), and this project closes
   risks with evidence, not argument, including its own.
1. `job.json` schema and the `evaluate` contract
2. Artifact extraction (vendor firmware): the piece that cannot be added
   later on a real user's machine
3. Kickstart generator: `job.json` -> kickstart (a per-target seam; Fedora
   first)
4. Live image: Fedora squashfs (both desktops) + orchestrator
5. Inject stage (artifacts before first boot); clean-slate restore from the
   stick; stick authoring in `evaluate`
6. `settle-in`: hardware checks, **the keep-Windows file pull** (mount
   NTFS, BITLK unlock, copy + checksum; the default path's data migration
   lives here, not in cutover), and reclaim
7. Shrink, boot handoff hardening, boot-entry rollback: **reviewed
   hardest.** These are the components that write to the internal disk.

**Decided (2026-09-08): build a vertical, then the matrix.** The list above
is by component. Every gate so far tests one component on its own, but the
failures that end a product like this live *between* components: the
folder map the Linux side cannot parse, the checksum taken before OneDrive
finished materializing, the stick that checks out on the machine that made
it and not on the target. So the order is now run as one **front-to-back,
one-click vertical** (a thin slice through every stage, rather than one
stage done fully), split at the commit line:

- **Reversible half first.** Plug in, one double-click, one UAC consent,
  walk away, come back to a verdict with Windows untouched:

  ```text
  schemas (step 1)
    -> evaluate's harvest, including OneDrive materialization (V8)
    -> the stick writer (R16)
    -> live image + kickstart generator (steps 3-4), booted through the handoff
    -> automatic hardware checks in the live session
    -> a clean reboot back to Windows, report on the stick
  ```

  OneDrive materialization (V8) is there because it is silent data loss,
  there is no harness for it today, and everything downstream touches
  files; it was missing from the list above. The stick writer is deliberately the
  first real writer, because it writes to a stick and not the internal
  disk, and it must refuse anything not removable, not the expected size,
  or not the device pointed at. This half proves V1 (unattended boot to a
  working desktop, untested anywhere today) and crosses no commit line.

  **Status 2026-09-08:** schemas, materialization, the writer and the live
  boot with identity + hardware checks have all fired on the rig
  (VALIDATION V8, V1; RISKS R8, R16). Since 2026-09-09 the stick carries
  both desktop images, and the live session reads the chosen one back
  against the manifest (R17). The physical Secure-Boot-on row came on
  2026-09-12 (the Aspire, `v1-live-boot.csv` row 4).
- **Destructive half second:** shrink, the ESP `EFI/Boot` snapshot and
  restore, the alongside install, the boot-chain check, the `settle-in`
  pull, the reclaim offer.

  **Status 2026-09-10:** the snapshot (`%pre`), the alongside install from
  the stick's squashfs, the boot-chain checklist and `outcome.json`
  (`%post`) have fired on the rig as the product's own kickstart:
  `docs/validation-results/v2-install.csv` row 3, `pass-plumbing`.

  **Status 2026-09-12:** the prologue is product code
  (`Invoke-Prologue.ps1`, `RUN-CONVERT.cmd`): re-validate, the R18 disk
  check with its four guardrails and its own restart, the re-measure by
  both read-only paths, the fork, the shrink, BitLocker suspension, the
  handoff, and a stopped `outcome.json` at every refusal. Its rig bench is
  `rig/hyperv/prologue.sh` (`docs/validation-results/r18-prologue.csv`).
  The restore half of the snapshot (rollback, Windows side:
  `Invoke-Rollback.ps1`, `r21-rollback.csv`) was built the same day.
  Still to build: `settle-in`.

**Where it is built.** First the Hyper-V rig: disposable, restorable from
a VHDX (a virtual disk file) in a minute, the place where a first vertical
gets broken and restarted twenty times. Then the ASUS G16 / Acer Aspire as
the first physical vertical.

**A borrowed machine is never the trailblazer.** It gives one attempt, no
baseline image, and someone's data. A borrowed Dell/Lenovo/HP is a
30-minute read-only visit: a V0 vendor row, and on a recent Intel machine
the first real firing of the RST/VMD check (V5).

**Then the matrix builds itself.** Once the vertical is one script writing
an evidence row per stage, a new vendor is the same half-hour visit, but it
fills a whole column (scan, harvest, author, handoff, live boot, hardware
check) instead of one V0 row. The result is a grid of vendors × stages,
strictly more informative for the same cost.

One precondition found on the first physical machine is **decided
(2026-09-08)** and built (2026-09-12). The Acer's C: carried NTFS's dirty
flag, and Windows refused to measure the shrink until a disk check ran
(RISKS R18). `evaluate` never repairs. It detects the flag and captures the
user's fork. The `upgrade_` prologue clears the flag as reversible prep
(step 1b above) with four guardrails, re-measures, and branches on the real
number. It is the first step in the flow that changes the internal disk,
which is why it sits behind intent capture and before the commit line, not
in a preflight check.
