# Shapes

The templates for everything the docs skill writes. Copy the shape, fill the
facts, keep it short. The checker reads the headings and the STATUS.md table
mechanically, so the punctuation matters: the parts of a heading are
separated by ` · ` (space, middle dot, space).

## A risk (RISKS.md)

Under `# Open` while open, moved under `# Closed` when closed. 40 lines at
most while open, 12 when closed. Five parts, always in this order.

```markdown
## R36: The kept Windows' clock runs hours wrong after every Linux boot · medium · built

**What.** Windows keeps the hardware clock in local time; Linux keeps it in
UTC. On the keep-Windows path both read the same clock, so after every Linux
boot Windows runs fast by the zone's whole offset.

**If real.** (It is: seen 2026-10-10 on the Aspire, 7 h fast.) Appointments
and file times are wrong until Windows syncs. No data is lost: medium.

**Decided (2026-10-12, the owner):** settle-in sets `RealTimeIsUniversal`
in the kept Windows so both systems read the clock as UTC.

**Closes when.** A Linux boot followed by a Windows boot on a real machine
leaves the Windows clock right, recorded in `settle-in-first-start.csv`.

**Where it stands.** `[##..]` built, unfired. Found on the slot cycles
([log](log/2026-10.md#2026-10-10-shim-in-windows-slot-on-the-aspire)).
Owed: the rig arm, then the Aspire.
```

The heading's last two parts are the severity (`critical`, `high`, `medium`,
`low`) and the state (`planned`, `built`, `held-on-rig`, `held-on-machine`,
`closed`). A closed risk names the row in "Where it stands":
`Closed by v2-install.csv line 11 (2026-10-10).`

## A gate (VALIDATION.md)

60 lines at most. The method stays here; the runs go to the log.

```markdown
## V1b: Installing alongside a shrunk Windows leaves Windows bootable · kills: the default path's safety net · RISKS R21 · held-on-machine

**Why it has its own gate.** One or two sentences.

**What passes.** The checklist, as the CSV's result words. Point to
`validation-results/README.md` for the vocabulary rather than repeating it.

**How it is run.** Rig arm, then the real machine. Scripts by name.

**Where it stands.** One line: the bar, the latest row, what is owed, a
link to the log.
```

## A STATUS.md row

STATUS.md is one table plus a five-line "Right now". Each open risk and each
gate has exactly one row. The evidence cell is read by the checker:
`` `file.csv` line N, `result` `` names a real line whose `result` column
says that word. Write `none yet` when there is no row.

```markdown
| ID | What | State | Latest evidence | Owed |
|---|---|---|---|---|
| V1b | install beside Windows, both boot | `[####]` held-on-machine | `v2-install.csv` line 11, `pass-plumbing` (2026-10-10, the Aspire) | the return applying the slot by itself; rig arm; a Windows update under the guard; the rollback on this machine |
| R36 | kept Windows' clock wrong after a Linux boot | `[##..]` built | none yet | rig arm, then the Aspire |
```

Line N is the CSV's line number as an editor shows it (line 1 is the
header), the same convention the CSVs' README already uses.

## A roadmap item (ROADMAP.md)

Numbered for good; a dropped item keeps its number and says "dropped"
with the date. Ten lines at most. The state is a link to STATUS.md.

```markdown
## 1. A real keep-Windows install with Secure Boot on

Fedora beside a shrunk Windows on a machine with a healthy drive, both
systems reached with no key pressed. Waits on: a second laptop (the Aspire's
drive is dying, R23). Proves: V1b, R21. Stands: see [STATUS.md](STATUS.md).
```

The "Decisions owed" section at the end of ROADMAP.md is a plain list, one
line each, naming the risk or doc that waits on it:

```markdown
## Decisions owed

- The kept Windows' clock after a Linux boot: UTC in Windows, or local time
  in Linux (R36).
```

## A log entry (docs/log/YYYY-MM.md)

Append-only, newest at the bottom. The heading is the anchor other docs
link to, so it starts with the date.

```markdown
## 2026-10-10: shim in Windows' slot on the Aspire (R21, R22, V1b; found R36)

What was done, in plain words, in order. What it proved (the row:
`v2-install.csv` line 11, `pass-plumbing`). What surprised. What it owes.
A failure is written here next to what fixed it.
```

An entry is as long as it needs to be. This is the one place with no
budget.

## An expiry line (a phase or session doc)

In the first ten lines of any doc under `docs/` that is not permanent:

```markdown
Expires when: the PowerShell scripts leave the stick (RISKS R32 closed).
```

## An archive line (docs/archive/)

The first line of an archived doc. Nothing else in it changes.

```markdown
Archived 2026-10-12: the cut-over sessions it was written for have run (RUST-PORT.md, "The cut-over sessions").
```

## A Decided line

Anywhere a decision lives (architecture.md, or a risk):

```markdown
**Decided (2026-10-10, the owner):** shim in Windows' slot on firmware that
ignores operating-system entries.
```

Full date, who decided, one or two sentences. "Amended (date):" follows the
same shape when a decision is changed, and the original line stays.
