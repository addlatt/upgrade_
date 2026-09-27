#!/usr/bin/env python3
"""
settle-in-verdict.py - one row of docs/validation-results/settle-in-first-start.csv
from a run's own evidence (RISKS R28, VALIDATION V10). Never by hand.

    settle-in-verdict.py <artifacts/v9/TAG> <csv> <machine> [--spoofed-wifi] [note]

Reads what `v9.sh stick-pull TAG` copied: outcome.json, job.json, and
settle-in/ (the bench marker's capture from the first Linux boot: report.json,
nm-parsed.txt, handoff-ls.txt, clock.txt, order.txt), plus
stick-credentials-dir.txt. Checks, each named in the row:

  installed   outcome.cutover.settle_in.installed (the adapter checked SHA256SUMS)
  handoff     credentials.wifi: moved == expected, removed_from_stick; the stick
              lists no wifi folder under artifacts/credentials
  clock       settle-in's clock result and why, as it said it (any result is
              recorded; "failed" fails the row). When it says "corrected" and the
              capture's clock was set by a synchronized time service (clock.txt), the
              corrected time must agree with that true time within 15 minutes of the
              capture: a correction that moved a right clock fails the row (rig run 10)
  wifi        with --spoofed-wifi (v9-job.py's three made-up networks):
              NetworkManager's own parse of the files settle-in wrote -
              "RigSpoof Home" wpa-psk autoconnect, "Rig;Open" hidden, not
              autoconnect, open; "RigSpoof Work" (enterprise) not set up;
              every file mode 600
              (without nm-parsed.txt, nm-all.txt - NetworkManager's connection list -
              shows the files loaded and the names read, which is only "partial")
  deleted     report.wifi.passwords_deleted, and no wifi folder in the handoff
  order       settle-in finished before NetworkManager and chronyd started
  sessions    every desktop sign-in reached its window manager (sessions.txt, or a
              journal-*.txt copied off the machine): a session that starts and never
              gets one is a black screen - a one-click failure (rig run 2)
  window      a desktop install: the FIRST sign-in contains the window's own line
              "settle-in-window: showing the summary", written on its first drawn frame
              (so it reached the screen, not just started); console: n/a
              (+ what the desktop did with its focus request: in front / not given focus)
  own_entry   settle-in removed the conversion's own one-time "upgrade_" firmware
              entry at first start (report.own_boot_entry), and no "upgrade_" entry
              is left in the first boot's firmware capture
  button      when efi-*before*.txt and efi-*after-button*.txt exist: exactly the
              stale entries left the boot order, the running entry stayed first, and
              the next boot (boots.log) started from it into the graphical sign-in

A spoofed pass is pass-plumbing: it closes plumbing, never a real-hardware
clause (rule #5).
"""
import csv, datetime, json, pathlib, re, sys

D = pathlib.Path(sys.argv[1]); CSV = pathlib.Path(sys.argv[2]); MACHINE = sys.argv[3]
spoofed = "--spoofed-wifi" in sys.argv[4:]
notes = [a for a in sys.argv[4:] if not a.startswith("--")]
S = D / "settle-in"
HEADER = ["timestamp", "machine", "settle_in_version", "installed", "handoff", "clock_result", "clock_why",
          "installer_clock_error", "wifi_result", "wifi_nm_parse", "passwords_deleted", "ran_before_network", "result", "notes",
          "sessions", "button", "window", "own_entry"]

def load(p):
    try: return json.load(open(p, encoding="utf-8-sig"))
    except Exception: return None
def text(p):
    try: return open(p, encoding="utf-8", errors="replace").read()
    except Exception: return ""

o = load(D / "outcome.json") or {}; rep = load(S / "report.json") or {}
fails = []

si = o.get("cutover", {}).get("settle_in", {})
installed = "y" if si.get("installed") else "n: %s" % si.get("why_not")
if not si.get("installed"): fails.append("not installed")

w = o.get("credentials", {}).get("wifi") or {}
stick_dir = text(D / "stick-credentials-dir.txt")
stick_has_wifi = bool(re.search(r"(?im)^wifi\s", stick_dir))
handoff = "moved %s/%s, stick %s" % (w.get("moved"), w.get("expected"), "still has wifi" if stick_has_wifi else "clear")
if not w or w.get("moved") != w.get("expected") or not w.get("removed_from_stick") or stick_has_wifi: fails.append("handoff")

c = rep.get("clock", {})
clock_result = c.get("result", "no report"); clock_why = c.get("why", "")
installer_err = c.get("install_records", {}).get("installer_clock_error", "")
if clock_result in ("failed", "attempting", "no report"): fails.append("clock " + clock_result)
ct = text(S / "clock.txt").splitlines()
if clock_result == "corrected" and ct and "NTPSynchronized=yes" in ct and ct[0].strip().isdigit():
    after = __import__("calendar").timegm(__import__("time").strptime(c.get("system_clock_after_utc", "1970-01-01T00:00:00Z"), "%Y-%m-%dT%H:%M:%SZ"))
    truth = int(ct[0])   # the capture, seconds to minutes after first start, on a synchronized clock
    if not (0 <= truth - after <= 900):
        clock_why = "the correction disagrees with the true time by %+.1f h (set %s; a synchronized clock read %s at the capture)" % ((after - truth) / 3600.0, c.get("system_clock_after_utc"), __import__("time").strftime("%Y-%m-%dT%H:%M:%SZ", __import__("time").gmtime(truth)))
        fails.append("the clock correction moved a right clock")

wr = rep.get("wifi", {})
wifi_result = "%s (%s created)" % (wr.get("result", "none"), wr.get("created", 0))
nm = [l.split("|") for l in text(S / "nm-parsed.txt").splitlines() if l.strip()]
nm_by_ssid = {r[1]: r for r in nm if len(r) >= 6}
if spoofed:
    want = {"RigSpoof Home": ("no", "yes", "wpa-psk"), "Rig;Open": ("yes", "no", "")}
    bad = []
    for ssid, (hidden, auto, km) in want.items():
        r = nm_by_ssid.get(ssid)
        if not r: bad.append("%s missing" % ssid); continue
        if (r[2], r[3], r[4]) != (hidden, auto, km): bad.append("%s parsed %s" % (ssid, r[2:5]))
        if r[5] != "600": bad.append("%s mode %s" % (ssid, r[5]))
    if "RigSpoof Work" in nm_by_ssid: bad.append("enterprise network was set up")
    work = [n for n in wr.get("networks", []) if n.get("ssid") == "RigSpoof Work"]
    if not work or work[0].get("result") != "not-set-up": bad.append("enterprise network not listed")
    partial = False
    if not nm and (S / "nm-all.txt").exists():
        rows = [l.split(":") for l in text(S / "nm-all.txt").splitlines() if l.strip()]
        loaded = {r[0]: r for r in rows if len(r) >= 4 and "/upgrade_-" in r[3]}
        bad = [n + " not loaded" for n in want if n not in loaded or loaded[n][2] != "802-11-wireless"]
        if "RigSpoof Work" in loaded: bad.append("enterprise network was set up")
        partial = not bad
    wifi_nm = ("partial: loaded %d, names exact; hidden/autoconnect/security not captured" % len(want) if partial
               else "y (%d)" % len(nm) if not bad else "n: " + "; ".join(bad))
    if bad: fails.append("wifi")
else:
    wifi_nm = "n/a (%d parsed)" % len(nm)

deleted = "y" if wr.get("passwords_deleted") and "wifi" not in text(S / "handoff-ls.txt").split() else "n"
if wr.get("result") not in (None, "nothing-to-do") and deleted != "y": fails.append("passwords not deleted")

order = text(S / "order.txt").splitlines()
def first(pat):
    for i, l in enumerate(order):
        if re.search(pat, l): return i
    return None
si_done = first(r"upgrade_-settle-in.*(Finished|finished)|settle-in: report written")
nm_start = first(r"Starting NetworkManager|NetworkManager\[\d+\]")
ch_start = first(r"Starting chronyd|chronyd\[\d+\]")
if si_done is None: before = "n: settle-in not in the journal"; fails.append("order")
elif (nm_start is not None and nm_start < si_done) or (ch_start is not None and ch_start < si_done): before = "n"; fails.append("order")
else: before = "y"

# --- desktop sessions: each "Starting Wayland user session" must reach a window manager
jl = [l for f in sorted(S.glob("sessions-before-*.txt")) for l in text(f).splitlines()] or text(S / "sessions.txt").splitlines() or [l for f in sorted(D.glob("journal-*.txt")) if "user" not in f.name for l in text(f).splitlines()]
# a sign-in starts a session: SDDM logs "Starting Wayland user session", GDM a PAM session for gdm-password
start_re = r"Starting Wayland user session|pam_unix\(gdm-password:session\): session opened"   # GDM writes gdm-password][PID]
starts, hung = 0, 0
for i, l in enumerate(jl):
    if re.search(start_re, l):
        starts += 1
        nxt = jl[i + 1:]
        end = next((k for k, x in enumerate(nxt) if re.search(start_re, x)), len(nxt))
        if not any(re.search(r"Started plasma-kwin_wayland|Started org\.gnome\.Shell@wayland", x) for x in nxt[:end]): hung += 1
# --- the window: shown in the first desktop sign-in?
chose = o.get("cutover", {}).get("install", {}).get("start_at") or (load(D / "job.json") or {}).get("intent", {}).get("start_at")
first = next((i for i, l in enumerate(jl) if re.search(start_re, l)), None)
if chose == "console":
    window = "n/a (text console chosen)"
elif first is None:
    window = "n/a (no sign-in in the captured log)"
else:
    nxt = jl[first + 1:]
    end = next((k for k, x in enumerate(nxt) if re.search(start_re, x)), len(nxt))
    lines = [x for x in nxt[:end] if "settle-in-window:" in x]
    shown = any("showing the summary" in x for x in lines)
    focus = [x.split("settle-in-window:", 1)[1].strip() for x in lines if "focus" in x or "in front" in x]
    window = ("shown in the first sign-in" + ("; " + focus[0] if focus else "")) if shown else "NOT shown in the first sign-in" + (": " + lines[-1].split("settle-in-window:", 1)[1].strip() if lines else " (no line from it)")
    if not shown: fails.append("the window did not show at the first sign-in")
sessions = "n/a (no session log)" if not jl else ("%d of %d reached the desktop" % (starts - hung, starts))
if hung: fails.append("a desktop sign-in never reached the desktop (black screen)")

# --- the button: before/after firmware, and the next boot
def efi(pat):
    f = sorted(D.glob(pat)); t = text(f[0]) if f else ""
    order = re.search(r"BootOrder: (\S+)", t); cur = re.search(r"BootCurrent: (\S+)", t)
    return (order.group(1).split(",") if order else None, cur.group(1) if cur else None, t)
b_order, b_cur, b_txt = efi("efi-*before*.txt"); a_order, a_cur, a_txt = efi("efi-*after-button*.txt")
if b_order is None and len(sorted(S.glob("efibootmgr-*.txt"))) >= 2:
    # the bench marker's per-boot captures: the first boot (before the button) and the last
    snaps = sorted(S.glob("efibootmgr-*.txt"))
    def efi_file(f):
        t = text(f); order = re.search(r"BootOrder: (\S+)", t); cur = re.search(r"BootCurrent: (\S+)", t)
        return (order.group(1).split(",") if order else None, cur.group(1) if cur else None, t)
    b_order, b_cur, b_txt = efi_file(snaps[0]); a_order, a_cur, a_txt = efi_file(snaps[-1])
if b_order is None or a_order is None:
    button = "n/a"
else:
    gone = [e for e in b_order if e not in a_order]
    stale_ok = all(re.search(r"Boot%s\*? Windows Boot Manager" % g, b_txt) for g in gone) and gone
    kept = [e for e in b_order if e not in gone] == a_order and a_order[0] == b_cur
    later = [l for l in text(D / "boots.log").splitlines() if l.startswith("linux-boot")]
    # the next boot starts where the person chose: the desktop's sign-in, or the text console (run 6)
    want_tgt = o.get("cutover", {}).get("install", {}).get("boot_target", "graphical.target")
    rebooted = len(later) >= 2 and ("BootCurrent=%s" % b_cur) in later[-1] and ("default_target=%s" % want_tgt) in later[-1] \
        and (want_tgt != "graphical.target" or "display_manager=active" in later[-1])
    button = "removed %s; order otherwise unchanged: %s; next boot from %s, to %s: %s" % (",".join(gone) or "nothing", "y" if kept else "n", b_cur, want_tgt, "y" if rebooted else "n")
    if not (stale_ok and kept and rebooted): fails.append("button")

# --- our own one-time entry, removed at first start (the owner, 2026-09-27)
own = rep.get("own_boot_entry")
snaps = sorted(S.glob("efibootmgr-*.txt"))
if own is None:
    own_entry = "n/a (settle-in before the own-entry removal)"
else:
    left = bool(snaps) and bool(re.search(r"Boot[0-9A-F]{4}\*? upgrade_\b", text(snaps[0])))
    own_entry = "%s%s" % (own.get("result"), "; still in the firmware" if left else ("; gone from the firmware" if snaps else ""))
    if own.get("result") not in ("removed", "already-gone") or left: fails.append("own entry")

result = ("pass-plumbing" if spoofed else "pass") if not fails else "fail"
if result != "fail" and spoofed and "partial" in wifi_nm: result = "partial"
if fails: notes.append("failed: " + ", ".join(fails))
row = [datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"), MACHINE, rep.get("settle_in_version", si.get("version", "")),
       installed, handoff, clock_result, clock_why, installer_err, wifi_result, wifi_nm, deleted, before, result, "; ".join(notes),
       sessions, button, window, own_entry]
new = not CSV.exists()
with open(CSV, "a", newline="") as f:
    wr_ = csv.writer(f)
    if new: wr_.writerow(HEADER)
    wr_.writerow(row)
for h, v in zip(HEADER, row): print("%-22s %s" % (h, v))
