#!/usr/bin/env python3
"""Pull arm (R20): write a Firefox bookmark backup (.jsonlz4) for the plant.

mozLz40\\0, the decompressed size (u32 LE), then one LZ4 block holding the
JSON as a single run of literals (a valid block: LZ4's last sequence is
literals only), so no LZ4 library is needed. Three bookmarks, one separator
and one place: query, which is not a bookmark.

    pull-firefox.py OUT
"""
import json, struct, sys

tree = {"guid": "root________", "title": "", "typeCode": 2, "root": "placesRoot", "children": [
    {"guid": "menu________", "title": "menu", "typeCode": 2, "root": "bookmarksMenuFolder", "children": [
        {"title": "Library", "typeCode": 1, "uri": "https://library.example/", "dateAdded": 1762500000000000},
        {"title": "Recent tags", "typeCode": 1, "uri": "place:type=6&sort=14"},
        {"typeCode": 3}]},
    {"guid": "toolbar_____", "title": "toolbar", "typeCode": 2, "root": "toolbarFolder", "children": [
        {"title": "Work", "typeCode": 2, "children": [
            {"title": "Calendar", "typeCode": 1, "uri": "https://calendar.example/"},
            {"title": "Docs & notes", "typeCode": 1, "uri": "https://docs.example/?a=1&b=2"}]}]},
    {"guid": "unfiled_____", "title": "unfiled", "typeCode": 2, "root": "unfiledBookmarksFolder", "children": []}]}
raw = json.dumps(tree).encode()

def literals(data):
    n = len(data)
    if n < 15:
        return bytes([n << 4]) + data
    out = bytearray([0xF0]); rest = n - 15
    while rest >= 255:
        out.append(255); rest -= 255
    out.append(rest)
    return bytes(out) + data

open(sys.argv[1], "wb").write(b"mozLz40\0" + struct.pack("<I", len(raw)) + literals(raw))
