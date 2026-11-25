# The knowledge tables

What the scanner knows about hardware, distributions and the Linux releases
the kit can carry. JSON, one file per table (decided 2026-10-14, the owner),
compiled into the scanner. Adding a device is one entry and a cited source.

| File | Holds | Key |
|---|---|---|
| `devices/wifi.json` | Wi-Fi adapters, then a fallback by vendor | PCI id `vvvv:dddd`, lower-case hex; vendor `vvvv` |
| `devices/gpu.json` | graphics adapters, then a rule by vendor | the same |
| `devices/audio.json` | audio quirks (smart amplifiers and the like) | `Match`, a regular expression over the device path |
| `devices/storage.json` | Intel VMD controller ids (RISKS R1) | PCI id |
| `devices/vendors.json` | machine quirks | `Match`, a regular expression over `vendor model` |
| `devices/apps.json` | installed programs with no Linux path | `Match`, a regular expression over the program's name |
| `programs.json` | the Linux or web version of a Windows program, for `settle-in`'s list (compiled into `settle-in`, not the scanner) | `Match`, the same; the first match wins |
| `releases.json` | the Linux releases the kit can carry, with what Secure Boot needs of each and the kernel each installs | the release id |

Every file starts with an `_about` entry for the reader. The scanner
ignores it; keep it true.

## The fields

- `Name`: shown to the person.
- `Driver`: the in-tree kernel module, or `proprietary` or `none`.
- `MinKernel`: the earliest kernel with usable support. Empty means long
  supported.
- `Status`: `ok` (works out of the box), `warn` (works with a caveat:
  firmware, a recent kernel, a setting) or `fail` (does not work, or needs
  hardware replaced). When unsure between two, pick the more cautious one
  (CLAUDE.md, rule 1).
- `Note`: one line, shown to the person verbatim. Say what happens and what
  to do, not "may or may not work".
- `Severity` (`apps.json` only): `blocker` (do not convert this machine for
  this program), `friction` (an alternative exists but is not a drop-in),
  `info`.
- `Kind` (`programs.json` only): `same` (the program itself runs on Linux),
  `alternative` (a different program does the job), `web` (use it in the
  browser), `built-in` (the desktop already does it), `not-needed` (a
  driver, a maker's tool or a part of Windows: counted, not listed).
  `Linux` names what to use; `Flathub` is its Flathub id, `Web` its address.
  `settle-in` installs nothing (decided 2026-11-07, the owner).
- `Source`: how we know. A kernel commit, the driver's supported-hardware
  list, a distribution's release notes, or a machine capture in
  `evaluate/windows/corpus/`. Empty means nobody has cited one yet; the
  entry stands, but a reviewer should know. "It should work" is not a
  source.
- `Verified`: the date the `Source` was last checked, or empty.

A PCI id is found in the report's "unmatched devices" section. Find yours,
add one entry, say how you know, open a pull request.

## The checks

`evaluate/scan/src/data.rs` refuses a table a check could not use, and
`./port-check.sh` runs that check: a status or severity outside the words
above, a pattern that is not a regular expression, an id that is not
`vvvv:dddd` in lower-case hex, a release whose boot file has no sha256, a
`Verified` date that is not a date. A wrong `ok` is not caught by any
check; that is what the source is for.

`programs.json`'s Flathub entries are checked against Flathub's own
catalog by `data/tools/check-programs.py`, which sets `Verified` to the day
Flathub listed the id and not as end-of-life, and clears it otherwise. Run
it before a pull request that touches the table; never type the date. Its
first run (2026-11-07) found `org.mozilla.Thunderbird` end-of-life on
Flathub (now `org.mozilla.thunderbird_esr`).

## The releases

Every fact under a release entry is measured from the release's own files
by `tools/measure-release.py` and pinned by sha256: the installer image,
each desktop's live image, and each boot file's SBAT generations, built-in
revocation levels and signing authorities. Under `Kernels` it records the
release each kernel image names itself (what `uname -r` says), read from the
image's boot header: the stick's kernel, and each kernel inside what gets
installed. The scanner compares the installed ones with the newest
`MinKernel` the computer's devices ask for, and a release that is older is
not offered (decided 2026-11-24, the owner; RISKS R3). There is no
hand-typed kernel table any more. Never type or edit one by hand:
run the tool, which replaces the entry with that id or adds a new one. The
kit builder and `rig/vm/fetch-release.sh` refuse any file whose hash
differs from the entry.

A release the kit does not offer yet is measured into its own file under
`data/measured/` (`measure-release.py --casper ... --out`), not into
`releases.json`: an entry in the table is an offer to a person, and no
program reads `data/measured/`. `measure-release.py --offer` moves such a
file into the table as it is. Ubuntu 26.04.1 went that way (2026-11-13).

## What is not here yet

A table of machine models and generations (which year of which line works
how) is the thing hundreds of supported models will need. Its shape waits
on enough real captures to see what the rules look like; the decision is
listed in `docs/ROADMAP.md` under "Decisions owed".
