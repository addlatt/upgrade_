# USB kit: the double-click starters for the diagnostics

Four small `.cmd` files a person can double-click on the stick, plus one
helper for the bench. Each `.cmd` asks for administrator rights itself (the
blue User Account Control prompt, one "Yes"), runs one of the kit's Rust
programs from the stick, and pauses so the result stays readable. Nothing is
typed. They are not the product's front door: that is `UPGRADE.cmd` and the
window (`upgrade_/windows/handoff-payload/`).

`./make-kit.sh` copies the four `.cmd` files to the root of the stick
layout (`dist/kit/stick/`), beside the programs they run, and checks first
that no `if` block in them carries an unescaped parenthesis. No PowerShell
script ships on the stick (since 2026-10-14).

## What's here

| File | Runs | What comes back on the stick |
|---|---|---|
| `DIAG-SECUREBOOT.cmd` | `upgrade-scan.exe diag secureboot` | `upgrade_\report\secureboot-diag-<time>.txt`: the firmware's revocation level (SBAT), the size of its key lists, and the SBAT data of the stick's boot files |
| `DIAG-SMART.cmd` | `upgrade-scan.exe diag smart` | `upgrade_\reports\smart-diag-<computer>.txt`: every drive's health counters and the disk error log |
| `DIAG-VOLUME.cmd` | `upgrade-scan.exe diag volume` | `upgrade_\reports\volume-diag-<computer>.txt`: why Windows says a volume needs a check |
| `RUN-STORAGE-MODE.cmd` | `upgrade-prologue.exe storage-mode start` | the V5 both-modes run, below |
| `Copy-Kit.ps1` | nothing on the stick: a bench helper, run on the host | see below |

The three diagnostics change nothing on the computer. Their files hold that
machine's hardware details: never commit one.

## `RUN-STORAGE-MODE.cmd`: the V5 one-click (both SATA modes)

One double-click, one UAC "Yes", one OK on the window that explains the two
visits to the setup screen. What `upgrade-prologue storage-mode` does, in
order:

- scans;
- arms a Safe Mode boot through a *copied* boot entry that boots exactly
  once (`bcdedit /bootsequence`);
- registers the SYSTEM startup task for the normal boots;
- sets a `*`-prefixed RunOnce that restarts the Safe Mode boot the moment
  the person signs in there (Task Scheduler does not run our task in Safe
  Mode: rig, 2026-09-14);
- restarts straight into the firmware setup (`shutdown /r /fw`).

The person changes SATA Mode there and signs in once at the Safe Mode
screen. The machine does the rest. Everything armed is undone on every way
out. What comes back: `upgrade_\storage-mode\leg1..3\` (a report and a
capture per mode) and `upgrade_\storage-mode\storage-mode.json` (the
record). Rows: `rig/v5-verdict.py --from-run <that folder>`. Captures are
curated into `evaluate/windows/corpus/` like any other, one per mode.

**A real capture is not `Synthetic`.** Unlike the VM and spoofed corpus
entries, a capture from a physical machine is ground truth for that machine.
Do not mark it `"Synthetic": true`.

## `Copy-Kit.ps1`: a bench helper, not shipped

It copies a built kit (`dist/kit/stick`) onto a stick that already has a
FAT32 volume, clears what a previous run left under `upgrade_\`, re-hashes
every file against the kit's `SHA256SUMS` and ejects. It never partitions or
formats, and it refuses anything that is not a removable USB volume. The
stick writer is `upgrade-stick.exe` (`evaluate/stick`, RISKS R16).
`make-kit.sh` does not pack this script.
