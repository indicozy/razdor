#!/usr/bin/env python3
"""UI regression check: renders the scenes of `scenes.txt` off-screen and compares them with
the references in the goldens folder.

    python3 -I tools/ui-regress/ui_regress.py run [--bin PATH] [--only TEXT] [--out DIR]
    python3 -I tools/ui-regress/ui_regress.py accept [--only TEXT] OUTDIR

`run` writes OUTDIR/shots/<case>.png, OUTDIR/<case>.diff.png for each changed case and
OUTDIR/report.md, and exits 1 when a case changed or crashed. `accept` copies the shots of a run
into the goldens. The goldens show the original game's art: they live outside the repo
(RAZDOR_GOLDENS, default /mnt/ai_models/razdor-goldens) and are never committed.
See tools/ui-regress/README.md.
"""

import argparse
import concurrent.futures
import datetime
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageChops

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
SCENES = HERE / "scenes.txt"
DEFAULT_GOLDENS = Path(os.environ.get("RAZDOR_GOLDENS", "/mnt/ai_models/razdor-goldens"))
DEFAULT_BIN = Path("/mnt/ai_models/razdor-scratch/ui-regress/target/release/razdor")
SCRATCH = Path("/mnt/ai_models/razdor-scratch/ui-regress/out")
TIMEOUT_S = 60
WORKERS = 4
CPUS = "8-19"
XVFB_FIRST_DISPLAY = 100  # :100, :101, … (one per case)


def install_dir():
    """RAZDOR_DT_DIR from the environment, else from the repo's .env (this worktree's, or the
    main checkout's, which holds the untracked .env)."""
    if os.environ.get("RAZDOR_DT_DIR"):
        return os.environ["RAZDOR_DT_DIR"].strip().strip('"')
    candidates = [REPO / ".env"]
    try:
        common = subprocess.run(["git", "-C", str(REPO), "rev-parse", "--git-common-dir"],
                                capture_output=True, text=True, check=True).stdout.strip()
        candidates.append(Path(common).resolve().parent / ".env")
    except (subprocess.CalledProcessError, OSError):
        pass
    for env in candidates:
        if env.is_file():
            for line in env.read_text(encoding="utf-8").splitlines():
                if line.startswith("RAZDOR_DT_DIR="):
                    return line.split("=", 1)[1].strip().strip('"').strip("'")
    sys.exit("RAZDOR_DT_DIR is not set (set it, or put it in the repo's .env)")


def load_cases(only=None):
    cases = []
    for lineno, raw in enumerate(SCENES.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [p.strip() for p in line.split("|")]
        if len(parts) < 3:
            sys.exit(f"{SCENES}:{lineno}: want 'name | scene | WxH | KEY=VAL;...'")
        name, scene, size = parts[0], parts[1], parts[2]
        extra = {}
        if len(parts) > 3 and parts[3]:
            for pair in parts[3].split(";"):
                if pair.strip():
                    k, _, v = pair.partition("=")
                    extra[k.strip()] = v.strip()
        if only and only not in name:
            continue
        cases.append({"name": name, "scene": scene, "size": size, "extra": extra, "key": f"{name}@{size}"})
    keys = [c["key"] for c in cases]
    dupes = sorted({k for k in keys if keys.count(k) > 1})
    if dupes:
        sys.exit(f"duplicate case names in {SCENES}: {dupes}")
    return cases


def run_case(case, index, out, dt_dir, binary):
    key = case["key"]
    work = out / "work" / case["name"]
    xdg = work / "xdg"
    saves = work / "saves"
    for d in (xdg, saves):
        d.mkdir(parents=True, exist_ok=True)
    os.chmod(xdg, 0o700)
    shot = out / "shots" / f"{key}.png"
    shot.parent.mkdir(parents=True, exist_ok=True)
    log = out / "logs" / f"{key}.log"
    log.parent.mkdir(parents=True, exist_ok=True)
    razdor_log = work / "razdor.log"
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", str(work)),
        "XDG_RUNTIME_DIR": str(xdg),
        "RAZDOR_LOG": str(razdor_log),
        "RAZDOR_SAVE_DIR": str(saves),
        "RAZDOR_DT_DIR": dt_dir,
        "RAZDOR_SIZE": case["size"],
        "RAZDOR_SCENE": case["scene"],
        "RAZDOR_SNAPSHOT": str(shot),
    }
    env.update(case["extra"])
    w, h = case["size"].split("x")
    # A display number of its own per case: `xvfb-run -a` races when several start at once.
    display = str(XVFB_FIRST_DISPLAY + index)
    cmd = ["nice", "-n", "19", "taskset", "-c", CPUS, "xvfb-run", "-n", display, "-w", "10", "-s", f"-screen 0 {w}x{h}x24",
           str(binary)]
    started = time.monotonic()
    with open(log, "w", encoding="utf-8") as fh:
        proc = subprocess.Popen(cmd, env=env, stdout=fh, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = proc.wait(timeout=TIMEOUT_S)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)  # xvfb-run and its Xvfb with it
            proc.wait()
            code = "timeout"
    elapsed = time.monotonic() - started
    status = "crashed" if code != 0 or not shot.exists() else "ok"
    if razdor_log.exists():
        shutil.copyfile(razdor_log, out / "logs" / f"{key}.razdor.log")
    shutil.rmtree(work, ignore_errors=True)
    return {"case": case, "status": status, "code": code, "secs": elapsed, "shot": shot}


def compare(ref_path, new_path, diff_path, tolerance, threshold):
    """(status, changed fraction, bbox). A size mismatch counts as a full change."""
    new = Image.open(new_path).convert("RGB")
    if not ref_path.exists():
        return "new", None, None
    ref = Image.open(ref_path).convert("RGB")
    if ref.size != new.size:
        make_diff(ref, new, None, diff_path)
        return "changed", 1.0, (0, 0, new.size[0], new.size[1])
    r, g, b = ImageChops.difference(ref, new).split()
    # The largest channel difference of each pixel, then the pixels above the tolerance.
    biggest = ImageChops.lighter(ImageChops.lighter(r, g), b)
    mask = biggest.point(lambda v: 255 if v > tolerance else 0)
    count = mask.histogram()[255]
    fraction = count / (new.size[0] * new.size[1])
    bbox = mask.getbbox()
    if fraction > threshold:
        make_diff(ref, new, mask, diff_path)
        return "changed", fraction, bbox
    return "ok", fraction, bbox


def make_diff(ref, new, mask, path):
    """Reference | new | new dimmed with the changed pixels in magenta, side by side."""
    w, h = new.size
    if mask is None:
        mask = Image.new("L", (w, h), 255)
        ref = ref.resize((w, h))
    dim = Image.blend(Image.new("RGB", (w, h), (0, 0, 0)), new, 0.35)
    overlay = Image.composite(Image.new("RGB", (w, h), (255, 0, 255)), dim, mask)
    panel = Image.new("RGB", (w * 3 + 8, h), (40, 40, 40))
    panel.paste(ref, (0, 0))
    panel.paste(new, (w + 4, 0))
    panel.paste(overlay, (2 * w + 8, 0))
    panel.save(path)


def cmd_run(args):
    cases = load_cases(args.only)
    if not cases:
        sys.exit("no case matches")
    binary = Path(args.bin)
    if not binary.is_file():
        sys.exit(f"no binary at {binary}: build it first (see tools/ui-regress/README.md)")
    dt_dir = install_dir()
    out = Path(args.out) if args.out else SCRATCH / datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    out.mkdir(parents=True, exist_ok=True)
    goldens = Path(args.goldens)
    started = time.monotonic()
    print(f"{len(cases)} cases, {WORKERS} at a time, into {out}", flush=True)
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=WORKERS) as pool:
        futures = [pool.submit(run_case, c, i, out, dt_dir, binary) for i, c in enumerate(cases)]
        for fut in concurrent.futures.as_completed(futures):
            r = fut.result()
            results.append(r)
            print(f"  {r['case']['key']}: {r['status']} (exit {r['code']}, {r['secs']:.1f} s)", flush=True)
    results.sort(key=lambda r: r["case"]["key"])
    rows = []
    for r in results:
        key = r["case"]["key"]
        if r["status"] == "crashed":
            rows.append((r, "crashed", None, None, None))
            continue
        diff = out / f"{key}.diff.png"
        status, fraction, bbox = compare(goldens / f"{key}.png", r["shot"], diff, args.tolerance, args.threshold)
        rows.append((r, status, fraction, bbox, diff if status == "changed" else None))
    elapsed = time.monotonic() - started
    write_report(out, rows, args, elapsed, goldens)
    counts = {s: sum(1 for row in rows if row[1] == s) for s in ("ok", "changed", "new", "crashed")}
    print(f"ok {counts['ok']}, changed {counts['changed']}, new {counts['new']}, crashed {counts['crashed']}; "
          f"{elapsed:.0f} s; report {out / 'report.md'}", flush=True)
    return 1 if counts["changed"] or counts["crashed"] else 0


def write_report(out, rows, args, elapsed, goldens):
    lines = [
        "# UI regression report",
        "",
        f"Run {datetime.datetime.now().isoformat(timespec='seconds')}, {len(rows)} cases in {elapsed:.0f} s. "
        f"Tolerance {args.tolerance} (max channel difference), threshold {args.threshold * 100:.3f}% of pixels. "
        f"References: `{goldens}`.",
        "",
        "| case | status | % changed | bbox | shot | reference | diff |",
        "|---|---|---|---|---|---|---|",
    ]
    for r, status, fraction, bbox, diff in rows:
        key = r["case"]["key"]
        pct = "" if fraction is None else f"{fraction * 100:.3f}"
        box = "" if not bbox else f"{bbox[0]},{bbox[1]} {bbox[2] - bbox[0]}x{bbox[3] - bbox[1]}"
        ref = goldens / f"{key}.png"
        lines.append(f"| {key} | {status} | {pct} | {box} | {r['shot']} | "
                     f"{ref if ref.exists() else ''} | {diff or ''} |")
    (out / "report.md").write_text("\n".join(lines) + "\n", encoding="utf-8")


def cmd_accept(args):
    out = Path(args.outdir)
    shots = sorted((out / "shots").glob("*.png"))
    if args.only:
        shots = [s for s in shots if args.only in s.stem]
    if not shots:
        sys.exit("no shots to accept")
    args.goldens.mkdir(parents=True, exist_ok=True)
    for s in shots:
        shutil.copyfile(s, args.goldens / s.name)
    print(f"accepted {len(shots)} shots into {args.goldens}")
    return 0


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="render the scenes and compare them with the goldens")
    r.add_argument("--bin", default=str(DEFAULT_BIN), help="the release razdor binary")
    r.add_argument("--only", help="only the cases whose name contains this text")
    r.add_argument("--out", help="output folder (default: a timestamped one under scratch)")
    r.add_argument("--goldens", default=str(DEFAULT_GOLDENS))
    r.add_argument("--tolerance", type=int, default=12, help="max channel difference that still counts as the same")
    r.add_argument("--threshold", type=float, default=0.0005, help="changed fraction of pixels that fails a case")
    r.set_defaults(func=cmd_run)
    a = sub.add_parser("accept", help="copy a run's shots into the goldens")
    a.add_argument("--only", help="only the shots whose name contains this text")
    a.add_argument("--goldens", type=Path, default=DEFAULT_GOLDENS)
    a.add_argument("outdir")
    a.set_defaults(func=cmd_accept)
    args = p.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
