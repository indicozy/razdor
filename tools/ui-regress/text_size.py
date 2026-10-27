#!/usr/bin/env python3
"""Text-size check: renders the phone scenes of `scenes.txt` off-screen with the text-size probe
(`RAZDOR_TEXT_PROBE`, src/ui/text_probe.rs) and measures every text drawn, in sp.

    python3 -I tools/ui-regress/text_size.py run [--bin PATH] [--only TEXT] [--out DIR] [--dp 2.625] [--floor 12] [--body 14]
    python3 -I tools/ui-regress/text_size.py accept OUTDIR

A text's size in sp is its capital's height / dp / 0.7 (a capital is about 0.7 of the font
size, the size Android counts in sp). Under `--floor` (12, Android's smallest recommended text)
it is too small, under `--body` (14) small. `run` writes OUTDIR/probe/<case>.tsv, the shots,
OUTDIR/marked/<case>.png (red boxes: too small, orange: small) and OUTDIR/report.md, and exits 1
when a case crashed or shows a too-small text that the baseline does not list. `accept` writes
a run's too-small texts as the baseline (RAZDOR_GOLDENS/text-size-baseline.json: the texts are the
game's, so it lives outside the repo). See the `text-size` skill.
"""

import argparse
import concurrent.futures
import datetime
import json
import sys
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import ui_regress  # noqa: E402

BASELINE = ui_regress.DEFAULT_GOLDENS / "text-size-baseline.json"
OUT = ui_regress.SCRATCH.parent.parent / "text-size"
DEFAULT_DP = 2.625  # a common phone


def phone_cases(only, dp, out):
    """The phone cases (name `phone-…` or a RAZDOR_DP of their own), each with the probe on and,
    when it has none, the phone's dp `dp` so it is laid out as on a phone."""
    cases = []
    for c in ui_regress.load_cases(only):
        if not (c["name"].startswith("phone-") or "RAZDOR_DP" in c["extra"]):
            continue
        c = dict(c, extra=dict(c["extra"]))
        c["extra"].setdefault("RAZDOR_DP", str(dp))
        c["extra"]["RAZDOR_TEXT_PROBE"] = str(out / "probe" / f"{c['key']}.tsv")
        cases.append(c)
    return cases


def parse(tsv, dp):
    """The texts of a probe file, without exact repeats: dicts with text, x, y, w, cap and sp."""
    if not tsv.exists():
        return []
    lines = tsv.read_text(encoding="utf-8").splitlines()
    if lines and lines[0].startswith("# dp\t"):
        try:
            dp = float(lines[0].split("\t")[1]) or dp
        except (IndexError, ValueError):
            pass
        lines = lines[1:]
    seen, texts = set(), []
    for line in lines:
        parts = line.split("\t", 5)
        if len(parts) < 6:
            continue
        try:
            size, cap, x, y, w = (float(p) for p in parts[:5])
        except ValueError:
            continue
        key = (parts[5], round(cap, 1), round(x), round(y))
        if key in seen:
            continue
        seen.add(key)
        texts.append({"text": parts[5], "x": x, "y": y, "w": w, "cap": cap, "size": size, "sp": cap / dp / 0.7})
    return texts


def mark(shot, texts, floor, body, path):
    """The shot with a box around each text under `body`: red under `floor`, orange else."""
    im = Image.open(shot).convert("RGB")
    draw = ImageDraw.Draw(im)
    for t in texts:
        if t["sp"] >= body:
            continue
        box = (t["x"] - 2, t["y"] - t["cap"] * 1.25, t["x"] + t["w"] + 2, t["y"] + t["cap"] * 0.35)
        draw.rectangle(box, outline=(255, 0, 0) if t["sp"] < floor else (255, 150, 0), width=3)
    path.parent.mkdir(parents=True, exist_ok=True)
    im.save(path)


def load_baseline():
    try:
        return json.loads(BASELINE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def cmd_run(args):
    out = Path(args.out) if args.out else OUT / datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    (out / "probe").mkdir(parents=True, exist_ok=True)
    cases = phone_cases(args.only, args.dp, out)
    if not cases:
        sys.exit("no phone case matches")
    binary = Path(args.bin)
    if not binary.is_file():
        sys.exit(f"no binary at {binary}: build it first (tools/ui-regress/README.md)")
    dt_dir = ui_regress.install_dir()
    (out / "limits.json").write_text(json.dumps({"floor": args.floor, "body": args.body, "dp": args.dp}), encoding="utf-8")
    print(f"{len(cases)} phone cases, {ui_regress.WORKERS} at a time, into {out}", flush=True)
    results = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=ui_regress.WORKERS) as pool:
        futures = [pool.submit(ui_regress.run_case, c, i, out, dt_dir, binary) for i, c in enumerate(cases)]
        for fut in concurrent.futures.as_completed(futures):
            r = fut.result()
            results.append(r)
            print(f"  {r['case']['key']}: {r['status']} ({r['secs']:.1f} s)", flush=True)
    results.sort(key=lambda r: r["case"]["key"])
    baseline = load_baseline()
    table, details, new_total, crashed = [], [], 0, 0
    for r in results:
        key = r["case"]["key"]
        if r["status"] != "ok":
            crashed += 1
            table.append(f"| {key} | {r['status']} | | | | | |")
            continue
        texts = parse(Path(r["case"]["extra"]["RAZDOR_TEXT_PROBE"]), float(r["case"]["extra"]["RAZDOR_DP"]))
        accepted = set(baseline.get(key, []))
        low = sorted((t for t in texts if t["sp"] < args.body), key=lambda t: t["sp"])
        too_small = [t for t in low if t["sp"] < args.floor]
        new = [t for t in too_small if t["text"] not in accepted]
        new_total += len(new)
        smallest = f"{min(t['sp'] for t in texts):.1f}" if texts else ""
        marked = ""
        if low:
            marked = out / "marked" / f"{key}.png"
            mark(r["shot"], texts, args.floor, args.body, marked)
            details.append(f"\n## {key}\n\n{marked}\n")
            for t in low:
                flag = " **NEW**" if t in new else ""
                details.append(f"- {t['sp']:.1f} sp (cap {t['cap']:.1f} px) at {t['x']:.0f},{t['y']:.0f}: {t['text'][:70]}{flag}")
        table.append(f"| {key} | ok | {len(texts)} | {len(too_small)} | {len(low) - len(too_small)} | {smallest} | {len(new)} |")
    report = [
        "# Text-size report",
        "",
        f"Run {datetime.datetime.now().isoformat(timespec='seconds')}: too small under {args.floor} sp, small under "
        f"{args.body} sp; sp = capital height / dp / 0.7. Baseline: `{BASELINE}`.",
        "",
        "| case | status | texts | too small | small | smallest sp | new too small |",
        "|---|---|---|---|---|---|---|",
        *table,
        *details,
    ]
    (out / "report.md").write_text("\n".join(report) + "\n", encoding="utf-8")
    print(f"crashed {crashed}, new too-small texts {new_total}; report {out / 'report.md'}", flush=True)
    return 1 if crashed or new_total else 0


def cmd_accept(args):
    out = Path(args.outdir)
    try:
        limits = json.loads((out / "limits.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        sys.exit(f"{out} is not a text-size run (no limits.json)")
    baseline = load_baseline()
    for tsv in sorted((out / "probe").glob("*.tsv")):
        small = sorted({t["text"] for t in parse(tsv, limits["dp"]) if t["sp"] < limits["floor"]})
        baseline[tsv.stem] = small
        print(f"  {tsv.stem}: {len(small)} texts")
    BASELINE.parent.mkdir(parents=True, exist_ok=True)
    BASELINE.write_text(json.dumps(baseline, indent=1, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")
    print(f"baseline written: {BASELINE}")
    return 0


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="render the phone scenes and measure their texts")
    r.add_argument("--bin", default=str(ui_regress.DEFAULT_BIN), help="the release razdor binary (with the probe)")
    r.add_argument("--only", help="only the cases whose name contains this text")
    r.add_argument("--out", help="output folder (default: a timestamped one under razdor-scratch/text-size)")
    r.add_argument("--dp", type=float, default=DEFAULT_DP, help="px per dp for phone cases without RAZDOR_DP")
    r.add_argument("--floor", type=float, default=12.0, help="too small under this many sp")
    r.add_argument("--body", type=float, default=14.0, help="small under this many sp")
    r.set_defaults(func=cmd_run)
    a = sub.add_parser("accept", help="write a run's too-small texts as the baseline")
    a.add_argument("outdir")
    a.set_defaults(func=cmd_accept)
    args = p.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
