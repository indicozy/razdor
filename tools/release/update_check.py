#!/usr/bin/env python3
"""Update check: does the previous Razdor release update to the new one, and does its data carry over?

Run by the release step once the pipeline is green and the new release is published:

    python3 -I tools/release/update_check.py --from v1.5.1 --to v1.5.2

Before the tag, with the locally built program (the carry-over check only):

    python3 -I tools/release/update_check.py --from v1.5.1 --from v1.5.0 --from v1.4.1 \\
        --new-bin /path/to/target/release/razdor

Desktop (Linux): the --from program, run off-screen on a private Xvfb with
`updates = always`, puts the --to program in place. The file on disk must then match the
--to release's SHA256SUMS line, and the updated program must start and report --to's version.

Android: the --from and --to APKs must both verify, be signed by the same certificate, have
the same package and a versionCode that does not go down (equal is allowed: Android installs
an equal versionCode over the old one). The new versionName is printed (INFO, not a failure).

Carry-over (Linux, every --from): the old release's real program, updates off, makes data on
a fresh folder set (its own game folder, a Hub mod as the release lays it out, a save of a
started map, its picture cache filled). The new program (the --to binary, or --new-bin) then
starts on the same folders: the data must keep its content (logical SHA-256 of the game and
mod files, a `.lz4` counted as its decoded bytes), the save answers «Continue the game?», the
picture cache is in the new format, the logs gain no error, and a second start changes nothing.
Then the game folder is marked as Razdor's and the first-start conversion runs: timed, its
screens shot, its result checked the same way. The scratch folders are the program's own
(the owner's install and home are only read; HOME and XDG_* point inside the scratch folder).

Exit codes: 0 every check passed (INFO rows allowed), 1 a check failed (the previous release
does not update to this one, or its data does not carry over), 2 the check could not run
(network, tool or input error; --to must be the newest published release and --from an older
one; the old data could not be made).

Only stdlib (ctypes for the key presses on the private display). The public GitHub API and
download URLs need no token. Windows, macOS and iOS are not covered (Wine has no curl; there is
no Mac here).
"""

import argparse
import ctypes
import hashlib
import json
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

REPO = "indicozy/razdor"
API_RELEASES = f"https://api.github.com/repos/{REPO}/releases?per_page=30"
UA = "razdor-update-check/1"
UPDATE_WAIT = 180  # seconds for the program to put the new file in place
START_SECONDS = 20  # seconds the updated program runs
POLL = 2

PASS, INFO, FAIL, ERROR = "PASS", "INFO", "FAIL", "ERROR"
EXIT_OK, EXIT_FAIL, EXIT_ERROR = 0, 1, 2

VERSION_RE = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)$")

# Carry-over (see check_carry)
CARRY_OLD_SECONDS = 75  # the old program plays a map: its picture cache fills, its session slot is written
CARRY_MOD_SECONDS = 40  # the old program continues its session with the test mod active
CARRY_NEW_SECONDS = 60  # each start of the new program
CARRY_OLD_ANSWERS = (6, 15)  # the old run: Enter on the scenario list, then (later: the window's animation) on its Start
CARRY_NEW_ANSWERS = (6,)  # a new run: Enter answers «Continue the game?» (seconds after «install loaded»)
CARRY_SCENE = "scenarios:Другой берег"  # the scenario list with a Community Update map picked (the old run)
CARRY_MOD_ID = "carry-test"  # the test Hub mod's id
CARRY_SHOTS = 15  # screenshots of a start, one every 2 s from its first frame
ERROR_WORDS = ("panicked", "error", "failed", "cannot", "could not", "does not build", "lacks")
LZ4_MAGIC = b"\x04\x22\x4d\x18"
PICTURE_SUFFIXES = (".ugs", ".lit")


class ToolError(Exception):
    """The check could not run: network, tool or input error (exit 2)."""


class CheckFail(Exception):
    """A check failed: the previous release does not update to this one (exit 1)."""


def parse_version(text):
    m = VERSION_RE.match(text.strip())
    if not m:
        raise ToolError(f"not a version: {text!r} (use vX.Y.Z)")
    return tuple(int(x) for x in m.groups())


def vtag(v):
    return "v%d.%d.%d" % v


def vnum(v):
    return "%d.%d.%d" % v


def api_headers(url):
    """The request's headers: `GH_TOKEN` (when set) goes to GitHub's API only, which limits
    anonymous calls to 60 an hour; release files and other hosts never get it."""
    headers = {"User-Agent": UA}
    token = os.environ.get("GH_TOKEN", "").strip()
    if token and url.startswith("https://api.github.com/"):
        headers["Authorization"] = "Bearer " + token
    return headers


def http_get(url, timeout=60):
    req = urllib.request.Request(url, headers=api_headers(url))
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read()
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise ToolError(f"cannot fetch {url}: {e}") from e


def download(url, dest, timeout=120):
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r, open(part, "wb") as f:
            shutil.copyfileobj(r, f, 1 << 20)
    except (urllib.error.URLError, OSError, ValueError) as e:
        part.unlink(missing_ok=True)
        raise ToolError(f"cannot download {url}: {e}") from e
    os.replace(part, dest)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_sums(text):
    out = {}
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2:
            out[parts[1].strip().lstrip("*")] = parts[0].lower()
    return out


def load_published():
    try:
        data = json.loads(http_get(API_RELEASES))
    except ValueError as e:
        raise ToolError(f"the release list is not JSON: {e}") from e
    pub = {}
    for r in data:
        if r.get("draft") or r.get("prerelease"):
            continue
        m = VERSION_RE.match(r.get("tag_name", ""))
        if m:
            pub[tuple(int(x) for x in m.groups())] = r
    if not pub:
        raise ToolError("no published release found")
    return pub


def asset_url(rel, name):
    for a in rel.get("assets", []):
        if a.get("name") == name:
            return a["browser_download_url"]
    raise CheckFail(f"{rel['tag_name']} has no asset {name}")


def sums_of(rel):
    url = asset_url(rel, "SHA256SUMS")
    return parse_sums(http_get(url).decode("utf-8", "replace"))


MNT_SCRATCH = Path("/mnt/ai_models/razdor-scratch")
ANDROID_SDK_DEFAULT = MNT_SCRATCH / "android-apk" / "sdk"


def pick_scratch(given, label):
    """The scratch folder `label` under the default root (or the given one). It runs downloaded
    programs, so it must be ours: not a symlink, owned by this user, not writable by others.
    Created 0700."""
    if given:
        path = Path(given).absolute()
    elif MNT_SCRATCH.is_dir():
        path = MNT_SCRATCH / "update-check" / label
    else:
        return Path(tempfile.mkdtemp(prefix="razdor-update-check-"))
    try:
        os.makedirs(path, mode=0o700, exist_ok=True)
    except OSError as e:
        raise ToolError(f"cannot create scratch {path}: {e}") from e
    st = os.lstat(path)
    if stat.S_ISLNK(st.st_mode):
        raise ToolError(f"scratch {path} is a symlink; use a real folder")
    if not stat.S_ISDIR(st.st_mode):
        raise ToolError(f"scratch {path} is not a folder")
    if st.st_uid != os.getuid():
        raise ToolError(f"scratch {path} is not owned by this user")
    if st.st_mode & 0o022:
        raise ToolError(f"scratch {path} is writable by group or others")
    os.chmod(path, 0o700)
    return path


def last_update_line(log):
    lines = [ln for ln in Path(log).read_text(encoding="utf-8", errors="replace").splitlines() if "update:" in ln]
    return lines[-1] if lines else "(no update line in the log)"


def reset(path):
    shutil.rmtree(path, ignore_errors=True)
    path.mkdir(parents=True)


def stop(proc):
    """Stop a process we started (its own session): SIGTERM, then SIGKILL after 15 s."""
    if proc is None or proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        proc.wait(15)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait(10)


def tail(path, n=25):
    try:
        lines = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as e:
        return f"(no log: {e})"
    return "\n".join(lines[-n:]) or "(empty log)"


def find_dt_dir():
    """RAZDOR_DT_DIR from the environment, else from the repo's `.env` (or the main checkout's,
    for a worktree). The value is never printed."""
    v = os.environ.get("RAZDOR_DT_DIR")
    if v:
        return v.strip().strip("\"'")
    root = Path(__file__).resolve().parents[2]
    candidates = [root / ".env"]
    try:
        out = subprocess.run(
            ["git", "-C", str(root), "rev-parse", "--path-format=absolute", "--git-common-dir"],
            capture_output=True, text=True, timeout=20,
        )
        if out.returncode == 0:
            candidates.append(Path(out.stdout.strip()).parent / ".env")
    except (OSError, subprocess.SubprocessError):
        pass
    for c in candidates:
        if c.is_file():
            for line in c.read_text(encoding="utf-8", errors="replace").splitlines():
                if line.startswith("RAZDOR_DT_DIR="):
                    return line.split("=", 1)[1].strip().strip("\"'")
    return None


# ---------------------------------------------------------------- desktop (Linux)

def start_xvfb(log):
    xvfb = shutil.which("Xvfb")
    if not xvfb:
        raise ToolError("Xvfb is not installed")
    for n in range(90, 200):
        if Path(f"/tmp/.X{n}-lock").exists() or Path(f"/tmp/.X11-unix/X{n}").exists():
            continue
        with open(log, "wb") as f:
            proc = subprocess.Popen(
                [xvfb, f":{n}", "-screen", "0", "1280x800x24", "-nolisten", "tcp"],
                stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT, start_new_session=True,
            )
        sock = Path(f"/tmp/.X11-unix/X{n}")
        t0 = time.monotonic()
        while time.monotonic() - t0 < 20:
            if proc.poll() is not None:
                break
            if sock.exists():
                time.sleep(1)
                return proc, f":{n}"
            time.sleep(0.5)
        stop(proc)
        raise ToolError(f"Xvfb did not start on :{n} (log {log})")
    raise ToolError("no free display number for Xvfb")


def game_env(display, xdg, saves, log, dt_dir):
    # Strip every RAZDOR_* variable of this shell (an update override, a log path...), and the
    # Wayland and X auth variables, so the program can only open a window on our Xvfb.
    env = {k: v for k, v in os.environ.items()
           if not k.startswith("RAZDOR_") and k not in ("WAYLAND_DISPLAY", "XAUTHORITY", "DISPLAY")}
    env.update({
        "DISPLAY": display,
        "XDG_RUNTIME_DIR": str(xdg),
        "RAZDOR_LOG": str(log),
        "RAZDOR_SAVE_DIR": str(saves),
        "RAZDOR_SIZE": "1280x800",
    })
    if dt_dir:
        env["RAZDOR_DT_DIR"] = dt_dir
    return env


def start_game(prog, env, log):
    with open(log, "wb") as f:
        return subprocess.Popen(
            [str(prog)], env=env, cwd=str(prog.parent), stdin=subprocess.DEVNULL,
            stdout=f, stderr=subprocess.STDOUT, start_new_session=True,
        )


def check_desktop(frm, to, rel_from, rel_to, scratch, dt_dir, rows, logs):
    if not sys.platform.startswith("linux"):
        raise ToolError("the desktop check runs on Linux only")
    base = scratch / "desktop"
    reset(base)
    prog = base / "razdor"  # the program copy; its folder has no `target` part, so it updates
    prev_name = f"razdor-{vnum(frm)}-linux"
    new_name = f"razdor-{vnum(to)}-linux"
    exp_prev = sums_of(rel_from).get(prev_name)
    exp_new = sums_of(rel_to).get(new_name)
    if exp_prev is None:
        raise CheckFail(f"{vtag(frm)} SHA256SUMS has no {prev_name}")
    if exp_new is None:
        raise CheckFail(f"{vtag(to)} SHA256SUMS has no {new_name}")

    download(asset_url(rel_from, prev_name), prog)
    prog.chmod(0o755)
    old_sha = sha256(prog)
    if old_sha != exp_prev:
        raise CheckFail(f"{prev_name} does not match its SHA256SUMS line")
    rows.append(("desktop: download " + prev_name, PASS, "sha256 matches SHA256SUMS"))

    saves = base / "saves"
    saves.mkdir()
    (saves / "audio.json").write_text(json.dumps({"updates": "always"}), encoding="utf-8")
    xdg = base / "xdg"
    xdg.mkdir()
    xdg.chmod(0o700)

    procs = []
    xvfb, display = start_xvfb(base / "xvfb.log")
    procs.append(xvfb)
    try:
        log1 = base / "run1.log"
        env1 = game_env(display, xdg, saves, log1, dt_dir)
        proc = start_game(prog, env1, log1)
        procs.append(proc)
        deadline = time.monotonic() + UPDATE_WAIT
        while proc.poll() is None and sha256(prog) == old_sha and time.monotonic() < deadline:
            time.sleep(POLL)
        rc = proc.poll()
        stop(proc)
        new_sha = sha256(prog)
        logs.append(("run 1 (update)", log1))
        if new_sha == old_sha:
            if "update: no check:" in Path(log1).read_text(encoding="utf-8", errors="replace"):
                raise ToolError(f"{vtag(frm)} could not check for updates (see its log): "
                                f"{last_update_line(log1)}")
            if rc is not None:
                raise CheckFail(f"{vtag(frm)} exited (code {rc}) before it put {vtag(to)} in place; "
                                f"last update line: {last_update_line(log1)}")
            raise CheckFail(f"{vtag(frm)} did not put {vtag(to)} in place within {UPDATE_WAIT} s; "
                            f"last update line: {last_update_line(log1)}")
        if new_sha != exp_new:
            raise CheckFail(f"the program after the update is not {new_name} (sha256 {new_sha[:12]}, "
                            f"SHA256SUMS {str(exp_new)[:12]})")
        in_place = "is in place" in Path(log1).read_text(encoding="utf-8", errors="replace")
        rows.append((f"desktop: {vtag(frm)} puts {vtag(to)} in place", PASS,
                     "sha256 matches SHA256SUMS" + ("" if in_place else " (no 'is in place' line in the log)")))

        log2 = base / "run2.log"
        env2 = game_env(display, xdg, saves, log2, dt_dir)
        proc2 = start_game(prog, env2, log2)
        procs.append(proc2)
        t0 = time.monotonic()
        while proc2.poll() is None and time.monotonic() - t0 < START_SECONDS:
            time.sleep(1)
        rc2 = proc2.poll()
        stop(proc2)
        logs.append(("run 2 (updated program)", log2))
        text = Path(log2).read_text(encoding="utf-8", errors="replace")
        m = re.search(r"Razdor (\d+\.\d+\.\d+) \(", text)
        started = m.group(1) if m else None
        if rc2 is not None:
            raise CheckFail(f"{vtag(to)} exited (code {rc2}) within {START_SECONDS} s of starting")
        if "panicked" in text:
            raise CheckFail(f"{vtag(to)} panicked at start (log {log2})")
        if started != vnum(to):
            raise CheckFail(f"the updated program logs version {started!r}, want {vnum(to)}")
        rows.append((f"desktop: {vtag(to)} starts after the update", PASS,
                     f"runs {START_SECONDS} s, logs Razdor {started}, no panic"))
    finally:
        for p in reversed(procs):
            stop(p)


# ---------------------------------------------------------------- carry-over (data an older release left)

class Keys:
    """Key presses on the private Xvfb through XTest (libXtst), for the one window a carry-over run
    must answer: «Continue the game?». Only our own display is ever opened."""

    def __init__(self, display):
        try:
            self.x11 = ctypes.CDLL("libX11.so.6")
            self.xt = ctypes.CDLL("libXtst.so.6")
        except OSError as e:
            raise ToolError(f"libX11 and libXtst are needed for the key presses: {e}") from e
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XStringToKeysym.restype = ctypes.c_ulong
        self.x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
        self.x11.XKeysymToKeycode.restype = ctypes.c_ubyte
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XFlush.argtypes = [ctypes.c_void_p]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.xt.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.dpy = self.x11.XOpenDisplay(display.encode())
        if not self.dpy:
            raise ToolError(f"cannot open display {display} for the key presses")

    def press(self, keysym):
        code = self.x11.XKeysymToKeycode(self.dpy, self.x11.XStringToKeysym(keysym.encode()))
        if not code:
            raise ToolError(f"no keycode for {keysym}")
        self.xt.XTestFakeKeyEvent(self.dpy, code, 1, 0)
        self.xt.XTestFakeKeyEvent(self.dpy, code, 0, 0)
        self.x11.XFlush(self.dpy)

    def close(self):
        if self.dpy:
            self.x11.XCloseDisplay(self.dpy)
            self.dpy = None


def shot(display, path):
    """A screenshot of the whole private display (ImageMagick `import`), kept in the scratch folder."""
    try:
        subprocess.run(["import", "-display", display, "-window", "root", str(path)],
                       capture_output=True, timeout=60)
    except (OSError, subprocess.SubprocessError):
        return None
    return path if path.is_file() else None


def lz4_block(src):
    """One LZ4 block (the standard block format), decoded in Python."""
    out = bytearray()
    i, n = 0, len(src)
    while i < n:
        token = src[i]
        i += 1
        lit = token >> 4
        if lit == 15:
            while True:
                b = src[i]
                i += 1
                lit += b
                if b != 255:
                    break
        out += src[i:i + lit]
        i += lit
        if i >= n:
            break
        off = src[i] | (src[i + 1] << 8)
        i += 2
        if off == 0 or off > len(out):
            raise ValueError("lz4: bad match offset")
        mlen = (token & 15) + 4
        if (token & 15) == 15:
            while True:
                b = src[i]
                i += 1
                mlen += b
                if b != 255:
                    break
        start = len(out) - off
        if off >= mlen:
            out += out[start:start + mlen]
        else:
            pattern = bytes(out[start:])
            out += (pattern * (mlen // off + 1))[:mlen]
    return bytes(out)


def lz4_frame(data):
    """An LZ4 frame (what a `<name>.lz4` holds), decoded to its bytes. The logical SHA-256 is what
    the check compares, so the frame's own checksums are skipped (when present they are read past)."""
    if data[:4] != LZ4_MAGIC:
        raise ValueError("lz4: not a frame")
    flg = data[4]
    if flg >> 6 != 1:
        raise ValueError("lz4: unknown version")
    block_sum, size_flag, content_sum = flg & 0x10, flg & 0x08, flg & 0x04
    pos = 6 + (8 if size_flag else 0) + (4 if flg & 0x01 else 0) + 1
    out = bytearray()
    while True:
        size = int.from_bytes(data[pos:pos + 4], "little")
        pos += 4
        if size == 0:
            break
        raw = size & 0x80000000
        size &= 0x7FFFFFFF
        block = data[pos:pos + size]
        pos += size
        if block_sum:
            pos += 4
        out += block if raw else lz4_block(block)
    return bytes(out)


def logical_tree(root, skip=()):
    """{logical path: (size, sha256, form)} of the files under `root`. A `.lz4` counts as its logical
    name, decoded; a plain file wins over a `.lz4` of the same name (the spec's rule 2). `.part`
    leftovers and the paths in `skip` (a name, or a folder's name) are left out."""
    plain, packed = {}, {}
    if not root.is_dir():
        return {}
    for p in root.rglob("*"):
        if p.is_symlink() or not p.is_file():
            continue
        rel = p.relative_to(root).as_posix()
        if rel.endswith(".part") or any(rel == s or rel.startswith(s + "/") for s in skip):
            continue
        if rel.endswith(".lz4"):
            packed[rel[:-4]] = p
        else:
            plain[rel] = p
    out = {}
    for name in sorted(set(plain) | set(packed)):
        if name in plain:
            data, form = plain[name].read_bytes(), "plain"
        else:
            data, form = lz4_frame(packed[name].read_bytes()), "lz4"
        out[name] = (len(data), hashlib.sha256(data).hexdigest(), form)
    return out


def listing(root, skip=()):
    """{relative path: size} of the regular files under `root`."""
    out = {}
    if not root.is_dir():
        return out
    for p in root.rglob("*"):
        if p.is_symlink() or not p.is_file():
            continue
        rel = p.relative_to(root).as_posix()
        if any(rel == s or rel.startswith(s + "/") for s in skip):
            continue
        out[rel] = p.stat().st_size
    return out


def count_suffix(root, suffixes):
    if not root.is_dir():
        return 0
    return sum(1 for p in root.rglob("*") if p.is_file() and p.name.lower().endswith(suffixes))


def cache_folders(data_dir):
    """The picture cache: {folder name: (entries, {(magic, format)})} under data_dir/razdor/cache/pictures."""
    root = data_dir / "razdor" / "cache" / "pictures"
    out = {}
    if root.is_dir():
        for d in sorted(root.iterdir()):
            if not d.is_dir():
                continue
            files = sorted(d.glob("*.rzpc"))
            kinds = set()
            for f in files:
                with open(f, "rb") as fh:
                    head = fh.read(8)
                kinds.add((head[:4], int.from_bytes(head[4:8], "little")))
            out[d.name] = (len(files), kinds)
    return out


def read_text(path):
    try:
        return Path(path).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


def conversion_line(text):
    """The first-start conversion's summary line for the game folder (the log writes the folder's path
    in its own spelling, so the match is on the folder's name), or None."""
    pattern = re.compile(r"\bstored: .*DiscordTimes: \d+ pictures compressed \(")
    for ln in text.splitlines():
        if pattern.search(ln):
            return ln
    return None


def error_lines(text):
    """The log's error-looking lines, without the Hub's network fallbacks (a catalog address that
    answers 404 is read from the next one: expected, not a failure)."""
    return {ln for ln in text.splitlines()
            if any(w in ln.lower() for w in ERROR_WORDS) and "curl:" not in ln and "index-v2.json" not in ln}


def watch(proc, log, seconds, keys, display, shots_dir, tag, game, answers=CARRY_NEW_ANSWERS):
    """Runs `proc` for up to `seconds` (or until it exits). With `keys`, presses Enter at each of
    `answers` (seconds after «install loaded»). With `shots_dir`, takes a screenshot every 2 s from
    the first frame (until the conversion's end + 4 s, at most CARRY_SHOTS) and one at the end.
    Times the first conversion of the game folder. Returns the times in s from the start (None when
    not seen) and the screenshot paths."""
    t0 = time.monotonic()
    times = {"install": None, "conversion": None, "answer": None, "shots": []}
    next_shot = 0.0
    pending = list(answers) if keys is not None else []
    while proc.poll() is None:
        el = time.monotonic() - t0
        if el >= seconds:
            break
        text = read_text(log)
        if times["install"] is None and "install loaded" in text:
            times["install"] = el
        if times["conversion"] is None and conversion_line(text):
            times["conversion"] = el
        if pending and times["install"] is not None and el >= times["install"] + pending[0]:
            pending.pop(0)
            if times["answer"] is None:
                times["answer"] = el
            keys.press("Return")
        conv_done = times["conversion"] is not None and el >= times["conversion"] + 4
        if shots_dir is not None and len(times["shots"]) < CARRY_SHOTS and el >= next_shot and not conv_done:
            path = shot(display, shots_dir / f"{tag}-{len(times['shots']):02d}-{int(el)}s.png")
            if path:
                times["shots"].append(path)
            next_shot = el + 2
        time.sleep(0.5)
    if shots_dir is not None:
        path = shot(display, shots_dir / f"{tag}-end.png")
        if path:
            times["shots"].append(path)
    return times


def build_mod(hub, src, base, rows):
    """Lays a Hub mod out as the release writes an installed mod (`hub/mods/<id>/files`, the kept
    `archives/1.zip`, `manifest.json`; `installed.json`; `active.json`): six pictures of the mod
    source (the Evolution install) that differ from the game folder `base`. A test mod: nothing of
    it is committed, and it lives only in the scratch folder."""
    picks = []
    for p in sorted((src / "Graphics").rglob("*")):
        if not p.is_file() or p.suffix.lower() not in PICTURE_SUFFIXES or p.stat().st_size > 2_000_000:
            continue
        rel = p.relative_to(src).as_posix()
        b = base / rel
        if not b.is_file() or (b.stat().st_size == p.stat().st_size and sha256(b) == sha256(p)):
            continue
        picks.append(rel)
        if len(picks) == 6:
            break
    if not picks:
        raise ToolError("no picture of the mod source differs from the game folder")
    mdir = hub / "mods" / CARRY_MOD_ID
    files = mdir / "files"
    manifest_files = []
    (mdir / "archives").mkdir(parents=True)
    with zipfile.ZipFile(mdir / "archives" / "1.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for rel in picks:
            dest = files / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src / rel, dest)
            z.write(dest, rel)
            manifest_files.append({"path": rel, "size": dest.stat().st_size, "sha256": sha256(dest)})
    (mdir / "manifest.json").write_text(json.dumps({"files": manifest_files, "sources": []},
                                                   ensure_ascii=False, indent=1), encoding="utf-8")
    installed = {"entries": {CARRY_MOD_ID: {
        "version": "1.0", "kind": "mod", "lang": "ru",
        "title": {"ru": "Тест переноса данных", "en": "Carry-over test"},
        "requires": [], "files": [f"mods/{CARRY_MOD_ID}/files/{rel}" for rel in picks]}}}
    (hub / "installed.json").write_text(json.dumps(installed, ensure_ascii=False, indent=1), encoding="utf-8")
    (hub / "active.json").write_text(json.dumps({"mod": CARRY_MOD_ID}), encoding="utf-8")
    rows.append(("carry: test Hub mod laid out as the release writes it", INFO,
                 f"{len(picks)} pictures of the mod source, archives/1.zip kept, made active"))


def carry_env(display, base, log):
    """The program's environment for a carry-over run: every RAZDOR_* of this shell is dropped; HOME and
    the XDG folders point inside the scratch folder, so the owner's saves, Downloads and games are never
    read or written."""
    env = {k: v for k, v in os.environ.items()
           if not k.startswith("RAZDOR_") and k not in (
               "WAYLAND_DISPLAY", "XAUTHORITY", "DISPLAY", "HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME",
               "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR", "XDG_DOWNLOAD_DIR")}
    env.update({
        "DISPLAY": display,
        "HOME": str(base / "home"),
        "XDG_DATA_HOME": str(base / "xdg" / "data"),
        "XDG_CONFIG_HOME": str(base / "xdg" / "config"),
        "XDG_CACHE_HOME": str(base / "xdg" / "cache"),
        "XDG_STATE_HOME": str(base / "xdg" / "state"),
        "XDG_RUNTIME_DIR": str(base / "run"),
        "XDG_DOWNLOAD_DIR": str(base / "downloads"),
        "RAZDOR_LOG": str(log),
        "RAZDOR_SAVE_DIR": str(base / "saves"),
        "RAZDOR_SIZE": "1280x800",
        "RAZDOR_NO_AUDIO": "1",
        "ALSA_CONFIG_PATH": str(base / "asound.conf"),
    })
    return env


def compare_rows(label, before, after, saves_before, saves_after):
    """Rows comparing the logical digests before and after the new program's start."""
    rows = []
    for part, kept_removed in (("game", ()), ("mod", ("archives/",))):
        b, a = before[part], after[part]
        changed = [k for k in b if k in a and b[k][:2] != a[k][:2]]
        removed = [k for k in b if k not in a and not any(k.startswith(s) for s in kept_removed)]
        archived = [k for k in b if k not in a and any(k.startswith(s) for s in kept_removed)]
        rows.append((f"{label}: {part} files keep their content", PASS if not changed and not removed else FAIL,
                     f"{len(b)} logical files; changed {changed[:3]}, removed {removed[:3]}"
                     + (f"; {len(archived)} kept zip(s) removed, as the new release does" if archived else "")))
    lost = [k for k in saves_before if k not in saves_after]
    changed_saves = [k for k in saves_before if k in saves_after and saves_before[k][:2] != saves_after[k][:2]]
    rows.append((f"{label}: saves kept", PASS if not lost else FAIL,
                 f"{len(saves_before)} save files; lost {lost[:3]}; changed (INFO) {changed_saves[:4]}"))
    return rows


def check_carry(frm, rel_from, new_bin, new_label, new_version, base, shots, dt_dir, mod_src, rows, logs):
    """The data carry-over check for one old release:

    1. The old release's real Linux program, updates off, runs on fresh folders: its own game folder
       (a copy of the install next to the program, unmarked as the old release leaves it), a Hub mod
       laid out as the release writes it, a map started (its session slot is the save), the picture
       cache filled by CARRY_OLD_SECONDS of play.
    2. The new program replaces it in the same folders and starts. The data is compared with what it
       was before (logical SHA-256 of each game and mod file, a `.lz4` counted as its decoded bytes),
       the save is answered with Enter, the picture cache is read, the logs are compared with the old
       run's error lines. The game folder must be left alone (it has no marker).
    3. A second start of the new program changes nothing.
    4. The harness marks the game folder as Razdor's (what a copy Razdor unpacked has; labelled, as
       the test's own doing) and the first-start conversion runs: timed, its screens shot, its result
       checked against the same digests; a second start changes nothing again.
    """
    if dt_dir is None or not Path(dt_dir).is_dir():
        raise ToolError("RAZDOR_DT_DIR is not set or not a folder: the carry-over check copies the install")
    label = f"{vtag(frm)} carry"
    reset(base)
    app = base / "app"
    app.mkdir()
    prog = app / "razdor"
    old_name = f"razdor-{vnum(frm)}-linux"
    exp_old = sums_of(rel_from).get(old_name)
    if exp_old is None:
        raise CheckFail(f"{vtag(frm)} SHA256SUMS has no {old_name}")
    download(asset_url(rel_from, old_name), prog)
    prog.chmod(0o755)
    if sha256(prog) != exp_old:
        raise CheckFail(f"{old_name} does not match its SHA256SUMS line")

    game = app / "DiscordTimes"  # the unpack folder next to the program (platform::unpack_dir)
    shutil.copytree(dt_dir, game, symlinks=False)
    data = base / "xdg" / "data"
    hub = data / "razdor" / "hub"  # platform::app_dir()/hub
    saves = base / "saves"
    for d in (base / "home", base / "run", base / "downloads", base / "xdg" / "config",
              base / "xdg" / "cache", base / "xdg" / "state", saves):
        d.mkdir(parents=True, exist_ok=True)
    (base / "run").chmod(0o700)
    (saves / "audio.json").write_text(json.dumps({"updates": "off"}), encoding="utf-8")
    # no sound device in the sandbox: ALSA's null device (the game's audio thread panics without one)
    (base / "asound.conf").write_text("pcm.!default {\n  type null\n}\nctl.!default {\n  type null\n}\n",
                                      encoding="utf-8")

    xvfb, display = start_xvfb(base / "xvfb.log")
    procs = [xvfb]
    keys = None
    try:
        keys = Keys(display)

        # 1. the old program: the base game started from the scenario list and played (its session
        #    slot is the save, its picture cache fills), then stopped
        log_old = base / "old.log"
        env_old = carry_env(display, base, log_old)
        env_old["RAZDOR_SCENE"] = CARRY_SCENE
        proc = start_game(prog, env_old, log_old)
        procs.append(proc)
        watch(proc, log_old, CARRY_OLD_SECONDS, keys, display, None, "old", game, CARRY_OLD_ANSWERS)
        stop(proc)
        old_text = read_text(log_old)
        logs.append((f"{vtag(frm)} old run", log_old))
        if "install loaded" not in old_text:
            raise ToolError(f"{vtag(frm)} did not load its install (log {log_old}); its data was not made")
        if "panicked" in old_text:
            raise ToolError(f"{vtag(frm)} panicked on its own data (log {log_old})")
        old_cache = cache_folders(data)
        old_entries = sum(n for n, _ in old_cache.values())
        session = sorted(p.name for p in saves.rglob("session*") if p.is_file())
        if not session or old_entries == 0:
            raise ToolError(f"{vtag(frm)} wrote no session or no picture cache (log {log_old})")

        # 1b. a Hub mod laid out as the release writes it, made active; the old program continues the
        #     same session with it (the scenario list shows only the mod's maps while a mod is active)
        if mod_src is not None:
            build_mod(hub, Path(mod_src), game, rows)
        else:
            rows.append((label + ": test Hub mod", INFO, "no mod source (RAZDOR_COMMUNITY_DIR unset): not checked"))
        log_old2 = base / "old2.log"
        env_old2 = carry_env(display, base, log_old2)
        proc = start_game(prog, env_old2, log_old2)
        procs.append(proc)
        watch(proc, log_old2, CARRY_MOD_SECONDS, keys, display, None, "old2", game, CARRY_NEW_ANSWERS)
        stop(proc)
        old2_text = read_text(log_old2)
        logs.append((f"{vtag(frm)} old run with the mod", log_old2))
        old_text += old2_text
        if "install loaded" not in old2_text or "panicked" in old2_text:
            raise ToolError(f"{vtag(frm)} with its mod: no install load, or a panic (log {log_old2})")
        mod_built = (hub / "mods" / CARRY_MOD_ID / "game").is_dir() if mod_src is not None else True
        mod_refused = "does not build" in old2_text
        made = mod_built and not mod_refused and bool(sorted(p.name for p in saves.rglob("session*") if p.is_file()))
        rows.append((f"{label}: the old run makes its data", PASS if made else ERROR,
                     f"{vtag(frm)} loaded its install (unmarked folder); session {session}; picture cache "
                     f"{old_entries} entries in {sorted(old_cache)}; with the mod: folder built {mod_built}, "
                     f"refused {mod_refused}"))
        if not made:
            raise ToolError(f"{vtag(frm)} with its mod: no mod folder or a refused mod (log {log_old2})")

        before = {
            "game": logical_tree(game, skip=(".razdor-owned",)),
            "mod": logical_tree(hub / "mods" / CARRY_MOD_ID / "files"),
            "saves": logical_tree(saves),
        }

        # 2. the new program replaces the old one in the same folders
        if new_bin is not None:
            shutil.copyfile(new_bin, prog)
            prog.chmod(0o755)
        new_sha = sha256(prog)

        def start_new(tag, seconds, shot_dir, answer):
            log = base / f"new-{tag}.log"
            env = carry_env(display, base, log)
            p = start_game(prog, env, log)
            procs.append(p)
            t = watch(p, log, seconds, keys if answer else None, display, shot_dir, tag, game)
            stop(p)
            logs.append((f"{vtag(frm)} new run {tag}", log))
            return t, read_text(log)

        t_a, new_a = start_new("A", CARRY_NEW_SECONDS, shots, True)
        m = re.search(r"Razdor (\d+\.\d+\.\d+) \(", new_a)
        logged = m.group(1) if m else None
        if new_version is not None and logged != vnum(new_version):
            raise CheckFail(f"the new program logs {logged!r}, want {vnum(new_version)}")
        started_ok = "install loaded" in new_a and "panicked" not in new_a
        rows.append((f"{label}: the new program starts on the old folders", PASS if started_ok else FAIL,
                     f"{new_label} (sha256 {new_sha[:12]}) logs Razdor {logged}; install loaded at "
                     f"{t_a['install'] and round(t_a['install'], 1)} s; Enter at "
                     f"{t_a['answer'] and round(t_a['answer'], 1)} s"))
        unmarked_left = t_a["conversion"] is None and count_suffix(game, (".lz4",)) == 0
        rows.append((f"{label}: unmarked game folder left alone", PASS if unmarked_left else FAIL,
                     "no conversion, no .lz4 in the game folder (spec: a desktop folder without the marker is "
                     "never touched)" if unmarked_left else
                     f"converted at {t_a['conversion']} s; {count_suffix(game, ('.lz4',))} .lz4 files"))
        cache_new = cache_folders(data)
        fmt_ok = bool(cache_new) and all(kinds == {(b"RZPC", 2)} for n, kinds in cache_new.values() if n) \
            and any(n for n, _ in cache_new.values())
        rows.append((f"{label}: picture cache in the new format", PASS if fmt_ok else FAIL,
                     "folders " + ", ".join(f"{k}: {n} entries" for k, (n, _) in cache_new.items())))
        if mod_src is not None:
            mod_ok = "does not build" not in new_a and (hub / "mods" / CARRY_MOD_ID / "game").is_dir()
            rows.append((f"{label}: the Hub mod loads", PASS if mod_ok else FAIL,
                         f"mod folder built; mod pictures now {count_suffix(hub / 'mods' / CARRY_MOD_ID / 'files', ('.lz4',))} "
                         f".lz4; kept archives/ still there: {(hub / 'mods' / CARRY_MOD_ID / 'archives').is_dir()}"))
        after_a = {
            "game": logical_tree(game, skip=(".razdor-owned",)),
            "mod": logical_tree(hub / "mods" / CARRY_MOD_ID / "files"),
            "saves": logical_tree(saves),
        }
        rows.extend(compare_rows(label + " (A)", before, after_a, before["saves"], after_a["saves"]))
        new_err = error_lines(new_a) - error_lines(old_text)
        rows.append((f"{label}: no new error lines (A)", PASS if not new_err else FAIL,
                     "none beyond the old run's" if not new_err else "; ".join(sorted(new_err))[:400]))
        rows.append((f"{label}: the save is answered (A)",
                     PASS if t_a["answer"] is not None and started_ok else FAIL,
                     f"Enter sent after the install loaded; {len(t_a['shots'])} screenshots in {shots}"))

        # A2: the second start of the same folders
        def state():
            # the Hub's catalog cache (hub/cache) is refreshed from the network at each start: not compared
            return {"game": listing(game, skip=(".razdor-owned",)),
                    "mod": listing(hub / "mods" / CARRY_MOD_ID, skip=("game",)),
                    "hub": listing(hub, skip=("cache",))}

        s_before = state()
        t_a2, new_a2 = start_new("A2", CARRY_NEW_SECONDS, None, True)
        s_after = state()
        changed = [k for part in s_before for k in s_before[part] if s_before[part][k] != s_after[part].get(k)]
        added = [k for part in s_after for k in s_after[part] if k not in s_before[part]]
        new_err2 = error_lines(new_a2) - error_lines(old_text)
        ok2 = not changed and not added and t_a2["conversion"] is None and "install loaded" in new_a2 \
            and "panicked" not in new_a2 and not new_err2
        rows.append((f"{label}: second start changes nothing (A2)", PASS if ok2 else FAIL,
                     f"changed {changed[:3]}, added {added[:3]}, conversion {t_a2['conversion']}, "
                     f"new error lines {sorted(new_err2)[:2]}"))

        # 3. the owned copy: the marker, then the conversion (timed, screens shot, digests kept)
        (game / ".razdor-owned").write_text(
            "harness marker: this copy is treated as Razdor's own (test only, not a player's file)\n",
            encoding="utf-8")
        before_b = logical_tree(game, skip=(".razdor-owned",))
        plain_before = count_suffix(game, PICTURE_SUFFIXES)
        shots_b = shots.parent / (shots.name + "-owned")
        shots_b.mkdir(parents=True, exist_ok=True)
        t_b, new_b = start_new("B", CARRY_NEW_SECONDS, shots_b, True)
        conv_line = conversion_line(new_b) or ""
        if t_b["conversion"] is None:
            rows.append((f"{label}: owned copy converted (B)", FAIL,
                         f"no conversion line; {plain_before} plain pictures before"))
        else:
            rows.append((f"{label}: owned copy converted (B)", PASS,
                         f"{round(t_b['conversion'], 1)} s from the start to the conversion's line (install loaded "
                         f"at {t_b['install'] and round(t_b['install'], 1)} s); {conv_line.split(': ', 2)[-1][:160]}"))
        rows.append((f"{label}: conversion screens (B)", INFO,
                     f"{len(t_b['shots'])} screenshots in {shots_b}; first {Path(t_b['shots'][0]).name if t_b['shots'] else '-'}"))
        after_b = logical_tree(game, skip=(".razdor-owned",))
        part_left = [p.name for p in game.rglob("*.part")]
        digests_ok = before_b.keys() == after_b.keys() and all(before_b[k][:2] == after_b[k][:2] for k in before_b)
        rows.append((f"{label}: owned copy keeps its digests (B)", PASS if digests_ok and not part_left else FAIL,
                     f"{len(after_b)} logical files equal before and after; .lz4 {count_suffix(game, ('.lz4',))}; "
                     f"plain pictures left {count_suffix(game, PICTURE_SUFFIXES)} (was {plain_before}); "
                     f".part leftovers {part_left[:2]}"))
        mod_b = logical_tree(hub / "mods" / CARRY_MOD_ID / "files")
        rows.append((f"{label}: mod files keep their digests (B)",
                     PASS if all(after_a["mod"].get(k, (0, 0))[:2] == v[:2] for k, v in mod_b.items()) and
                     set(mod_b) == set(after_a["mod"]) else FAIL,
                     f"{len(mod_b)} mod files compared with the state after A"))
        new_err_b = error_lines(new_b) - error_lines(old_text)
        rows.append((f"{label}: no new error lines (B)", PASS if not new_err_b and "panicked" not in new_b else FAIL,
                     "none beyond the old run's" if not new_err_b else "; ".join(sorted(new_err_b))[:400]))
        if mod_src is not None:
            mod_b_ok = "does not build" not in new_b and "install loaded" in new_b
            rows.append((f"{label}: the Hub mod loads on the owned copy (B)", PASS if mod_b_ok else FAIL,
                         "the active mod builds at this start" if mod_b_ok else
                         "the active mod does not build at the start (the base game is played): "
                         + "; ".join(ln for ln in new_b.splitlines() if "does not build" in ln)[:300]))
        rows.append((f"{label}: the save is answered on the owned copy (B)",
                     PASS if t_b["answer"] is not None and "install loaded" in new_b else FAIL,
                     f"Enter at {t_b['answer'] and round(t_b['answer'], 1)} s"))

        # B2: the converted folder, second start
        b2_before = listing(game, skip=(".razdor-owned",))
        t_b2, new_b2 = start_new("B2", CARRY_NEW_SECONDS, None, True)
        b2_after = listing(game, skip=(".razdor-owned",))
        changed_b2 = [k for k in b2_before if b2_before[k] != b2_after.get(k)] + [k for k in b2_after if k not in b2_before]
        ok_b2 = not changed_b2 and t_b2["conversion"] is None and "install loaded" in new_b2 and "panicked" not in new_b2
        rows.append((f"{label}: second start changes nothing (B2)", PASS if ok_b2 else FAIL,
                     f"game files changed or added {changed_b2[:3]}; conversion line {t_b2['conversion']}"))
    finally:
        if keys is not None:
            keys.close()
        for p in reversed(procs):
            stop(p)


def carry_over(frm, rel_from, new_bin, new_label, new_version, scratch_sub, dt_dir, mod_src, rows, logs):
    """Runs check_carry for one --from; a failure is a row (FAIL: the data does not carry over; ERROR:
    the check could not run). The screenshots are kept in the scratch root's `shots/<from>`."""
    shots = scratch_sub.parent / "shots" / vtag(frm)
    shots.mkdir(parents=True, exist_ok=True)
    try:
        check_carry(frm, rel_from, new_bin, new_label, new_version, scratch_sub / "carry", shots,
                    dt_dir, mod_src, rows, logs)
    except CheckFail as e:
        rows.append((f"{vtag(frm)} carry", FAIL, str(e)))
    except ToolError as e:
        rows.append((f"{vtag(frm)} carry", ERROR, str(e)))


# ---------------------------------------------------------------- Android

def build_tools(sdk):
    base = Path(sdk) / "build-tools"
    cands = []
    if base.is_dir():
        for d in base.iterdir():
            if (d / "apksigner").exists() and (d / "aapt2").exists():
                key = tuple(int(x) if x.isdigit() else 0 for x in re.split(r"[.\-]", d.name))
                cands.append((key, d))
    if not cands:
        raise ToolError(f"no build-tools with apksigner and aapt2 under {base}")
    return max(cands)[1]


def run_tool(cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        raise ToolError(f"cannot run {cmd[0]}: {e}") from e


def badging(aapt2, apk):
    r = run_tool([str(aapt2), "dump", "badging", str(apk)])
    if r.returncode != 0:
        raise CheckFail(f"{apk.name}: aapt2 dump badging failed: {r.stderr.strip()[-200:]}")
    line = next((ln for ln in r.stdout.splitlines() if ln.startswith("package:")), None)
    if line is None:
        raise CheckFail(f"{apk.name}: no package line in aapt2 output")

    def field(key):
        m = re.search(rf"\b{key}='([^']*)'", line)
        return m.group(1) if m else None

    code = field("versionCode")
    return {"name": field("name"), "code": int(code) if code and code.isdigit() else None,
            "vname": field("versionName")}


def check_android(frm, to, rel_from, rel_to, scratch, rows, logs):
    sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or str(ANDROID_SDK_DEFAULT)
    if not Path(sdk).is_dir():
        raise ToolError("the Android SDK is not found: set ANDROID_HOME")
    bt = build_tools(sdk)
    apksigner, aapt2 = bt / "apksigner", bt / "aapt2"

    base = scratch / "android"
    reset(base)
    prev_name, new_name = f"razdor-{vnum(frm)}.apk", f"razdor-{vnum(to)}.apk"
    exp_prev = sums_of(rel_from).get(prev_name)
    exp_new = sums_of(rel_to).get(new_name)
    if exp_prev is None:
        raise CheckFail(f"{vtag(frm)} SHA256SUMS has no {prev_name}")
    if exp_new is None:
        raise CheckFail(f"{vtag(to)} SHA256SUMS has no {new_name}")
    apks = {}
    for rel, name, exp in ((rel_from, prev_name, exp_prev), (rel_to, new_name, exp_new)):
        dest = base / name
        download(asset_url(rel, name), dest)
        if sha256(dest) != exp:
            raise CheckFail(f"{name} does not match its SHA256SUMS line")
        apks[name] = dest
    rows.append(("android: download both APKs", PASS, "sha256 matches SHA256SUMS"))

    certs = {}
    for name, path in apks.items():
        r = run_tool([str(apksigner), "verify", "--print-certs", str(path)])
        if r.returncode != 0:
            raise CheckFail(f"{name}: apksigner verify failed: {(r.stderr or r.stdout).strip()[-200:]}")
        digests = re.findall(r"certificate SHA-256 digest: ([0-9a-fA-F]+)", r.stdout)
        if not digests:
            raise CheckFail(f"{name}: apksigner printed no signer certificate")
        certs[name] = tuple(sorted(d.lower() for d in digests))
    rows.append(("android: signatures verify", PASS, f"apksigner verify ok for {prev_name} and {new_name}"))

    if certs[prev_name] != certs[new_name]:
        raise CheckFail(f"signer certificates differ: {prev_name} {certs[prev_name]} vs "
                        f"{new_name} {certs[new_name]}")
    rows.append(("android: same signing certificate", PASS, ", ".join(certs[new_name])[:64]))

    old = badging(aapt2, apks[prev_name])
    new = badging(aapt2, apks[new_name])
    if old["name"] != new["name"]:
        raise CheckFail(f"package differs: {old['name']} vs {new['name']}")
    rows.append(("android: same package", PASS, str(new["name"])))

    if old["code"] is None or new["code"] is None:
        raise CheckFail(f"no versionCode (old {old['code']}, new {new['code']})")
    if new["code"] < old["code"]:
        raise CheckFail(f"versionCode goes down: {old['code']} -> {new['code']}")
    rows.append(("android: versionCode does not go down", PASS, f"{old['code']} -> {new['code']}"))

    if new["vname"] == vnum(to):
        rows.append(("android: versionName", PASS, f"{new['vname']} (old {old['vname']})"))
    else:
        rows.append(("android: versionName", INFO,
                     f"new {new['vname']!r}, want {vnum(to)} (old {old['vname']!r}); not a failure"))


# ---------------------------------------------------------------- main

def fetch_release_file(rel, name, dest):
    """A release's file, downloaded and checked against its SHA256SUMS line."""
    exp = sums_of(rel).get(name)
    if exp is None:
        raise CheckFail(f"{rel['tag_name']} SHA256SUMS has no {name}")
    download(asset_url(rel, name), dest)
    dest.chmod(0o755)
    if sha256(dest) != exp:
        raise CheckFail(f"{name} does not match its SHA256SUMS line")
    return dest


def main(argv=None):
    ap = argparse.ArgumentParser(description="Check that the previous release updates to the new one, and that its data carries over.")
    ap.add_argument("--from", dest="frms", action="append", required=True, metavar="vX.Y.Z",
                    help="a previous release, e.g. v1.5.1 (repeat for several)")
    ap.add_argument("--to", help="the new release, e.g. v1.5.2 (must be the newest published): after the release")
    ap.add_argument("--new-bin", help="before the tag: a locally built razdor (target/release/razdor); "
                    "no self-update, only the data carry-over check")
    ap.add_argument("--scratch", help="scratch folder (default: $RAZDOR_SCRATCH, else "
                    f"{MNT_SCRATCH}/update-check/<to> or pre-tag, else a new private temp folder)")
    ap.add_argument("--keep", action="store_true", help="keep the scratch folders (also on success)")
    ap.add_argument("--mod-src", help="the mod source for the carry-over's test Hub mod: an install whose "
                    "Graphics differ from the game (default: $RAZDOR_COMMUNITY_DIR/evolution/install)")
    a = ap.parse_args(argv)
    if bool(a.to) == bool(a.new_bin):
        ap.error("give --to (after the release) or --new-bin (before the tag), one of them")

    rows, logs = [], []
    root = None
    subs = []
    title = f"Update check {' '.join(a.frms)} -> {a.to}" if a.to else \
        f"Carry-over check (before the tag) from {' '.join(a.frms)} with {a.new_bin}"
    try:
        frms = [parse_version(f) for f in a.frms]
        to = parse_version(a.to) if a.to else None
        pub = load_published()
        newest = max(pub)
        if to is not None and to != newest:
            raise ToolError(f"{vtag(to)} is not the newest published release ({vtag(newest)}); "
                            "the updater offers only the newest, so the check needs it")
        for frm in frms:
            if frm not in pub:
                raise ToolError(f"{vtag(frm)} is not a published release")
            if to is not None and frm >= to:
                raise ToolError(f"--from {vtag(frm)} must be older than --to {vtag(to)}")
        new_bin = None
        if a.new_bin:
            new_bin = Path(a.new_bin).absolute()
            if not new_bin.is_file():
                raise ToolError(f"--new-bin {new_bin} is not a file")
        root = pick_scratch(a.scratch or os.environ.get("RAZDOR_SCRATCH"),
                            vtag(to) if to else "pre-tag")
        dt_dir = find_dt_dir()
        if not dt_dir:
            rows.append(("note: RAZDOR_DT_DIR", INFO, "not set: the carry-over check cannot copy the install"))
        mod_src = a.mod_src or (os.path.join(os.environ["RAZDOR_COMMUNITY_DIR"], "evolution", "install")
                                if os.environ.get("RAZDOR_COMMUNITY_DIR") else None)
        mod_src = Path(mod_src) if mod_src and Path(mod_src).is_dir() else None

        for frm in frms:
            sub = root / vtag(frm)
            os.makedirs(sub, mode=0o700, exist_ok=True)
            subs.append(sub)
            if to is not None:
                for label, fn in (("desktop", lambda: check_desktop(frm, to, pub[frm], pub[to], sub, dt_dir, rows, logs)),
                                  ("android", lambda: check_android(frm, to, pub[frm], pub[to], sub, rows, logs))):
                    try:
                        fn()
                    except CheckFail as e:
                        rows.append((f"{vtag(frm)} {label}", FAIL, str(e)))
                    except ToolError as e:
                        rows.append((f"{vtag(frm)} {label}", ERROR, str(e)))
                # the new program the release ships: the published Linux file, as the updater would put it
                try:
                    new_rel = pub[to]
                    fetched = fetch_release_file(new_rel, f"razdor-{vnum(to)}-linux",
                                                 root / "new-bin" / vtag(to) / "razdor")
                    carry_over(frm, pub[frm], fetched, f"{vtag(to)} (published)", to, sub, dt_dir, mod_src, rows, logs)
                except CheckFail as e:
                    rows.append((f"{vtag(frm)} carry", FAIL, str(e)))
                except ToolError as e:
                    rows.append((f"{vtag(frm)} carry", ERROR, str(e)))
            else:
                carry_over(frm, pub[frm], new_bin, f"the local build ({sha256(new_bin)[:12]})", None,
                           sub, dt_dir, mod_src, rows, logs)
    except ToolError as e:
        rows.append(("input / network", ERROR, str(e)))

    results = {r for _, r, _ in rows}
    code = EXIT_FAIL if FAIL in results else EXIT_ERROR if ERROR in results else EXIT_OK

    for label, path in logs:
        if code != EXIT_OK:
            print(f"--- {label} log tail ({path}) ---\n{tail(path)}\n---", file=sys.stderr)

    width = max([len(c) for c, _, _ in rows] + [5])
    print(title)
    print(f"{'check':<{width}}  {'result':<6}  detail")
    for check, result, detail in rows:
        print(f"{check:<{width}}  {result:<6}  {detail}")
    print(f"exit {code}")

    if root and code == EXIT_OK and not a.keep:
        for sub in subs:
            shutil.rmtree(sub, ignore_errors=True)
        shutil.rmtree(root / "new-bin", ignore_errors=True)
        # the screenshots stay (the report names them); the root goes only when nothing is left in it
        try:
            root.rmdir()
        except OSError:
            pass
    return code


if __name__ == "__main__":
    sys.exit(main())
