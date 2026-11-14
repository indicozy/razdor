#!/usr/bin/env python3
"""Reads Razdor's performance test (Settings → Other → «Тест производительности») and prints a Markdown table.

Input: a file or stdin, either pasted logcat (`adb logcat -s RazdorBench`, any logcat format) or the text
file the test saves (`razdor-bench-*.txt`). Only the `RazdorBench key=value…` lines are read; the device
line comes from the first of them. Several runs in one paste: the last line of each cell wins.

A run that had profiling on (its trigger file's `profile` line) also has `RazdorBenchStages cell=… <stage>_p50=…
<stage>_p90=…` lines: they make a second table, the stage breakdown of each cell's frames (ms, p50 / p90). Without
them the output is the first table alone, as before.

    python3 -I tools/perf/phone_bench.py logcat.txt
    adb logcat -d -s RazdorBench | python3 -I tools/perf/phone_bench.py -o result.md

Stdlib only. Exit 0 with a table; 1 when the paste has no RazdorBench line; 2 for a bad argument.
A run without its `RazdorBench DONE` line is reported as unfinished (the table still shows the cells there are).
"""

import argparse
import re
import sys

LINE = re.compile(r"RazdorBench (cell=.*)$")
STAGE_LINE = re.compile(r"RazdorBenchStages (cell=.*)$")
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


def read_records(source):
    """The bench lines of a source, in order: ("DONE", None), ("cell", payload) for `RazdorBench cell=…`,
    ("stages", payload) for `RazdorBenchStages cell=…`. The payload starts at `cell=`."""
    for raw in source:
        if "RazdorBench DONE" in raw:
            yield "DONE", None
            continue
        m = STAGE_LINE.search(raw)
        if m:
            yield "stages", m.group(1).strip()
            continue
        m = LINE.search(raw)
        if m:
            yield "cell", m.group(1).strip()


def read_lines(source):
    """The RazdorBench lines' payloads (from `cell=` on, or `DONE`), in order (the stage lines are left out)."""
    for kind, payload in read_records(source):
        if kind != "stages":
            yield "DONE" if kind == "DONE" else payload


def stage_titles(stages):
    """The stage keys the cells timed, in the order they first appear (the bench writes them in the probe's order)."""
    keys = []
    for pairs in stages.values():
        for key in pairs:
            if key.endswith("_p50") and key[:-4] not in keys:
                keys.append(key[:-4])
    return keys


# Per-frame counts, not times (the lab's GL counters: draw calls and bytes uploaded), with their titles.
COUNT_TITLES = {"gl_draws": "GL draws (calls per frame)", "gl_bytes": "GL bytes (uploaded per frame)"}


def title(key):
    """A stage key's column title: the key with spaces (ai move), the GPU ones as GPU (GPU sky)."""
    if key in COUNT_TITLES:
        return COUNT_TITLES[key]
    if key.startswith("gpu_"):
        return "GPU " + key[4:].replace("_", " ")
    text = key.replace("_", " ")
    return text[:1].upper() + text[1:]


def stage_table(cells, stages):
    """The second table: one row per cell, `p50 / p90` ms of each stage its frames timed (`–` when not timed)."""
    keys = stage_titles(stages)
    if not keys:
        return []
    titles = [title(k) for k in keys]
    head = ["| Cell | Frames | GPU frames | Replayed | " + " | ".join(titles) + " |", "|" + "---|" * (4 + len(keys))]
    ids = list(cells) + [cid for cid in stages if cid not in cells]
    body = []
    for cid in ids:
        pairs = stages.get(cid, {})
        name = cells.get(cid, {}).get("name", cid)
        row = [name, pairs.get("frames", "–"), pairs.get("gpu_frames", "–"), pairs.get("replayed", "–")]
        for key in keys:
            p50, p90 = pairs.get(key + "_p50"), pairs.get(key + "_p90")
            row.append(f"{float(p50):.2f} / {float(p90):.2f}" if p50 is not None and p90 is not None else "–")
        body.append("| " + " | ".join(str(c) for c in row) + " |")
    note = ("Stage times in ms per frame, p50 / p90 (the GL draws and bytes are per frame counts; `Replayed` counts the "
            "frames the lab's replay floor issued again, see docs/flows/testing-and-tools.md): the CPU stages over the frames with no GPU sample, the GPU "
            "stages over the sampled frames only (`gpu_frames`). The stages nest: `route`, `look` and `armies` are "
            "inside `tick`, `terrain` and the others inside `world`, so read a row as parts, not a sum.")
    return ["", "## Where the frame time goes (stages)", "", *head, *body, "", note]


def table(cells, device, done, stages=None):
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
    if stages:
        lines += stage_table(cells, stages)
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
    cells, stages, device, done, seen = {}, {}, None, False, False
    for kind, payload in read_records(source):
        seen = True
        if kind == "DONE":
            done = True
            continue
        pairs = parse_pairs(payload)
        if "cell" not in pairs:
            continue
        if kind == "stages":
            stages[pairs["cell"]] = pairs
            continue
        if device is None:
            device = {k: pairs.get(k, "?") for k in DEVICE_KEYS}
        cells[pairs["cell"]] = pairs
    if args.file:
        source.close()
    if not seen:
        print("phone_bench: no RazdorBench line in the input", file=sys.stderr)
        return 1
    text = table(cells, device or {}, done, stages)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            f.write(text)
    else:
        sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
