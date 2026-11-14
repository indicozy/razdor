#!/usr/bin/env python3
"""Runs Razdor's terrain A/B test (backlog 1637) on one phone over adb, for the Samsung Remote Test Lab.

Usage (the phone is the lab's, connected through its Remote Debug Bridge on this PC):

    python3 -I tools/perf/samsung_lab.py --serial localhost:<port> --out <dir> \\
        [--apks /mnt/ai_models/razdor-ab-1637/v2] [--only A,C] [--choice all|mine] [--map «Другой берег»] \\
        [--timeout-min 40] [--install-timeout-min 20] [--dry-run]
    python3 -I tools/perf/samsung_lab.py --out <dir> --compare-only      # rebuild compare.md from <dir>/<X>/

For each build X in A, B, C, D (`razdor-ab-X.apk` in --apks, checked against SHA256SUMS there):

1. the serial must be `device` (else it says what to do: accept the USB-debugging prompt on the lab's
   screen, click Connect in the lab again, adb connect again);
2. the phone is recorded in <out>/device.txt (model, brand, Android, SoC, GL renderer);
3. the APK is installed with `install -r -g`; a phone that holds another signature gets the old app
   uninstalled first (its data goes: once, on a fresh phone);
4. the app starts and the script waits for the game's install to load (`install loaded:` in razdor.log;
   the first run downloads the game from the Hub: up to --install-timeout-min, with progress);
5. the app is force-stopped, `bench-autorun.txt` (`all` and `map=…`) is pushed into the app's Razdor
   folder, and the app starts again: the app runs the performance test by itself (`ui/bench_autorun.rs`);
6. the script waits for `RazdorBench DONE` (logcat) or the `bench-autorun.finished` file, up to --timeout-min;
7. <out>/<X>/ gets the results file (`razdor-bench-*.txt`), razdor.log, the trigger file and the full logcat;
   razdor.log's `terrain options:` line must name build X (else the script stops with exit 5);
8. the app is force-stopped.

Between builds the game files are kept (`install -r` over the same signature keeps the app's data).
At the end, phone_bench.py's table is written per build (<out>/<X>/table.md) and <out>/compare.md: the cells as rows,
the builds as columns (frame ms median and CPU ms median), with the device line.

Limitation: the app's folder is `/sdcard/Android/data/io.github.indicozy.razdor/files/Razdor/`. Android 11 and later
may refuse adb access to another app's folder under /sdcard/Android/data. The script reads razdor.log and pushes the
trigger there: if that is refused, it stops with exit 4 and says so. The release APKs are not debuggable, so
`run-as` does not help. The bench lines themselves also go to logcat, so the table can still be made from a logcat dump.

Exit codes: 0 done; 2 bad argument or APK missing or wrong; 3 the phone is not ready (adb state); 4 install, run
or folder access failed; 5 a build's log names another build.

Stdlib only. The remote folder and the package are constants below; a dry run prints every adb command instead.
"""

import argparse
import hashlib
import os
import re
import shlex
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import phone_bench  # noqa: E402  (the same folder: the bench lines' parser and the table)

PACKAGE = "io.github.indicozy.razdor"
APP_DIR = f"/sdcard/Android/data/{PACKAGE}/files/Razdor"
VARIANTS = ["A", "B", "C", "D"]
# The terrain option set each build fixes (ui/terrain.rs `ab_options`, the module doc's table).
LABELS = {"A": "opt=1+2+4", "B": "opt=1+2", "C": "opt=1+4", "D": "opt=none"}
MAP_DEFAULT = "Другой берег"
TERRAIN_RE = re.compile(r"terrain options: (opt=\S+).*?\(A/B build (\w)\)")
RESULT_RE = re.compile(r"^razdor-bench-.*\.txt$")
DENIED = ("Permission denied", "Operation not permitted")
POLL_SECS = 15

EXIT_OK, EXIT_ARG, EXIT_DEVICE, EXIT_RUN, EXIT_BUILD = 0, 2, 3, 4, 5


class LabError(Exception):
    def __init__(self, message, code=EXIT_RUN):
        super().__init__(message)
        self.code = code


def say(msg):
    print(msg, flush=True)


class Lab:
    """The phone, reached through adb. A dry run prints the commands and reads nothing."""

    def __init__(self, serial, dry_run):
        self.serial = serial
        self.dry_run = dry_run

    def _cmd(self, args, serial=True):
        cmd = ["adb"]
        if serial and self.serial:
            cmd += ["-s", self.serial]
        return cmd + list(args)

    def adb(self, *args, serial=True, timeout=300):
        """Runs adb; returns (exit code, stdout + stderr as text)."""
        cmd = self._cmd(args, serial)
        if self.dry_run:
            say("  $ " + shlex.join(cmd))
            return 0, ""
        try:
            p = subprocess.run(cmd, capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return 124, "adb timed out"
        except FileNotFoundError:
            raise LabError("adb is not installed or not on the PATH", EXIT_ARG)
        return p.returncode, (p.stdout + p.stderr).decode("utf-8", "replace")

    def shell(self, *args, timeout=120):
        return self.adb("shell", *args, timeout=timeout)

    def read(self, path):
        """A text file of the phone: (True, text) or (False, why)."""
        rc, out = self.shell("cat", path)
        return (rc == 0, out)


def check_device(lab, serial):
    """Returns once the serial is `device`; otherwise raises with what the owner has to do."""
    if lab.dry_run:
        say(f"  $ adb devices        (the serial {serial} must be listed as `device`)")
        return
    rc, out = lab.adb("devices", serial=False)
    state = None
    for line in out.splitlines()[1:]:
        parts = line.split()
        if len(parts) >= 2 and parts[0] == serial:
            state = parts[1]
    if state == "device":
        return
    hints = {
        "unauthorized": "accept the USB-debugging prompt on the phone's screen in the lab (Allow), then click Connect "
        "in the lab again and run adb connect again",
        "offline": f"the bridge dropped the phone: click Connect in the lab again, then `adb disconnect {serial}` and "
        f"`adb connect {serial}`",
        None: f"not connected: keep `./rdb` running, click Connect in the lab, then `adb connect {serial}`",
    }
    raise LabError(f"adb: {serial} is {state or 'not listed'}. " + hints.get(state, f"state {state!r} is not `device`"), EXIT_DEVICE)


def device_info(lab):
    """The phone's model, brand, Android, SoC and GL renderer, as `key: value` lines."""
    props = {
        "brand": "ro.product.brand",
        "model": "ro.product.model",
        "android": "ro.build.version.release",
        "sdk": "ro.build.version.sdk",
        "soc": "ro.soc.model",
        "board": "ro.board.platform",
    }
    info = {}
    for key, prop in props.items():
        rc, out = lab.shell("getprop", prop)
        info[key] = out.strip() if rc == 0 and out.strip() else "?"
    rc, out = lab.shell("dumpsys", "SurfaceFlinger")
    gles = next((ln.strip() for ln in out.splitlines() if ln.strip().startswith("GLES:")), "") if rc == 0 else ""
    info["gl"] = gles.split(":", 1)[1].strip() if gles else "?"
    if lab.dry_run:
        info = {k: "(dry run)" for k in info}
    return info


def write_device(out_dir, info):
    path = os.path.join(out_dir, "device.txt")
    with open(path, "w", encoding="utf-8") as f:
        for key, value in info.items():
            f.write(f"{key}: {value}\n")
    return path


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def check_apk(apks, letter, dry_run):
    """The APK's path, after its SHA-256 is checked against SHA256SUMS (when that file is there)."""
    path = os.path.join(apks, f"razdor-ab-{letter}.apk")
    if not os.path.isfile(path):
        if dry_run:
            say(f"  (dry run: {path} is not there; a real run stops here)")
            return path
        raise LabError(f"missing APK: {path}", EXIT_ARG)
    sums = os.path.join(apks, "SHA256SUMS")
    if os.path.isfile(sums):
        want = None
        with open(sums, encoding="utf-8") as f:
            for line in f:
                parts = line.split()
                if len(parts) >= 2 and os.path.basename(parts[-1].lstrip("*")) == os.path.basename(path):
                    want = parts[0]
        if want is None:
            raise LabError(f"{path} is not listed in SHA256SUMS", EXIT_ARG)
        if not dry_run and sha256_of(path) != want:
            raise LabError(f"{path}: the SHA-256 does not match SHA256SUMS", EXIT_ARG)
    return path


def install(lab, apk):
    rc, out = lab.adb("install", "-r", "-g", apk, timeout=900)
    if "Success" in out or lab.dry_run:
        return
    if any(s in out for s in ("INSTALL_FAILED_UPDATE_INCOMPATIBLE", "INSTALL_FAILED_VERSION_DOWNGRADE", "signatures do not match")):
        say("  another signature is on the phone: uninstalling the old app (its data goes), then a fresh install")
        lab.adb("uninstall", PACKAGE, timeout=300)
        rc, out = lab.adb("install", "-g", apk, timeout=900)
        if "Success" in out:
            return
    raise LabError(f"install failed: {out.strip() or rc}")


def wake_and_start(lab):
    # The screen stays on while the script works (the lab phone sleeps otherwise, and a download waits on it).
    lab.shell("svc", "power", "stayon", "true")
    lab.shell("input", "keyevent", "KEYCODE_WAKEUP")
    lab.shell("monkey", "-p", PACKAGE, "-c", "android.intent.category.LAUNCHER", "1")


def force_stop(lab):
    lab.shell("am", "force-stop", PACKAGE)


def folder_size_kb(lab):
    rc, out = lab.shell("du", "-sk", f"/sdcard/Android/data/{PACKAGE}/files")
    return out.split()[0] if rc == 0 and out.split() else "?"


def wait_install(lab, minutes):
    """Waits for `install loaded:` in razdor.log (the first run's download included)."""
    if lab.dry_run:
        say(f"  (dry run: polls {APP_DIR}/razdor.log every {POLL_SECS} s for `install loaded:`, up to {minutes} min)")
        return
    deadline = time.time() + minutes * 60
    last_line = ""
    while True:
        ok, text = lab.read(f"{APP_DIR}/razdor.log")
        if not ok:
            if any(d in text for d in DENIED):
                raise LabError(f"adb cannot read {APP_DIR}/razdor.log: {text.strip()} (see the limitation in this script's docstring)")
            # No log yet: the folder is made when the install loads, so the first run waits here.
        else:
            if "install failed to load" in text:
                raise LabError("the game's install failed to load: " + next(ln for ln in text.splitlines() if "install failed to load" in ln))
            if "install loaded:" in text:
                say(f"  install loaded: {next(ln for ln in text.splitlines() if 'install loaded:' in ln).split('install loaded:')[1].strip()}")
                return
            for ln in text.splitlines():
                if "game download" in ln or "install:" in ln:
                    last_line = ln.strip()
        if time.time() > deadline:
            raise LabError(f"the game's install did not load in {minutes} min (last log line: {last_line or 'none'})")
        say(f"  waiting for the game ({folder_size_kb(lab)} KB on the phone): {last_line or 'no log yet'}")
        time.sleep(POLL_SECS)


def push_trigger(lab, out_dir, choice, map_name):
    """Writes the trigger file in the variant's folder and pushes it into the app's Razdor folder."""
    trigger = os.path.join(out_dir, "bench-autorun.txt")
    if not lab.dry_run:
        with open(trigger, "w", encoding="utf-8") as f:
            f.write(f"{choice}\nmap={map_name}\n")
    lab.shell("rm", "-f", f"{APP_DIR}/bench-autorun.txt", f"{APP_DIR}/bench-autorun.done", f"{APP_DIR}/bench-autorun.finished", f"{APP_DIR}/bench-autorun.error")
    rc, out = lab.adb("push", trigger, f"{APP_DIR}/bench-autorun.txt", timeout=120)
    if rc != 0 and not lab.dry_run:
        raise LabError(f"adb push into {APP_DIR} was refused: {out.strip()} (see the limitation in this script's docstring)")


def wait_run(lab, minutes):
    """Waits for the run's end: `RazdorBench DONE` in logcat, or the finished file. An error file stops it."""
    if lab.dry_run:
        say(f"  (dry run: polls logcat for `RazdorBench DONE` and {APP_DIR}/bench-autorun.finished, up to {minutes} min)")
        return
    deadline = time.time() + minutes * 60
    while True:
        rc, out = lab.adb("logcat", "-d", "-s", "RazdorBench:I", timeout=120)
        cells = out.count("RazdorBench cell=")
        if "RazdorBench DONE" in out:
            say(f"  RazdorBench DONE ({cells} cells)")
            return
        ok, text = lab.read(f"{APP_DIR}/bench-autorun.finished")
        if ok:
            say(f"  finished: {text.strip()}")
            return
        ok, text = lab.read(f"{APP_DIR}/bench-autorun.error")
        if ok:
            raise LabError("the app did not run the test: " + text.strip())
        if time.time() > deadline:
            raise LabError(f"the test did not finish in {minutes} min ({cells} cells done)")
        say(f"  running: {cells} cells logged so far")
        time.sleep(POLL_SECS)


def collect(lab, out_dir):
    """Pulls the results file, razdor.log, the trigger's leftovers and the full logcat into out_dir."""
    if lab.dry_run:
        say(f"  (dry run: pulls {APP_DIR}/razdor-bench-*.txt, razdor.log, bench-autorun.*; dumps logcat)")
        return
    rc, listing = lab.shell("ls", APP_DIR)
    names = [n.strip() for n in listing.split()] if rc == 0 else []
    if rc != 0 and any(d in listing for d in DENIED):
        raise LabError(f"adb cannot list {APP_DIR}: {listing.strip()} (see the limitation in this script's docstring)")
    for name in names:
        if RESULT_RE.match(name) or name in ("razdor.log", "bench-autorun.finished", "bench-autorun.error", "bench-autorun.done"):
            lab.adb("pull", f"{APP_DIR}/{name}", out_dir, timeout=300)
    rc, logcat = lab.adb("logcat", "-d", timeout=300)
    with open(os.path.join(out_dir, "logcat.txt"), "w", encoding="utf-8", errors="replace") as f:
        f.write(logcat)


def check_build(out_dir, letter):
    """razdor.log's terrain line must name this build (its letter and its option set)."""
    path = os.path.join(out_dir, "razdor.log")
    if not os.path.isfile(path):
        raise LabError("razdor.log was not pulled: cannot check which build ran", EXIT_BUILD)
    with open(path, encoding="utf-8", errors="replace") as f:
        found = [m for m in TERRAIN_RE.finditer(f.read())]
    if not found:
        raise LabError("razdor.log has no `terrain options:` line (the map was not drawn?)", EXIT_BUILD)
    label, built = found[-1].group(1), found[-1].group(2)
    if built != letter or label != LABELS[letter]:
        raise LabError(f"the log names build {built} ({label}), expected {letter} ({LABELS[letter]})", EXIT_BUILD)
    say(f"  build check: {label} (A/B build {built}) as expected")


def load_cells(path):
    """The cells of a results file (or a logcat dump): ({cell id: pairs}, done)."""
    cells, done = {}, False
    with open(path, encoding="utf-8", errors="replace") as f:
        for payload in phone_bench.read_lines(f):
            if payload == "DONE":
                done = True
                continue
            pairs = phone_bench.parse_pairs(payload)
            if "cell" in pairs:
                cells[pairs["cell"]] = pairs
    return cells, done


def variant_results(out_dir, letter):
    """The variant's results file (the newest `razdor-bench-*.txt`), else its logcat dump."""
    folder = os.path.join(out_dir, letter)
    names = sorted(n for n in os.listdir(folder) if RESULT_RE.match(n)) if os.path.isdir(folder) else []
    if names:
        return os.path.join(folder, names[-1])
    logcat = os.path.join(folder, "logcat.txt")
    return logcat if os.path.isfile(logcat) else None


def fmt_ms(value):
    try:
        return f"{float(value):.1f}"
    except ValueError:
        return value


def compare(out_dir, letters):
    """Writes <out>/<X>/table.md per build and <out>/compare.md (cells as rows; frame and CPU medians as columns)."""
    results = {}
    for letter in letters:
        path = variant_results(out_dir, letter)
        if path is None:
            continue
        table_path = os.path.join(out_dir, letter, "table.md")
        phone_bench.main([path, "-o", table_path])
        cells, done = load_cells(path)
        results[letter] = (cells, done, path)
    if not results:
        raise LabError(f"no results under {out_dir}", EXIT_RUN)
    order = []
    for letter in letters:
        if letter in results:
            for cid in results[letter][0]:
                if cid not in order:
                    order.append(cid)
    first = results[next(iter(results))][0]
    names = {cid: pairs.get("name", cid) for cid, pairs in first.items()}
    for letter in results:
        for cid, pairs in results[letter][0].items():
            names.setdefault(cid, pairs.get("name", cid))
    head = ["| Cell | " + " | ".join(f"{v} frame ms (median) | {v} CPU ms (median)" for v in results) + " |"]
    head.append("|---|" + "---|---|" * len(results))
    rows = []
    for cid in order:
        cols = []
        for letter in results:
            pairs = results[letter][0].get(cid, {})
            cols.append(pairs.get("frame_ms_med", "–"))
            cols.append(pairs.get("cpu_ms_med", "–"))
        rows.append(f"| {names[cid]} | " + " | ".join(fmt_ms(c) for c in cols) + " |")
    device = {}
    dev_path = os.path.join(out_dir, "device.txt")
    if os.path.isfile(dev_path):
        with open(dev_path, encoding="utf-8") as f:
            device = dict(ln.split(": ", 1) for ln in f.read().splitlines() if ": " in ln)
    lines = ["# Razdor terrain A/B test (backlog 1637)", ""]
    if device:
        lines.append(f"Phone: {device.get('brand', '?')} {device.get('model', '?')}, Android {device.get('android', '?')}, "
                     f"SoC {device.get('soc', '?')}, GL: {device.get('gl', '?')}")
    for letter in results:
        cells, done, path = results[letter]
        pairs = next(iter(cells.values()), {})
        lines.append(f"- {letter} ({LABELS[letter]}): {'finished' if done else 'NOT finished'}, "
                     f"screen {pairs.get('screen', '?')}, {os.path.basename(path)}")
    lines += ["", *head, *rows, ""]
    for letter in letters:
        if letter not in results:
            lines.append(f"Build {letter}: no results.")
    text = "\n".join(lines) + "\n"
    with open(os.path.join(out_dir, "compare.md"), "w", encoding="utf-8") as f:
        f.write(text)
    return os.path.join(out_dir, "compare.md")


def run_variant(lab, args, letter, out_dir):
    apk = check_apk(args.apks, letter, lab.dry_run)
    say(f"== build {letter} ({LABELS[letter]}): {apk}")
    if not lab.dry_run:
        os.makedirs(out_dir, exist_ok=True)
    install(lab, apk)
    wake_and_start(lab)
    wait_install(lab, args.install_timeout_min)
    force_stop(lab)
    push_trigger(lab, out_dir, args.choice, args.map)
    lab.adb("logcat", "-c")
    wake_and_start(lab)
    try:
        wait_run(lab, args.timeout_min)
    finally:
        collect(lab, out_dir)
        force_stop(lab)
    if not lab.dry_run:
        check_build(out_dir, letter)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--serial", help="the phone's adb serial (localhost:<port> through the lab's rdb)")
    ap.add_argument("--apks", default="/mnt/ai_models/razdor-ab-1637/v2", help="the folder with razdor-ab-{A,B,C,D}.apk")
    ap.add_argument("--out", required=True, help="the results folder (device.txt, <X>/, compare.md)")
    ap.add_argument("--only", default="A,B,C,D", help="the builds to run, e.g. A,C")
    ap.add_argument("--choice", default="all", choices=["all", "mine"], help="the test's choice (default: all presets)")
    ap.add_argument("--map", default=MAP_DEFAULT, help=f"the map's file name in the game (default: {MAP_DEFAULT})")
    ap.add_argument("--timeout-min", type=float, default=40, help="the test's time limit per build, in minutes")
    ap.add_argument("--install-timeout-min", type=float, default=20, help="the game's install time limit, in minutes")
    ap.add_argument("--dry-run", action="store_true", help="print the adb commands; touch no phone")
    ap.add_argument("--compare-only", action="store_true", help="rebuild compare.md from the folders under --out")
    args = ap.parse_args(argv)
    letters = [x.strip().upper() for x in args.only.split(",") if x.strip()]
    if not letters or any(x not in VARIANTS for x in letters):
        say("samsung_lab: --only takes letters from A, B, C, D")
        return EXIT_ARG
    if args.compare_only:
        say(f"compare: {compare(args.out, letters)}")
        return EXIT_OK
    if not args.serial and not args.dry_run:
        say("samsung_lab: --serial is needed (localhost:<port> from the lab's rdb)")
        return EXIT_ARG
    serial = args.serial or "localhost:0"
    lab = Lab(args.serial, args.dry_run)
    if not args.dry_run:
        os.makedirs(args.out, exist_ok=True)
    try:
        check_device(lab, serial)
        info = device_info(lab)
        if not args.dry_run:
            say(f"phone: {info.get('brand', '?')} {info.get('model', '?')}, Android {info.get('android', '?')}, GL {info.get('gl', '?')}")
        if not args.dry_run:
            write_device(args.out, info)
        for letter in letters:
            run_variant(lab, args, letter, os.path.join(args.out, letter))
        if args.dry_run:
            say("(dry run: no table, no compare.md)")
            return EXIT_OK
        say(f"compare: {compare(args.out, letters)}")
    except LabError as e:
        say(f"samsung_lab: {e}")
        return e.code
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
