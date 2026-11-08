#!/usr/bin/env python3
"""
check-programs.py - check every Flathub id in data/programs.json against
Flathub's own catalog, and set that entry's Verified date only when the
catalog has it and has not marked it end-of-life (rule #2: the table says
a Linux version exists because Flathub said so on that date, not because
someone typed it).

    check-programs.py [--table data/programs.json] [--offline]

For each entry with a Flathub id: GET https://flathub.org/api/v2/appstream/<id>.
A 200 whose JSON names the same id and is not end-of-life sets Verified to
today and Source to the app's Flathub page. Anything else clears Verified
and is printed as FAIL, and the tool exits 1, so a stale id is seen.
Entries without a Flathub id are left as they are. --offline only checks
the table's shape. The table keeps one entry per line.
"""
import argparse, datetime, json, pathlib, re, sys, urllib.error, urllib.request

KINDS = {"same", "alternative", "web", "built-in", "not-needed"}
FIELDS = ["Match", "Kind", "Linux", "Flathub", "Web", "Note", "Source", "Verified"]


def shape(entries):
    bad = []
    for i, e in enumerate(entries):
        where = f"entry {i + 1} ({e.get('Match', '?')[:40]})"
        if list(e.keys()) != FIELDS:
            bad.append(f"{where}: the fields are {list(e.keys())}, not {FIELDS}")
            continue
        if e["Kind"] not in KINDS:
            bad.append(f"{where}: Kind {e['Kind']!r} is not one of {sorted(KINDS)}")
        if e["Kind"] != "not-needed" and not e["Linux"]:
            bad.append(f"{where}: a listed program needs its Linux name")
        if e["Flathub"] and not re.fullmatch(r"[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]+){2,}", e["Flathub"]):
            bad.append(f"{where}: {e['Flathub']!r} is not a Flathub id")
        if e["Kind"] == "web" and not e["Web"].startswith("https://"):
            bad.append(f"{where}: a web entry needs its https address")
        if e["Verified"] and not re.fullmatch(r"\d{4}-\d{2}-\d{2}", e["Verified"]):
            bad.append(f"{where}: Verified {e['Verified']!r} is not a date")
    return bad


def ask(app_id):
    req = urllib.request.Request(f"https://flathub.org/api/v2/appstream/{app_id}", headers={"User-Agent": "upgrade_-check-programs"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            body = json.load(r)
    except urllib.error.HTTPError as e:
        return f"HTTP {e.code}"
    except Exception as e:  # network, JSON
        return f"no answer ({e})"
    if not isinstance(body, dict) or body.get("id") != app_id:
        return "the catalog answered for another id"
    if body.get("is_eol"):
        return "marked end-of-life"
    return None


def write(path, about, entries):
    lines = ["{", '  "_about": ' + json.dumps(about, indent=4, ensure_ascii=False).replace("\n}", "\n  }") + ",", '  "Programs": [']
    lines += ["    " + json.dumps(e, ensure_ascii=False) + ("," if i < len(entries) - 1 else "") for i, e in enumerate(entries)]
    lines += ["  ]", "}"]
    path.write_text("\n".join(lines) + "\n")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--table", default=str(pathlib.Path(__file__).resolve().parent.parent / "programs.json"))
    ap.add_argument("--offline", action="store_true")
    a = ap.parse_args()
    path = pathlib.Path(a.table)
    t = json.loads(path.read_text())
    bad = shape(t["Programs"])
    for b in bad:
        print("FAIL ", b)
    if bad or a.offline:
        sys.exit(1 if bad else 0)
    today = datetime.date.today().isoformat()
    fails = 0
    for e in t["Programs"]:
        if not e["Flathub"]:
            continue
        why = ask(e["Flathub"])
        if why:
            e["Verified"] = ""
            fails += 1
            print(f"FAIL  {e['Flathub']}: {why}")
        else:
            e["Verified"] = today
            e["Source"] = f"https://flathub.org/apps/{e['Flathub']}"
            print(f"ok    {e['Flathub']}")
    write(path, t["_about"], t["Programs"])
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
