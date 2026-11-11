#!/usr/bin/env python3
"""Compares a bench result with a baseline and fails on a performance regression.

  python3 -I tools/perf/compare.py <new result.json> <baseline.json> [--md <table.md>]

Exit codes: 0 no regression; 1 a regression, or a cell that could not be measured; 2 the two are not
comparable (another backend, size or refresh rate; `--force` compares anyway), or nothing in common.

Rules (per cell `scene/view/effects`, present in both files):
  - Frame time (ms) and CPU work (ms, the frame probe) regress at +5%. When both cells are capped at
    the refresh rate, only CPU work counts (the frame time is pinned to the cap there). When a cell is
    uncapped and the other is not, frame time is compared and CPU work too if both have it.
  - RSS and VRAM regress at +10%, load time at +10%.
  - Noise: a change counts only when its medians differ by more than the threshold, by more than the
    absolute floor (RSS 10 MB, VRAM 10 MB, load 0.05 s), and by more than twice the run-to-run spread
    (the larger of the two sides' range of runs). A baseline without its runs (a documented number)
    has no spread: only the new run's spread counts.
  - A change the other way past the same bounds is reported as faster / smaller.
"""

import argparse
import json
import sys

THRESH = {'ms': 5.0, 'work': 5.0, 'rss': 10.0, 'vram': 10.0, 'load': 10.0}
FLOOR = {'rss': 10.0, 'vram': 10.0, 'load': 0.05}
LABEL = {'ms': 'frame ms', 'work': 'CPU ms', 'rss': 'RSS MB', 'vram': 'VRAM MB', 'load': 'load s'}
SINGLE_RUN_BAND = 15.0  # a cell with one run only (a smoke test): changes under this % are noise
BUSY_LOAD1 = 9.0  # 1-minute load on the 12 cores the bench uses; above it the numbers are noisy


def spread(cell, metric):
    runs = cell.get(metric + '_runs') or []
    return max(runs) - min(runs) if len(runs) > 1 else 0.0


def judge(new, base, metric, pct):
    """(verdict, change %) for one metric of one cell; verdict is regression, faster, same or n/a."""
    nv, bv = new.get(metric), base.get(metric)
    if nv is None or bv is None:
        return 'n/a', None
    if new.get('runs') == 1 or base.get('runs') == 1:
        pct = max(pct, SINGLE_RUN_BAND)  # one run has no spread to tell noise from a change
    delta = nv - bv
    change = (delta / bv * 100.0) if bv else (float('inf') if delta else 0.0)
    noise = 2 * max(spread(new, metric), spread(base, metric))
    floor = FLOOR.get(metric, 0.0)
    if change > pct and delta > floor and delta > noise:
        return 'regression', change
    if change < -pct and -delta > floor and -delta > noise:
        return 'faster', change
    return 'same', change


def metrics_for(new, base):
    """Which metrics a cell is judged on."""
    if new.get('capped') and base.get('capped'):
        out = ['work']
    else:
        out = ['ms']
        if new.get('work') is not None and base.get('work') is not None:
            out.append('work')
    out += ['rss', 'vram', 'load']
    return out


def load(path):
    try:
        with open(path, encoding='utf-8') as f:
            return json.load(f)
    except (OSError, ValueError) as e:
        print(f'compare: cannot read {path}: {e}', file=sys.stderr)
        sys.exit(2)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('new')
    ap.add_argument('baseline')
    ap.add_argument('--md', help='also write the table here')
    ap.add_argument('--force', action='store_true', help='compare even if backend, size or refresh differ')
    args = ap.parse_args()
    new, base = load(args.new), load(args.baseline)
    nm, bm = new['meta'], base['meta']
    mismatch = [k for k in ('backend', 'size', 'refresh') if nm.get(k) != bm.get(k)]
    if mismatch and not args.force:
        print(f'compare: not comparable, {", ".join(mismatch)} differ: new {[nm.get(k) for k in mismatch]} '
              f'vs baseline {[bm.get(k) for k in mismatch]} (--force to compare anyway)', file=sys.stderr)
        return 2

    common = [k for k in new['scenes'] if k in base['scenes']]
    if not common:
        print('compare: no cell in common with the baseline', file=sys.stderr)
        return 2
    rows, regressions, faster, failed = [], [], [], []
    for key in common:
        n, b = new['scenes'][key], base['scenes'][key]
        for metric in metrics_for(n, b):
            verdict, change = judge(n, b, metric, THRESH[metric])
            if verdict == 'n/a':
                continue
            rows.append((key, LABEL[metric], b.get(metric), n.get(metric), change, verdict))
            if verdict == 'regression':
                regressions.append(f'{key} {LABEL[metric]}')
            elif verdict == 'faster':
                faster.append(f'{key} {LABEL[metric]}')
    for key in base['scenes']:
        if key not in new['scenes']:
            failed.append(f'{key}: not measured in the new run')
    for key in new.get('errors', {}):
        failed.append(f'{key}: the new run failed')
    for key in new['scenes']:
        if key not in base['scenes']:
            print(f'compare: {key} has no baseline (not compared)', file=sys.stderr)

    def fmt(v):
        return '–' if v is None else f'{v:.2f}'
    lines = [f"# Bench compare: {nm['label']} vs {bm['label']}", '',
             f"Backend {nm['backend']} ({nm.get('machine', '')}), size {nm['size']}, refresh "
             f"{nm['refresh'] or 'uncapped'}; baseline {bm.get('build', '')} ({bm.get('machine', '')}).", '',
             '| cell | metric | baseline | new | change | verdict |', '|---|---|---:|---:|---:|---|']
    for key, label, bv, nv, change, verdict in rows:
        ch = '–' if change is None else f'{change:+.1f}%'
        mark = {'regression': 'REGRESSION', 'faster': 'faster', 'same': 'same'}[verdict]
        lines.append(f'| {key} | {label} | {fmt(bv)} | {fmt(nv)} | {ch} | {mark} |')
    lines.append('')
    lines.append(f'{len(regressions)} regression(s), {len(faster)} faster, {len(common)} cell(s) compared.')
    busy_cells = [k for k in common if (new['scenes'][k].get('load1') or 0) > BUSY_LOAD1
                  or (base['scenes'][k].get('load1') or 0) > BUSY_LOAD1]
    if busy_cells:
        lines.append(f'- warning: the machine was busy (1-minute load over {BUSY_LOAD1:g}) in {len(busy_cells)} '
                     f'cell(s), e.g. {busy_cells[0]}: read those numbers with care, or rerun the bench.')
    if any(new['scenes'][k].get('runs') == 1 for k in common):
        lines.append(f'- note: one run per cell; changes under {SINGLE_RUN_BAND:g}% are not judged.')
    for item in regressions:
        lines.append(f'- regression: {item}')
    for item in failed:
        lines.append(f'- not measured: {item}')
    text = '\n'.join(lines) + '\n'
    print(text)
    if args.md:
        with open(args.md, 'w', encoding='utf-8') as f:
            f.write(text)
    return 1 if regressions or failed else 0


if __name__ == '__main__':
    sys.exit(main())
