#!/usr/bin/env python3
"""A made-up release server for upgrade-fetch (roadmap item 24, rule #5).

    rig/fetch/serve.py DIR [--port 8044] [--cut N[,N...]] [--flip OFFSET] [--no-range] [--no-length]

Serves the files in DIR over plain HTTP with byte ranges, the way a mirror
does. The faults are on purpose, each once, in order:
  --cut N       drop the connection after N bytes of the next response
  --flip OFFSET send the byte at OFFSET of every file with one bit changed
  --no-range    ignore Range headers (send the whole file again)
  --no-length   send no Content-Length
Every request is printed, so a run leaves its trace.
"""
import argparse, http.server, os, socketserver, sys

ap = argparse.ArgumentParser()
ap.add_argument("dir")
ap.add_argument("--port", type=int, default=8044)
ap.add_argument("--cut", default="")
ap.add_argument("--flip", type=int, default=-1)
ap.add_argument("--no-range", action="store_true")
ap.add_argument("--no-length", action="store_true")
a = ap.parse_args()
cuts = [int(x) for x in a.cut.split(",") if x]
root = os.path.abspath(a.dir)


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_HEAD(self):
        name = os.path.basename(self.path.split("?")[0])
        p = os.path.join(root, name)
        ok = bool(name) and os.path.isfile(p)
        self.send_response(200 if ok else 404)
        if ok and not a.no_length:
            self.send_header("Content-Length", str(os.path.getsize(p)))
        elif not ok:
            self.send_header("Content-Length", "0")
        self.end_headers()
        print(f"HEAD {name} ({'found' if ok else '404'})", flush=True)

    def do_GET(self):
        name = os.path.basename(self.path.split("?")[0])
        p = os.path.join(root, name)
        if not name or not os.path.isfile(p):
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            print(f"404 {self.path}", flush=True)
            return
        size = os.path.getsize(p)
        start = 0
        rng = self.headers.get("Range")
        if rng and not a.no_range and rng.startswith("bytes="):
            start = int(rng[6:].split("-")[0])
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{size - 1}/{size}")
        else:
            self.send_response(200)
        if not a.no_length:
            self.send_header("Content-Length", str(size - start))
        else:
            self.send_header("Connection", "close")
        self.end_headers()
        cut = cuts.pop(0) if cuts else None
        print(f"GET {name} from {start} ({rng or 'no range'}){' cut at %d' % cut if cut is not None else ''}", flush=True)
        sent = 0
        with open(p, "rb") as f:
            f.seek(start)
            while True:
                b = f.read(1 << 20)
                if not b:
                    break
                if a.flip >= 0 and start + sent <= a.flip < start + sent + len(b):
                    i = a.flip - start - sent
                    b = b[:i] + bytes([b[i] ^ 1]) + b[i + 1:]
                if cut is not None and sent + len(b) >= cut:
                    self.wfile.write(b[: cut - sent])
                    self.wfile.flush()
                    self.connection.shutdown(2)
                    print(f"  dropped after {cut} bytes", flush=True)
                    return
                try:
                    self.wfile.write(b)
                except (BrokenPipeError, ConnectionResetError):
                    print(f"  the client left after {sent} bytes", flush=True)
                    return
                sent += len(b)
        print(f"  sent {sent} bytes", flush=True)

    def log_message(self, *args):
        pass


class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


print(f"serving {root} on port {a.port}", flush=True)
S(("0.0.0.0", a.port), H).serve_forever()
