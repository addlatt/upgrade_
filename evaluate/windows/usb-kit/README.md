# USB kit — run the scanner on a machine with one click

A tiny launcher so a **non-technical person, or a headless/borrowed test
machine, can run the scanner without typing anything**. It pairs the one-click
`RUN-SCANNER.cmd` with the single-file scanner build.

## What's here

- `RUN-SCANNER.cmd` — double-click launcher. Self-elevates (UAC), runs the
  scanner to screen, saves a report and a `-DumpMachine` capture next to
  itself, and pauses so the verdict stays readable.

## Making a stick

The launcher needs the **inlined single-file** scanner beside it — not the
source `evaluate/windows/upgrade-scan.ps1`, which dot-sources `data/` and won't
run standalone. So copy two files into one folder on the USB:

1. `evaluate/windows/usb-kit/RUN-SCANNER.cmd`
2. `dist/upgrade-scan.ps1`  (rebuild first with `./build.sh` if `data/` or the
   scanner changed — R9: nothing enforces `dist/` matching source)

That's the whole kit. From WSL, roughly:

```sh
./build.sh
D=/mnt/d   # wherever the stick mounts; or copy via Explorer
cp dist/upgrade-scan.ps1 evaluate/windows/usb-kit/RUN-SCANNER.cmd "$D"/
```

## Running it

On the target machine: open the USB, **double-click `RUN-SCANNER.cmd`**, and
click **Yes** on the blue User Account Control prompt. The "Yes" is the one
unavoidable step — Windows requires consent to run anything elevated, and this
project does not bypass UAC. Nothing is typed.

Admin matters: storage-mode detection, shrinkable-space (`Room to keep
Windows`), and BitLocker all return nothing unelevated.

## What comes back on the stick

- `upgrade-report-*.txt` — the human-readable report (also shown on screen).
- `machine-capture.json` — hardware-only capture. Bring it back and curate it
  into `evaluate/windows/corpus/` with an `Expected` block, so this machine is
  regression-tested forever (CLAUDE.md rule #5). Both filenames are gitignored,
  so a capture left in this folder won't be committed by accident.

**A real capture is not `Synthetic`.** Unlike the VM/spoof corpus entries, a
capture from a physical machine is ground truth for that machine — do not mark
it `"Synthetic": true`.
