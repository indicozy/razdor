#!/usr/bin/env python3
"""Razdor performance bench: one binary over the fixed scene matrix, for the release check.

The release skill runs it headless on the RTX 4060 (`--gpu-4060`, the release default): a private
Xvfb, the game drawn by VirtualGL on the 4060's EGL device, uncapped, about 10 minutes, no window and
no owner needed. `compare.py` then judges the result against the last release's baseline. The owner's
monitor (`--display`) is an optional extra (its 180 Hz cap hides close-up costs); `--xvfb` is software
GL, relative numbers only. Stdlib only: run it as `python3 -I tools/perf/bench.py ...`, pinned like every
heavy job (CPUs 8-19 at nice 19; the bench adds that prefix to the game itself, but pin the bench too).

  # the release check: the RTX 4060, headless, uncapped
  nice -n 19 taskset -c 8-19 python3 -I tools/perf/bench.py --bin <razdor> --gpu-4060 --commit <id> \
    --label X.Y.Z --out <result.json> --md <table.md>
  # the owner's GPU (a fullscreen window on :0, moved to i3 workspace 6 if asked; optional extra)
  python3 -I tools/perf/bench.py --bin <razdor> --display :0 --workspace 6 --out <result.json>
  # software GL on an invisible display (numbers relative only)
  python3 -I tools/perf/bench.py --bin <razdor> --xvfb --scenes rk3-start --runs 1 --out <r.json>

Matrix (the same scenes as docs/perf/2026-11-12-atmosphere-performance.md):
  scenes  db-zoomout (Другой берег, explored, zoomed out to v1.4.1's limit), db-wholemap (the whole
          map on screen, RAZDOR_MAP_FIT), rk3-start (РК3-Столица at the start)
  views   flat, 25d (tilted), third (third person), set in the saved settings
  effects off (the game's defaults: no atmosphere), on (sun shadows, clouds, live water, weather, the
          living world, 23:00), set by RAZDOR_* switches
  --gpu-4060 runs GPU_MATRIX (six cells: rk3-start flat and 25d off; db-zoomout 25d and third, off and
          on), 3 runs each. A run lasts --seconds (25) after its first frame and its first
          --skip-seconds (5) of frames are dropped (the frame log is written every 5 s, so about 20 s
          are kept). The other backends run each cell --runs (3) times for --frames (400) frames, the
          first --skip (100) dropped.

Per run: frame ms (median of total_ms), CPU work ms (median of work_ms, the frame probe), p90 ms, load
time (to the first frame), peak RSS, VRAM by PID (nvidia-smi, GPU runs), draws and verts when the probe
has them, and the renderer (GPU runs: the game's own `window open` line). Per cell: the median over the
runs and the runs' values (their spread is what compare.py uses against noise). A cell whose median frame
rate is within 3% of `--refresh` is `capped`: the frame time then says nothing and compare.py looks at CPU
work only. The GPU backend is uncapped (refresh 0).

The game's own switches (RAZDOR_*) are dropped from the environment; the settings file is written
into a private save dir per run, so the owner's saves and settings are never read or changed.

Exit codes: 0 all cells measured; 1 a run failed (the table says which); 2 usage or a missing install or
binary; 3 the GPU is not the RTX 4060 (device check) or the renderer is not NVIDIA (no run is made).
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
import signal
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
CAP_TOLERANCE = 0.97  # a cell within 97%..103% of the refresh rate is capped (held by vsync)
SCHEMA = 1

# The headless RTX 4060 (--gpu-4060). The card is checked against its PCI address and name first.
GPU_BACKEND = 'gpu-4060'
GPU_DEVICE = '/dev/dri/card2'  # VirtualGL's EGL device; the 5060 Ti is card3 (never used here)
GPU_PCI = '0000:05:00.0'
GPU_NAME = 'RTX 4060'
GPU_MATRIX = [  # the six cells of the release check (scene, view, effects)
    ('rk3-start', 'flat', 'off'), ('rk3-start', '25d', 'off'),
    ('db-zoomout', '25d', 'off'), ('db-zoomout', '25d', 'on'),
    ('db-zoomout', 'third', 'off'), ('db-zoomout', 'third', 'on'),
]
GPU_SIZE = '1024x768'
GPU_SECONDS = 25.0       # wall time after the first frame; the frame log is flushed every 5 s
GPU_SKIP_SECONDS = 5.0   # frames dropped from the start of each run
GPU_DISPLAYS = range(90, 100)  # the private Xvfb's display numbers (never :0)
# VirtualGL as the proven recipe runs it (tools/perf, docs/trailer-shots.md): VGL_REFRESHRATE this high
# means VirtualGL does not pace the frames, so the game runs uncapped.
VGL_ENV = {'LD_PRELOAD': '/usr/lib64/libGL.so.1:/usr/lib64/libEGL.so.1', 'VGL_REFRESHRATE': '100000'}


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


def vram_mb(pid, gpu_index=None):
    """VRAM of the PID from the nvidia-smi process table, None without NVIDIA. With `gpu_index` only
    that GPU's rows count (the GPU runs); without it, every GPU's."""
    try:
        out = subprocess.run(['nvidia-smi'], capture_output=True, text=True, timeout=10).stdout
    except (OSError, subprocess.TimeoutExpired):
        return None
    total = None
    for line in out.splitlines():
        m = re.match(r'\|\s*(\d+)\s', line)  # a GPU's row of the process table starts with its index
        if not m or (gpu_index is not None and int(m.group(1)) != gpu_index):
            continue
        if str(pid) not in line.split():
            continue
        mem = re.findall(r'(\d+)MiB', line)
        if mem:
            total = (total or 0) + int(mem[-1])
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


def prepare_run_dir(run_dir, view):
    """A fresh run folder with the run's settings file (the private save dir)."""
    if os.path.isdir(run_dir):
        shutil.rmtree(run_dir)
    os.makedirs(os.path.join(run_dir, 'saves'))
    with open(os.path.join(run_dir, 'saves', 'audio.json'), 'w', encoding='utf-8') as f:
        json.dump(settings_for(view), f)


def game_env(args, dt_dir, scene, effect, run_dir, snapshot):
    """The game's environment: the RAZDOR_* switches of the cell, the run's private folders. `snapshot`
    adds the frame-count snapshot that ends the display and Xvfb runs (the GPU runs end on the clock)."""
    csv_path = os.path.join(run_dir, 'frames.csv')
    log_path = os.path.join(run_dir, 'razdor.log')
    env = {k: v for k, v in os.environ.items() if not k.startswith('RAZDOR_')}
    env.pop('DISPLAY', None)
    env.update({
        '__GL_SYNC_TO_VBLANK': '0', 'vblank_mode': '0',  # vsync off: the frame rate is what the GPU allows
        'RAZDOR_DT_DIR': dt_dir, 'RAZDOR_LOG': log_path, 'RAZDOR_SAVE_DIR': os.path.join(run_dir, 'saves'),
        'RAZDOR_SIZE': args.size, 'RAZDOR_PROFILE': '1',
        'RAZDOR_PROFILE_FRAMES': csv_path, 'RAZDOR_SCENE_QUIET': '1', 'RAZDOR_NO_AUDIO': '1',
    })
    if snapshot:
        env.update({'RAZDOR_SNAPSHOT': os.path.join(run_dir, 'shot.png'),
                    'RAZDOR_SNAPSHOT_FRAMES': str(args.frames)})
    env.update(SCENES[scene])
    env.update(EFFECTS[effect])
    return env, csv_path, log_path


def has_atmosphere(binary):
    """Whether the binary knows the atmosphere switches (the effects-on cells need them). A release
    before the atmosphere work (v1.4.1) does not: its «on» cells run as «off»."""
    with open(binary, 'rb') as f:
        return b'RAZDOR_SHADOWS' in f.read()


def read_text(path):
    if not os.path.exists(path):
        return ''
    return open(path, encoding='utf-8', errors='replace').read()


def read_rows(csv_path):
    """The frame rows. A row cut short (the game stopped mid-write) is dropped, not parsed."""
    if not os.path.exists(csv_path):
        return []
    rows = []
    for r in csv.DictReader(open(csv_path, encoding='utf-8', errors='replace')):
        if None in r or None in r.values() or not all(r.get(k) for k in ('frame', 'total_ms', 'work_ms')):
            continue
        rows.append(r)
    return rows


def frame_result(rows, load1, rss, vram, log_text):
    """One run's numbers from its frame rows (the medians, p90, load time, memory)."""
    totals = sorted(float(r['total_ms']) for r in rows)
    result = {
        'ms': statistics.median(totals),
        'work': statistics.median(float(r['work_ms']) for r in rows),
        'p90': totals[min(len(totals) - 1, int(0.9 * (len(totals) - 1) + 0.5))],
        'load1': load1,
        'load': None,
        'rss': rss,
        'vram': vram,
        'draws': None,
        'verts': None,
        'renderer': None,
    }
    for key in ('draws', 'verts'):
        vals = [float(r[key]) for r in rows if r.get(key) not in (None, '')]
        if vals:
            result[key] = statistics.median(vals)
    for line in log_text.splitlines():
        m = re.match(r'\[\s*([\d.]+)\] -- first frame shown', line)
        if m:
            result['load'] = float(m.group(1))
    return result


def run_once(args, binary, dt_dir, scene, view, effect, rep, run_dir):
    """One display or Xvfb run: returns the run's numbers, or raises RuntimeError with the reason."""
    load1 = os.getloadavg()[0]  # other work on the machine shows here (compare.py warns)
    prepare_run_dir(run_dir, view)
    w, h = args.size.split('x')
    env, csv_path, log_path = game_env(args, dt_dir, scene, effect, run_dir, snapshot=True)
    if args.display:
        env['DISPLAY'] = args.display
    cmd = PIN + [binary]
    if args.xvfb:
        cmd = PIN + ['xvfb-run', '-a', '-s', f'-screen 0 {w}x{h}x24'] + [binary]
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    rss = vram = None  # None until the game's process is seen (no RSS is not 0 MB)
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
            rss = r if rss is None else max(rss, r)
        v = vram_mb(game_pid) if args.display else None
        if v is not None:
            seen_vram = True
            vram = v if vram is None else max(vram, v)
        if not moved and time.monotonic() - started > 0.5:
            moved = move_to_workspace(game_pid, args.workspace)
            if not moved and time.monotonic() - started > 8:
                log(f'bench: could not move the window to workspace {args.workspace} (i3-msg)')
                moved = True
    if proc.returncode not in (0, None):
        raise RuntimeError(f'exit code {proc.returncode}')
    rows = [r for r in read_rows(csv_path) if int(r['frame']) > args.skip]
    if not rows:
        raise RuntimeError('no frame rows (the game did not run the frames)')
    return frame_result(rows, load1, rss, vram if seen_vram else None, read_text(log_path))


def gpu_index_for_device():
    """The nvidia-smi index of the RTX 4060, after checking that GPU_DEVICE is that card (PCI
    GPU_PCI) and that nvidia-smi names it. Exits 3 when it is not."""
    card = os.path.basename(GPU_DEVICE)
    if not os.path.exists(GPU_DEVICE):
        die(f'{GPU_DEVICE} does not exist: VirtualGL needs the RTX 4060 render device', code=3)
    try:
        pci = os.path.basename(os.path.realpath(f'/sys/class/drm/{card}/device'))
    except OSError:
        pci = ''
    if pci != GPU_PCI:
        die(f'{GPU_DEVICE} is PCI {pci or "unknown"}, not the RTX 4060 ({GPU_PCI}); '
            f'check /sys/class/drm/{card}/device and set GPU_DEVICE', code=3)
    try:
        out = subprocess.run(['nvidia-smi', '--query-gpu=index,pci.bus_id,name', '--format=csv,noheader'],
                             capture_output=True, text=True, timeout=20, check=True).stdout
    except (OSError, subprocess.SubprocessError):
        die('nvidia-smi failed: cannot name the GPU', code=3)
    for line in out.splitlines():
        parts = [p.strip() for p in line.split(',')]
        if len(parts) == 3 and parts[1].lower().endswith(':' + GPU_PCI.split(':', 1)[1].lower()):
            if GPU_NAME not in parts[2]:
                die(f'PCI {GPU_PCI} is {parts[2]!r}, not the {GPU_NAME}', code=3)
            return int(parts[0])
    die(f'nvidia-smi does not list the GPU at PCI {GPU_PCI}', code=3)


def free_display():
    """A display number in GPU_DISPLAYS with no server and no lock file (never :0)."""
    for n in GPU_DISPLAYS:
        if not os.path.exists(f'/tmp/.X11-unix/X{n}') and not os.path.exists(f'/tmp/.X{n}-lock'):
            return n
    raise RuntimeError(f'no free display in :{GPU_DISPLAYS[0]}-:{GPU_DISPLAYS[-1]} for the private Xvfb')


def stop_group(proc):
    """Stops a process and its group: SIGTERM, then SIGKILL after 10 s."""
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()


def start_xvfb(number, size, xdg):
    """A private Xvfb on `:number` with a screen of `size`, the same size as the game's window."""
    w, h = size.split('x')
    env = {k: v for k, v in os.environ.items() if not k.startswith('RAZDOR_')}
    env.pop('DISPLAY', None)
    env['XDG_RUNTIME_DIR'] = xdg
    proc = subprocess.Popen(PIN + ['Xvfb', f':{number}', '-screen', '0', f'{w}x{h}x24', '+extension', 'GLX',
                                   '-nolisten', 'tcp', '-noreset'],
                            env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    sock = f'/tmp/.X11-unix/X{number}'
    t0 = time.monotonic()
    while not os.path.exists(sock):
        if proc.poll() is not None or time.monotonic() - t0 > 20:
            stop_group(proc)
            raise RuntimeError(f'Xvfb :{number} did not start')
        time.sleep(0.1)
    return proc


def gpu_env_base(display, xdg):
    env = {k: v for k, v in os.environ.items() if not k.startswith('RAZDOR_')}
    env.pop('DISPLAY', None)
    env.update({'DISPLAY': f':{display}', 'XDG_RUNTIME_DIR': xdg, **VGL_ENV})
    return env


def gpu_renderer(display, xdg):
    """The renderer string VirtualGL gives on the 4060's device (`glxinfo -B`), exits 3 when not NVIDIA."""
    try:
        out = subprocess.run(PIN + ['vglrun', '-d', GPU_DEVICE, 'glxinfo', '-B'], env=gpu_env_base(display, xdg),
                             capture_output=True, text=True, timeout=120).stdout
    except (OSError, subprocess.TimeoutExpired) as e:
        die(f'vglrun glxinfo failed: {e}', code=3)
    m = re.search(r'OpenGL renderer string: (.*)', out)
    renderer = m.group(1).strip() if m else ''
    if 'NVIDIA' not in renderer:
        die(f'the renderer is {renderer or "unknown"!r}, not NVIDIA: the frames would not come from the RTX 4060',
            code=3)
    return renderer


def gpu_run_once(args, binary, dt_dir, scene, view, effect, run_dir, display, xdg, gpu_index):
    """One uncapped run on the RTX 4060: `--seconds` after the first frame, then the game is stopped."""
    load1 = os.getloadavg()[0]
    prepare_run_dir(run_dir, view)
    env, csv_path, log_path = game_env(args, dt_dir, scene, effect, run_dir, snapshot=False)
    env.update(gpu_env_base(display, xdg))
    proc = subprocess.Popen(PIN + ['vglrun', '-d', GPU_DEVICE, binary], env=env,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    rss = vram = None  # None until the game's process is seen (no RSS is not 0 MB)
    seen_vram = False
    started = time.monotonic()
    first = None  # the monotonic time the game logged its first frame
    stopped = False
    last_vram = 0.0
    while proc.poll() is None:
        time.sleep(0.2)
        now = time.monotonic()
        if now - started > args.run_timeout:
            stop_group(proc)
            raise RuntimeError(f'timed out after {args.run_timeout} s')
        if first is None and 'first frame shown' in read_text(log_path):
            first = now
        if first is not None and not stopped and now - first >= args.seconds:
            stop_group(proc)
            stopped = True
            break
        pids = pids_of(binary)
        if not pids:
            continue
        r = rss_mb(pids[0])
        if r is not None:
            rss = r if rss is None else max(rss, r)
        # VRAM once per run, half way through: polling nvidia-smi every second during uncapped runs hung the
        # RTX 4060's GSP firmware (Xid 119, 2026-11-12), which then needed a reset.
        if not seen_vram and first is not None and now - first >= args.seconds / 2 and last_vram == 0.0:
            last_vram = now
            v = vram_mb(pids[0], gpu_index)
            if v is not None:
                seen_vram = True
                vram = v if vram is None else max(vram, v)
    if not stopped:
        if proc.returncode != 0:
            raise RuntimeError(f'exit code {proc.returncode}')
        raise RuntimeError('the game quit before the run was over')
    log_text = read_text(log_path)
    m = re.search(r'window open: (.*)', log_text)
    window = m.group(1).strip() if m else ''
    if 'NVIDIA' not in window:
        raise RuntimeError(f'the game drew on {window or "no window"}, not the NVIDIA GPU')
    rows = read_rows(csv_path)
    kept, acc = [], 0.0
    for r in rows:  # the first --skip-seconds of frames go (by frame time, not by frame count)
        if acc >= args.skip_seconds * 1000.0:
            kept.append(r)
        acc += float(r['total_ms'])
    if not kept:
        raise RuntimeError(f'no frame rows after the first {args.skip_seconds:g} s')
    result = frame_result(kept, load1, rss, vram if seen_vram else None, log_text)
    result['renderer'] = window
    return result


def summarise(runs, refresh):
    cell = {}
    for key in ('ms', 'work', 'p90', 'load', 'rss', 'vram', 'draws', 'verts', 'load1'):
        vals = [r[key] for r in runs if r.get(key) is not None]
        cell[key] = round(statistics.median(vals), 3) if vals else None
        cell[key + '_runs'] = [round(v, 3) for v in vals]
    cell['fps'] = round(1000 / cell['ms'], 1)
    cell['capped'] = bool(refresh) and CAP_TOLERANCE * refresh <= cell['fps'] <= refresh / CAP_TOLERANCE
    cell['runs'] = len(runs)
    cell['renderer'] = next((r['renderer'] for r in runs if r.get('renderer')), None)
    return cell


def markdown(result):
    m = result['meta']
    if m['backend'] == GPU_BACKEND:
        how = f"{m['runs']} run(s) of {m['seconds']:g} s (first {m['skip_seconds']:g} s dropped)"
    else:
        how = f"{m['runs']} run(s) of {m['frames']} frames (first {m['skip']} dropped)"
    lines = [
        f"# Razdor bench: {m['label']}", '',
        f"Build `{m['build']}` (commit {m['commit']}), backend **{m['backend']}**, size {m['size']}, "
        f"refresh {m['refresh'] or 'uncapped'}, {how}, {m['date']}.",
    ]
    if m.get('renderer'):
        lines.append(f"Renderer `{m['renderer']}`.")
    if m.get('atmosphere_switches') is False and any(k.endswith('/on') for k in result['scenes']):
        lines.append('Note: this build has no atmosphere switches (RAZDOR_SHADOWS and the rest): its «on» cells '
                     'ran as «off» and say nothing about the effects.')
    lines += [
        '',
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


def run_cells(args, cells, base, result, once):
    """Runs every cell `args.runs` times with `once(scene, view, effect, rep, run_dir)` and records it."""
    for scene, view, effect in cells:
        key = f'{scene}/{view}/{effect}'
        runs = []
        for rep in range(args.runs):
            run_dir = os.path.join(base, f'{scene}-{view}-{effect}-{rep}')
            log(f'bench: {key} run {rep + 1}/{args.runs}')
            try:
                runs.append(once(scene, view, effect, rep, run_dir))
            except RuntimeError as e:
                result['errors'][f'{key}#{rep}'] = str(e)
                log(f'bench: {key} run {rep + 1} failed: {e}')
        if runs:
            result['scenes'][key] = summarise(runs, args.refresh)
            c = result['scenes'][key]
            log(f"bench: {key}: {c['fps']} fps, {c['ms']} ms, work {c['work']} ms")


def run_gpu_matrix(args, binary, dt_dir, cells, base, result):
    """The RTX 4060 matrix: a private Xvfb, the renderer check, the cells, and the Xvfb stopped at the end."""
    gpu_index = gpu_index_for_device()
    xdg = os.path.join(base, 'xdg')
    shutil.rmtree(xdg, ignore_errors=True)
    os.makedirs(xdg, mode=0o700)
    try:
        display = free_display()
    except RuntimeError as e:
        die(str(e), code=2)
    xvfb = None
    try:
        try:
            xvfb = start_xvfb(display, args.size, xdg)
        except RuntimeError as e:
            die(str(e), code=2)
        result['meta']['renderer'] = gpu_renderer(display, xdg)
        result['meta']['display'] = f':{display}'
        log(f"bench: renderer {result['meta']['renderer']} on :{display}")
        run_cells(args, cells, base, result,
                  lambda scene, view, effect, rep, run_dir: gpu_run_once(
                      args, binary, dt_dir, scene, view, effect, run_dir, display, xdg, gpu_index))
    finally:
        if xvfb is not None:
            stop_group(xvfb)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--bin', required=True, help='the razdor binary to measure')
    back = ap.add_mutually_exclusive_group(required=True)
    back.add_argument('--display', metavar=':N', help='run on this display (the owner: :0)')
    back.add_argument('--xvfb', action='store_true', help='software GL on an invisible Xvfb (relative numbers)')
    back.add_argument('--gpu-4060', '--egl-4060', dest='gpu', action='store_true',
                      help='the RTX 4060, headless: a private Xvfb, VirtualGL, uncapped (the release check)')
    ap.add_argument('--workspace', type=int, help='i3 workspace to move the game window to (display runs)')
    ap.add_argument('--out', required=True, help='the result JSON to write')
    ap.add_argument('--md', help='also write the Markdown table here (default: printed to stdout)')
    ap.add_argument('--label', default='', help='a name for the run (in the JSON and the table)')
    ap.add_argument('--commit', default='unknown',
                    help='the git commit the binary was built from (the release passes `git rev-parse --short HEAD`)')
    ap.add_argument('--machine', default='unlabelled', help='the machine, for the baseline file')
    ap.add_argument('--dt-dir', help='the install (default: RAZDOR_DT_DIR)')
    ap.add_argument('--size', help='render size (default: 2560x1440 on a display, 1280x800 on Xvfb, 1024x768 on the 4060)')
    ap.add_argument('--refresh', type=int, help='the monitor rate for the cap check (default 180 on a display, 0 otherwise)')
    ap.add_argument('--scenes', default=','.join(SCENES), help='comma list, default all')
    ap.add_argument('--views', default=','.join(VIEWS), help='comma list, default all')
    ap.add_argument('--effects', default=','.join(EFFECTS), help='comma list, default off,on')
    ap.add_argument('--runs', type=int, default=3)
    ap.add_argument('--frames', type=int, default=400, help='frames per run (not the 4060 backend)')
    ap.add_argument('--skip', type=int, default=100, help='frames dropped per run (not the 4060 backend)')
    ap.add_argument('--seconds', type=float, default=GPU_SECONDS, help='the 4060 backend: seconds per run after the first frame')
    ap.add_argument('--skip-seconds', type=float, default=GPU_SKIP_SECONDS,
                    help='the 4060 backend: seconds of frames dropped per run')
    ap.add_argument('--run-timeout', type=int, default=600, help='seconds per run')
    ap.add_argument('--work', help='scratch folder for the run dirs (default: target/perf-bench)')
    ap.add_argument('--keep', action='store_true', help='keep the run folders (frame CSVs, logs)')
    args = ap.parse_args()

    binary = os.path.abspath(args.bin)
    if not os.access(binary, os.X_OK):
        die(f'not an executable: {args.bin}')
    if args.gpu:
        if args.workspace:
            die('--workspace is for the owner\'s display run, not the 4060 backend')
        if args.refresh:
            die('the 4060 backend is uncapped: no --refresh')
        if not args.size:
            args.size = GPU_SIZE
        args.refresh = 0
    else:
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
    if args.gpu:
        cells = [c for c in GPU_MATRIX if c[0] in scenes and c[1] in views and c[2] in effects]
    else:
        cells = [(s, v, e) for s in scenes for v in views for e in effects]
    if not cells:
        die('no cell selected')

    work = args.work or os.path.join(REPO, 'target', 'perf-bench')
    label = args.label or os.path.basename(binary)
    base = os.path.join(work, re.sub(r'[^A-Za-z0-9_.-]+', '_', label))
    os.makedirs(base, exist_ok=True)
    backend = GPU_BACKEND if args.gpu else ('display' if args.display else 'xvfb')
    meta = {
        'label': label, 'machine': args.machine,
        'build': f'{os.path.basename(binary)} sha256:{sha256_12(binary)}', 'commit': args.commit,
        'bench_tool': git_commit(),
        'backend': backend,
        'display': args.display or '', 'size': args.size, 'refresh': args.refresh,
        'workspace': args.workspace, 'runs': args.runs, 'frames': None if args.gpu else args.frames,
        'skip': None if args.gpu else args.skip,
        'host_cpu': platform.machine(), 'date': datetime.date.today().isoformat(),
    }
    if args.gpu:
        meta.update({'seconds': args.seconds, 'skip_seconds': args.skip_seconds, 'device': GPU_DEVICE,
                     'gpu': f'NVIDIA GeForce {GPU_NAME}', 'gpu_pci': GPU_PCI, 'renderer': None})
    meta['atmosphere_switches'] = has_atmosphere(binary)
    if not meta['atmosphere_switches'] and any(e == 'on' for _, _, e in cells):
        log('bench: this build has no atmosphere switches: its effects-on cells run as effects-off')
    result = {'schema': SCHEMA, 'meta': meta, 'scenes': {}, 'errors': {}}
    log(f'bench: {label}: {len(cells)} cells x {args.runs} runs ({backend})')
    if args.gpu:
        run_gpu_matrix(args, binary, dt_dir, cells, base, result)
    else:
        run_cells(args, cells, base, result,
                  lambda scene, view, effect, rep, run_dir: run_once(
                      args, binary, dt_dir, scene, view, effect, rep, run_dir))
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
