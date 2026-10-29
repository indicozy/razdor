#!/usr/bin/env python3
"""Desktop A/B of a map walk: two Razdor binaries, one save, the same steps.

Runs each binary off-screen under Xvfb (one Xvfb for the session), with the walk scene staged
from a save (`RAZDOR_SCENE=walk:<map>:dx,dy` + `RAZDOR_SCENE_SAVE`), the frame probe on and its
per-frame CSV (`RAZDOR_PROFILE_FRAMES`). Runs alternate A, B, A, B. Razdor is pinned to one
E-core and Xvfb to another (CLAUDE.local.md). Then prints and writes a comparison of the frame
times per kind (walk-step, walk, idle), the worst frames, and a fingerprint of the steps of each
run, so a run that walked differently shows.

Stdlib only; run as `python3 -I tools/walk-ab/walk_ab.py --help`. See README.md.
"""

import argparse
import csv
import hashlib
import math
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

KINDS = ["walk-step", "walk", "idle"]
# The frame probe's timed columns (CSV). The tick's parts are inside `tick`; `autosave` is outside it.
# `ai_move` is inside `armies` and `ai_plan` inside `ai_move` (both "of which", not parts).
TIMED = ["tick", "terrain", "world", "fog", "minimap", "route", "look", "armies", "script", "day", "snap", "autosave",
         "ai_move", "ai_plan"]
# The session slot's look (outside the tick, `Stage::Session`): a column of newer probe builds only,
# so an older build's CSV (the A side of a comparison with the main branch) reads it as zero.
OPTIONAL_TIMED = ["session"]
TICK_PARTS = ["route", "look", "armies", "script", "day", "snap"]
# The table's rows per kind: the timed columns and three that are worked out here: `tick-rest` (the
# tick less its parts), `armies-other` (armies less ai_move: the non-managed armies and the midnights)
# and `rest` (the work less tick, world, fog, minimap and autosave). All clamped at zero, as the
# probe's summary clamps them (frame_probe.rs: tick_rest_ms, rest_ms).
STAGES = ["tick", "route", "look", "armies", "ai_move", "ai_plan", "armies-other", "script", "day", "snap", "tick-rest",
          "terrain", "world", "fog", "minimap", "autosave", "session", "rest"]
# The parts of a walk-step frame's work that add up to it (world includes terrain).
SHARE_PARTS = ["route", "look", "armies", "script", "day", "snap", "tick-rest", "world", "fog", "minimap", "autosave", "session", "rest"]
# Of the armies part: ai_move (ai_plan inside it) and the rest of the armies (armies-other).
ARMIES_PARTS = ["ai_move", "ai_plan", "armies-other"]
# The phone's work p50 the desktop run is scaled to (ms): k = PHONE_WORK_P50 / A's walk work p50.
PHONE_WORK_P50 = 3.0
# The scene's log line when it fails: `RAZDOR_SCENE=<scene>: <reason>` (snapshot.rs). The start-up
# dump of the variables quotes the value (`RAZDOR_SCENE="walk:…"`), so it does not match.
SCENE_ERROR = re.compile(r'RAZDOR_SCENE=[^"\s]+: ')
# A clock label as the play log writes it: "1204, month 5, day 22, 20 h" (or the Russian form).
CLOCK = re.compile(r"(\d{4})\D+?(\d{1,2})\D+?(\d{1,2})\D+?(\d{1,2})\s*(?:h\b|ч)")


class RunError(Exception):
    pass


def parse_args(argv):
    p = argparse.ArgumentParser(description="Desktop A/B of a map walk (see README.md).")
    p.add_argument("--a", required=True, type=Path, help="razdor binary A")
    p.add_argument("--b", required=True, type=Path, help="razdor binary B (may equal A)")
    p.add_argument("--save", required=True, type=Path, help="the .rzsave to walk from")
    p.add_argument("--walk", required=True, help="the walk's offset dx,dy from the saved hero's tile")
    p.add_argument("--frames", required=True, type=int, help="frames per run (RAZDOR_QUIT_AFTER)")
    p.add_argument("--repeats", type=int, default=3, help="repeats of the pair A, B (default 3)")
    p.add_argument("--out", required=True, type=Path, help="output dir (runs, logs, report.md)")
    p.add_argument("--map", default="-", help="the scene's map name; unused with a save (default -)")
    p.add_argument("--size", default="1440x3168", help="RAZDOR_SIZE (default 1440x3168, a phone)")
    p.add_argument("--display", type=int, default=99, help="Xvfb display number (default 99)")
    p.add_argument("--xvfb-cpu", default="16", help="E-core for Xvfb (default 16)")
    p.add_argument("--razdor-cpu", default="19", help="E-core for Razdor (default 19)")
    p.add_argument("--timeout", type=int, default=900, help="seconds per run (default 900)")
    p.add_argument("--quiet", action="store_true",
                   help="RAZDOR_SCENE_QUIET=1: the scene's windows are dropped each frame. Needed for a walk past "
                        "the noon report (it stops the hero while it is open, and the rest of the run is 'other').")
    p.add_argument("--detail", default="",
                   help="frame numbers whose stage split the report prints for every run, e.g. 13,69,70 (the midnight "
                        "and noon frames of the 06 walk)")
    p.add_argument("--perf", action="store_true",
                   help="wrap the first run in perf record -F 999 -g --call-graph dwarf (perf.data in its run dir)")
    cfg = p.parse_args(argv)
    if not re.fullmatch(r"-?\d+,-?\d+", cfg.walk):
        p.error("--walk must be dx,dy")
    if cfg.repeats < 1 or cfg.frames < 1:
        p.error("--repeats and --frames must be 1 or more")
    try:
        cfg.detail = [int(x) for x in cfg.detail.split(",") if x.strip()]
    except ValueError:
        p.error("--detail must be frame numbers, e.g. 13,69")
    return cfg


def require_tools(cfg):
    for tool in ["Xvfb", "taskset"] + (["perf"] if cfg.perf else []):
        if shutil.which(tool) is None:
            raise SystemExit(f"{tool} is not on PATH")
    for binary in (cfg.a, cfg.b):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise SystemExit(f"not an executable: {binary}")
    if not cfg.save.is_file():
        raise SystemExit(f"no save at {cfg.save}")
    dt = os.environ.get("RAZDOR_DT_DIR", "")
    if not dt:
        raise SystemExit("RAZDOR_DT_DIR must name the Discord Times install (the save needs it)")
    return dt


def start_xvfb(cfg, screen):
    """Starts Xvfb on its own display, pinned to its own E-core; waits until it accepts clients."""
    lock = Path(f"/tmp/.X{cfg.display}-lock")
    if lock.exists():
        raise SystemExit(f"display :{cfg.display} is in use ({lock}); pass --display")
    sock = Path(f"/tmp/.X11-unix/X{cfg.display}")
    cfg.out.mkdir(parents=True, exist_ok=True)
    log = open(cfg.out / "xvfb.log", "wb")
    proc = subprocess.Popen(
        ["taskset", "-c", cfg.xvfb_cpu, "Xvfb", f":{cfg.display}", "-screen", "0", f"{screen[0]}x{screen[1]}x24", "-nolisten", "tcp"],
        stdin=subprocess.DEVNULL, stdout=log, stderr=log,
    )
    deadline = time.monotonic() + 15
    while not sock.exists():
        if proc.poll() is not None or time.monotonic() > deadline:
            proc.kill()
            raise SystemExit(f"Xvfb did not start on :{cfg.display} (see {cfg.out / 'xvfb.log'})")
        time.sleep(0.1)
    return proc


def run_env(cfg, dt_dir, run_dir, scene):
    """The environment of one run: the parent's, without any RAZDOR_* variable (a run must not
    inherit the owner's snapshot or profiling settings), plus the run's own."""
    env = {k: v for k, v in os.environ.items() if not k.startswith("RAZDOR_") and k != "WAYLAND_DISPLAY"}
    xdg = run_dir / "xdg"
    xdg.mkdir(parents=True, exist_ok=True)
    os.chmod(xdg, 0o700)
    env.update({
        "DISPLAY": f":{cfg.display}",
        "XDG_RUNTIME_DIR": str(xdg),
        "RAZDOR_DT_DIR": dt_dir,
        "RAZDOR_LOG": str(run_dir / "razdor.log"),
        "RAZDOR_SAVE_DIR": str(run_dir / "saves"),
        "RAZDOR_SIZE": cfg.size,
        "RAZDOR_SCENE": scene,
        "RAZDOR_SCENE_SAVE": str(cfg.save),
        "RAZDOR_PROFILE": "1",
        "RAZDOR_PROFILE_FRAMES": str(run_dir / "frames.csv"),
        "RAZDOR_QUIT_AFTER": str(cfg.frames),
        "RAZDOR_NO_AUDIO": "1",
        "LP_NUM_THREADS": "1",
    })
    if cfg.quiet:
        env["RAZDOR_SCENE_QUIET"] = "1"
    return env


def run_once(cfg, dt_dir, label, index, binary, perf):
    run_dir = cfg.out / "runs" / f"{label}-{index}"
    if run_dir.exists():
        shutil.rmtree(run_dir)
    run_dir.mkdir(parents=True)
    scene = f"walk:{cfg.map}:{cfg.walk}"
    env = run_env(cfg, dt_dir, run_dir, scene)
    cmd = ["taskset", "-c", cfg.razdor_cpu]
    if perf:
        cmd += ["perf", "record", "-F", "999", "-g", "--call-graph", "dwarf", "-o", str(run_dir / "perf.data"), "--"]
    cmd.append(str(binary))
    started = time.monotonic()
    with open(run_dir / "stdout.txt", "wb") as out, open(run_dir / "stderr.txt", "wb") as err:
        try:
            proc = subprocess.run(cmd, env=env, stdin=subprocess.DEVNULL, stdout=out, stderr=err, timeout=cfg.timeout)
            code = proc.returncode
        except subprocess.TimeoutExpired:
            raise RunError(f"{label}-{index} did not finish in {cfg.timeout} s (see {run_dir})")
    log = (run_dir / "razdor.log").read_text(errors="replace") if (run_dir / "razdor.log").exists() else ""
    if code != 0:
        raise RunError(f"{label}-{index} exited with {code} (log: {run_dir / 'razdor.log'})")
    for line in log.splitlines():
        if SCENE_ERROR.search(line):
            raise RunError(f"{label}-{index}: the scene did not stage: {line.strip()}")
    rows = read_rows(run_dir / "frames.csv")
    if not rows:
        raise RunError(f"{label}-{index}: no frame rows in {run_dir / 'frames.csv'}")
    steps = [r["frame"] for r in rows if r["kind"] == "walk-step"]
    return {
        "label": f"{label}-{index}",
        "binary": str(binary),
        "rows": rows,
        "steps": steps,
        "fingerprint": hashlib.sha256(",".join(str(s) for s in steps).encode()).hexdigest()[:12],
        "clock": clock_labels(run_dir / "razdor-play.log"),
        "secs": time.monotonic() - started,
    }


def read_rows(path):
    if not path.exists():
        return []
    with open(path, newline="") as f:
        reader = csv.DictReader(f)
        have = set(reader.fieldnames or [])
        if not {"frame", "kind", "work_ms", "total_ms", "rebuilds", *TIMED} <= have:
            raise RunError(f"{path} lacks the stage columns (a probe build before the tick's parts): rebuild and rerun")
        out = []
        for r in reader:
            row = {
                "frame": int(r["frame"]),
                "kind": r["kind"],
                "work_ms": float(r["work_ms"]),
                "total_ms": float(r["total_ms"]),
                "rebuilds": int(r["rebuilds"]),
            }
            row.update({c: float(r[c]) for c in TIMED})
            row.update({c: float(r[c]) if c in have else 0.0 for c in OPTIONAL_TIMED})
            row["tick-rest"] = max(0.0, row["tick"] - sum(row[p] for p in TICK_PARTS))
            row["armies-other"] = max(0.0, row["armies"] - row["ai_move"])
            row["rest"] = max(0.0, row["work_ms"] - (row["tick"] + row["world"] + row["fog"] + row["minimap"] + row["autosave"] + row["session"]))
            out.append(row)
    return out


def clock_labels(path):
    """The in-game dates the play log names, as (year, month, day, hour) in order (empty when
    the log has none: a walk without events writes none)."""
    if not path.exists():
        return []
    return [tuple(int(g) for g in m.groups()) for m in CLOCK.finditer(path.read_text(errors="replace"))]


def nearest_rank(sorted_vals, p):
    """The nearest-rank percentile, as the frame probe computes it (0 when empty)."""
    if not sorted_vals:
        return 0.0
    rank = min(len(sorted_vals), max(1, math.ceil(p * len(sorted_vals) / 100)))
    return sorted_vals[rank - 1]


def dist(vals):
    s = sorted(vals)
    return {"p50": nearest_rank(s, 50), "p90": nearest_rank(s, 90), "p99": nearest_rank(s, 99), "max": s[-1] if s else 0.0, "n": len(s)}


def pooled(runs, kind):
    return [r for run in runs for r in run["rows"] if r["kind"] == kind]


def metric_lines(rows_a, rows_b, k):
    """The rows of one kind's table: name, A, B, B/A, A x k, B x k (ms)."""
    spec = [("work", "work_ms", ["p50", "p90", "p99", "max"])]
    spec += [(s, s, ["p50", "p99", "max"]) for s in STAGES]
    out = []
    for name, key, quantiles in spec:
        da, db = dist([r[key] for r in rows_a]), dist([r[key] for r in rows_b])
        for q in quantiles:
            a, b = da[q], db[q]
            ratio = f"{b / a:.2f}" if a > 0 else "-"
            out.append((f"{name} {q}", a, b, ratio, a * k, b * k))
    return out


def table(rows):
    head = ("metric", "A ms", "B ms", "B/A", "phone~ A", "phone~ B")
    body = [head] + [(m, f"{a:.2f}", f"{b:.2f}", r, f"{ak:.2f}", f"{bk:.2f}") for m, a, b, r, ak, bk in rows]
    widths = [max(len(str(row[i])) for row in body) for i in range(len(head))]
    lines = []
    for n, row in enumerate(body):
        lines.append("  ".join(str(c).ljust(widths[i]) if i == 0 else str(c).rjust(widths[i]) for i, c in enumerate(row)))
        if n == 0:
            lines.append("  ".join("-" * w for w in widths))
    return "\n".join(lines)


def grid(body):
    """Left-aligned first column, right-aligned others; the first row is the header."""
    widths = [max(len(str(row[i])) for row in body) for i in range(len(body[0]))]
    lines = []
    for n, row in enumerate(body):
        lines.append("  ".join(str(c).ljust(widths[i]) if i == 0 else str(c).rjust(widths[i]) for i, c in enumerate(row)))
        if n == 0:
            lines.append("  ".join("-" * w for w in widths))
    return "\n".join(lines)


def share_lines(runs):
    """Where the walk-step frames' work goes, over all runs: each part's total and its share of
    the work. The parts add up to the work (the sum is printed as a check)."""
    rows = pooled(runs, "walk-step")
    if not rows:
        return ["(no walk-step frames)"]
    work = sum(r["work_ms"] for r in rows)
    body = [("part", "total ms", "share", "per frame ms")]
    parts = [(p, sum(r[p] for r in rows)) for p in SHARE_PARTS]
    for name, total in parts:
        body.append((name, f"{total:.2f}", f"{100 * total / work:.1f}%", f"{total / len(rows):.3f}"))
    body.append(("work", f"{work:.2f}", "100.0%", f"{work / len(rows):.3f}"))
    summed = sum(total for _, total in parts)
    text = grid(body).splitlines()
    # The armies part's own split (its sub-stages are not parts: ai_plan is inside ai_move).
    of_armies = [("armies", f"{sum(r['armies'] for r in rows):.2f}", "", "")]
    for name in ARMIES_PARTS:
        total = sum(r[name] for r in rows)
        of_armies.append((f"  {name}", f"{total:.2f}", f"{100 * total / work:.1f}%", f"{total / len(rows):.3f}"))
    text.append("")
    text.extend(grid([("of work", "total ms", "share", "per frame ms"), *of_armies]).splitlines())
    text.append(f"parts sum to {summed:.2f} ms of {work:.2f} ms over {len(rows)} walk-step frames ({len(runs)} runs)")
    return text


SPLIT_COLUMNS = ["work", "phone~", "tick", "route", "look", "armies", "ai_move", "ai_plan", "armies-other", "script",
                 "day", "snap", "tick-rest", "world", "fog", "minimap", "autosave", "rest"]


def split_lines(pairs, k):
    """One row per (run, frame) with the stage split: `pairs` are (run label, row)."""
    keys = {"work": "work_ms"}
    body = [("run", "frame", "kind", *SPLIT_COLUMNS)]
    for label, r in pairs:
        cells = []
        for col in SPLIT_COLUMNS:
            if col == "phone~":
                cells.append(f"{r['work_ms'] * k:.1f}")
            else:
                cells.append(f"{r[keys.get(col, col)]:.2f}")
        body.append((label, r["frame"], r["kind"], *cells))
    return grid(body).splitlines()


def worst_frames(runs, n=5):
    frames = [(run["label"], r) for run in runs for r in run["rows"]]
    frames.sort(key=lambda lr: lr[1]["work_ms"], reverse=True)
    head = ("run", "frame", "kind", "work", "tick", "terrain", "world", "fog", "minimap", "rebuilds")
    body = [head]
    for label, r in frames[:n]:
        body.append((label, r["frame"], r["kind"], f"{r['work_ms']:.2f}", f"{r['tick']:.2f}", f"{r['terrain']:.2f}",
                     f"{r['world']:.2f}", f"{r['fog']:.2f}", f"{r['minimap']:.2f}", r["rebuilds"]))
    widths = [max(len(str(row[i])) for row in body) for i in range(len(head))]
    return "\n".join("  ".join(str(c).ljust(widths[i]) if i == 0 else str(c).rjust(widths[i]) for i, c in enumerate(row)) for row in body)


def report(cfg, runs_a, runs_b):
    rows_walk_a = pooled(runs_a, "walk")
    if not rows_walk_a:
        raise SystemExit("A has no walk frames: the walk did not start (check the save and --walk)")
    a_walk_p50 = dist([r["work_ms"] for r in rows_walk_a])["p50"]
    if a_walk_p50 <= 0:
        raise SystemExit("A's walk work p50 is 0: cannot scale")
    k = PHONE_WORK_P50 / a_walk_p50
    out = []
    out.append(f"# Walk A/B: {cfg.walk} from {cfg.save.name}, {cfg.frames} frames, {cfg.repeats} repeat(s)")
    out.append("")
    out.append(f"A: {cfg.a}")
    out.append(f"B: {cfg.b}")
    out.append(f"k = {PHONE_WORK_P50} / A walk work p50 ({a_walk_p50:.3f} ms) = {k:.3f}; phone~ columns are the desktop times x k.")
    out.append("")
    out.append("## Runs")
    out.append("")
    out.append("run         steps  step-fingerprint  first..last step frame  in-game dates (first -> last)")
    for run in runs_a + runs_b:
        steps = run["steps"]
        span = f"{steps[0]}..{steps[-1]}" if steps else "-"
        dates = "-"
        if run["clock"]:
            dates = f"{run['clock'][0]} -> {run['clock'][-1]}"
        out.append(f"{run['label']:<11} {len(steps):>5}  {run['fingerprint']:<16}  {span:<22}  {dates}")
    out.append("")
    for kind in KINDS:
        ra, rb = pooled(runs_a, kind), pooled(runs_b, kind)
        out.append(f"## {kind}: A n={len(ra)}, B n={len(rb)}")
        out.append("")
        if not ra or not rb:
            out.append("(no frames of this kind)")
            out.append("")
            continue
        out.append("```")
        out.append(table(metric_lines(ra, rb, k)))
        out.append("```")
        out.append("")
    out.append("## Where the walk-step work goes (all runs pooled)")
    out.append("")
    out.append("```")
    out.extend(share_lines(runs_a + runs_b))
    out.append("```")
    out.append("")
    steps = [(run["label"], r) for run in runs_a + runs_b for r in run["rows"] if r["kind"] == "walk-step"]
    steps.sort(key=lambda lr: lr[1]["work_ms"], reverse=True)
    out.append("## Worst walk-step frames: stage split (top 5 of all runs; phone~ = work x k)")
    out.append("")
    out.append("```")
    out.extend(split_lines(steps[:5], k))
    out.append("```")
    out.append("")
    if cfg.detail:
        out.append(f"## Frames {','.join(str(f) for f in cfg.detail)}: stage split of every run")
        out.append("")
        out.append("```")
        named = [(run["label"], r) for run in runs_a + runs_b for r in run["rows"] if r["frame"] in cfg.detail]
        named.sort(key=lambda lr: (lr[1]["frame"], lr[0]))
        out.extend(split_lines(named, k))
        out.append("```")
        out.append("")
    out.append("## Worst frames by work (top 5 of all runs)")
    out.append("")
    out.append("```")
    out.append(worst_frames(runs_a + runs_b))
    out.append("```")
    text = "\n".join(out) + "\n"
    (cfg.out / "report.md").write_text(text)
    return text


def main(argv=None):
    cfg = parse_args(argv)
    dt_dir = require_tools(cfg)
    w, h = (int(x) for x in cfg.size.lower().split("x"))
    cfg.out.mkdir(parents=True, exist_ok=True)
    runs_a, runs_b = [], []
    xvfb = start_xvfb(cfg, (w, h))
    try:
        first = True
        for i in range(1, cfg.repeats + 1):
            for label, binary, bucket in (("A", cfg.a, runs_a), ("B", cfg.b, runs_b)):
                print(f"run {label}-{i} ...", file=sys.stderr, flush=True)
                run = run_once(cfg, dt_dir, label, i, binary, perf=cfg.perf and first)
                first = False
                bucket.append(run)
                print(f"  {run['label']}: {len(run['steps'])} steps, {len(run['rows'])} frames, {run['secs']:.1f} s", file=sys.stderr, flush=True)
    finally:
        xvfb.terminate()
        try:
            xvfb.wait(timeout=5)
        except subprocess.TimeoutExpired:
            xvfb.kill()
    print(report(cfg, runs_a, runs_b))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except RunError as e:
        print(f"walk_ab: {e}", file=sys.stderr)
        sys.exit(1)
