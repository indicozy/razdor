#!/usr/bin/env python3
"""Update check: does the previous Razdor release update to the new one?

Run by the release step once the pipeline is green and the new release is published:

    python3 -I tools/release/update_check.py --from v1.5.0 --to v1.5.1

Desktop (Linux): the --from program, run off-screen on a private Xvfb with
`updates = always`, puts the --to program in place. The file on disk must then match the
--to release's SHA256SUMS line, and the updated program must start and report --to's version.

Android: the --from and --to APKs must both verify, be signed by the same certificate, have
the same package and a versionCode that does not go down (equal is allowed: Android installs
an equal versionCode over the old one). The new versionName is printed (INFO, not a failure).

Exit codes: 0 every check passed (INFO rows allowed), 1 a check failed (the previous release
does not update to this one), 2 the check could not run (network, tool or input error; --to
must be the newest published release and --from an older one).

Only stdlib. The public GitHub API and download URLs need no token. Windows, macOS and iOS
are not covered (Wine has no curl; there is no Mac here).
"""

import argparse
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


def http_get(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": UA})
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


def pick_scratch(given, frm, to):
    """The scratch folder. It runs downloaded programs, so it must be ours: not a symlink, owned by
    this user, not writable by others. Created 0700."""
    if given:
        path = Path(given).absolute()
    elif MNT_SCRATCH.is_dir():
        path = MNT_SCRATCH / "update-check" / f"{vtag(frm)}-{vtag(to)}"
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

def main(argv=None):
    ap = argparse.ArgumentParser(description="Check that the previous release updates to the new one.")
    ap.add_argument("--from", dest="frm", required=True, help="the previous release, e.g. v1.5.0")
    ap.add_argument("--to", required=True, help="the new release, e.g. v1.5.1 (must be the newest published)")
    ap.add_argument("--scratch", help="scratch folder (default: $RAZDOR_SCRATCH, else "
                    f"{MNT_SCRATCH}/update-check/<from>-<to>, else a new private temp folder)")
    ap.add_argument("--keep", action="store_true", help="keep the scratch folder (also on success)")
    a = ap.parse_args(argv)

    rows, logs = [], []
    scratch = None
    try:
        frm, to = parse_version(a.frm), parse_version(a.to)
        pub = load_published()
        newest = max(pub)
        if to != newest:
            raise ToolError(f"{vtag(to)} is not the newest published release ({vtag(newest)}); "
                            "the updater offers only the newest, so the check needs it")
        if frm >= to:
            raise ToolError(f"--from {vtag(frm)} must be older than --to {vtag(to)}")
        if frm not in pub:
            raise ToolError(f"{vtag(frm)} is not a published release")
        scratch = pick_scratch(a.scratch or os.environ.get("RAZDOR_SCRATCH"), frm, to)
        dt_dir = find_dt_dir()
        if not dt_dir:
            rows.append(("note: RAZDOR_DT_DIR", INFO, "not set; the program runs without the install"))

        for label, fn in (("desktop", lambda: check_desktop(frm, to, pub[frm], pub[to], scratch, dt_dir, rows, logs)),
                          ("android", lambda: check_android(frm, to, pub[frm], pub[to], scratch, rows, logs))):
            try:
                fn()
            except CheckFail as e:
                rows.append((label, FAIL, str(e)))
            except ToolError as e:
                rows.append((label, ERROR, str(e)))
    except ToolError as e:
        rows.append(("input / network", ERROR, str(e)))

    results = {r for _, r, _ in rows}
    code = EXIT_FAIL if FAIL in results else EXIT_ERROR if ERROR in results else EXIT_OK

    for label, path in logs:
        if code != EXIT_OK:
            print(f"--- {label} log tail ({path}) ---\n{tail(path)}\n---", file=sys.stderr)

    width = max([len(c) for c, _, _ in rows] + [5])
    print(f"Update check {a.frm} -> {a.to}")
    print(f"{'check':<{width}}  {'result':<6}  detail")
    for check, result, detail in rows:
        print(f"{check:<{width}}  {result:<6}  {detail}")
    print(f"exit {code}")

    if scratch and code == EXIT_OK and not a.keep:
        for sub in ("desktop", "android"):
            shutil.rmtree(scratch / sub, ignore_errors=True)
        try:
            scratch.rmdir()
        except OSError:
            pass
    return code


if __name__ == "__main__":
    sys.exit(main())
