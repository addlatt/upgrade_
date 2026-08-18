# upgrade_ architecture

Three modules:

```
   evaluate            upgrade_             settle-in
   ---------           ---------            ---------
   read + decide       do the thing         confirm + hand over
   Windows             Windows -> Linux     Linux
   reversible          contains the         after the fact
                       commit line
```

The boundary between modules is **commitment**, not operating system. An
earlier draft split on OS boundaries (Windows / live USB / first boot) and it
put reversible and irreversible work inside the same phase. That is the wrong
seam: it hides the only distinction the user actually cares about, which is
whether they can still change their mind.

**On the name.** The middle module shares the project's name on purpose. The
conversion is the product; everything else exists to make it safe. Where the
distinction matters, "the `upgrade_` module" and "the project".

---

## The commit line

Exactly one moment in this system is irreversible: **the first partition
write.** It sits inside `upgrade_`, roughly two thirds of the way through.

Everything before it — including the full disk image, which may take hours — is
additive. Nothing has been destroyed. The one-time boot entry means even a
total failure of the live environment simply boots Windows again.

Two rules follow, and they are not negotiable:

1. **The interface must say "you can still cancel" until that exact moment**,
   and must stop saying it the instant the line is crossed. A user who spent
   two hours watching a progress bar deserves to know which side of the line
   they are on.
2. **Everything that could refuse must refuse before the line.** Identity
   checks, checksum verification, capacity checks, hardware refusals. After the
   line, the only remaining safety mechanism is rollback, which is slower,
   scarier, and depends on hardware that might itself fail.

---

## Module 1: evaluate

**Runs in Windows. Read-only. Repeatable. Makes no changes, ever.**

### It is the last moment Windows exists

This is the responsibility that is easy to miss, and impossible to add later.

Some things needed by the finished Linux system exist *only* inside the Windows
install and cannot be recovered once it is gone. The clearest example is vendor
audio firmware: on 2023+ laptops with Cirrus smart amplifiers, Linux can drive
the speakers but needs blobs that ship in the Windows driver package. Wipe
first and the speakers are silent with no local way to fix it.

So `evaluate` extracts artifacts, not just facts:

| Artifact | Why it can't wait |
|---|---|
| Vendor audio firmware (Cirrus CS35L41/56, TI TAS2781) from the driver store | Gone with Windows; the fix for silent speakers |
| BitLocker recovery key | Unrecoverable; needed to avoid a lockout mid-conversion |
| Wi-Fi profiles and PSKs | Needed to reconnect; the user may not know their own password |
| Browser profiles | Bookmarks, history, extensions, Firefox passwords |
| OEM firmware/ACPI quirks | Vendor-shipped, Windows-only |

This turns "if necessary we inject from the USB at runtime" from a contingency
into a designed pipeline: **evaluate extracts -> the USB carries -> `upgrade_`
injects before first boot.**

### It captures intent, not just state

If any decision is missing, `upgrade_` has to stop and ask a human, and
walk-away dies. So `evaluate` also collects: chosen distribution, account name,
password (hashed immediately to SHA-512 crypt), and target external drive.

### It refuses

`evaluate` is where every refusal lives, because it is the only module that can
refuse for free:

- RED scanner verdict. No override flag, ever.
- No external drive, or one too small.
- Multiple user accounts present (see RISKS R5) — migrating one and abandoning
  the rest silently is a data-loss bug.
- Folder sizing was truncated, so the backup estimate is unreliable (RISKS R6).
- RST/VMD active. Cannot be automated; see below.

### Output

A complete, validated **job spec** (`job.json`, versioned) plus an `artifacts/`
directory. Exit criterion: `upgrade_` can execute it start to finish with zero
further human input.

### Current status

Largely built. `evaluate/windows/upgrade-scan.ps1` (state reading, refusals)
and `evaluate/windows/Harvest-UpgradeState.ps1` (state + intent scaffolding) exist
and are tested. **Not yet built:** artifact extraction, intent capture UI,
multi-user handling, `job.json` schema.

---

## Module 2: upgrade_

**Starts in Windows, finishes in Linux. Contains the commit line.**

### Stage 1 — prologue (reversible)

Runs in Windows, elevated once.

1. Re-validate `job.json` against the live machine. Anything changed since
   `evaluate` ran — different drive, less free space — stops here.
2. Measure real write throughput to the external drive and show a **computed**
   time estimate. 378 GB over USB 2 is not a two-hour job, and the user must
   learn that before walking away, not after.
3. Image the Windows partition to the external drive. This is the rollback path.
4. Stage user data and extracted artifacts. Checksum everything.
5. Hard confirmation: type the word, not a checkbox.
6. Suspend BitLocker, write the one-time boot entry, reboot.

Still fully reversible. Nothing has been destroyed.

#### The handoff

`upgrade_` creates a one-time UEFI boot entry so the user never touches a
firmware menu — no vendor-specific boot key, no hostile BIOS screens.

```
bcdedit /copy {bootmgr} /d "upgrade_"        -> returns {guid}
bcdedit /set {guid} device partition=<usb>:
bcdedit /set {guid} path \EFI\BOOT\BOOTX64.EFI
bcdedit /set {fwbootmgr} bootsequence {guid}  -> next boot only
```

`bootsequence` applies to the next boot only, so any failure leaves the machine
booting Windows normally. That property is what makes this safe to attempt.

**BitLocker must be suspended first** (`manage-bde -protectors -disable C:
-rebootcount 1`). Changing boot configuration invalidates the TPM measurement
and triggers a recovery-key prompt — on a machine nobody is sitting at.

**RST/VMD cannot be automated.** No portable API exists; the settings live at
vendor-specific offsets in UEFI setup variables and writing them blindly bricks
machines. `evaluate` hard-refuses. This is permanent, not a v1 limitation.

### Stage 2 — cutover (crosses the line)

Runs in the live environment, unattended.

7. Re-verify machine identity against `job.json` — same disk serial, same
   firmware mode. A mismatch means the USB was moved to another computer:
   **abort.** Do not partition a stranger's laptop.
8. Verify every staged checksum. A bad backup stops here, while Windows exists.

> **=== COMMIT LINE ===** everything below destroys something

9. Partition and install Fedora via kickstart.
10. **Inject artifacts** — vendor firmware, drivers — into the installed system
    *before* first boot, so the user's first impression is working hardware.
11. Restore files, Wi-Fi profiles (to NetworkManager keyfiles), browser
    profiles.
12. Wipe the credential portion of the staged state.
13. Write `outcome.json` and logs to the USB. This is the only telemetry and it
    stays on the stick unless the user chooses to submit it.
14. Reboot.

### Rollback is a mode of upgrade_, not a fourth module

If stage 2 fails after the commit line, rollback restores the Windows image from
the external drive. It is reachable from the USB and it is `upgrade_`'s
responsibility — assigning it here rather than leaving it implied is deliberate,
because unowned recovery paths are discovered by the person whose laptop is
already a brick.

### Design constraint: offline

The live image carries Fedora as a squashfs on the USB and does **not** install
over the network. The machines most likely to need converting are exactly the
ones with Broadcom Wi-Fi or a card too new for the shipped kernel — a network
installer fails hardest on the hardware we most expect to see. Costs a 16–32 GB
stick; removes an entire class of mid-conversion failure.

### Design constraint: external drive mandatory

Sized for a full Windows image plus user data, verified writable and fast enough
before anything begins.

This turns away users who don't own one, and that is the correct trade. The
alternative — shrinking NTFS and shuffling data inside one disk — has a failure
mode where a power cut loses both the original and the copy. A tool aimed at
non-technical people cannot ship that. Anyone who wants it can dual-boot by
hand; we are not the right tool for everyone, and pretending otherwise is how
this project would hurt someone.

### Current status

Nothing built. Deliberate — these are the only components that write, and they
are last in the build order so they can be reviewed hardest.

---

## Module 3: settle-in

**Runs on Linux at first boot. Auto-launches once.**

It looks like a welcome screen. It is a safety gate.

1. **Verify hardware.** Speakers — *not headphones* — Wi-Fi, display and
   brightness, suspend/resume. Smart-amp silence is the most common post-install
   complaint and it is invisible if you only test with headphones plugged in.
2. **Confirm the data arrived.** Counts and checksums against `outcome.json`.
3. **Hand off.** Where files went, what replaced what, what is gone and is not
   coming back.
4. **Decide.** Keep, or roll back. Only after the user confirms things work does
   it offer to reclaim the external backup. Never automatically.
5. **Exit.**

### Scope boundary

`settle-in` does not teach Linux, install applications, run a tour, or check in
later. The verification step is the reason it exists; broadening it into a
welcome experience would dilute the one thing it is for.

### Current status

Nothing built.

---

## Contracts between modules

| File | Written by | Read by |
|---|---|---|
| `job.json` + `artifacts/` | evaluate | upgrade_ |
| `outcome.json` + logs | upgrade_ | settle-in |

Both schemas are versioned. A USB written by one release will eventually be read
by another; a module that meets a version it does not understand must refuse,
not guess.

---

## Interface and privilege model

### One elevation, no stored credentials

Ships as an `.exe` whose manifest declares `requestedExecutionLevel =
requireAdministrator`. Windows prompts once at launch; every privileged
operation runs inside that process.

Measured on a stock Windows 11 machine: user in the Administrators group,
unelevated token, `ConsentPromptBehaviorAdmin = 5` — the default. Elevation is
**a single Yes click, not a password prompt**, for the overwhelming majority of
home users.

**Techniques we will not use:** `runas /savecred`, scheduled tasks with "run
with highest privileges", COM elevation moniker abuse. All are documented
UAC-bypass patterns that Defender targets by name. They would get the tool
flagged *and* genuinely weaken the machine, for nothing the manifest doesn't
already provide.

**We never ask for a Microsoft account password.** A Microsoft-branded
credential box is indistinguishable from phishing, and it is unnecessary:
`manage-bde -protectors -get C:` returns the recovery key with local admin
alone.

**The only credential we create** is the new Linux account password — typed
once, hashed immediately to SHA-512 crypt, and only the hash reaches the USB.

**Prompt budget for the entire conversion: three.** One UAC consent click, one
password the user chooses, one polkit prompt in `settle-in` when reclaiming the
old partition.

### Stack

**C# WPF targeting .NET Framework 4.8**, single `.exe`.

- 4.8 is preinstalled on every Windows 10 1903+ and Windows 11 machine
  (verified: Release 533509 on the test system). Zero runtime install.
- The manifest gives single-prompt elevation.
- It **double-clicks**. A `.ps1` opens in Notepad, which fails at step one for
  exactly this audience.

The exe orchestrates rather than reimplements: it hosts a PowerShell runspace
in-process and calls the existing scripts, receiving structured objects. That
keeps `data/*.ps1` community-editable, which is what makes the hardware
database improve.

Not WebView2 — present on Windows 11, not guaranteed on Windows 10.

### Code signing is the gating item

See RISKS R12. The finished tool elevates, reads BitLocker keys, exports Wi-Fi
passwords, images disks and rewrites boot configuration — behaviourally an exact
match for an infostealer followed by ransomware. Unsigned, Defender may
quarantine it and the target user stops there permanently. Needs an OV/EV
certificate, a legal entity, and reputation that accrues with elapsed time.
Start before there is anything to sign.

---

## What migrates, and what silently doesn't

| Item | Ports? | Notes |
|---|---|---|
| Documents, Pictures, Desktop, Downloads, Videos, Music | yes | Resolved via known-folder APIs, so OneDrive redirection is handled. |
| Wi-Fi networks + passwords | yes | `netsh wlan export profile key=clear` -> NetworkManager keyfiles. |
| Enterprise / 802.1X Wi-Fi | **no** | Certificates and auth methods don't map cleanly. Flag and skip. |
| Firefox profile | yes | Bookmarks, history, extensions **and saved passwords** — the NSS key database is cross-platform. |
| Chrome/Edge bookmarks, history, extensions | yes | Profile directory ports. |
| Chrome/Edge **saved passwords** | **no** | DPAPI-encrypted; no Linux equivalent. They silently will not appear. Must be said in `evaluate`, not discovered in `settle-in`. |
| OneDrive cloud-only files | **needs care** | Placeholders copy as empty files. Detect and force-download, or refuse. RISKS R8. |
| Installed applications | no | Out of scope for v1. |
| Windows settings, Outlook data, licences | no | Say so plainly in the handoff sheet. |

## Non-goals (v1)

- Any distribution other than Fedora Workstation
- Dual-boot. Keeping both doubles the failure surface.
- Migrating Windows applications
- Machines `evaluate` flags RED. No override, ever.

---

## Layout

```
data/            knowledge base — community PRs land here, expected to churn
  devices.ps1      hardware: Wi-Fi, GPU, audio, storage quirks
  distros.ps1      distribution kernel table
schemas/         job.json / outcome.json contracts — change rarely, review hard

evaluate/        module 1 — read, capture intent, refuse
  windows/
upgrade_/        module 2 — do the conversion; holds the commit line
  windows/         prologue (reversible): image, stage, boot handoff
  linux/           cutover (irreversible): partition, install, inject, restore
settle-in/       module 3 — verify, hand over, stop
  linux/

dist/            built single-file artifacts users download
docs/            architecture, risks
build.sh         inlines data/ into dist/
```

Three decisions worth stating, because they will be questioned:

**Module-first, platform-second.** Modules span platforms — `evaluate` is
Windows-only, `upgrade_` starts in Windows and finishes in Linux, `settle-in` is
Linux-only. A platform-first tree would scatter one module across two roots and
hide the pipeline. A flat module tree would mix PowerShell and Python/bash with
different toolchains in one directory. `upgrade_/windows/` and `upgrade_/linux/`
keeps the mental model and separates the toolchains.

**`data/` is top-level, not inside `evaluate/`.** It is the highest-traffic
contribution target, and "adding a device is a one-line PR" only works if a
newcomer finds the file in seconds. It is also genuinely shared: `upgrade_`
needs the firmware mapping to inject, and `settle-in` needs the quirk data to
know what to verify.

**`schemas/` is not merged into `data/`.** They have opposite change profiles.
`data/` should churn constantly through drive-by PRs; `schemas/` are the
contracts between modules and must change rarely and under careful review.
Different review bars deserve different directories, and a shared junk drawer
would blur them.

## Build order

1. `job.json` schema and the `evaluate` contract
2. Artifact extraction (vendor firmware) — the piece that cannot be added later
3. Kickstart generator: `job.json` -> Fedora kickstart
4. Live image: Fedora squashfs + orchestrator
5. Restore and inject stage
6. `settle-in`
7. Imaging, boot handoff, rollback — **written last, reviewed hardest.** The
   only components that write.
