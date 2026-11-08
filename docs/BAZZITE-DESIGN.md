# Bazzite: the design and the plan to build it

Expires when: Bazzite's key step holds on a real machine with Secure Boot
on (V16), and what was built is described in `architecture.md`; then this
doc goes to `docs/archive/`.

Roadmap item 23 (a second installer family) and item 24 (the picker). The
risks: R40 (a second installer offline and unattended), R42 (the key step),
R34 (Secure Boot revokes boot files). The gates: V14 (the release starts
where it is used), V16 (the key step). The owner decided the four calls
on 2026-11-08, each as recommended below; the decision's one home is
`architecture.md`, "Linux releases beyond Fedora", and roadmap item 24.
Nothing is built yet. What was measured, and
how, is in the log
([2026-11-07](log/2026-11.md#2026-11-07-bazzites-live-iso-measured-and-a-design-for-the-key-step-r40-r42-v14-v16-roadmap-items-23-24)).

## The short version

```text
 window: pick your Linux          Fedora  |  Bazzite  |  Ubuntu
          Bazzite's button: "needs you at the keyboard for one minute"
    v
 the window fetches Bazzite (8.4 GB), checks every byte, writes the stick
    v
 Convert (the last click). Nothing has changed yet.
    v
 restart 1: the stick starts (Fedora's own signed kernel: no key needed)
    %pre: is Universal Blue's key already enrolled?
    |-- yes ------------------------------------------> install, walk away
    '-- no: write the key request (mokutil), nothing erased
    v
 restart 2: the blue screen waits for a person (it never times out)
    the person: any key, Enroll MOK, Continue, Yes, the password, Reboot
    v
 restart 3: the stick again
    %pre: enrolled now?  yes > install, walk away
                         no  > refuse, nothing erased, back to Windows:
                               "The key step was missed. Click Convert to try again."
```

Five things decide it. Each is below with what was measured, what is
still a guess, and a recommendation.

## 1. The ISO already carries Bazzite offline (measured)

The biggest unknown going in: does Bazzite's installer need the internet?
On its current ISO, no.

| Fact | Measured value |
|---|---|
| The file | `bazzite-stable-live-amd64.iso`, 8,423,129,088 bytes, sha256 `82ff3b40...9356` (matches Bazzite's own `-CHECKSUM`) |
| What it is | a live Bazzite system (Fedora's live layout): `LiveOS/squashfs.img`, 8,003,764,224 bytes |
| The system image inside | `ghcr.io/ublue-os/bazzite:stable`, made 2026-10-13, in the live system's `/var/lib/containers/storage` |
| How Bazzite's own kickstart installs it | `ostreecontainer --url=ghcr.io/ublue-os/bazzite:stable --transport=containers-storage`: from the stick, not the network |
| The installer and tools on it | `anaconda`, `liveinst`, `mokutil`, `efibootmgr`, `bootc` |

So the "image on the stick" problem is already solved by Bazzite. What is
still unmeasured (R40): how to start that installer with our kickstart and
nobody at the keyboard (a live system does not start Anaconda by itself;
`systemd.run=` on the kernel line is the first thing to try on the rig),
and whether Bazzite's own `%post` line `bootc switch ... --transport
registry` touches the network.

**Recommendation:** use Bazzite's own live ISO, unmodified, with our
kickstart (our `%pre` and `%post`, plus Bazzite's own install lines). Not
Fedora's installer with Bazzite's image fed in: more moving parts, and
nobody else runs it that way.

## 2. The key step: folded into Convert (decided 2026-11-08)

Measured: the stick's kernel is Fedora's (`7.2.9-201.fc44`, signed by
Fedora's CA), so the stick starts with Secure Boot on and no key. The
installed kernel (`7.2.9-ogc3.1.fc44`) is signed by "ublue kernel", so the
installed system starts only after Universal Blue's key is enrolled. The
key: `O=Universal Blue, CN=ublue kernel`, sha256 `4e5c6847...70d1`, valid
to 2124.

Two designs, compared:

| | Folded into Convert (above) | A separate key restart before Convert |
|---|---|---|
| Clicks | one (Convert) | two ("Prepare Bazzite", then Convert) |
| Restarts before the install | three | four |
| Who writes the request | the stick, with `mokutil` (the tool shim's own makers ship) | Windows (a new firmware writer, rule #4) or a stick start anyway |
| Who checks it | the stick's `%pre` (exists) | Windows cannot: see below. The stick's `%pre` has to check anyway |
| If the person misses it | `%pre` refuses, nothing erased, back to Windows | the same, one step earlier |

Why Windows cannot check: the key list Linux reads (`MokListRT`) is made
by shim at each start, and lives only until the next restart. A Windows
start never goes through shim, so in Windows the list does not exist
(shim's source; to confirm on the rig). The check therefore belongs on the
stick, and once it is there, a separate key restart only adds a click.

**Correction to the earlier picture.** The first sketch hoped for "one restart
instead of two". The key step costs two extra restarts either way: one to
show the blue screen, one after it. Folding it into Convert saves a click
and a visit, not a restart.

**What is not known, and the rig must show (R42):**

- Where the blue screen's own restart lands. The one-time start
  (`BootNext`) is used up by then, so the firmware's normal order probably
  starts Windows. Two answers to choose between on the rig: (a) `%pre`
  puts the stick first in the order and puts it back afterwards (three
  restarts, a firmware write from Linux, as settle-in's rollback already
  does), or (b) Windows starts, and the prologue's startup task (R24) sends
  the machine back to the stick (four restarts, nothing new written).
- That the blue screen appears from the stick at all. Shim looks for
  MokManager (`mmx64.efi`) beside itself; the ISO keeps it in
  `EFI/fedora/`, and today's stick carries no `mmx64.efi`. The kit must put
  it in `EFI/BOOT/`.
- That the installed kernel starts. It has no `.sbat` section (measured:
  empty); shim checks SBAT, and how it treats a kernel with none is to see
  on a real machine.
- That nobody coming is safe. Bazzite asks for `mokutil --timeout -1`, so
  the screen waits forever instead of the default 10 seconds. A machine
  left at the blue screen has nothing erased; walk-away only waits.

## 3. The password and the check

The blue screen reads the keyboard as US QWERTY and hides what is typed.
mokutil accepts any password from 1 to 256 characters for an import
(`PASSWORD_MIN 1` in its source). **Decided (2026-11-08, the owner):** `linux`, all
lower-case letters that sit in the same place on US, UK, German and French
keyboards, and is said on the screen before Convert. Bazzite's own
`universalblue` types wrong on AZERTY. Whether the blue screen takes five
letters is the rig's to show.

The check in `%pre`, before anything is erased:

| Refusal (proposed numbers) | When | Words |
|---|---|---|
| 30 | the key is not in `MokListRT` after the key step | "The key step was missed, so nothing was erased. Click Convert to try again." |
| 31 | the key file on the stick differs from its measured sha256 | "The stick's Bazzite key is not the one we checked. Nothing was erased." |

Two misses in a row: refuse and stay refused until the person clicks
Convert again. No loop.

## 4. The read-only system: what changes after the install

Bazzite keeps `/usr` read-only; `/etc`, `/var` and `/boot` can be written.

| Today (Fedora) | On Bazzite (from the image, to confirm on the rig) |
|---|---|
| `post.rs` writes `/etc/grub.d/25_windows_kept` and runs `grub2-mkconfig` | Bazzite's start menu is bootupd's fixed config, which reads `/boot/grub2/custom.cfg` (`usr/lib/bootupd/grub2-static/configs.d/41_custom.cfg`, measured). The 'Windows (kept)' entry goes there; no `grub2-mkconfig` |
| `/boot/efi/EFI/fedora/grubenv` | the same folder: Bazzite installs into `EFI/fedora` (its Anaconda profile, `efi_dir = fedora`) |
| `systemctl` set-default, disable a setup app | the same (`/etc` is writable) |
| settle-in installed into the system | under `/usr/local` (which is `/var/usrlocal` on Bazzite) and `/etc/systemd`, to confirm |

Bazzite's own kickstart also deletes `EFI/fedora` on the ESP before it
installs. Our kickstart does not include that line: on a disk that already
holds a Fedora it would remove that Fedora's start files (R39).

## 5. Where Fedora is still written in

| Place | For Bazzite |
|---|---|
| the stick's `grub.cfg` | a second shape: the live kernel, `root=live:LABEL=UPGDATA`, our kickstart |
| the handoff's `EFI/fedora` paths | likely unchanged: Bazzite also uses `EFI/fedora` |
| `make-kit.sh` | a second layout: the image on `UPGDATA`, `mmx64.efi` in `EFI/BOOT` |
| `post.rs` | the `custom.cfg` route above |
| settle-in's `bootentry.rs` | the entry is still `EFI\fedora\shimx64.efi`; to confirm |

The seam is cut with Fedora's goldens unchanged.

## 6. The stick, the download and the picker

| Release | Download | Largest file | Fits the FAT32 boot partition? | At 100 Mbps | Writing at 10 MB/s |
|---|---|---|---|---|---|
| Fedora 44 KDE | 4.2 GB | 3.0 GB | yes | about 6 min | about 7 min |
| Bazzite KDE | 8.4 GB | 8.0 GB | **no** (FAT32 stops at 4 GiB) | about 11 min | about 14 min |
| Bazzite GNOME | 7.2 GB | not measured | not measured | about 10 min | about 12 min |

- **The image goes on `UPGDATA`**, the stick's exFAT partition. The live
  system's start-up code carries `exfat.ko` and `dmsquash-live-root`
  (measured in its initrd); that it starts from there is the rig's to show.
- **A 16 GB stick at least** for Bazzite. The stick-fit check must know.
- **The ISO's address has no version in it.** `bazzite-stable-live-amd64.iso`
  is rebuilt now and then (this one 2026-10-13, while Bazzite's own stable
  release is 2026-11-05). Each rebuild changes its bytes, so the window,
  which refuses any byte that differs from the measured sha256, would refuse
  every Bazzite download until it is measured again. An older file still
  sits at the old name (`bazzite-stable-amd64.iso`, 2025-10-18). Needed: a
  fixed copy, or a versioned address (a decision owed).
- **NVIDIA machines** have their own ISOs (`bazzite-nvidia-open-...`). The
  scanner knows the GPU, so the picker can choose; which one is a decision
  owed.

The window's words while it downloads: "Getting Bazzite: 3.1 of 8.4 GB.
Nothing on this computer has changed." A download that fails or differs:
"The download did not match, so nothing was changed. Try again." Both
come before Convert.

## 7. The words before the last click (drafts)

> **Bazzite needs you for one minute.** A few minutes after you click
> Convert, the computer restarts and shows a blue screen. Press any key,
> then choose **Enroll MOK**, **Continue**, **Yes**, type `linux`, and
> choose **Reboot**. After that you can walk away. If you miss it, nothing
> is erased and Windows comes back.

With a picture of each blue screen, taken on the rig (owed).

## What the owner decided (2026-11-08)

1. The key step folded into Convert, not a separate restart before it.
2. The password `linux`.
3. A fixed copy of the ISO the project keeps, so its measured sha256
   holds. Where it is hosted is still owed: GitHub's release files must be
   under 2 GiB, and the ISO is 8.4 GB.
4. The picker comes first, with Fedora alone; Bazzite joins once V16
   holds (roadmap item 24).

## The order of work

1. Teach `measure-release.py` the live-ISO layout and record Bazzite in
   `data/releases.json` (the scanner then judges it; that is a product
   change, so it waits for the yes).
2. Cut the seam, Fedora's goldens unchanged.
3. The rig: the unattended start, then the key step on `UPGRIGMOK` (the
   Hyper-V guest that trusts shim, keys driven over WMI), every arm of V16.
4. A real machine with Secure Boot on, the erase first.
