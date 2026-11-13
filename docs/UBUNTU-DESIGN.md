# Ubuntu: what was measured, and what building it would take

Expires when: what was built for Ubuntu is described in `architecture.md`
and gate V18 has its real-machine row; then this doc goes to
`docs/archive/`.

Roadmap item 23 (a second installer family). The risks: R40 (a second
installer offline and unattended), R45 (the installer picks the disk
itself), R34 (Secure Boot revokes boot files), R21 (Windows still starts).
**Decided (2026-11-12, the owner):** Ubuntu is the second family (RISKS
R40). Sections 1 to 8 are the measuring step as it was written that day,
by hand and before anything was built
([log](log/2026-11.md#2026-11-12-ubuntu-measured-and-installed-offline-and-unattended-on-the-rig-r40-r45-r21-r34-roadmap-item-23)).
What has been built since, and its rig rows, is in "What is built" at
the end and in [STATUS.md](STATUS.md) (V18).

## The short version

```text
 Does it install with no network and nobody typing?   yes  (rig, one run)
 Does it need a person at the keyboard?               no   (no key step)
 Does the installed system start, Secure Boot on?     yes  (rig, Hyper-V's firmware only)
 Does a failed check stop it before anything is       yes  (rig: the disk file's sha256
 written?                                                   the same before and after)
 Can it install beside Windows?                       yes  (rig, one try; see section 4)
 Tried on a real machine?                             no
```

Ubuntu is weeks of work, not refused. Every "yes" above is one run by hand
on a Hyper-V guest: plumbing, not proof (rule #5), and no row.

## 1. The ISO (measured)

Ubuntu Desktop 26.04.1 LTS, the current long-term release on Ubuntu's own
release page. Read from its files by `measure-release.py --casper` into
`data/measured/ubuntu-26.04.1.json`. It is not in `data/releases.json`, so
nobody is offered it.

| Fact | Measured value |
|---|---|
| The file | `ubuntu-26.04.1-desktop-amd64.iso`, 6,482,409,472 bytes, sha256 `601e30fb...9dd2` (matches Ubuntu's own `SHA256SUMS`) |
| What it is | one ISO that is live system, installer and package store at once (the "casper" layout) |
| Stick shim, `EFI/boot/bootx64.efi` | shim 15.8, signed under Microsoft's third-party authority (UEFI CA 2011 only, not 2023) |
| GRUB and MokManager beside it | GRUB 2.14 (`grub,5`), signed by Canonical. It reads `boot/grub/grub.cfg`, not `EFI/BOOT/grub.cfg` |
| Live kernel, `casper/vmlinuz` | 7.0.0-30, signed by Canonical; no `.sbat` section |
| Installed kernel | the same file, byte for byte (sha256 `0ad39b13...7785`). No second key, so no key step like Bazzite's (R42) |
| Installed shim and GRUB | from packages in the ISO's own `pool/`: the same shim as the stick's; GRUB signed by Canonical |
| The system image | 41 squashfs layers in `casper/`; the largest is 3,432,136,704 bytes |
| Over FAT32's 4 GiB, or split on the ISO | none: every file fits the stick's FAT32 partition, and `iso.rs` would not refuse any |
| Packages on the ISO | 155, 1.29 GB: the kernel, shim, GRUB, two NVIDIA driver sets |
| Store (snap) packages | 13, seeded inside the base layer, Firefox among them |

The whole ISO, unpacked, is 6.1 GB. Today's kit is written with an 8 GB
boot partition (`--boot-size-gb 8`), so it fits there with the kit.

## 2. Offline and unattended (the rig, one run)

A throwaway guest: Secure Boot on (the "Microsoft UEFI Certificate
Authority" template), the network adapter connected to nothing, an empty
40 GB disk. The "stick" was a FAT32 partition holding the ISO's files
unchanged, plus three things of ours.

| What starts it | Where |
|---|---|
| the word `autoinstall` on the kernel line | our `boot/grub/grub.cfg` on the stick (not signed, so Secure Boot does not mind) |
| `autoinstall.yaml` | at the root of the stick, beside `casper/` |
| `apt: {fallback: offline-install}` | in that file |

What happened, with nothing typed:

- The stick started under Secure Boot from plain FAT32.
- No confirmation was asked. Without `autoinstall` on the kernel line the
  installer stops and asks (its documentation; not tried).
- The install took 4 minutes 25 seconds from the early hook to the late
  one. Every package came from the stick (`file:/cdrom`). Nothing waited
  on the network.
- The installer put its own entry, "Ubuntu", first in the firmware's order
  and turned the guest off (`shutdown: poweroff`).
- The first start, Secure Boot still on, reached the sign-in screen in
  under a minute. All 13 store packages finished loading with no network.
  One store step, "Initialize device", failed for want of a network; it
  is the store's registration and tries again later.

## 3. The hooks (the rig)

| Ubuntu's hook | When it ran | Today's equivalent |
|---|---|---|
| `early-commands` | as soon as the installer started, before it looked at any disk | Anaconda's `%pre` (`upgrade-installer verify`) |
| `late-commands` | after the install, the new system at `/target` | `%post` (`upgrade-installer outcome`, `post.rs`) |
| `error-commands` | one second after a hook failed | none today |

**Refuse before erase holds.** With the early hook made to exit 7 on a disk
holding a real Windows, the installer stopped at once. The disk file's
sha256 was the same before and after (`1D69479A...1E6C`): not one byte
written.

Two things a build must handle:

- **A refusal stops and waits for ever.** The screen says "Installation
  failed" with a "Restart installer" button. Nothing sends the machine
  back to Windows. Our hook must restart the machine itself, as `verify`
  does today.
- **A job left running by `error-commands` did not survive.** A
  power-off started in the background from that hook never fired.

## 4. Beside Windows (the rig, one try)

The disk was a copy of the rig's real Windows disk: Windows' 100 MiB boot
partition, its reserved partition, a BitLocker Windows partition shrunk to
leave 32 GiB free. The file said `layout: {name: direct, mode: use_gap}`.

| Checked | Found |
|---|---|
| Windows' three partitions | same places, same sizes |
| Windows' reserved and Windows partitions | byte for byte the same (sha256 of each, before and after) |
| The new Linux | one ext4 partition filling the 32 GiB gap. No separate `/boot`; a swap file |
| Windows' boot partition | used as it was, not formatted. All 143 files still there, `bootmgfw.efi` unchanged, 67 MB free after |
| Windows' fallback loader, `EFI\Boot\bootx64.efi` | **replaced** by Ubuntu's shim. Fedora does the same (R21) |
| The firmware's order | "Ubuntu" put first |
| The start menu | lists "Windows Boot Manager" by itself |
| The installed Ubuntu | started with Secure Boot on |

Not tried: starting that Windows afterwards. Hyper-V cannot trust both
signers at once (`rig/hyperv/README.md`), so that takes a real machine.

Three cautions:

- `mode: use_gap` is in the installer's source and worked, but its
  documentation does not mention it. It takes "the largest gap", with no
  way to say which gap or how big.
- The documented way is a list of storage actions with `preserve: true` on
  every kept partition. The installer wrote exactly such a list itself in
  this run (in its log). Writing our own is untried.
- Reclaim (R41) moves Linux's `/boot` and `/`. Ubuntu has only `/`.

## 5. The new risk: the installer picks the disk itself (R45)

With `autoinstall` on the kernel line the stick asks nothing, on whatever
computer it starts. Left to its defaults it takes "the largest disk" or
"the largest gap". Fedora's kickstart is fenced by `%pre`; Ubuntu's file
has no fence yet. The shape that would give it one:

```text
 the stick's autoinstall.yaml: no disk, no account, only our early hook
    v
 early hook (our checks: this machine, this job, the marker, the disk)
    |-- any check fails > restart to Windows, nothing written
    '-- all pass: write the real file, naming the disk by serial
                  and every kept partition one by one
    v
 the installer reads the file again (its documentation says it does)
```

That it reads the file again is from the documentation, not from a run.

## 6. What is not measured

- Anything on a real machine: real firmware, real Wi-Fi, a real stick.
- A firmware that trusts only Microsoft's 2023 authority. This shim is
  signed under 2011 only; the scanner's judgement (R34) would say so.
- Windows starting after the install beside it; the 'Windows (kept)'
  entry; shim in Windows' slot (R21, R22) with `EFI\ubuntu` paths.
- Where a real firmware's order lands after the install.
- A stick carrying fewer of the 41 layers.
- The Wi-Fi networks carried over (decided 2026-10-02: a direction is not
  finished until it does), settle-in installed from the late hook, the
  language, keyboard and time zone from `job.json`.
- The NVIDIA drivers on the ISO (`drivers: {install: true}`), offline.
- What a person sees after signing in the first time.
- Whether the ISO's address still serves this file after 26.04.2 comes out.
- Other Ubuntu flavours (Kubuntu and the rest). They are other ISOs and
  may use another installer. Only the GNOME desktop was measured.

## 7. Ubuntu beside Bazzite, for the owner's call

| | Ubuntu 26.04.1 | Bazzite |
|---|---|---|
| ISO measured | yes | yes |
| Installs offline, unattended | yes, on the rig | on paper; the unattended start is not tried |
| A person at the keyboard | no | yes, once, at the blue key screen (R42) |
| Fits the FAT32 boot partition | yes (largest file 3.4 GB) | no (8.0 GB; needs the exFAT partition) |
| Stick size | 8 GB | 16 GB |
| Download | 6.5 GB, a versioned address at Ubuntu | 8.4 GB, from a copy the project must host |
| Installer | a new one (subiquity): a new generator, new hooks | Anaconda again: kickstart, `%pre`, `%post` reused |
| Installed system | writable, like Fedora | read-only `/usr`; `post.rs` and settle-in change |
| Beside Windows | tried once on the rig | not tried |

## 8. The size of the rest (an estimate, not a measurement)

| Piece | What it is | Rough size |
|---|---|---|
| The answer-file generator | a crate beside `upgrade_/kickstart/` (227 lines today), plus the storage list for both paths | about a week |
| The hooks | `upgrade-installer`'s checks and outcome (1,900 lines) bound to Ubuntu's hooks: other paths, no Anaconda console, the restart on refusal, R45's fence | one to two weeks |
| The four places Fedora is written in | the stick's `grub.cfg` (Ubuntu's GRUB reads another path), the handoff's `EFI/fedora`, `make-kit.sh`, `post.rs`, settle-in's `bootentry.rs`. Bazzite needs the same cut | about a week, shared |
| `upgrade-fetch` | a third layout: one ISO, whole folders copied (today it copies named files), long file names, the room check | about a week |
| The rig rows | the erase, beside Windows, the refusals, R45's arms, written by a harness | about a week |
| A real machine | Secure Boot on, the erase first | after all of the above |

Four to six weeks of sessions before a real machine, by this count.

## The calls, and where each stands

1. Which family comes second: **decided (2026-11-12, the owner): Ubuntu.**
2. The small desktop (`ubuntu-desktop-minimal`) or the full one: built
   with the small one, the installer's default and what was measured. The
   owner's call is still owed (ROADMAP, "Decisions owed").
3. Beside Windows: our own storage list, not `use_gap`. Built that way: it
   is the documented way and it names every partition (R45).
4. A separate `/boot`, like Fedora: built that way, so reclaim (R41) sees
   one shape. Still the owner's to change (ROADMAP, "Decisions owed").

## What is built

| Piece | Where | Stands |
|---|---|---|
| The answer-file generator | `upgrade_/autoinstall/` (`upgrade-autoinstall`) | the erase and beside Windows; rig rows |
| The stick's own file | `upgrade-autoinstall stick-file`; the job writer's `--autoinstall`; the prologue accepts no other text there | names no disk and no account (R45); rig rows for the file, unit cases for the Windows side |
| The early check | `upgrade-installer early` (`subiquity.rs`) | this machine, this job, the marker, the stick's files read back, `verify`'s hardware checks; rig rows |
| Beside Windows | the same check: every partition kept by name, Linux in the one free space big enough, a safety copy of Windows' startup files first | rig rows; Windows not started after (the rig cannot) |
| After the install | `upgrade-installer late`: the 'Windows (kept)' entry and the start target (`post.rs`), then `outcome.json`, the handoff folder and settle-in (`outcome.rs`), all shared with Fedora | rig rows |
| The stick's menu | `handoff-payload/grub-ubuntu.cfg`, at `boot/grub/grub.cfg` on every kit | rig rows: starts the installer, leaves the mark, hands over once converted |
| The download | `upgrade-fetch`: one ISO, every file but its own menu | a rehearsal (`v17-fetch.csv` lines 14 to 17); only `rehearse --table` can plan it |
| Shim in Windows' slot | `upgrade-prologue`: `EFI\<distro>` from the install's record | unit cases |
| The rig harnesses | `rig/hyperv/ubuntu-arms.sh`, `ubuntu-verdict.py`; `rig/fetch/arms.sh` | write `ubuntu-install.csv` and `v17-fetch.csv` |

## What is not built, or not run

1. The entry in `data/releases.json`. It is the offer to a person, and it
   changes the scanner's "Linux releases" line on every machine. The
   owner's call (ROADMAP, "Decisions owed").
2. The Windows side with an Ubuntu stick: the window, the prologue's start
   and the handoff have unit cases only.
3. Reclaim (R41) for Ubuntu: the stick's menu has no branch for it, and
   settle-in refuses to arm it on a stick without Fedora's installer.
4. A real stick, the real internet, Wi-Fi, and a real machine with Secure
   Boot on, the erase first.
