#!/usr/bin/env python3
"""
v1-verdict.py - one row of docs/validation-results/v1-live-boot.csv from
the run's own evidence: the V0 harness row the guest wrote when Windows
came back (v0-handoff.csv, the last row), and the report the %pre verifier
left on the stick (upgrade_/report/verify.json). Never by hand.

    v1-verdict.py <artifacts dir> <csv> <harness version> <firmware>
"""
import csv, json, sys, datetime, pathlib

A = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); HARNESS = sys.argv[3]; FIRMWARE = sys.argv[4]
HEADER = ["timestamp", "harness", "firmware", "secureboot", "handoff_result", "windows_returned", "stage2_booted",
          "identity", "esp", "display", "wifi", "audio_firmware", "storage_include", "result", "notes"]

v0 = None
p = A / "v0-handoff.csv"
if p.exists():
    rows = list(csv.DictReader(open(p, encoding="utf-8-sig")))
    v0 = rows[-1] if rows else None
verify = None
p = A / "verify.json"
if p.exists():
    try: verify = json.load(open(p, encoding="utf-8-sig"))
    except Exception as e: verify = {"parse_error": str(e)}

notes = []
handoff = v0["result"] if v0 else "no-row"
returned = v0["windows_returned"] if v0 else "n"
sb = v0["secureboot"] if v0 else "unknown"
if v0: notes.append("v0 row: " + v0["notes"][:160])
stage2 = "y" if (verify and "identity" in verify) else "n"
ident = verify["identity"]["result"] if stage2 == "y" else "not-reached"
esp = verify["storage"]["esp_result"] if stage2 == "y" else "not-reached"
hw = verify["hardware"] if stage2 == "y" else {}
disp = hw.get("display", "not-reached"); wifi = hw.get("wifi", "not-reached"); audio = hw.get("audio_firmware", "not-reached")
inc = "y" if (stage2 == "y" and verify["storage"].get("include_written")) else "n"
if stage2 == "y":
    notes.append(f"verify {verify.get('verify_version')} mode={verify.get('mode')} kernel={verify.get('kernel')} disk={verify['identity'].get('disk')} matched_by={verify['identity'].get('matched_by')} "
                 f"display='{hw.get('display_detail','')}' wifi='{hw.get('wifi_detail','')}' esp={verify['storage'].get('esp')} sb_var={verify.get('secure_boot')}")

if v0 is None or returned != "y":
    result = "windows-not-returned"
elif handoff != "fired-once":
    result = "handoff-failed"
elif stage2 != "y":
    result = "stage2-not-reached"
elif ident != "pass":
    result = "identity-mismatch"
elif inc != "y" or esp == "fail" or disp == "fail" or wifi == "fail" or audio == "fail":
    result = "verify-incomplete"
else:
    result = "pass-plumbing"

row = [datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ"), HARNESS, FIRMWARE, sb, handoff, returned, stage2,
       ident, esp, disp, wifi, audio, inc, result, " | ".join(notes)]
new = not CSV.exists()
with open(CSV, "a", newline="", encoding="utf-8") as f:
    w = csv.writer(f, quoting=csv.QUOTE_ALL, lineterminator="\n")
    if new: w.writerow(HEADER)
    w.writerow(row)
print(f"v1-verdict: {result} (handoff={handoff} stage2={stage2} identity={ident} esp={esp} display={disp}) -> {CSV}")
sys.exit(0 if result == "pass-plumbing" else 1)
