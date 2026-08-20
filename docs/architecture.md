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

**On the name.** The middle module's directory shares the project's name on
purpose — the conversion is the product. In prose, always "the converter" for
the module and "the project" for the whole, so no sentence ever has to
disambiguate `upgrade_` from `upgrade_`.

---

## The commit line

Exactly one moment in a conversion is irreversible: **the first destructive
write.** Which moment that is depends on the path chosen at intent capture:

- **Clean-slate path** (files staged to the stick): the disk wipe, inside the
  cutover. Everything before it — staging, checksums, the reboot into the
  live environment — is additive, and a failure at any earlier point simply
  boots Windows again.
- **Safety-copy path** (files stay on the internal disk): the **reclaim** —
  deleting the Windows partition — which lives in `settle-in` and happens
  only after verification passes and the user explicitly consents. Until
  reclaim, Windows is physically intact and bootable; rollback is restoring a
  boot entry, not restoring an image.

Two rules follow, and they are not negotiable:

1. **The interface must say "you can still cancel" until that exact moment**,
   and must stop saying it the instant the line is crossed. A user watching a
   progress bar deserves to know which side of the line they are on.
2. **Everything that could refuse must refuse before the line.** Identity
   checks, checksum verification, capacity checks, hardware refusals — and on
   the clean-slate path, a live-session hardware check with a human present,
   because there is no rollback on the other side.

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
| BitLocker recovery key | Unrecoverable; unlocks the data volume on the safety-copy path, and avoids a lockout mid-conversion |
| Wi-Fi profiles and PSKs | Needed to reconnect; the user may not know their own password |
| Browser profiles | Bookmarks, history, extensions, Firefox passwords |
| OEM firmware/ACPI quirks | Vendor-shipped, Windows-only |

This turns "if necessary we inject from the USB at runtime" from a contingency
into a designed pipeline: **evaluate extracts -> the USB carries -> `upgrade_`
injects before first boot.**

### It captures intent, not just state

If any decision is missing, the converter has to stop and ask a human, and
walk-away dies. So `evaluate` also collects: the conversion path (clean slate
now, or keep a safety copy of Windows for a while — the choice is offered
only when the machine qualifies for both), the desktop (KDE or GNOME — two
screenshots, one question), account name, and password (hashed immediately to
SHA-512 crypt).

The path choice carries a consequence the interface must state plainly: the
clean-slate path requires the user to be present for a two-minute hardware
check in the live session before anything is destroyed, because it has no
rollback. The safety-copy path is true walk-away.

### It writes the stick

`evaluate` authors the USB stick itself — live image plus an exFAT staging
partition — so the user never meets an ISO or a burning tool. (An ISO is also
published for technical users and repair events.) Raw disk writes are the one
place this tool could destroy data *before* the commit line, so device
selection is defensive: removable-bus devices only, size and volume label
confirmed with the user, refusal if anything is ambiguous. See RISKS R16.

### It refuses

`evaluate` is where every refusal lives, because it is the only module that can
refuse for free:

- RED scanner verdict. No override flag, ever.
- Neither path fits: user data too large for the stick AND too little
  shrinkable space to keep it in place. The refusal is a **gap report**, not a
  door slam — exactly how many GB to free (largest folders listed) or what
  stick size would change the answer, so re-running `evaluate` converges.
- Multiple user accounts present (see RISKS R5) — migrating one and abandoning
  the rest silently is a data-loss bug. Refused in v1; the harvest schema is
  shaped per-user from the start so multi-user support is an extension, not a
  rewrite.
- Folder sizing was truncated, so the staging estimate is unreliable (RISKS R6).
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

### Two paths, one stick

There is no external drive anywhere in this design. Everything travels on the
USB stick, or never moves at all:

|  | clean slate | safety copy |
|---|---|---|
| User files travel via | the stick's exFAT staging partition | never leave the internal disk |
| Qualifies when | data + artifacts fit the stick | shrinkable space ≥ Linux (~20 GB) + data + headroom |
| Windows afterwards | gone at the wipe | intact until `settle-in` reclaims it |
| Rollback | none — the wipe is the commit line | full, until reclaim |
| Walk-away | after a 2-minute human check in the live session | total |

When a machine qualifies for both, the user chooses at intent capture, with
the trade stated in one sentence each. When it qualifies for neither,
`evaluate` refuses with a gap report.

### Stage 1 — prologue (runs in Windows, reversible)

1. Re-validate `job.json` against the live machine. Anything changed since
   `evaluate` ran stops here.
2. **Clean slate:** stage user files and artifacts to the stick's exFAT
   partition with per-file checksums, at a measured write speed shown as a
   **computed** time estimate — cheap flash at 20 MB/s is not a ten-minute
   job, and the user must learn that before walking away. Windows reads its
   own BitLocker volume, so encryption never enters this path.
   **Safety copy:** disable pagefile and hibernation, then shrink C: with
   `Resize-Partition` — Microsoft's own code path, the most-tested NTFS
   resize there is, and it works with BitLocker still on. Stage only
   artifacts to the stick.
3. Hard confirmation: type the word, not a checkbox.
4. Suspend BitLocker, write the one-time boot entry, reboot.

Still fully reversible. Nothing has been destroyed; even the shrink can be
undone by growing the partition back.

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

### Stage 2 — cutover (runs in the live environment)

5. Re-verify machine identity against `job.json` — same disk serial, same
   firmware mode. A mismatch means the USB was moved to another computer:
   **abort.** Do not partition a stranger's laptop.
6. Verify every staged checksum by reading it back from the stick. This is
   also the counterfeit-flash test (RISKS R17): a stick that lied about its
   capacity fails here, while Windows still exists.
7. Automated hardware verification, before anything is destroyed: Wi-Fi
   associates, the display drives at native resolution, amp firmware loads.
   What the old design discovered in `settle-in` is now a refusal gate.

**Clean slate** (the user is present, by design):

8. The live desktop asks for two minutes: speakers — *not headphones* —
   display, Wi-Fi. A human ear is the only test for a smart amp, and this is
   the last moment the system can refuse for free.

> **=== COMMIT LINE (clean slate) ===** everything below destroys something

9. Wipe, partition, install via kickstart with the chosen desktop.
10. **Inject artifacts** — vendor firmware, drivers — into the installed
    system *before* first boot, so the first impression is working hardware.
11. Restore files from the stick, Wi-Fi profiles to NetworkManager keyfiles,
    browser profiles.
12. Wipe the credential portion of the stick — but **leave the staged
    files**: until the user says otherwise, the stick is their only backup,
    and `settle-in` tells them so.
13. Write `outcome.json` and logs to the stick. This is the only telemetry
    and it stays there unless the user chooses to submit it. Reboot.

**Safety copy** (nobody is present, by design):

8. Create partitions in the freed space. The Windows partition and the
   existing ESP are never reformatted; Fedora's bootloader is added alongside
   `bootmgfw.efi`, which is what keeps rollback a boot-menu entry rather than
   a restore.
9. Install via kickstart with `--onpart`, touching the new partitions only.
10. Unlock the NTFS volume with the harvested recovery key (`cryptsetup`
    BITLK, RISKS R19), copy user files into the new home, verify checksums
    against the manifest.
11. Inject artifacts, restore Wi-Fi and browser profiles, wipe credentials
    from the stick, write `outcome.json` and logs. Reboot.

Nothing destructive has happened on this path. The commit line waits in
`settle-in`, at reclaim.

**A note on a reversal.** An earlier revision rejected any single-disk design,
citing the unattended resize-and-move whose power-cut failure loses both the
original and the copy. The safety-copy path is not that design: the resize is
Windows' own shrink, the files are *copied* rather than moved, the original
NTFS data is never modified until a checksummed copy exists, and never deleted
until the user consents at reclaim.

### Rollback is a mode of upgrade_, not a fourth module

On the safety-copy path, rollback means restoring the Windows boot entry —
seconds, not hours — reachable from the stick and from `settle-in`. Owning it
here rather than leaving it implied is deliberate: unowned recovery paths are
discovered by the person whose laptop is already a brick. On the clean-slate
path there is no rollback, which is precisely why that path has a human gate
before its commit line; the honest fallback is Microsoft's own install media
(the digital licence reactivates on the same hardware) plus the files still on
the stick.

### Design constraint: offline

The live image carries Fedora as a squashfs on the USB — both desktops, so
the intent-capture choice costs a question, not a download — and does **not**
install over the network. The machines most likely to need converting are
exactly the ones with Broadcom Wi-Fi or a card too new for the shipped kernel
— a network installer fails hardest on the hardware we most expect to see.
Costs stick capacity; removes an entire class of mid-conversion failure.

### Design constraint: one stick, honestly sized

`evaluate` computes what the stick must hold — live image, artifacts, and on
the clean-slate path the staged files — and refuses a stick that cannot, with
the gap report saying what would fit. The stick is also, briefly, the only
copy of the user's files on that path, which is why the read-back verification
in step 6 is a hard gate, not a warning.

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
3½. On the clean-slate path: tell the user to **keep the stick** — it still
   holds their files, and it is their only backup until they no longer want
   one.
4. **Decide** (safety-copy path). Keep, or roll back to Windows — a boot-menu
   restore, not an image restore. Only after the user confirms things work
   does it offer the **reclaim**: delete the Windows partition and grow into
   the space, stated plainly as the one irreversible act on this path. Offered
   exactly once, never nagged; declining leaves a `reclaim` command behind for
   whenever they are ready.
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

**Prompt budget for the entire conversion: four.** One UAC consent click, one
confirmation that the stick about to be written is the right device, one
password the user chooses, and one polkit prompt in `settle-in` at reclaim.
(The clean-slate path adds its two-minute live-session hardware check — a
gate, not a prompt.)

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
passwords, writes raw USB devices, resizes partitions and rewrites boot
configuration — behaviourally an exact match for an infostealer followed by
ransomware. Unsigned, Defender may
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

- Any distribution other than Fedora — **in v1.** The seams are cut for
  more: the distribution lives in `job.json`, the kickstart generator is
  per-target, and nothing outside `upgrade_/linux/` knows what is being
  installed. Multi-distro is an intended open-source future, not a v1 promise.
- Dual-boot as a product. The safety-copy path keeps Windows *temporarily*,
  as a rollback that ends at reclaim — it is not a supported two-OS machine.
- Migrating Windows applications
- Machines `evaluate` flags RED. No override, ever.

---

## Platform scope: Apple hardware

Three distinct cases with three different answers, and the deciding reason is
not the one people expect.

### Apple Silicon (M1 onward) — out of scope, and not because Apple blocks it

It is worth being precise here, because the intuitive assumption is wrong.
Apple Silicon Macs boot third-party operating systems as a **designed feature**.
From the Asahi Linux project's own description:

> "Apple allows booting unsigned/custom kernels on Apple Silicon Macs without a
> jailbreak! This isn't a hack or an omission, but an actual feature that Apple
> built into these devices. That means that, unlike iOS devices, Apple does not
> intend to lock down what OS you can use on Macs (though they probably won't
> help with the development)."
> — <https://asahilinux.org/about/>

Two things do conflict with this project's specific promise:

**You cannot fully replace the OS.** Apple firmware must remain on the disk:

> "2.5GB of this is used for the 'stub' macOS partition, that includes critical
> components such as Apple's bootloader and firmware, and a full copy of the
> macOS recovery image. This is required by the design of these platforms."

Note the nuance: the mandatory piece is the 2.5 GB stub, not a full macOS
install. Keeping full macOS is currently *recommended* because firmware updates
cannot yet be applied from Linux — and Asahi expects that to change, at which
point they will "be comfortable recommending Linux-only setups." So this is a
statement about today, not a permanent property.

**Walk-away is impossible.** Changing boot policy requires 1TR — physically
holding the power button and entering an admin password. No software can
automate that.

But the decisive reason is neither of those. **Asahi already does this, and does
it better than we would.** Their installer handles partitioning, boot policy and
the stub correctly, backed by the kernel and GPU work that makes the machines
usable. Building a competing path would hand users a worse version of something
that already exists and is actively maintained.

**Decision: never in scope. Refer people to Asahi.** That is the outcome that
serves the user, and the scanner should say so by name.

### Intel Macs with T2 (2018–2020) — a hard maybe

The T2 chip owns the SSD controller, keyboard, trackpad, audio and camera.
Linux needs out-of-tree drivers (the `apple-bce` work from the t2linux project),
Secure Boot must be disabled through Startup Security Utility in recovery, and
audio support has historically been partial.

Achievable, but it is a project rather than an install, and "click convert and
walk away" is not an honest description of it. Out of scope for v1.

### Intel Macs without T2 (pre-2018) — the genuinely interesting case

Ordinary x86 machines with conventional firmware. Among the easiest Linux
targets that exist. The main pain is Broadcom Wi-Fi, which `data/devices.ps1`
already covers because it is the same chip family that plagues PC laptops.

And the timing matches this project's founding argument exactly. macOS Tahoe is
the final release supporting Intel Macs, and it supports only four models — the
2020 iMac, the 2019 16-inch MacBook Pro, the 2020 four-port 13-inch MacBook Pro,
and the 2019 Mac Pro. Every other Intel Mac is *already* out of support.

That is the same situation that motivated this project — working machines
declared obsolete by a vendor's roadmap, with no security updates and no upgrade
path — with a different logo on the lid. Unlike Apple Silicon, nobody is
currently serving these users well.

### What supporting Intel Macs would actually cost

Not a port. A parallel implementation:

- **`evaluate/macos/`** — every collection mechanism differs. `system_profiler`,
  `ioreg`, `diskutil` and `security` replace WMI, the registry, `netsh` and
  `manage-bde`.
- **No `bcdedit` equivalent.** The one-time UEFI boot entry is the mechanism
  that makes walk-away possible on Windows. macOS has `bless` and Startup Disk,
  which behave differently and need their own design.
- **FileVault instead of BitLocker**, with a different key-escrow model.

Shares the philosophy, the risk register and `data/`. Shares almost no code.

**Decision: not v1.** Revisit pre-T2 Intel Macs after the Windows path ships and
has real-world conversion data behind it.

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

0. **The spine spike, before anything else**: a hello-world conversion in a
   VM — `bcdedit` handoff → live boot → kickstart → reboot into Fedora. No
   data, no artifacts, sacrificial machines only. Walk-away rests entirely on
   the handoff working (RISKS R15), and this project closes risks with
   evidence, not argument — including its own.
1. `job.json` schema and the `evaluate` contract
2. Artifact extraction (vendor firmware) — the piece that cannot be added
   later on a real user's machine
3. Kickstart generator: `job.json` -> kickstart (per-target seam; Fedora
   first)
4. Live image: Fedora squashfs (both desktops) + orchestrator
5. Restore and inject stage; stick authoring in `evaluate`
6. `settle-in`, including reclaim
7. Shrink, boot handoff hardening, boot-entry rollback — **reviewed
   hardest.** The components that write to the internal disk.
