#!/usr/bin/env python3
"""docs-check.py - do the docs still follow the house rules?

Run through ./docs-check.sh from the repo root. Reads every doc, changes
nothing, prints PASS/FAIL lines and a budget table, exits 1 on any FAIL.

The rules it enforces are written out in .claude/skills/docs/SKILL.md
("What the checker enforces"). The numbers come from
.claude/skills/docs/budgets.json. Tune them there, not here.
"""
import csv
import json
import re
import sys
from pathlib import Path

ROOT = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else Path(__file__).resolve().parents[2]
CFG = json.loads((ROOT / ".claude/skills/docs/budgets.json").read_text())
LEGEND = set(CFG["legend"])
STATES = CFG["states"]
SEVERITIES = CFG["severities"]
PERMANENT = set(CFG["permanent"])

fails = 0
def fail(where, what):
    global fails
    fails += 1
    print(f"    FAIL  {where}: {what}")
def ok(what):
    print(f"    PASS  {what}")
def section(title):
    print(f"\n{title}")

def rel(p): return str(p.relative_to(ROOT))

# ---- which docs are which ------------------------------------------------
all_md = sorted([ROOT / "README.md", ROOT / "CLAUDE.md"] + list((ROOT / "docs").rglob("*.md")))
def kind(p):
    r = rel(p)
    if r in PERMANENT: return "permanent"
    if r.startswith("docs/log/"): return "log"
    if r.startswith("docs/archive/"): return "archive"
    return "transient"
living = [p for p in all_md if kind(p) == "permanent"]
texts = {p: p.read_text(encoding="utf-8").splitlines() for p in all_md}

# ---- 1. style ------------------------------------------------------------
section("1. style (CLAUDE.md, 'How we write')")
EMOJI = re.compile("[\U0001F300-\U0001FAFF☀-➿✅❌⭐]")
BAR = re.compile(r"\[[#.]{4}\]")
n = 0
for p in all_md:
    if kind(p) == "archive": continue
    for i, line in enumerate(texts[p], 1):
        if "—" in line: fail(f"{rel(p)}:{i}", "em dash"); n += 1
        if "–" in line: fail(f"{rel(p)}:{i}", "en dash"); n += 1
        if EMOJI.search(line): fail(f"{rel(p)}:{i}", "emoji"); n += 1
        for b in BAR.findall(line):
            if b not in LEGEND: fail(f"{rel(p)}:{i}", f"bar {b} is not in the legend"); n += 1
if not n: ok("no em dashes, en dashes, emoji or stray bars")

# ---- 2. dated decisions --------------------------------------------------
section("2. every Decided line carries its date")
DATE = re.compile(r"\d{4}-\d{2}-\d{2}")
n = 0
for p in all_md:
    if kind(p) == "archive": continue
    lines = texts[p]
    for i, line in enumerate(lines):
        for m in re.finditer(r"\bDecided\b", line):
            # A decision record reads "Decided (" or "Decided ...:" within a
            # few words. Prose that merely uses the word ("Decided and built,
            # then ...", 'dated "Decided" lines') is not checked.
            if line[max(0, m.start() - 1):m.start()] in ('"', "'"): continue
            window = line[m.end():] + " " + (lines[i + 1] if i + 1 < len(lines) else "")
            window = window[:120]
            if not re.match(r"[^.]{0,30}[(:]", window): continue
            if DATE.search(window) or "(YYYY" in window: continue
            if re.search(r"same (day|night|evening|morning)", window):
                fail(f"{rel(p)}:{i+1}", "Decided 'the same day': write the date"); n += 1
            else:
                fail(f"{rel(p)}:{i+1}", "Decided without a date"); n += 1
if not n: ok("every Decided is dated")

# ---- 3. journal paragraphs in living docs --------------------------------
section("3. dated journal paragraphs live in docs/log/, not in living docs")
JOURNAL = re.compile(r"^\s*(?:[-*]\s+)?\*\*\(?\d{4}-\d{2}-\d{2}")
n = 0
for p in living:
    for i, line in enumerate(texts[p], 1):
        if JOURNAL.match(line) and not re.search(r"Decided|Amended", line):
            fail(f"{rel(p)}:{i}", "dated paragraph: move it to the log, leave a 'Where it stands' link"); n += 1
if not n: ok("no journal paragraphs in living docs")

# ---- 4. RISKS.md structure ---------------------------------------------
section("4. RISKS.md: headings, sections, IDs")
RISK_H = re.compile(r"^## (R\d+): (.*)$")
risks = {}      # id -> dict(state, severity, line, section, length)
def parse_heading(rest):
    parts = [x.strip() for x in rest.split(" · ")]
    return parts
risks_path = ROOT / "docs/RISKS.md"
if risks_path.exists():
    lines = texts[risks_path]
    cur_section = None; cur = None
    for i, line in enumerate(lines, 1):
        if line.startswith("# "):
            cur_section = line[2:].strip()
            if cur: cur["length"] = i - cur["line"]; cur = None
        m = RISK_H.match(line)
        if line.startswith("## ") and cur:
            cur["length"] = i - cur["line"]; cur = None
        if m:
            rid, rest = m.groups()
            parts = parse_heading(rest)
            sev = next((x for x in parts[1:] if x in SEVERITIES), None)
            state_part = parts[-1].split(" ")[0] if len(parts) > 1 else ""
            state = state_part if state_part in STATES else None
            if rid in risks: fail(f"docs/RISKS.md:{i}", f"duplicate {rid}")
            if not sev: fail(f"docs/RISKS.md:{i}", f"{rid}: no severity ({', '.join(SEVERITIES)}) in the heading")
            if not state: fail(f"docs/RISKS.md:{i}", f"{rid}: heading must end with a state ({', '.join(STATES)}); found '{parts[-1][:30]}'")
            closed_section = cur_section is not None and cur_section.lower().startswith(("closed", "resolved"))
            if state == "closed" and not closed_section:
                fail(f"docs/RISKS.md:{i}", f"{rid} is closed but not under '# Closed'")
            if state and state != "closed" and closed_section:
                fail(f"docs/RISKS.md:{i}", f"{rid} is {state} but sits under '# {cur_section}'")
            cur = risks[rid] = {"state": state, "severity": sev, "line": i, "section": cur_section, "length": 0}
    if cur: cur["length"] = len(lines) - cur["line"] + 1
    ok(f"{len(risks)} risks parsed") if risks else fail("docs/RISKS.md", "no risk headings found")
else:
    fail("docs/RISKS.md", "missing")

# ---- 5. VALIDATION.md gates --------------------------------------------
section("5. VALIDATION.md: gate headings and IDs")
GATE_H = re.compile(r"^## (V\d+[a-z]?)\b:?\s*(.*)$")
gates = {}
val_path = ROOT / "docs/VALIDATION.md"
if val_path.exists():
    lines = texts[val_path]; cur = None
    for i, line in enumerate(lines, 1):
        if line.startswith("## ") and cur:
            cur["length"] = i - cur["line"]; cur = None
        if line.startswith("# ") and cur:
            cur["length"] = i - cur["line"]; cur = None
        m = GATE_H.match(line)
        if m:
            gid, rest = m.groups()
            parts = parse_heading(rest)
            state_part = parts[-1].split(" ")[0] if parts else ""
            state = state_part if state_part in STATES else None
            if gid in gates: fail(f"docs/VALIDATION.md:{i}", f"duplicate {gid}")
            if not state: fail(f"docs/VALIDATION.md:{i}", f"{gid}: heading must end with a state ({', '.join(STATES)})")
            cur = gates[gid] = {"state": state, "line": i, "length": 0}
    if cur: cur["length"] = len(lines) - cur["line"] + 1
    ok(f"{len(gates)} gates parsed") if gates else fail("docs/VALIDATION.md", "no gate headings found")
else:
    fail("docs/VALIDATION.md", "missing")

# ---- 6. STATUS.md against the CSVs --------------------------------------
section("6. STATUS.md: one row per ID, evidence matches the CSV")
status_path = ROOT / "docs/STATUS.md"
ROW = re.compile(r"^\|\s*(R\d+|V\d+[a-z]?)\s*\|(.*)\|\s*$")
EVID = re.compile(r"`?([\w-]+\.csv)`?\s+line\s+(\d+)(?:,\s*|\s+)`?([a-z][a-z0-9-]*)`?")
status_ids = {}
if not status_path.exists():
    fail("docs/STATUS.md", "missing: the one home for 'where we are'")
else:
    for i, line in enumerate(texts[status_path], 1):
        m = ROW.match(line)
        if not m: continue
        sid, rest = m.groups()
        if sid in status_ids: fail(f"docs/STATUS.md:{i}", f"{sid} has two rows")
        status_ids[sid] = i
        cells = [c.strip() for c in rest.split("|")]
        evidence = cells[2] if len(cells) > 2 else ""
        if "none yet" in evidence: continue
        em = EVID.search(evidence)
        if not em:
            fail(f"docs/STATUS.md:{i}", f"{sid}: evidence must read `file.csv` line N, `result` (or 'none yet')"); continue
        fname, lineno, word = em.group(1), int(em.group(2)), em.group(3)
        fpath = ROOT / "docs/validation-results" / fname
        if not fpath.exists():
            fail(f"docs/STATUS.md:{i}", f"{sid}: {fname} does not exist"); continue
        rows = fpath.read_text(encoding="utf-8").splitlines()
        if lineno < 2 or lineno > len(rows):
            fail(f"docs/STATUS.md:{i}", f"{sid}: {fname} has no line {lineno}"); continue
        header = next(csv.reader([rows[0]]))
        if "result" in header:
            vals = next(csv.reader([rows[lineno - 1]]))
            actual = vals[header.index("result")] if len(vals) > header.index("result") else ""
            if actual != word:
                fail(f"docs/STATUS.md:{i}", f"{sid}: {fname} line {lineno} says '{actual}', STATUS says '{word}'")
    for rid, r in risks.items():
        if r["state"] != "closed" and rid not in status_ids:
            fail("docs/STATUS.md", f"no row for open risk {rid}")
    for gid in gates:
        if gid not in status_ids: fail("docs/STATUS.md", f"no row for gate {gid}")
    for sid in status_ids:
        if sid not in risks and sid not in gates:
            fail(f"docs/STATUS.md:{status_ids[sid]}", f"{sid} is not a defined risk or gate")
    if status_ids: ok(f"{len(status_ids)} rows checked against the CSVs")

# ---- 7. references to IDs -----------------------------------------------
section("7. every R## and V## mentioned is defined")
IDREF = re.compile(r"\b(R\d+|V\d+b?)\b(?!\.\d)")
n = 0
for p in all_md:
    if kind(p) == "archive": continue
    seen = set()
    for i, line in enumerate(texts[p], 1):
        for m in IDREF.finditer(line):
            tid = m.group(1)
            if tid in risks or tid in gates or tid in seen: continue
            # skip version-like tokens (V1.21) and the CSV column words
            if re.match(r"[RV]\d+$", tid) and (tid in risks or tid in gates): continue
            seen.add(tid); n += 1
            fail(f"{rel(p)}:{i}", f"{tid} is not defined in RISKS.md or VALIDATION.md")
if not n: ok("all IDs resolve")

# ---- 8. relative links --------------------------------------------------
section("8. relative links point at files that exist")
LINK = re.compile(r"\]\(([^)\s]+)\)")
n = 0
for p in all_md:
    if kind(p) == "archive": continue
    for i, line in enumerate(texts[p], 1):
        for target in LINK.findall(line):
            if re.match(r"[a-z]+:", target) or target.startswith("#"): continue
            path = target.split("#")[0]
            if not (p.parent / path).exists():
                fail(f"{rel(p)}:{i}", f"link to {target} does not resolve"); n += 1
if not n: ok("all relative links resolve")

# ---- 9. expiry and archive ----------------------------------------------
section("9. transient docs say when they expire; archived docs say when they were archived")
n = 0
for p in all_md:
    k = kind(p); head = texts[p][:10]
    if k == "transient" and not any(l.startswith("Expires when:") for l in head):
        fail(rel(p), "not permanent, not log, not archive: needs an 'Expires when:' line in its first ten lines"); n += 1
    if k == "archive" and not re.match(r"Archived \d{4}-\d{2}-\d{2}:", texts[p][0] if texts[p] else ""):
        fail(rel(p), "archived doc must start with 'Archived YYYY-MM-DD: <why>'"); n += 1
if not n: ok("every transient doc expires, every archived doc is dated")

# ---- 10. budgets --------------------------------------------------------
section("10. budgets (overhead + per_unit x units; from budgets.json)")
print(f"    {'doc':<40} {'lines':>6} {'budget':>7} {'units':>6}  state")
for docname, b in CFG["docs"].items():
    p = ROOT / docname
    if not p.exists():
        print(f"    {docname:<40} {'-':>6} {'-':>7} {'-':>6}  missing"); continue
    lines = texts.get(p) or p.read_text().splitlines()
    units = 0; budget = b["overhead"]
    if "unit" in b:
        rx = re.compile(b["unit"])
        heads = [l for l in lines if rx.search(l)]
        if docname == "docs/RISKS.md":
            open_n = sum(1 for r in risks.values() if r["state"] != "closed")
            closed_n = len(risks) - open_n
            units = len(risks)
            budget += open_n * b["per_unit"] + closed_n * b.get("closed_per_unit", b["per_unit"])
        else:
            units = len(heads); budget += units * b["per_unit"]
    over = len(lines) > budget
    print(f"    {docname:<40} {len(lines):>6} {budget:>7} {units:>6}  {'OVER' if over else 'ok'}")
    if over: fail(docname, f"{len(lines)} lines against a budget of {budget}")
# unit caps
for rid, r in risks.items():
    cap = CFG["docs"]["docs/RISKS.md"].get("closed_unit_max" if r["state"] == "closed" else "unit_max")
    if cap and r["length"] > cap:
        fail(f"docs/RISKS.md:{r['line']}", f"{rid} is {r['length']} lines; an {'open' if r['state'] != 'closed' else 'closed'} risk fits in {cap}. Move history to the log")
for gid, g in gates.items():
    cap = CFG["docs"]["docs/VALIDATION.md"].get("unit_max")
    if cap and g["length"] > cap:
        fail(f"docs/VALIDATION.md:{g['line']}", f"{gid} is {g['length']} lines; a gate fits in {cap}. Move runs to the log")

print()
if fails:
    print(f"docs-check: {fails} FAIL")
    sys.exit(1)
print("docs-check: all PASS")
