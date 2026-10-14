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
| `distros.json` | the distribution kernel table | the distribution's name |
| `releases.json` | the Linux releases the kit can carry, with what Secure Boot needs of each | the release id |

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

## The distribution table

`distros.json` goes stale faster than anything else here, and a stale
kernel number gives advice that is confident and wrong. `Kernel` is the
kernel a fresh install of the current stable release gives on first boot,
never a backport. `"Approx": true` marks a number nobody checked against
the release notes. When you refresh the table, set its `Verified` date and
cite the release notes; the scanner warns the person when that date is
over 120 days old.

## The releases

Every fact under a release entry is measured from the release's own files
by `tools/measure-release.py` and pinned by sha256: the installer image,
each desktop's live image, and each boot file's SBAT generations, built-in
revocation levels and signing authorities. Never type or edit one by hand:
run the tool, which replaces the entry with that id or adds a new one. The
kit builder and `rig/vm/fetch-release.sh` refuse any file whose hash
differs from the entry.

## What is not here yet

A table of machine models and generations (which year of which line works
how) is the thing hundreds of supported models will need. Its shape waits
on enough real captures to see what the rules look like; the decision is
listed in `docs/ROADMAP.md` under "Decisions owed".
