# Roadmap

What we mean to build, in what order, what each step waits on, and the
decisions still owed. This is the only doc that says so. Where each step
stands is `STATUS.md`; why it is a risk is `RISKS.md`; how it gets proven
is `VALIDATION.md`.

Items keep their numbers for good, because other docs and commits say
"roadmap item 3". A dropped item stays, marked dropped with a date. An
item's state is a link to `STATUS.md`, never a bar of its own.

**The ordering rule (decided 2026-09-08, the owner).** Validate the killers
before building anything that writes. The build is a vertical, not a list:
one front-to-back, one-click flow, reversible half first, destructive half
second, trailblazed on the rig and then on a machine we own. A borrowed
machine is a half-hour read-only visit that fills one column of the matrix.
The killers in order: Tier 1, no product if they fail (V0, V1, V1b); Tier 2,
a core promise breaks (V4, V3, V2, V9, V10, V11); Tier 3, silent data loss
(V8); Tier 4, kills adoption (V5, V6, V7, V12, V13, V14). V5 and V6 start
whenever there is an afternoon, because one costs an afternoon and the other
a calendar.

Done before the numbering began (all `[####]` in `STATUS.md`): the handoff
on a real machine (V0), the schemas, kickstart generator, stick writer and
image read-back (R16, R17), the disk check, shrink, handoff, alongside
install and undo on the rig (R18, R21), the walk-away resume (R24), the
folder map and the stick-fit gate (R5, R6).

## 0. One-click erase and install

Erase every internal drive, install Fedora, keep nothing, a 2-minute
cancellable countdown as the commit line. Proves: V9, R27, R35. Done on the
Aspire 2026-10-04 (run 11) and the cancel 2026-10-06. Still owed: other
brands. Stands: see [STATUS.md](STATUS.md).

## 1. A real keep-Windows install with Secure Boot on

Fedora beside a shrunk Windows, both systems reached with no key pressed.
Proves: V1b, R21, R22, R36. The Aspire did it by hand (2026-10-10) and then
by itself (2026-10-11: the return applied the slot from its own evidence,
the clock held). Owed there: a Windows update under the guard, the
rollback. Waits on, for the healthy-drive row: a second laptop (the
Aspire's drive is dying, R23). Stands: see [STATUS.md](STATUS.md).

## 2. settle-in

Hardware check on first boot, copying the person's files (unlock BitLocker,
copy, double-check), a "you used these programs, here is the Linux version"
list, undo from the Linux side, clean-up. The clock, Wi-Fi and remote
access are done (2026-10-04, the Aspire). Proves: V10, R28, V3, R19, R13.
Waits on: V3 for the pull's integrity; V8 for never copying a stub (R8).
Stands: see [STATUS.md](STATUS.md).

## 3. The rest of the collecting

Browsers (R20), the BitLocker key, staging on the exFAT partition, a screen
that asks what the person wants. Wi-Fi, the clock and remote access are
done. Proves: R20, R13, R26 (the staging is what the discard offer waits
on). Stands: see [STATUS.md](STATUS.md).

## 4. A proper window instead of the black console

`UPGRADE.exe`, a Rust window that runs the programs on the stick (decided
2026-09-27, the owner, replacing the 2026-09-13 WPF plan). The verify and
stop flows are built; convert and erase come next; its words are drafts
until the owner approves them. Proves: V12, R31. Stands: see
[STATUS.md](STATUS.md).

## 5. More brands

Dell, Lenovo and HP, half an hour each, look but do not touch: the handoff
and the fail-safe rows on each firmware. Proves: V0, R15, R29. Waits on:
access to the machines. Stands: see [STATUS.md](STATUS.md).

## 6. Code signing

Takes time, not code, so the clock starts as soon as there is an entity to
hold the certificate. Proves: V6, R12, R14. Stands: see
[STATUS.md](STATUS.md).

## 7. Rescue mode

For machines turned down because of their drive (R23's case): copy off
whatever can still be read, and check every copy. Proves: R17, R23. Waits
on: item 2's file pull. Stands: see [STATUS.md](STATUS.md).

## 8. Offer to delete Windows when it cannot be kept

The fork "ask me then": a stop, then a sign-in offer with real numbers and a
typed sentence (designed 2026-09-26). Proves: R26. Waits on: item 3's
staging on the exFAT partition, the live-session check, and a first
physical keep-Windows install (item 1). Stands: see [STATUS.md](STATUS.md).

## 9. Old Surfaces (Path B)

Eleven models cannot get Windows 11. Same Windows path; the scanner names
what will not work on Linux, per model. Proves: V7, R2. Waits on: the
Windows path shipping (decided 2026-09-27: devices come after v1). The
research: `research/device-feasibility.md`, `research/future-paths.md`.

## 10. x86 handhelds (Path A)

ROG Ally, Legion Go, MSI Claw. Same Windows path; typing and cancelling
without a keyboard. Proves: V7, R31. Waits on: item 9's door.

## 11. Intel Macs from before 2018 (Path C)

The biggest stranded group after Windows. A new front half on macOS; Wi-Fi
without internet and reading macOS's own files. Proves: new risks the day
it is picked (R2 first). Waits on: the Windows path shipping.

## 12. The TV choice

Smart TVs are blocked, so the TV path is a converted laptop on HDMI: a "TV"
option in the launcher menu, and `settle-in` configures it. Proves: V10.
Waits on: item 2.

## 13. Snapdragon X laptops (Path D)

An ARM build of Fedora. Proves: V14, V1. Waits until people are stranded on
them.

## 14. Intel Macs with T2 (Path E)

One step done by hand in Recovery, and a decision on a non-standard kernel.
Proves: V1, R29. Waits on: item 11.

## 15. Apple Silicon Macs (Path F)

Wrap Asahi Linux's installer rather than rebuild it: upgrade_ scans,
collects and settles in around it. Proves: V10. Waits on: item 11. Together,
11, 14 and 15 cover every Mac made since 2008.

## 16. Keep the computer awake while it works

On Windows and on Linux (decided 2026-10-05, the owner, under R33). Nothing
stops the machine going to sleep halfway through; it happened once on the
Aspire. Proves: R33, R24. Stands: see [STATUS.md](STATUS.md).

## 17. Finish the Rust port: the PowerShell leaves the product

One Rust product (decided 2026-09-27 and, for the leaving, 2026-10-11, the
owner). Done: the whole Windows side in Rust, the cut-over on the Aspire
(2026-10-10), and phase 1 of the leaving: the ten product scripts, their
recorders, the launchers, `build.sh` and `dist/` gone, the port check
Rust-only with frozen goldens. Waits on: the rig answering again for the
unforced row. Owed: phase 2's last rows (the stick writer is built, 2026-10-14; its
R16 write rows and the rig's refusals are still owed, then the script goes),
the `owed` ledger lines (see STATUS.md, V13). Done 2026-10-14 as well:
phase 3 (the data tables to JSON, `CLAUDE.md` rewritten). Proves: V13, R32. The phases: `RUST-PORT.md`,
"What comes next". Stands: see [STATUS.md](STATUS.md).

## 18. Further out

Fill the hardware list from linux-hardware.org instead of by hand, and let
people opt in to sharing how their switch went, so the list gets smarter
with every one. Proves: V7, R3. Waits on: the scanner shipping.

## 19. Rust unification: everything that can be Rust is Rust

The first of three checkpoints, taken in order 19, 20, 21 (decided
2026-10-12, the owner). Item 17's phases 2 and 3 (done 2026-10-14), then
the Linux side the person's computer runs: the installer's `verify.sh` and
`outcome.sh` (`upgrade-installer`, built 2026-10-14), `settle-in`'s starter
script. Build and check tools follow where feasible; the rig harness stays.
The stick writer writes disks, so R16's rows run again (rule #4); the
installer re-earns V1's, V9's and R21's rows.
Proves: V13, R32. Stands: see [STATUS.md](STATUS.md).

## 20. Product unification: fewer choices, one way through

Cut what the person does not need. After an erase the way back is the
walk-away only; the guided stick goes (R30, R33). Verify and Probe leave
the person's window, kept behind a developer flag for the rows (R31). Erase
stops being its own flow and becomes Convert's clean-slate answer, keeping
its own sentence and drive list (R27). Kept on purpose: KDE or GNOME, the
console start, items 8 and 12. A refusal is never trimmed (rule #1).
Proves: V11, V12. Stands: see [STATUS.md](STATUS.md).

## 21. Test optimization

Faster, steadier proof, never less of it. A check on every pull request
that runs `./port-check.sh` (R9); a rig whose guest channel survives fast
power cycles; the 45 `owed` ledger lines. No case or frozen golden is
dropped to save time (rule #5). Proves: R9, V13. Stands: see
[STATUS.md](STATUS.md).

## Decisions owed

The owner's open calls, one line each, naming what waits on them. When one
is decided, the dated line goes in its home and the item here is removed.

- How the erase countdown keeps Fedora's installer shell off its console
  (R38): stop the shell while it runs, count down on a console the
  installer gives no shell, or turn that shell off in the kickstart. The
  next erase on a real machine waits on it, and so do the rig's arms C and
  D: arm C cancelled itself on 2026-10-16 (`v9-erase.csv` line 40).
- Whether R34's release check (held on the Aspire 2026-10-04) meets R11's
  closing clause, or R11's wording still promises more than it verifies.
- R8's closing clause (`-Materialize` against a signed-in client) predates
  the 2026-09-26 decision not to download; what clause replaces it.
- R12's "What" still describes imaging a disk, a design that is gone; the
  wording.
- The words shown to people are drafts awaiting approval: the erase
  launcher's "If you change your mind later", `UPGRADE.exe`'s screens (R31),
  the go-back screen and its Wi-Fi sentence (R30, R33), the SSH line.
- Whether a wimlib-made `install.swm` split installs cleanly with Secure
  Boot on (R30).
- What the way back does on a machine with no firmware product key, where
  Windows Setup stops at its key page and walk-away breaks (R33).
- The shape of a table of machine models and generations, the thing
  hundreds of supported models will need: waits on enough real captures to
  see what the rules look like (R32; `data/README.md`).
- What item 21 (test optimization) covers beyond the pull-request check,
  the rig and the owed ledger lines (R9).
- Choosing among several Linux releases on one stick, and fetching a release
  from Windows: not built, not scheduled (V14, R34).
- Whether reclaim switches the kept-Windows machine's clock back to UTC once
  Windows is gone (R36 says "may").
- Who owns `shutdown /g` (Automatic Restart Sign-On), named in
  `architecture.md` as a courtesy to add later (R24).
- How the rollback makes Windows first on firmware where a `bcdedit`
  order change does not survive the restart (the Acer, `r21-rollback.csv`
  line 6): another write from Windows, or the Linux-side twin
  (`efibootmgr`, which held there; named in `architecture.md`, Rollback)
  (R21).
- `architecture.md`'s Stack still describes the window running the
  PowerShell scripts as child processes (2026-09-27); it needs a dated line
  once the cut-over to Rust lands (R32).
