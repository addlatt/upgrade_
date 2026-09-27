# USB kit: run the scanner on a machine with one click

A tiny launcher so that **a non-technical person, or a borrowed or headless
test machine, can run the scanner without typing anything**. It pairs the
one-click `RUN-SCANNER.cmd` with the single-file scanner build.

## What's here

- `RUN-SCANNER.cmd`: the double-click launcher. It asks for admin rights
  itself (the UAC prompt), runs the scanner on screen, saves a report and a
  `-DumpMachine` capture next to itself, and pauses so the verdict stays
  readable.

## Making a stick

For a V0 handoff run, don't copy anything by hand. `./make-kit.sh` at the
repo root builds complete, verified stick layouts under `dist/kit/`
(scanner + this launcher + the handoff harness, launchers and payload). See
`upgrade_/windows/handoff-payload/README.md`. What follows is the
scanner-only kit.

The launcher needs the **inlined single-file** scanner beside it. Not the
source `evaluate/windows/upgrade-scan.ps1`: that one pulls in the files
under `data/` as it starts (dot-sourcing) and won't run on its own. So copy two files into
one folder on the USB:

1. `evaluate/windows/usb-kit/RUN-SCANNER.cmd`
2. `dist/upgrade-scan.ps1`  (rebuild first with `./build.sh` if `data/` or
   the scanner changed. R9: nothing checks that `dist/` matches the source)

That's the whole kit. From WSL, roughly:

```sh
./build.sh
D=/mnt/d   # wherever the stick mounts; or copy via Explorer
cp dist/upgrade-scan.ps1 evaluate/windows/usb-kit/RUN-SCANNER.cmd "$D"/
```

## Running it

On the target machine: open the USB, **double-click `RUN-SCANNER.cmd`**, and
click **Yes** on the blue User Account Control prompt. That "Yes" is the one
step that can't be avoided. Windows requires consent to run anything with
admin rights, and this project does not get around UAC. Nothing is typed.

Admin rights matter: storage-mode detection, shrinkable space (`Room to
keep Windows`) and BitLocker all come back empty without them.

## What comes back on the stick

- `upgrade-report-*.txt`: the report for people to read (also shown on
  screen).
- `machine-capture-<date>-<time>.json`: a hardware-only capture, one per run
  (the name carries the scan time, so two runs on one visit never overwrite
  each other). Bring it back and curate it into `evaluate/windows/corpus/`
  with an `Expected` block, so this machine is regression-tested forever
  (CLAUDE.md rule #5). Both filenames are gitignored, so a capture left in
  this folder won't be committed by accident.

**A real capture is not `Synthetic`.** Unlike the VM and spoofed corpus
entries, a capture from a physical machine is ground truth for that machine.
Do not mark it `"Synthetic": true`.

## `RUN-STORAGE-MODE.cmd`: the V5 one-click (both SATA modes)

Pairs with `Test-StorageMode.ps1` and the single-file scanner on the same
stick (`make-kit.sh` lays out all three). One double-click, one UAC "Yes",
one OK on the window that explains the two visits to the setup screen.

What the harness does, in order:

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
curated into `corpus/` like any other, one per mode.
