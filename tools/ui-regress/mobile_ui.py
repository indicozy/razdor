#!/usr/bin/env python3
"""Mobile UI check: renders the phone screens off-screen with the text probe
(`RAZDOR_TEXT_PROBE`, src/ui/text_probe.rs) and the touch probe (`RAZDOR_TOUCH_PROBE`,
src/ui/touch_probe.rs), and reports the text and touch targets that are too small for a finger.

    python3 -I tools/ui-regress/mobile_ui.py run [--bin PATH] [--only TEXT] [--out DIR] [--floor 12] [--body 14]
                                               [--too-small 40] [--small 48] [--gap 8]
    python3 -I tools/ui-regress/mobile_ui.py accept OUTDIR

The cases are generated here: PROFILES x orientations x SCREENS (72 at the time of writing), each
named `mobile-<profile>-<orientation>-<screen>`.

Texts: a text's size in sp is its capital's height / dp / 0.7 (a capital is about 0.7 of the font
size, the size Android counts in sp). Under `--floor` (12, Android's smallest recommended text) it
is too small, under `--body` (14) small.

Touch targets: a target's size is its hit rectangle, clipped to the screen, in dp (px / dp) and mm
(dp * 25.4 / 160). Under `--too-small` (40 dp on its smaller side) it is too small, under `--small`
(48 dp) small. Two targets closer than `--gap` dp (neither inside the other, and their projections
overlapping on one axis, or overlapping each other) are crowded.

`run` writes OUTDIR/probe/<case>.tsv, OUTDIR/touch/<case>.tsv, OUTDIR/shots, OUTDIR/marked/<case>.png
(every target outlined cyan, too small red, small orange, crowded pairs joined by magenta, the texts
boxed as before), OUTDIR/view/<case>.png (the marked shot scaled to a 540 px short side), and
OUTDIR/report.md. It exits 1 when a case crashed or shows a too-small text or target that the
baseline does not list. `accept` writes a run's too-small texts and targets as the baseline
(RAZDOR_GOLDENS/mobile-ui-baseline.json: the texts are the game's, so it lives outside the repo).
See the `text-size` skill and tools/ui-regress/README.md.
"""

import argparse
import concurrent.futures
import datetime
import json
import sys
import time
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import ui_regress  # noqa: E402

BASELINE = ui_regress.DEFAULT_GOLDENS / "mobile-ui-baseline.json"
OUT = ui_regress.SCRATCH.parent.parent / "mobile-ui"
MM_PER_INCH = 25.4
DPI_BASE = 160.0  # Android's density unit: 1 dp is 1/160 inch
VIEW_SHORT_SIDE = 540

RED = (255, 0, 0)
ORANGE = (255, 150, 0)
CYAN = (0, 255, 255)
MAGENTA = (255, 0, 255)

# name, portrait width, portrait height, px per dp. small = an ordinary modern phone, big = a dense
# large phone (the owner's OnePlus 15 class).
PROFILES = [("small", 1080, 2400, 2.75), ("big", 1440, 3120, 3.5)]
ORIENTATIONS = ("portrait", "landscape")
# name, scene, extra env: the phone screens of `scenes.txt`, with the scene strings copied from it.
SCREENS = [
    ("title", "title", {}),
    ("options", "options", {}),
    ("scenarios", "scenarios:РК1-Начало пути", {}),
    ("map", "map:РК1-Начало пути", {}),
    ("minimap", "minimap:РК1-Начало пути", {}),
    ("settings", "settings:РК1-Начало пути", {}),
    ("keys", "settings:РК1-Начало пути:keys", {}),
    ("help", "map:РК1-Начало пути", {"RAZDOR_SCENE_OPEN": "3:keys"}),
    ("save", "save:РК1-Начало пути", {}),
    ("victory", "victory:РК1-Начало пути", {}),
    ("barracks", "building:РК1-Начало пути:0:barracks", {}),
    ("store", "building:РК1-Начало пути:0:store", {}),
    ("market", "building:РК3-Столица:8:market", {}),
    ("army", "army:РК1-Начало пути", {}),
    ("hero", "hero:РК1-Начало пути:0", {}),
    ("spells", "spells:РК1-Начало пути", {}),
    ("battle", "battle:РК1-Начало пути:0", {}),
    ("classes", "classes:РК1-Начало пути", {}),
]


def build_cases(only, out):
    """The cases of every profile, orientation and screen (name containing `only`), each with the
    text and touch probes on, writing into `out`."""
    cases = []
    for pname, pw, ph, dp in PROFILES:
        for orient in ORIENTATIONS:
            w, h = (pw, ph) if orient == "portrait" else (ph, pw)
            for sname, scene, extra in SCREENS:
                name = f"mobile-{pname}-{orient}-{sname}"
                if only and only not in name:
                    continue
                key = f"{name}@{w}x{h}"
                env = dict(extra)
                # The phone defaults of `display::default_text_size` (1.25 on a phone, 1.0 on the
                # desktop build the snapshot runs) and `display::default_large_windows` (true on a
                # phone, false on the desktop), so the cases measure the text players see. A
                # SCREENS entry may override them.
                env.setdefault("RAZDOR_TEXT_SIZE", "1.25")
                env.setdefault("RAZDOR_LARGE_WINDOWS", "1")
                env["RAZDOR_DP"] = str(dp)
                env["RAZDOR_TEXT_PROBE"] = str(out / "probe" / f"{key}.tsv")
                env["RAZDOR_TOUCH_PROBE"] = str(out / "touch" / f"{key}.tsv")
                cases.append({"name": name, "scene": scene, "size": f"{w}x{h}", "extra": env, "key": key, "dp": dp})
    return cases


def parse(tsv, dp):
    """The texts of a text probe file, without exact repeats: dicts with text, x, y, w, cap and sp."""
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


def parse_targets(tsv, screen):
    """The touch targets of a touch probe file: dicts with kind, the rectangle clipped to the screen
    (`screen` = (w, h) px), enabled and place. Zero-area and off-screen rectangles are dropped, and
    exact repeats (the same rounded rectangle and place, whatever the kind) are kept once."""
    if not tsv.exists():
        return []
    seen, targets = set(), []
    for line in tsv.read_text(encoding="utf-8").splitlines():
        if line.startswith("#"):
            continue
        parts = line.split("\t", 6)
        if len(parts) < 7:
            continue
        try:
            x, y, w, h = (float(p) for p in parts[1:5])
        except ValueError:
            continue
        x0, y0 = max(0.0, x), max(0.0, y)
        x1, y1 = min(float(screen[0]), x + w), min(float(screen[1]), y + h)
        if x1 <= x0 or y1 <= y0:
            continue
        place = parts[6]
        key = (round(x0, 1), round(y0, 1), round(x1 - x0, 1), round(y1 - y0, 1), place)
        if key in seen:
            continue
        seen.add(key)
        targets.append({"kind": parts[0], "x": x0, "y": y0, "w": x1 - x0, "h": y1 - y0,
                        "enabled": parts[5] == "1", "place": place})
    return targets


def text_centre(t):
    """The centre of a text's box (x, y - cap * 1.25, x + w, y + cap * 0.35) in px."""
    return t["x"] + t["w"] / 2, t["y"] - t["cap"] * 0.45


def label_of(target, texts):
    """The texts whose box centre is inside the target, in reading order, at most two, joined by
    ' / ' and cut to 40 characters; '(no text)' when none."""
    inside = []
    for t in sorted(texts, key=lambda t: (t["y"], t["x"])):
        cx, cy = text_centre(t)
        if target["x"] <= cx <= target["x"] + target["w"] and target["y"] <= cy <= target["y"] + target["h"]:
            if t["text"] not in inside:
                inside.append(t["text"])
    if not inside:
        return "(no text)"
    return " / ".join(inside[:2])[:40]


def contains(a, b):
    """Whether rectangle a contains rectangle b."""
    return a["x"] <= b["x"] and a["y"] <= b["y"] and a["x"] + a["w"] >= b["x"] + b["w"] and a["y"] + a["h"] >= b["y"] + b["h"]


def crowded_pairs(targets, dp, gap_limit):
    """The pairs of targets less than `gap_limit` dp apart, nearest first: each as a dict with
    the two targets and the gap in dp. Neither may contain the other. Partly overlapping pairs
    have gap 0; pairs whose projections overlap on one axis have the distance on the other; pairs
    apart on both axes are not compared."""
    pairs = []
    for i, a in enumerate(targets):
        for b in targets[i + 1:]:
            if contains(a, b) or contains(b, a):
                continue
            ox = min(a["x"] + a["w"], b["x"] + b["w"]) - max(a["x"], b["x"])
            oy = min(a["y"] + a["h"], b["y"] + b["h"]) - max(a["y"], b["y"])
            if ox > 0 and oy > 0:
                gap = 0.0
            elif ox > 0:
                gap = max(b["y"] - (a["y"] + a["h"]), a["y"] - (b["y"] + b["h"]))
            elif oy > 0:
                gap = max(b["x"] - (a["x"] + a["w"]), a["x"] - (b["x"] + b["w"]))
            else:
                continue
            if gap / dp < gap_limit:
                pairs.append({"a": a, "b": b, "gap": gap / dp})
    pairs.sort(key=lambda p: p["gap"])
    return pairs


def analyse(case, out, limits):
    """The texts, touch targets and crowded pairs of one case's probe files in run folder `out`.
    Each target gets its size in dp and mm, its class (`too small`, `small` or ''), its label and
    its baseline identity; each text its sp."""
    dp = float(case["dp"])
    screen = tuple(int(v) for v in case["size"].split("x"))
    texts = parse(out / "probe" / f"{case['key']}.tsv", dp)
    targets = parse_targets(out / "touch" / f"{case['key']}.tsv", screen)
    for t in targets:
        t["w_dp"] = t["w"] / dp
        t["h_dp"] = t["h"] / dp
        t["min_dp"] = min(t["w_dp"], t["h_dp"])
        t["w_mm"] = t["w_dp"] * MM_PER_INCH / DPI_BASE
        t["h_mm"] = t["h_dp"] * MM_PER_INCH / DPI_BASE
        t["cls"] = "too small" if t["min_dp"] < limits["too_small"] else "small" if t["min_dp"] < limits["small"] else ""
        t["label"] = label_of(t, texts)
        t["id"] = f"{t['place']}|{t['label']}"
        t["x_dp"] = t["x"] / dp
        t["y_dp"] = t["y"] / dp
    crowded = crowded_pairs(targets, dp, limits["gap"])
    return texts, targets, crowded


def load_baseline():
    try:
        return json.loads(BASELINE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return {}


def mark(shot, texts, targets, crowded, limits, path):
    """The shot with every target outlined cyan (2 px), the too small ones red and the small ones
    orange (4 px), the crowded pairs joined by a magenta line between their centres, and the texts
    under the body size boxed (red: too small, orange: small)."""
    im = Image.open(shot).convert("RGB")
    draw = ImageDraw.Draw(im)
    for t in targets:
        draw.rectangle((t["x"], t["y"], t["x"] + t["w"], t["y"] + t["h"]), outline=CYAN, width=2)
    for t in targets:
        if t["cls"]:
            colour = RED if t["cls"] == "too small" else ORANGE
            draw.rectangle((t["x"], t["y"], t["x"] + t["w"], t["y"] + t["h"]), outline=colour, width=4)
    for p in crowded:
        (ax, ay), (bx, by) = centre(p["a"]), centre(p["b"])
        draw.line([(ax, ay), (bx, by)], fill=MAGENTA, width=3)
    for t in texts:
        if t["sp"] >= limits["body"]:
            continue
        box = (t["x"] - 2, t["y"] - t["cap"] * 1.25, t["x"] + t["w"] + 2, t["y"] + t["cap"] * 0.35)
        draw.rectangle(box, outline=RED if t["sp"] < limits["floor"] else ORANGE, width=3)
    path.parent.mkdir(parents=True, exist_ok=True)
    im.save(path)


def centre(t):
    """The centre of a target in px."""
    return t["x"] + t["w"] / 2, t["y"] + t["h"] / 2


def write_view(marked, path):
    """The marked shot scaled (LANCZOS) so its shorter side is VIEW_SHORT_SIDE px."""
    im = Image.open(marked)
    scale = VIEW_SHORT_SIDE / min(im.size)
    path.parent.mkdir(parents=True, exist_ok=True)
    im.resize((round(im.width * scale), round(im.height * scale)), Image.Resampling.LANCZOS).save(path)


def case_report(case, out, limits, baseline, result):
    """The per-case figures and report lines: a dict with the counts, the lines and the new items."""
    texts, targets, crowded = result["texts"], result["targets"], result["crowded"]
    key = case["key"]
    accepted_texts = set(baseline.get(key, {}).get("texts", []))
    accepted_targets = set(baseline.get(key, {}).get("targets", []))
    low = sorted((t for t in texts if t["sp"] < limits["body"]), key=lambda t: t["sp"])
    too_small_text = [t for t in low if t["sp"] < limits["floor"]]
    new_texts = [t for t in too_small_text if t["text"] not in accepted_texts]
    flagged = sorted((t for t in targets if t["cls"]), key=lambda t: t["min_dp"])
    too_small_targets = [t for t in flagged if t["cls"] == "too small"]
    new_targets = [t for t in too_small_targets if t["id"] not in accepted_targets]
    details = []
    if low or flagged or crowded:
        details.append(f"\n## {key}\n\n{out / 'view' / (key + '.png')}\n")
        for t in low:
            flag = " **NEW**" if t in new_texts else ""
            details.append(f"- {t['sp']:.1f} sp (cap {t['cap']:.1f} px) at {t['x']:.0f},{t['y']:.0f}: {t['text'][:70]}{flag}")
        for t in flagged:
            flag = " **NEW**" if t in new_targets else ""
            dis = "" if t["enabled"] else " (disabled)"
            details.append(f"- {t['w_dp']:.1f} x {t['h_dp']:.1f} dp ({t['w_mm']:.1f} x {t['h_mm']:.1f} mm) "
                           f"\"{t['label']}\" {t['place']} [{t['kind']}]{dis}{flag}")
        for p in crowded:
            a, b = p["a"], p["b"]
            details.append(f"- crowded {p['gap']:.1f} dp: \"{a['label']}\" ({a['place']} @ {a['x_dp']:.0f},{a['y_dp']:.0f} dp) "
                           f"↔ \"{b['label']}\" ({b['place']} @ {b['x_dp']:.0f},{b['y_dp']:.0f} dp)")
    smallest = f"{min(t['sp'] for t in texts):.1f}" if texts else ""
    row = (f"| {key} | ok | {len(texts)} | {len(too_small_text)} | {len(low) - len(too_small_text)} | {smallest} | "
           f"{len(targets)} | {len(too_small_targets)} | {len(flagged) - len(too_small_targets)} | {len(crowded)} | "
           f"{len(new_texts) + len(new_targets)} |")
    return {
        "row": row,
        "details": details,
        "new_texts": len(new_texts),
        "new_targets": len(new_targets),
    }


def cmd_run(args):
    out = Path(args.out).resolve() if args.out else (OUT / datetime.datetime.now().strftime("%Y%m%d-%H%M%S")).resolve()
    (out / "probe").mkdir(parents=True, exist_ok=True)
    (out / "touch").mkdir(parents=True, exist_ok=True)
    (out / "specs").mkdir(parents=True, exist_ok=True)
    cases = build_cases(args.only, out)
    if not cases:
        sys.exit("no mobile case matches")
    binary = Path(args.bin)
    if not binary.is_file():
        sys.exit(f"no binary at {binary}: build it first (tools/ui-regress/README.md)")
    dt_dir = ui_regress.install_dir()
    limits = {"floor": args.floor, "body": args.body, "too_small": args.too_small, "small": args.small, "gap": args.gap}
    (out / "limits.json").write_text(json.dumps(limits), encoding="utf-8")
    (out / "cases.json").write_text(
        json.dumps([{"key": c["key"], "dp": c["dp"], "size": c["size"]} for c in cases], ensure_ascii=False),
        encoding="utf-8")
    print(f"{len(cases)} mobile cases, {ui_regress.WORKERS} at a time, into {out}", flush=True)
    started = time.monotonic()
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
        case = r["case"]
        key = case["key"]
        if r["status"] != "ok":
            crashed += 1
            table.append(f"| {key} | {r['status']} |" + " |" * 9)
            continue
        texts, targets, crowded = analyse(case, out, limits)
        marked = out / "marked" / f"{key}.png"
        mark(r["shot"], texts, targets, crowded, limits, marked)
        write_view(marked, out / "view" / f"{key}.png")
        figures = case_report(case, out, limits, baseline, {"texts": texts, "targets": targets, "crowded": crowded})
        table.append(figures["row"])
        details.extend(figures["details"])
        new_total += figures["new_texts"] + figures["new_targets"]
    elapsed = time.monotonic() - started
    report = [
        "# Mobile UI report",
        "",
        f"Run {datetime.datetime.now().isoformat(timespec='seconds')}, {len(results)} cases in {elapsed:.0f} s. "
        f"Texts: too small under {args.floor:g} sp, small under {args.body:g} sp (sp = capital height / dp / 0.7). "
        f"Touch targets: too small under {args.too_small:g} dp on the smaller side, small under {args.small:g} dp; "
        f"crowded when closer than {args.gap:g} dp. Baseline: `{BASELINE}`.",
        "",
        "| case | status | texts | too small text | small text | smallest sp | targets | too small targets | small targets | crowded | new |",
        "|---|---|---|---|---|---|---|---|---|---|---|",
        *table,
        *details,
    ]
    (out / "report.md").write_text("\n".join(report) + "\n", encoding="utf-8")
    print(f"crashed {crashed}, new too-small texts and targets {new_total}; report {out / 'report.md'}", flush=True)
    return 1 if crashed or new_total else 0


def cmd_accept(args):
    out = Path(args.outdir).resolve()
    try:
        limits = json.loads((out / "limits.json").read_text(encoding="utf-8"))
        cases = json.loads((out / "cases.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        sys.exit(f"{out} is not a mobile UI run (no limits.json or cases.json)")
    baseline = load_baseline()
    for case in cases:
        key = case["key"]
        if not (out / "probe" / f"{key}.tsv").exists():
            print(f"  {key}: no probe (crashed in the run), baseline kept")
            continue
        texts, targets, _ = analyse(case, out, limits)
        small = sorted({t["text"] for t in texts if t["sp"] < limits["floor"]})
        bad = sorted({t["id"] for t in targets if t["cls"] == "too small"})
        baseline[key] = {"texts": small, "targets": bad}
        print(f"  {key}: {len(small)} texts, {len(bad)} targets")
    BASELINE.parent.mkdir(parents=True, exist_ok=True)
    BASELINE.write_text(json.dumps(baseline, indent=1, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8")
    print(f"baseline written: {BASELINE}")
    return 0


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="render the phone screens and measure their texts and touch targets")
    r.add_argument("--bin", default=str(ui_regress.DEFAULT_BIN), help="the release razdor binary (with the probes)")
    r.add_argument("--only", help="only the cases whose name contains this text")
    r.add_argument("--out", help="output folder (default: a timestamped one under razdor-scratch/mobile-ui)")
    r.add_argument("--floor", type=float, default=12.0, help="a text is too small under this many sp")
    r.add_argument("--body", type=float, default=14.0, help="a text is small under this many sp")
    r.add_argument("--too-small", type=float, default=40.0, help="a touch target is too small under this many dp (smaller side)")
    r.add_argument("--small", type=float, default=48.0, help="a touch target is small under this many dp (smaller side)")
    r.add_argument("--gap", type=float, default=8.0, help="two targets closer than this many dp are crowded")
    r.set_defaults(func=cmd_run)
    a = sub.add_parser("accept", help="write a run's too-small texts and targets as the baseline")
    a.add_argument("outdir")
    a.set_defaults(func=cmd_accept)
    args = p.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
