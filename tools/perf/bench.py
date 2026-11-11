#!/usr/bin/env python3
"""Razdor performance bench: one binary over the fixed scene matrix, for the release check.

The release skill runs it on the owner's desktop (RTX 5060 Ti, the monitor's GPU) and compares the
result with the last release's baseline (`compare.py`). Stdlib only: run it as
`python3 -I tools/perf/bench.py ...`, pinned like every heavy job (CPUs 8-19 at nice 19; the bench
adds that prefix to the game itself, but pin the bench too).

  # the owner's GPU (a fullscreen window on :0, moved to i3 workspace 6 if asked)
  python3 -I tools/perf/bench.py --bin <razdor> --display :0 --workspace 6 --out <result.json>
  # software GL on an invisible display (numbers relative only)
  python3 -I tools/perf/bench.py --bin <razdor> --xvfb --scenes rk3-start --runs 1 --out <r.json>

Matrix (the same as docs/perf/2026-11-12-atmosphere-performance.md):
  scenes  db-zoomout (Другой берег, explored, zoomed out to v1.4.1's limit), db-wholemap (the whole
          map on screen, RAZDOR_MAP_FIT), rk3-start (РК3-Столица at the start)
  views   flat, 25d (tilted), third (third person), set in the saved settings
  effects off (the game's defaults: no atmosphere), on (sun shadows, clouds, live water, weather,
          the living world, 23:00), set by RAZDOR_* switches
  Each cell runs `--runs` times (3) for `--frames` frames (400); the first `--skip` (100) are dropped.

Per run: frame ms (median of total_ms), CPU work ms (median of work_ms, the frame probe), p90 ms, load
time (to the first frame), peak RSS, VRAM by PID (nvidia-smi, GPU runs only), draws and verts when the
probe has them. Per cell: the median over the runs and the runs' values (their spread is what
compare.py uses against noise). A cell whose median frame rate is within 3% of `--refresh` is
`capped`: the frame time then says nothing and compare.py looks at CPU work only.

The game's own switches (RAZDOR_*) are dropped from the environment; the settings file is written
into a private save dir per run, so the owner's saves and settings are never read or changed.
"""

import argparse
import csv
import datetime
import hashlib
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
PIN = ['nice', '-n', '19', 'taskset', '-c', '8-19']  # the desktop keeps CPUs 0-7 (CLAUDE.local.md)

SCENES = {
    'db-zoomout': {'RAZDOR_SCENE': 'map:Другой берег', 'RAZDOR_SCENE_EXPLORED': '1', 'RAZDOR_MAP_ZOOM': '0.05'},
    'db-wholemap': {'RAZDOR_SCENE': 'map:Другой берег', 'RAZDOR_SCENE_EXPLORED': '1',
                    'RAZDOR_MAP_ZOOM': '0.05', 'RAZDOR_MAP_FIT': '1'},
    'rk3-start': {'RAZDOR_SCENE': 'map:РК3-Столица'},
}
VIEWS = {
    'flat': {'map_tilt': False, 'map_first_person': False},
    '25d': {'map_tilt': True, 'map_first_person': False},
    'third': {'map_tilt': True, 'map_first_person': True},
}
EFFECTS = {
    'off': {},
    'on': {'RAZDOR_LIGHT_HOUR': '23', 'RAZDOR_SHADOWS': 'sun', 'RAZDOR_MIST': 'clouds',
           'RAZDOR_LIVE_WATER': '1', 'RAZDOR_WEATHER': '1', 'RAZDOR_LIVING': '1'},
}
# Every run's settings: these keys, the view's map_* keys, and nothing else (the game's defaults).
BASE_SETTINGS = {
    'display': 'fullscreen', 'show_fps': True, 'frame_cap': 0, 'record_game': False,
    'music_muted': True, 'sfx_muted': True, 'updates': 'off', 'expert_ai': True,
    'map_sprite_size': 1.5, 'map_render_scale': 1.0, 'map_extended': True, 'map_zoom': 1.0,
}
CAP_TOLERANCE = 0.97  # a cell at >= 97% of the refresh rate is capped
SCHEMA = 1


def die(msg, code=2):
    print(f'bench: {msg}', file=sys.stderr)
    sys.exit(code)


def log(msg):
    print(msg, file=sys.stderr, flush=True)


def dt_dir_from(args):
    """The install: --dt-dir, else RAZDOR_DT_DIR from the environment or the repo's .env."""
    if args.dt_dir:
        return args.dt_dir
    if os.environ.get('RAZDOR_DT_DIR'):
        return os.environ['RAZDOR_DT_DIR']
    env_file = os.path.join(REPO, '.env')
    if os.path.exists(env_file):
        for line in open(env_file, encoding='utf-8'):
            m = re.match(r'\s*RAZDOR_DT_DIR\s*=\s*(.*)$', line)
            if m:
                return m.group(1).strip().strip('"').strip("'")
    die('no install: pass --dt-dir, or set RAZDOR_DT_DIR (environment or .env)')


def git_commit():
    try:
        return subprocess.run(['git', '-C', REPO, 'rev-parse', '--short', 'HEAD'],
                              capture_output=True, text=True, check=True).stdout.strip()
    except Exception:
        return 'unknown'


def sha256_12(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            h.update(chunk)
    return h.hexdigest()[:12]


def pids_of(binary):
    """PIDs whose executable is this file (the game, not the xvfb-run wrapper)."""
    want = os.stat(binary)
    found = []
    for p in os.listdir('/proc'):
        if not p.isdigit():
            continue
        try:
            st = os.stat(f'/proc/{p}/exe')
        except OSError:
            continue
        if (st.st_dev, st.st_ino) == (want.st_dev, want.st_ino):
            found.append(int(p))
    return found


def rss_mb(pid):
    try:
        for line in open(f'/proc/{pid}/status'):
            if line.startswith('VmRSS'):
                return int(line.split()[1]) / 1024
    except OSError:
        pass
    return None


def vram_mb(pid):
    """VRAM of the PID from the nvidia-smi process table (all GPUs), None without NVIDIA."""
    try:
        out = subprocess.run(['nvidia-smi'], capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    total = None
    for line in out.splitlines():
        if re.search(rf'\b{pid}\b', line):
            m = re.findall(r'(\d+)MiB', line)
            if m:
                total = (total or 0) + int(m[-1])
    return total


def move_to_workspace(pid, ws):
    r = subprocess.run(['i3-msg', f'[pid={pid}] move container to workspace number {ws}'],
                       capture_output=True, text=True)
    try:
        return all(x.get('success') for x in json.loads(r.stdout))
    except (ValueError, AttributeError, TypeError):
        return False


def settings_for(view):
    s = dict(BASE_SETTINGS)
    s.update(VIEWS[view])
    return s


def run_once(args, binary, dt_dir, scene, view, effect, rep, run_dir):
    """One run: returns the run's numbers, or raises RuntimeError with the reason."""
    load1 = os.getloadavg()[0]  # other work on the machine shows here (compare.py warns)
    if os.path.isdir(run_dir):
        shutil.rmtree(run_dir)
    os.makedirs(os.path.join(run_dir, 'saves'))
    with open(os.path.join(run_dir, 'saves', 'audio.json'), 'w', encoding='utf-8') as f:
        json.dump(settings_for(view), f)
    w, h = args.size.split('x')
    csv_path = os.path.join(run_dir, 'frames.csv')
    log_path = os.path.join(run_dir, 'razdor.log')
    env = {k: v for k, v in os.environ.items() if not k.startswith('RAZDOR_')}
    env.pop('DISPLAY', None)
    env.update({
        '__GL_SYNC_TO_VBLANK': '0', 'vblank_mode': '0',  # vsync off: the frame rate is what the GPU allows
        'RAZDOR_DT_DIR': dt_dir, 'RAZDOR_LOG': log_path, 'RAZDOR_SAVE_DIR': os.path.join(run_dir, 'saves'),
        'RAZDOR_SIZE': args.size, 'RAZDOR_SNAPSHOT': os.path.join(run_dir, 'shot.png'),
        'RAZDOR_SNAPSHOT_FRAMES': str(args.frames), 'RAZDOR_PROFILE': '1',
        'RAZDOR_PROFILE_FRAMES': csv_path, 'RAZDOR_SCENE_QUIET': '1', 'RAZDOR_NO_AUDIO': '1',
    })
    if args.display:
        env['DISPLAY'] = args.display
    env.update(SCENES[scene])
    env.update(EFFECTS[effect])
    cmd = PIN + [binary]
    if args.xvfb:
        cmd = PIN + ['xvfb-run', '-a', '-s', f'-screen 0 {w}x{h}x24'] + [binary]
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    rss = vram = 0.0
    seen_vram = False
    moved = not args.workspace
    started = time.monotonic()
    game_pid = None
    while proc.poll() is None:
        time.sleep(0.2)
        if time.monotonic() - started > args.run_timeout:
            proc.kill()
            proc.wait()
            raise RuntimeError(f'timed out after {args.run_timeout} s')
        pids = pids_of(binary)
        if not pids:
            continue
        game_pid = pids[0]
        r = rss_mb(game_pid)
        if r is not None:
            rss = max(rss, r)
        v = vram_mb(game_pid) if args.display else None
        if v is not None:
            seen_vram = True
            vram = max(vram, v)
        if not moved and time.monotonic() - started > 0.5:
            moved = move_to_workspace(game_pid, args.workspace)
            if not moved and time.monotonic() - started > 8:
                log(f'bench: could not move the window to workspace {args.workspace} (i3-msg)')
                moved = True
    if proc.returncode not in (0, None):
        raise RuntimeError(f'exit code {proc.returncode}')
    rows = []
    if os.path.exists(csv_path):
        rows = [r for r in csv.DictReader(open(csv_path, encoding='utf-8'))
                if int(r['frame']) > args.skip]
    if not rows:
        raise RuntimeError('no frame rows (the game did not run the frames)')
    totals = sorted(float(r['total_ms']) for r in rows)
    result = {
        'ms': statistics.median(totals),
        'work': statistics.median(float(r['work_ms']) for r in rows),
        'p90': totals[min(len(totals) - 1, int(0.9 * (len(totals) - 1) + 0.5))],
        'load1': load1,
        'load': None,
        'rss': rss,
        'vram': vram if seen_vram else None,
        'draws': None,
        'verts': None,
    }
    for key in ('draws', 'verts'):
        vals = [float(r[key]) for r in rows if r.get(key) not in (None, '')]
        if vals:
            result[key] = statistics.median(vals)
    if os.path.exists(log_path):
        for line in open(log_path, encoding='utf-8', errors='replace'):
            m = re.match(r'\[\s*([\d.]+)\] -- first frame shown', line)
            if m:
                result['load'] = float(m.group(1))
    return result


def summarise(runs, refresh):
    cell = {}
    for key in ('ms', 'work', 'p90', 'load', 'rss', 'vram', 'draws', 'verts', 'load1'):
        vals = [r[key] for r in runs if r.get(key) is not None]
        cell[key] = round(statistics.median(vals), 3) if vals else None
        cell[key + '_runs'] = [round(v, 3) for v in vals]
    cell['fps'] = round(1000 / cell['ms'], 1)
    cell['capped'] = bool(refresh) and cell['fps'] >= CAP_TOLERANCE * refresh
    cell['runs'] = len(runs)
    return cell


def markdown(result):
    m = result['meta']
    lines = [
        f"# Razdor bench: {m['label']}", '',
        f"Build `{m['build']}` (commit {m['commit']}), backend **{m['backend']}**, size {m['size']}, "
        f"refresh {m['refresh'] or 'uncapped'}, {m['runs']} run(s) of {m['frames']} frames "
        f"(first {m['skip']} dropped), {m['date']}.", '',
        '| scene | view | effects | FPS | frame ms | CPU ms | p90 ms | load s | RSS MB | VRAM MB | draws | capped |',
        '|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|:---:|',
    ]

    def f(v, nd=2):
        return '–' if v is None else f'{v:.{nd}f}'
    for key, c in result['scenes'].items():
        scene, view, effect = key.split('/')
        lines.append(f"| {scene} | {view} | {effect} | {f(c['fps'], 0)} | {f(c['ms'])} | {f(c['work'])} | "
                     f"{f(c['p90'])} | {f(c['load'])} | {f(c['rss'], 0)} | {f(c['vram'], 0)} | "
                     f"{f(c['draws'], 0)} | {'yes' if c['capped'] else ''} |")
    for key, err in result.get('errors', {}).items():
        lines.append(f'\nFAILED {key}: {err}')
    return '\n'.join(lines) + '\n'


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--bin', required=True, help='the razdor binary to measure')
    back = ap.add_mutually_exclusive_group(required=True)
    back.add_argument('--display', metavar=':N', help='run on this display (the owner: :0)')
    back.add_argument('--xvfb', action='store_true', help='software GL on an invisible Xvfb (relative numbers)')
    back.add_argument('--egl-4060', action='store_true',
                      help='headless RTX 4060 (not available: Razdor has no headless EGL device surface yet)')
    ap.add_argument('--workspace', type=int, help='i3 workspace to move the game window to (display runs)')
    ap.add_argument('--out', required=True, help='the result JSON to write')
    ap.add_argument('--md', help='also write the Markdown table here (default: printed to stdout)')
    ap.add_argument('--label', default='', help='a name for the run (in the JSON and the table)')
    ap.add_argument('--commit', default='unknown',
                    help='the git commit the binary was built from (the release passes `git rev-parse --short HEAD`)')
    ap.add_argument('--machine', default='unlabelled', help='the machine, for the baseline file')
    ap.add_argument('--dt-dir', help='the install (default: RAZDOR_DT_DIR)')
    ap.add_argument('--size', help='render size (default: 2560x1440 on a display, 1280x800 on Xvfb)')
    ap.add_argument('--refresh', type=int, help='the monitor rate for the cap check (default 180 on a display, 0 on Xvfb)')
    ap.add_argument('--scenes', default=','.join(SCENES), help='comma list, default all')
    ap.add_argument('--views', default=','.join(VIEWS), help='comma list, default all')
    ap.add_argument('--effects', default=','.join(EFFECTS), help='comma list, default off,on')
    ap.add_argument('--runs', type=int, default=3)
    ap.add_argument('--frames', type=int, default=400)
    ap.add_argument('--skip', type=int, default=100)
    ap.add_argument('--run-timeout', type=int, default=600, help='seconds per run')
    ap.add_argument('--work', help='scratch folder for the run dirs (default: target/perf-bench)')
    ap.add_argument('--keep', action='store_true', help='keep the run folders (frame CSVs, logs)')
    args = ap.parse_args()

    if args.egl_4060:
        die('--egl-4060 is not available: Razdor has no headless EGL device surface yet '
            '(the trailer work needs one); use --display or --xvfb', code=3)
    binary = os.path.abspath(args.bin)
    if not os.access(binary, os.X_OK):
        die(f'not an executable: {args.bin}')
    if args.xvfb and not shutil.which('xvfb-run'):
        die('xvfb-run not found')
    if args.workspace and not shutil.which('i3-msg'):
        die('--workspace needs i3-msg')
    if args.display and not args.size:
        args.size = '2560x1440'
    if args.xvfb and not args.size:
        args.size = '1280x800'
    if args.refresh is None:
        args.refresh = 180 if args.display else 0
    dt_dir = dt_dir_from(args)
    if not os.path.isdir(dt_dir):
        die(f'the install is not a folder: {dt_dir}')
    scenes = [s for s in args.scenes.split(',') if s]
    views = [v for v in args.views.split(',') if v]
    effects = [e for e in args.effects.split(',') if e]
    for name, allowed in (('scene', (scenes, SCENES)), ('view', (views, VIEWS)), ('effect', (effects, EFFECTS))):
        for x in allowed[0]:
            if x not in allowed[1]:
                die(f'unknown {name}: {x} (one of {", ".join(allowed[1])})')

    work = args.work or os.path.join(REPO, 'target', 'perf-bench')
    label = args.label or os.path.basename(binary)
    base = os.path.join(work, re.sub(r'[^A-Za-z0-9_.-]+', '_', label))
    os.makedirs(base, exist_ok=True)
    result = {
        'schema': SCHEMA,
        'meta': {
            'label': label, 'machine': args.machine,
            'build': f'{os.path.basename(binary)} sha256:{sha256_12(binary)}', 'commit': args.commit,
            'bench_tool': git_commit(),
            'backend': 'display' if args.display else ('xvfb' if args.xvfb else 'egl-4060'),
            'display': args.display or '', 'size': args.size, 'refresh': args.refresh,
            'workspace': args.workspace, 'runs': args.runs, 'frames': args.frames, 'skip': args.skip,
            'host_cpu': platform.machine(), 'date': datetime.date.today().isoformat(),
        },
        'scenes': {}, 'errors': {},
    }
    log(f'bench: {label}: {len(scenes) * len(views) * len(effects)} cells x {args.runs} runs')
    for scene in scenes:
        for view in views:
            for effect in effects:
                key = f'{scene}/{view}/{effect}'
                runs = []
                for rep in range(args.runs):
                    run_dir = os.path.join(base, f'{scene}-{view}-{effect}-{rep}')
                    log(f'bench: {key} run {rep + 1}/{args.runs}')
                    try:
                        runs.append(run_once(args, binary, dt_dir, scene, view, effect, rep, run_dir))
                    except RuntimeError as e:
                        result['errors'][f'{key}#{rep}'] = str(e)
                        log(f'bench: {key} run {rep + 1} failed: {e}')
                if runs:
                    result['scenes'][key] = summarise(runs, args.refresh)
                    c = result['scenes'][key]
                    log(f"bench: {key}: {c['fps']} fps, {c['ms']} ms, work {c['work']} ms")
    if not args.keep:
        shutil.rmtree(base, ignore_errors=True)
    with open(args.out, 'w', encoding='utf-8') as f:
        json.dump(result, f, ensure_ascii=False, indent=2)
        f.write('\n')
    text = markdown(result)
    if args.md:
        with open(args.md, 'w', encoding='utf-8') as f:
            f.write(text)
    else:
        print(text)
    return 1 if result['errors'] else 0


if __name__ == '__main__':
    sys.exit(main())
