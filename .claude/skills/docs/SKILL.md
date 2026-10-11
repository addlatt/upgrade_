---
name: docs
description: The one way to change this repo's docs (README.md, CLAUDE.md, docs/STATUS.md, architecture.md, RISKS.md, VALIDATION.md, docs/log/, validation-results/). Use it whenever anything needs writing down - a rig or Aspire run, a new CSV row, a finding, a decision by the owner, a new risk or gate, a doc that looks stale, repeated or too long - and before any pull request that touches a .md file. It gives each fact one home, keeps the docs under a size budget that scales with the project, and runs ./docs-check.sh to prove the docs still agree with the CSVs and with each other.
---

# Maintaining the docs

Read `CLAUDE.md` first if you have not. Its rules (refuse by default, evidence
not argument, the commit line, trust is spent once, spoof everything) are why
the docs matter: the only asset this project has is that what it says can be
trusted. A doc that is stale is a doc that lies, and a doc nobody can read to
the end is a doc nobody checks.

## Why this skill exists

By 2026-10-11 the docs had grown from 2,000 to 9,600 lines in seven weeks.
Every event was written in four to seven places (the Aspire's run 10 was
narrated in seven docs). Eight sections claimed to say "where we are", and
they disagreed. Thirteen open risks sat under a heading called "Resolved"
because new ones were appended at the end of the file. One fact changing
meant five copies to find, and three were missed.

The fix is structural, not effort: **one home per fact, history in a log,
budgets that scale with the project, and a checker that fails when any of
it drifts.** This skill is the procedure for working under those rules.

## The map: one home per fact

Every kind of statement has exactly one doc it may live in. Everything else
links there. If you are about to write a sentence and it belongs in another
doc, write a link instead.

| What you are writing | Its one home |
|---|---|
| The mission, the rules, conventions, how we write, how sessions work | `CLAUDE.md` |
| How the thing works, and dated design decisions about how it works | `docs/architecture.md` |
| What is unproven, why it matters, what would close it | `docs/RISKS.md` (one `R##` each) |
| How a thing gets proven, and what counts as a pass | `docs/VALIDATION.md` (one `V##` each) |
| What a CSV's columns and result words mean | `docs/validation-results/README.md` |
| The rows themselves | `docs/validation-results/*.csv` (ground truth; never edited, only appended) |
| **Where everything stands right now** | `docs/STATUS.md`, and nowhere else |
| **What happened, and when** (runs, findings, fixes, the story) | `docs/log/YYYY-MM.md`, append-only |
| The public pitch | `README.md` (links to STATUS.md for state) |
| A phase or a session (a port, a cut-over, a handoff prompt) | its own doc with an `Expires when:` line, then `docs/archive/` |
| Long-term research (devices, future paths) | `docs/research/` |

Two consequences worth saying out loud:

- **A dated paragraph is a log entry.** A living doc never opens a paragraph
  with `**2026-10-04: ...**`. That paragraph goes in the log, and the living
  doc gets a one-line "Where it stands" that links to it. The only dated
  lines a living doc keeps are decisions: `Decided (2026-10-04, the owner):`.
- **A status bar belongs in STATUS.md.** A risk or gate carries one state
  word in its heading; the table in STATUS.md carries the bar, the latest
  row and what is owed. README.md and CLAUDE.md say "see STATUS.md".

## The words for state

Five states, each tied to one bar. Same vocabulary in STATUS.md, RISKS.md
headings and VALIDATION.md headings.

| State | Bar | Means |
|---|---|---|
| `planned` | `[#...]` | designed, nothing built |
| `built` | `[##..]` | code exists, never run against anything |
| `held-on-rig` | `[###.]` | passed on the rig (VMs, spoofed hardware); plumbing only |
| `held-on-machine` | `[####]` | passed on a real machine, at least one row |
| `closed` | `[####]` | the closing clause is met; the row that met it is named |

`[FAIL]` is a flag, not a state: a real failure that is still in the record.
`[....]` is "not started" and only STATUS.md uses it.

A risk closes only on evidence (rule #2). `held-on-machine` on one machine
is one data point, and the heading says so when the matrix is still owed.

## The four jobs

Every change to the docs is one of these. Do the steps in order, then run
the checker, then open the pull request (below).

### Record: a run, a row or a finding

The common case after a rig or Aspire run.

1. The CSV row is already written (by the harness, or by hand for a
   physical run). The row is the fact. Read it.
2. Write the log entry in `docs/log/YYYY-MM.md`: date, title, the IDs it
   touches, what happened in plain words, what it proved, what it owes.
   Shape in `references/shapes.md`. Newest entry at the bottom.
3. Update the row in `docs/STATUS.md` for each ID touched: state, bar,
   latest evidence (`file.csv line N, result`), what is owed.
4. If the state word of a risk or gate changed, change it in its heading
   and refresh its "Where it stands" line (one line, links to the log).
5. A finding that is a new unknown is a new risk: do "New" as well.

Do not retell the run anywhere else. Not in README.md, not in CLAUDE.md,
not in the risk's body.

### Decide: a dated decision

1. Write `**Decided (YYYY-MM-DD, the owner): ...**` in its one home: the
   risk it answers, or the architecture section it changes. Full date,
   every time; never "the same day".
2. If the decision changes a risk's stakes, change that risk's severity in
   the same commit and say why in its "If real" paragraph.
3. Log entry (short) and STATUS.md row if the state changed.
4. A decision that touches two docs (architecture and a risk) is written in
   full once and linked from the other.

### New: a risk or a gate

1. Take the next number. Risks are `R##` in RISKS.md, gates `V##` in
   VALIDATION.md; both counted from the headings, never reused.
2. Write it in the house shape (`references/shapes.md`): what, if real
   (the damage, hence the severity), decided lines if any, closes when,
   where it stands. An open risk fits in 40 lines. If it does not, the
   extra is history and goes to the log.
3. Put it in the right section: open risks under `# Open`, closed under
   `# Closed`. New risks go in the open section, not at the end of the file.
4. Add its row to STATUS.md.
5. If it needs its own CSV, add the CSV with a header row and its entry in
   `docs/validation-results/README.md` (columns, result words, what a
   pass requires).

### Check and compact

Run this whenever a doc looks stale, repeats another, or the checker
fails on size. Also once at the end of any long day of changes.

1. `./docs-check.sh`. Read every FAIL line.
2. Stale: find the fact's one home, fix it there, replace the other
   copies with links.
3. Over budget: move dated paragraphs to the log (verbatim, with their
   date; nothing is lost), then cut each risk or gate to its five parts.
   Close what the evidence has closed; move it under `# Closed`.
4. Expired: a phase doc whose `Expires when:` condition is met moves to
   `docs/archive/` with `Archived YYYY-MM-DD: <why>` as its first line.
   Links to it are updated in the same commit.
5. Run the checker again. It must pass before the pull request.

## What the checker enforces

`./docs-check.sh` (which runs `docs/tools/docs-check.py`) reads every doc
and fails on:

- style: an em dash or en dash, an emoji, a bar outside the legend
  (CLAUDE.md, "How we write", is the spec);
- an undated `Decided`, or one dated "the same day";
- a dated journal paragraph (`**2026-..`) in a living doc;
- a risk or gate heading without a severity and state from the vocabulary,
  a duplicate ID, an open risk under `# Closed` or a closed one outside it;
- a STATUS.md row whose evidence names a CSV line that does not exist or
  whose `result` column says something else; a risk or gate with no row;
  a row with no risk or gate;
- an `R##` or `V##` mentioned anywhere that is not defined;
- a relative link to a file that is not there;
- a doc under `docs/` that is neither permanent, log, archive, nor carries
  `Expires when:`; an archived doc without its `Archived` line;
- a doc over its budget, or a single risk or gate over its unit cap.

It prints a table of each doc's lines against its budget whether it fails
or not. The numbers live in `budgets.json` next to this file; tune them
there. The checker never edits a file.

## Budgets: a function of the project, not the calendar

A doc's budget is its fixed overhead plus an allowance per unit it holds. A
unit is something the project actually has: an open risk, a gate, a module,
a CSV. So RISKS.md may grow when a risk is found, and must shrink when one
closes. It may never grow because a month passed. The log is the only
unbounded doc, and it is append-only.

Today's numbers (from `budgets.json`): an open risk 40 lines, a closed one
12; a gate 60; an architecture module 80; a CSV entry 30; README.md and
CLAUDE.md 250 each flat; two STATUS.md lines per ID.

## Writing

The voice is CLAUDE.md's "How we write": short sentences, plain words, facts
exact, no em dashes, no emoji, metaphors only where they carry weight. Two
habits matter most here. Simplifying the words never softens a claim, a
refusal or a date. And a failure stays in the record, in plain words, next to
what fixed it; compaction moves it to the log, it does not delete it.

## Finishing a change

Docs changes follow the same path as code (CLAUDE.md, "Working alongside
other sessions"): your own worktree, a branch, `./docs-check.sh` passing,
a pull request the owner merges. The commit message says what fact changed
and which home it lives in. Never commit a machine report.

## References

- `references/shapes.md`: the templates. A risk, a gate, a STATUS.md row,
  a log entry, an expiry line, an archive line. Read it before writing any
  of those.
- `CLAUDE.md`, "How we write" and "When the design changes": the voice, and
  the rule that architecture, RISKS and VALIDATION must agree.
- `docs/validation-results/README.md`: what each CSV's words mean, so the
  STATUS.md evidence cell says the same thing the row says.
