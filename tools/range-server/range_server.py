#!/usr/bin/env python3
"""A local HTTP server with Range support, for testing Razdor's parallel downloads.

Serves the files under --root on 127.0.0.1 (never another interface). Options model what the
test needs:

  --throttle N        send at most N bytes a second per connection (GitVerse's per-connection
                      speed is about 140-500 KB/s; eight connections add up)
  --no-range          ignore Range headers (answer 200 with the whole file)
  --cut-once START    the first request for bytes starting at START is sent short (half its
                      bytes, with a matching Content-Length): a part that comes back short
  --fail-once START   the first request for bytes starting at START is answered 500
  --fail-always START every request for bytes starting at START is answered 500

Usage: range_server.py --root DIR [--port 8765] [--throttle N] ...
Prints "ready <port>" on stdout once it listens. Stops on SIGTERM or Ctrl-C.
"""
import argparse
import os
import re
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CHUNK = 16384


class State:
    def __init__(self, args):
        self.args = args
        self.lock = threading.Lock()
        self.once_used = set()

    def first_time(self, key):
        with self.lock:
            if key in self.once_used:
                return False
            self.once_used.add(key)
            return True


def make_handler(state, root):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, fmt, *args):
            sys.stderr.write("range_server: %s %s\n" % (self.command, fmt % args))

        def do_HEAD(self):
            self.handle_get(head=True)

        def do_GET(self):
            self.handle_get(head=False)

        def handle_get(self, head):
            name = os.path.basename(self.path.split("?", 1)[0])
            path = os.path.join(root, name)
            if not name or not os.path.isfile(path):
                self.send_error(404)
                return
            size = os.path.getsize(path)
            args = state.args
            rng = None if args.no_range else self.headers.get("Range")
            start, end = 0, size - 1
            ranged = False
            if rng:
                m = re.fullmatch(r"bytes=(\d+)-(\d*)", rng.strip())
                if not m:
                    self.send_error(416)
                    return
                start = int(m.group(1))
                if m.group(2):
                    end = min(int(m.group(2)), size - 1)
                if start > end or start >= size:
                    self.send_response(416)
                    self.send_header("Content-Range", "bytes */%d" % size)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                ranged = True
            if ranged and args.fail_always is not None and start == args.fail_always:
                self.send_error(500)
                return
            if ranged and args.fail_once is not None and start == args.fail_once and state.first_time(("fail", start)):
                self.send_error(500)
                return
            length = end - start + 1
            if ranged and args.cut_once is not None and start == args.cut_once and state.first_time(("cut", start)):
                length = max(1, length // 2)
                end = start + length - 1
            self.send_response(206 if ranged else 200)
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(length))
            if ranged:
                self.send_header("Content-Range", "bytes %d-%d/%d" % (start, end, size))
            self.end_headers()
            if head:
                return
            with open(path, "rb") as f:
                f.seek(start)
                left = length
                while left > 0:
                    chunk = f.read(min(CHUNK, left))
                    if not chunk:
                        break
                    try:
                        self.wfile.write(chunk)
                    except (BrokenPipeError, ConnectionResetError):
                        # The client gave up (a cancel, a retry): not an error of the server.
                        return
                    left -= len(chunk)
                    if args.throttle:
                        time.sleep(len(chunk) / float(args.throttle))

    return Handler


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--root", required=True)
    p.add_argument("--port", type=int, default=0)
    p.add_argument("--throttle", type=int, default=0)
    p.add_argument("--no-range", action="store_true")
    p.add_argument("--cut-once", type=int, default=None)
    p.add_argument("--fail-once", type=int, default=None)
    p.add_argument("--fail-always", type=int, default=None)
    args = p.parse_args()
    state = State(args)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), make_handler(state, os.path.abspath(args.root)))
    server.daemon_threads = True
    print("ready %d" % server.server_address[1], flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
