#!/usr/bin/env python3
"""Reads Razdor's performance test (Settings → Other → «Тест производительности») and prints a Markdown table.

Input: a file or stdin, either pasted logcat (`adb logcat -s RazdorBench`, any logcat format) or the text
file the test saves (`razdor-bench-*.txt`). Only the `RazdorBench key=value…` lines are read; the device
line comes from the first of them. Several runs in one paste: the last line of each cell wins.

    python3 -I tools/perf/phone_bench.py logcat.txt
    adb logcat -d -s RazdorBench | python3 -I tools/perf/phone_bench.py -o result.md

Stdlib only. Exit 0 with a table; 1 when the paste has no RazdorBench line; 2 for a bad argument.
A run without its `RazdorBench DONE` line is reported as unfinished (the table still shows the cells there are).
"""

import argparse
import re
import sys

LINE = re.compile(r"RazdorBench (cell=.*)$")
PAIR = re.compile(r'(\w+)=("[^"]*"|\S+)')

# (key, column title, format, the device line's key or None)
COLUMNS = [
    ("name", "Cell", "{}"),
    ("view", "View", "{}"),
    ("fps", "FPS", "{:.1f}"),
    ("frame_ms_med", "Frame ms (median)", "{:.1f}"),
    ("frame_ms_p90", "Frame ms (p90)", "{:.1f}"),
    ("frame_ms_p99", "Frame ms (p99)", "{:.1f}"),
    ("cpu_ms_med", "CPU ms (median)", "{:.1f}"),
    ("slow", "Slow frames (>33 ms)", "{}"),
    ("load_ms", "Load ms", "{}"),
    ("rss_mb", "RAM MB", "{}"),
]
DEVICE_KEYS = ["gpu", "screen", "os", "version", "vsync"]


def parse_pairs(text):
    """The key=value pairs of a line, quotes removed."""
    out = {}
    for key, value in PAIR.findall(text):
        out[key] = value[1:-1] if value.startswith('"') and value.endswith('"') else value
    return out


def read_lines(source):
    """The RazdorBench lines' payloads (from `cell=` on, or `DONE`), in order."""
    for raw in source:
        if "RazdorBench DONE" in raw:
            yield "DONE"
            continue
        m = LINE.search(raw)
        if m:
            yield m.group(1).strip()


def table(cells, device, done):
    head = ["| " + " | ".join(title for _, title, _ in COLUMNS) + " |", "|" + "---|" * len(COLUMNS)]
    body = []
    for cell in cells.values():
        row = []
        for key, _, fmt in COLUMNS:
            value = cell.get(key, "")
            try:
                row.append(fmt.format(float(value)) if fmt.endswith("f}") else fmt.format(value))
            except (ValueError, TypeError):
                row.append(str(value))
        body.append("| " + " | ".join(row) + " |")
    lines = ["# Razdor performance test", ""]
    lines += [f"- {k}: {device.get(k, '?')}" for k in DEVICE_KEYS]
    lines += ["", *head, *body, ""]
    if not done:
        lines.append("Note: no `RazdorBench DONE` line: the run did not finish.")
    return "\n".join(lines) + "\n"


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("file", nargs="?", help="a pasted logcat or a saved test file (default: stdin)")
    ap.add_argument("-o", "--out", help="write the table here (default: stdout)")
    args = ap.parse_args(argv)
    if args.file:
        try:
            source = open(args.file, encoding="utf-8", errors="replace")
        except OSError as e:
            print(f"phone_bench: {e}", file=sys.stderr)
            return 2
    else:
        source = sys.stdin
    cells, device, done, seen = {}, None, False, False
    for payload in read_lines(source):
        seen = True
        if payload == "DONE":
            done = True
            continue
        pairs = parse_pairs(payload)
        if "cell" not in pairs:
            continue
        if device is None:
            device = {k: pairs.get(k, "?") for k in DEVICE_KEYS}
        cells[pairs["cell"]] = pairs
    if args.file:
        source.close()
    if not seen:
        print("phone_bench: no RazdorBench line in the input", file=sys.stderr)
        return 1
    text = table(cells, device or {}, done)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
