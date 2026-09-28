#!/usr/bin/env python3
"""Refresh settle-in/data/windows-media.json from Microsoft's own pages.

"Go back to Windows" never hands out Windows. The person downloads the
installer from Microsoft in their own browser, and settle-in checks the file
against the SHA-256 table Microsoft prints on the same page ("Verify your
download"). This script copies that table here, with where and when it was
read, so the check works offline and a changed table shows up in a diff.

A file whose hash is not in this table is refused (rule #1). When Microsoft
replaces its ISOs, run this again and commit the result.

    python3 settle-in/tools/refresh-windows-media.py
"""
import datetime, html, json, os, re, sys, urllib.request

PAGES = [
    ("10", "https://www.microsoft.com/en-us/software-download/windows10ISO"),
    ("11", "https://www.microsoft.com/en-us/software-download/windows11"),
]
# A Linux browser: Microsoft sends Windows browsers to its media tool instead
# of the ISO page.
UA = "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"
ROW = re.compile(r"([A-Z][A-Za-z()\- ]*?) (64-bit|32-bit|x64|Arm64) ([0-9A-F]{64})")


def text(url):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    page = urllib.request.urlopen(req, timeout=60).read().decode("utf-8", "replace")
    page = re.sub(r"<script.*?</script>|<style.*?</style>", "", page, flags=re.S)
    return re.sub(r"\s+", " ", html.unescape(re.sub(r"<[^>]+>", " ", page)))


def rows(windows, s):
    start = s.find("Verify your download")
    if start < 0:
        sys.exit(f"Windows {windows}: no 'Verify your download' section; the page changed, look at it by hand")
    out = []
    for lang, arch, sha in ROW.findall(s[start:]):
        # the first row carries the table's heading text in front of it
        lang = re.sub(r"^.*Hash Code ", "", lang).strip()
        out.append({"windows": windows, "language": lang, "arch": "x64" if arch in ("64-bit", "x64") else arch, "sha256": sha.lower()})
    if len(out) < 20:
        sys.exit(f"Windows {windows}: only {len(out)} hash rows; the page changed, look at it by hand")
    return out


def main():
    media, sources = [], []
    for windows, url in PAGES:
        s = text(url)
        media += rows(windows, s)
        sources.append({"windows": windows, "url": url})
    doc = {
        "about": "SHA-256 of Microsoft's own Windows installer ISOs, as printed on Microsoft's download pages. Refreshed by tools/refresh-windows-media.py.",
        "read_utc": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "sources": sources,
        "media": media,
    }
    here = os.path.dirname(os.path.abspath(__file__))
    path = os.path.join(here, "..", "data", "windows-media.json")
    with open(path, "w") as f:
        json.dump(doc, f, indent=1)
        f.write("\n")
    print(f"{len(media)} rows -> {os.path.normpath(path)}")


if __name__ == "__main__":
    main()
