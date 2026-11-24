#!/usr/bin/env python3
"""Android update check: does the old APK take the new one over, and does its data survive?

Release step 6c (after the release, with the published APKs; the emulator run):

    python3 -I tools/release/android_update_check.py --from v1.5.3 --to v1.5.4

Before the tag (step 3c): only the static rules, and only with a release-signed APK (the pipeline's APK is
unsigned until its signing job, and an APK signed with another key cannot install over a release):

    python3 -I tools/release/android_update_check.py --from v1.5.3 --new-apk /path/signed.apk --static-only

What it checks:

1. The APKs (`update_check.check_apk_pair`): both verify, the same signing certificate, the same
   package, a versionCode that does not go down. The old APK comes from its release and must match
   its SHA256SUMS line; with `--new-apk` the new one is a local file (a local build is not signed
   with the release key, so its certificate is INFO, and the install decides).
2. On a private headless emulator (a copy of the AVD; a private Xvfb, never the owner's display;
   `-gpu host`, Mesa, the software renderer; pinned to the `ai-resources cpus` at nice 19): the old APK is
   installed and started, its log names its version. Then the data is made the way a player's is: the game's
   own settings file `saves/audio.json` with two values the game keeps (`music_volume` 0.37, `orientation`
   landscape, not the defaults) and a file in the saves folder, pushed with `adb push`, then given to the app's
   uid (`adb root`, `chown`: the push writes files as the shell user, and the app then cannot read them). The old
   app is started again to read them; it must read them (the values survive its own run, no permission error).
3. The new APK is installed over it with `adb install -r`; the package reports the new version; it starts, its
   log names the new version, and no panic or fatal exception shows in the log or in logcat for its process; the
   saves file is unchanged, and `saves/audio.json` still holds the two values (compared field by field: the new
   version may add fields or migrate the file; a missing file or a changed value fails).

Exit codes: 0 every check passed (INFO rows allowed), 1 a check failed (the install or the data),
2 the check could not run (network, the SDK, the emulator would not boot, an input error).

The adb calls go through `Device`, which the tests replace with a fake (`tools/release/test_android.py`).
Scratch: `--scratch`, else the same rule as update_check (`/mnt/ai_models/razdor-scratch/update-check/<to>`
or `android-<from>` for a pre-tag run); the AVD copy (2.4 GB) and the emulator's home are removed at the
end unless `--keep`, the screenshots and logs stay in the scratch folder.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import update_check as uc  # noqa: E402
from update_check import CheckFail, ToolError, PASS, INFO, FAIL, ERROR, EXIT_OK, EXIT_FAIL, EXIT_ERROR  # noqa: E402

PKG = "io.github.indicozy.razdor"
APP_FILES = f"/storage/emulated/0/Android/data/{PKG}/files/Razdor"
REMOTE_LOG = f"{APP_FILES}/razdor.log"
REMOTE_SAVES = f"{APP_FILES}/saves"  # the game's saves folder (rules::save::default_dir)
REMOTE_SAVE_FILE = f"{REMOTE_SAVES}/carry-test.txt"
REMOTE_AUDIO = f"{REMOTE_SAVES}/audio.json"  # the game's settings (ui::audio::Settings::path)
SAVE_FILE_TEXT = b"carry-over test: a file in the saves folder\n"
# The settings the test sets: two values the game keeps, both away from their defaults (music 0.6, orientation Auto).
# Settings is serde(default), so a file with only these keys loads; the other keys take their defaults.
AUDIO_TEST = {"music_volume": 0.37, "orientation": "landscape"}
AUDIO_EPS = 0.005  # the volume is saved in 1% steps; a float's round trip stays well inside this
AVD_NAME = "razdor"
DEFAULT_AVD = uc.MNT_SCRATCH / "android-apk" / "avd"
DEFAULT_PORT = 5554
BOOT_SECONDS = 600  # the first boot of the image took about two minutes
LOG_SECONDS = 90  # the app writes its log soon after it starts
RUN_SECONDS = 30  # each run of an app: the program plays a little (its start screen)
GPU = "host"  # swiftshader_indirect segfaults on the Xvfb run here (exit -11); host GL on Xvfb with Mesa boots
VERSION_LINE = re.compile(r"Razdor (\d+\.\d+\.\d+) \(")
PANIC_WORDS = ("panicked", "FATAL EXCEPTION", "fatal signal")


# ---------------------------------------------------------------- pure parts (tested)

def parse_dumpsys_version(text):
    """(versionName, versionCode) from `dumpsys package <pkg>`; None for a missing one."""
    name = re.search(r"^\s*versionName=(\S+)", text, re.M)
    code = re.search(r"^\s*versionCode=(\d+)", text, re.M)
    return (name.group(1) if name else None, int(code.group(1)) if code else None)


def parse_user_id(text):
    """The app's uid from `pm list packages -U <pkg>` (`package:<pkg> uid:<n>`), or None. (dumpsys has no
    `userId=` line on this Android: it shows `appId=` in some versions.)"""
    m = re.search(rf"package:{re.escape(PKG)}\s+uid:(\d+)", text or "")
    return int(m.group(1)) if m else None


def parse_pid(text):
    """The first pid in a `pidof` / `ps` answer, or None."""
    for tok in (text or "").split():
        if tok.isdigit():
            return int(tok)
    return None


def log_version(text):
    """The version the app's log names on its first line (the app writes it at each start), or None."""
    for line in (text or "").splitlines():
        m = VERSION_LINE.search(line)
        if m:
            return m.group(1)
    return None


def log_panics(text):
    """The crash lines of the app's own log (the file is the app's: every line there is its own)."""
    return [ln.strip() for ln in (text or "").splitlines() if any(w in ln for w in PANIC_WORDS)]


def logcat_panics(text, pid):
    """The crash lines of one process in logcat (`threadtime` format: the pid is the third field)."""
    out = []
    for line in (text or "").splitlines():
        toks = line.split()
        if len(toks) > 2 and toks[2] == str(pid) and any(w in line for w in PANIC_WORDS):
            out.append(line.strip())
    return out


def signature(line):
    """A crash line without its time, pid and tag: the same panic in two runs has the same signature."""
    return re.sub(r"^\S+\s+\S+\s+\d+\s+\d+\s+\S+\s+\S+\s*:?\s*", "", line.strip())


def audio_bytes(values=None):
    """The test's `saves/audio.json`: the values as the game's JSON (the other settings take their defaults)."""
    return (json.dumps(AUDIO_TEST if values is None else values, indent=2) + "\n").encode("utf-8")


def audio_check(text, values=None):
    """(result, detail) for the test's values in the game's `saves/audio.json` after an update. `text` is the file's
    text, or None when it is missing. Each field must be there and equal (the volume within AUDIO_EPS); extra
    fields and a rewrite by the app (new fields, a migrated layout) are fine. The detail names the fields kept."""
    want = AUDIO_TEST if values is None else values
    if text is None:
        return FAIL, "saves/audio.json is missing after the update"
    try:
        got = json.loads(text)
    except ValueError:
        return FAIL, "saves/audio.json is no longer JSON after the update"
    if not isinstance(got, dict):
        return FAIL, "saves/audio.json is not a JSON object after the update"
    kept, lost = [], []
    for key, value in want.items():
        now = got.get(key)
        if isinstance(value, float):
            ok = isinstance(now, (int, float)) and not isinstance(now, bool) and abs(now - value) <= AUDIO_EPS
        else:
            ok = now == value
        if ok:
            kept.append(f"{key} = {value!r}")
        else:
            lost.append(f"{key} is {now!r}, want {value!r}")
    if lost:
        return FAIL, "settings lost or changed after the update: " + "; ".join(lost)
    return PASS, "kept: " + ", ".join(kept)


def denied_lines(text):
    """The app's own log lines that report a file it could not read or write (its diag line names the path)."""
    return [ln.strip() for ln in (text or "").splitlines() if "Permission denied" in ln and "audio.json" in ln]


def emulator_command(sdk, avd_home, avd_name, port, cpus, gpu=GPU, nice=19):
    """The emulator's argv, pinned and at low priority. The DISPLAY and the environment go in
    `emulator_env`."""
    emulator = os.path.join(sdk, "emulator", "emulator")
    return ["taskset", "-c", cpus, "nice", "-n", str(nice), emulator, "-avd", avd_name,
            "-no-window", "-gpu", gpu, "-feature", "-Vulkan", "-no-audio", "-no-boot-anim", "-no-snapshot",
            "-no-metrics", "-port", str(port)]


def emulator_env(sdk, avd_home, emu_home, display, base_env=None):
    """The emulator's environment: the private display only, the SDK and the AVD folder, and a
    software GL vendor (so no GPU is used, whatever the X server offers)."""
    env = {k: v for k, v in (base_env or os.environ).items()
           if not k.startswith("RAZDOR_") and k not in ("DISPLAY", "WAYLAND_DISPLAY", "XAUTHORITY")}
    env.update({
        "DISPLAY": display,
        "ANDROID_SDK_ROOT": sdk,
        "ANDROID_HOME": sdk,
        "ANDROID_AVD_HOME": str(avd_home),
        "ANDROID_EMULATOR_HOME": str(emu_home),
        "__GLX_VENDOR_LIBRARY_NAME": "mesa",
        "LIBGL_ALWAYS_SOFTWARE": "1",
    })
    return env


def avd_ini_text(text, avd_dir):
    """The AVD's registration file with its path pointed at `avd_dir` (the copy)."""
    out = []
    for line in text.splitlines():
        out.append(f"path={avd_dir}" if line.startswith("path=") else line)
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- the device (adb)

class Device:
    """The adb calls of one emulator. `runner(argv, timeout)` gives (returncode, stdout, stderr);
    the tests pass a fake."""

    def __init__(self, adb, serial, runner=None):
        self.adb = adb
        self.serial = serial
        self.runner = runner or self._run

    @staticmethod
    def _run(argv, timeout):
        try:
            r = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired as e:
            raise ToolError(f"adb timed out: {' '.join(argv[-3:])}") from e
        except OSError as e:
            raise ToolError(f"cannot run adb: {e}") from e
        return r.returncode, r.stdout, r.stderr

    def call(self, args, timeout=60):
        return self.runner([self.adb, "-s", self.serial] + list(args), timeout)

    def shell(self, cmd, timeout=30):
        rc, out, err = self.call(["shell", cmd], timeout)
        return rc, out

    def listed(self):
        rc, out, _ = self.runner([self.adb, "devices"], 20)
        return any(line.split()[:1] == [self.serial] for line in out.splitlines()[1:])

    def booted(self):
        rc, out = self.shell("getprop sys.boot_completed", 15)
        return rc == 0 and out.strip() == "1"

    def install(self, apk, timeout=300):
        rc, out, err = self.call(["install", "-r", str(apk)], timeout)
        if rc != 0 or "Success" not in out:
            raise CheckFail(f"adb install failed: {(out + err).strip()[-300:]}")

    def uninstall(self):
        self.call(["uninstall", PKG], 60)  # a clean start: a missing package is fine

    def version(self):
        rc, out = self.shell(f"dumpsys package {PKG}", 60)
        return parse_dumpsys_version(out)

    def pid(self):
        rc, out = self.shell(f"pidof {PKG}", 15)
        return parse_pid(out)

    def start(self):
        self.shell(f"monkey -p {PKG} -c android.intent.category.LAUNCHER 1", 60)

    def force_stop(self):
        self.shell(f"am force-stop {PKG}", 30)

    def cat(self, remote):
        """The remote file's text, or None when it is not there. It is pulled, not catted: `adb shell`
        runs through a terminal, which turns LF into CRLF and would change the bytes."""
        fd, tmp = tempfile.mkstemp(prefix="adb-pull-")
        os.close(fd)
        try:
            if not self.pull(remote, tmp):
                return None
            return Path(tmp).read_bytes().decode("utf-8", errors="replace")
        finally:
            os.unlink(tmp)

    def push(self, local, remote):
        self.shell(f"mkdir -p {os.path.dirname(remote)}", 30)
        rc, out, err = self.call(["push", str(local), remote], 120)
        if rc != 0:
            raise ToolError(f"adb push failed: {(out + err).strip()[-200:]}")

    def root(self):
        """`adb root` (the emulator image lets adbd run as root): the shell is root from here, until the next boot."""
        rc, out, err = self.call(["root"], 60)
        if rc != 0:
            raise ToolError(f"adb root refused: {(out + err).strip()[-200:]}")
        self.call(["wait-for-device"], 120)
        rc, out = self.shell("id -u", 30)
        if out.strip() != "0":
            raise ToolError("adb root did not give a root shell")

    def app_uid(self):
        """The app's uid (`pm list packages -U`)."""
        rc, out = self.shell(f"pm list packages -U {PKG}", 60)
        uid = parse_user_id(out)
        if uid is None:
            raise ToolError(f"no uid for {PKG} in `pm list packages -U`: {out.strip()[-200:]}")
        return uid

    def give_to_app(self, remote_dir, uid):
        """Gives a folder and everything in it to the app's uid. The push and the mkdir make them as the shell user,
        and then the app cannot read or write them (`Permission denied`; the game ignores the setting). Needs root."""
        rc, out = self.shell(f"chown -R {uid}:{uid} {remote_dir} && stat -c %u {remote_dir}", 60)
        if out.strip() != str(uid):
            raise ToolError(f"could not give {remote_dir} to uid {uid}: {out.strip()[-200:]}")

    def pull(self, remote, local):
        rc, out, err = self.call(["pull", remote, str(local)], 120)
        return rc == 0 and Path(local).is_file()

    def logcat(self):
        rc, out = self.shell("logcat -d -t 2000", 60)
        return out if rc == 0 else ""

    def kill_emulator(self):
        self.call(["emu", "kill"], 30)


# ---------------------------------------------------------------- the emulator run

def wait_for(check, seconds, what, interval=5):
    t0 = time.monotonic()
    while time.monotonic() - t0 < seconds:
        if check():
            return True
        time.sleep(interval)
    raise ToolError(f"{what} did not happen within {seconds} s")


def wait_log(dev, want, seconds=None):
    """The app's log once it names `want` (the version it runs), else the last one read."""
    seconds = LOG_SECONDS if seconds is None else seconds
    t0 = time.monotonic()
    while True:
        text = dev.cat(REMOTE_LOG)
        if text and log_version(text) == want:
            return text
        if time.monotonic() - t0 >= seconds:
            return text
        time.sleep(3)


def copy_avd(src, dst):
    """A copy of the AVD (its folder and its registration file), so the emulator writes to the copy."""
    src, dst = Path(src), Path(dst)
    if not (src / f"{AVD_NAME}.avd").is_dir() or not (src / f"{AVD_NAME}.ini").is_file():
        raise ToolError(f"no AVD {AVD_NAME} under {src}")
    dst.mkdir(parents=True, exist_ok=True)
    if not (dst / f"{AVD_NAME}.avd").exists():
        shutil.copytree(src / f"{AVD_NAME}.avd", dst / f"{AVD_NAME}.avd", symlinks=False)
    (dst / f"{AVD_NAME}.ini").write_text(
        avd_ini_text((src / f"{AVD_NAME}.ini").read_text(encoding="utf-8"), dst / f"{AVD_NAME}.avd"),
        encoding="utf-8")


def cpus_for_run():
    try:
        r = subprocess.run(["ai-resources", "cpus"], capture_output=True, text=True, timeout=20)
        if r.returncode == 0 and re.fullmatch(r"\d+-\d+", r.stdout.strip()):
            return r.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        pass
    return "8-19"


def sdk_root(given):
    sdk = given or os.environ.get("ANDROID_SDK_ROOT") or os.environ.get("ANDROID_HOME") or str(uc.ANDROID_SDK_DEFAULT)
    if not Path(sdk, "emulator", "emulator").is_file() or not Path(sdk, "platform-tools", "adb").is_file():
        raise ToolError(f"the Android SDK (emulator, platform-tools) is not under {sdk}: set ANDROID_HOME")
    return sdk


def start_emulator(sdk, avd_home, emu_home, display, opts, cpus, log_path):
    """Starts one emulator on the private display; its output goes to `log_path`."""
    env = emulator_env(sdk, avd_home, emu_home, display)
    with open(log_path, "wb") as f:
        return subprocess.Popen(emulator_command(sdk, avd_home, AVD_NAME, opts.port, cpus, opts.gpu),
                                env=env, stdin=subprocess.DEVNULL, stdout=f, stderr=subprocess.STDOUT,
                                start_new_session=True)


def wait_boot(dev, proc, log_path):
    """Waits until the emulator has booted; returns the seconds it took."""
    t0 = time.monotonic()
    while not dev.booted():
        if proc.poll() is not None:
            raise ToolError(f"the emulator stopped (exit {proc.returncode}) before it booted: see {log_path}")
        if time.monotonic() - t0 > BOOT_SECONDS:
            raise ToolError(f"the emulator did not boot within {BOOT_SECONDS} s: see {log_path}")
        time.sleep(5)
    return int(time.monotonic() - t0)


def stop_emulator(dev, proc, scratch):
    """Stops one emulator: adb's `emu kill`, then the process group (its disk changes are kept)."""
    try:
        dev.kill_emulator()
    except ToolError:
        pass
    uc.stop(proc)


def emulator_run(old_apk, new_apk, old_name, new_name, scratch, opts, rows, logs):
    """Installs the old APK on a fresh headless emulator, makes data, installs the new APK over it,
    and checks the result. Rows are appended; a failed check raises CheckFail."""
    sdk = sdk_root(opts.sdk)
    adb = str(Path(sdk, "platform-tools", "adb"))
    serial = f"emulator-{opts.port}"
    dev = Device(adb, serial)
    if dev.listed():
        raise ToolError(f"a device is already on {serial}: stop it, or pick --port")

    avd_home = scratch / "avd"
    emu_home = scratch / "emu-home"
    emu_home.mkdir(parents=True, exist_ok=True)
    copy_avd(opts.avd_src, avd_home)
    cpus = cpus_for_run()
    xvfb = None
    emus = []  # every emulator process this run started (stopped at the end)
    env_holder = {}

    def boot(n):
        """Starts the emulator (run n of this check) and waits until it has booted; returns the seconds."""
        log_path = scratch / f"emulator-{n}.log"
        proc = start_emulator(sdk, avd_home, emu_home, env_holder["display"], opts, cpus, log_path)
        emus.append(proc)
        logs.append((f"emulator {n}", log_path))
        return wait_boot(dev, proc, log_path)

    try:
        xvfb, display = uc.start_xvfb(scratch / "xvfb.log")
        env_holder["display"] = display
        secs = boot(1)
        rows.append(("android: emulator boots headless", PASS,
                     f"{serial} on a private display, GPU {opts.gpu}, CPUs {cpus}, {secs} s"))

        # the old APK, a clean install
        dev.uninstall()
        dev.install(old_apk)
        name, code = dev.version()
        if name != old_name:
            raise CheckFail(f"the old APK installed as {name!r}, want {old_name}")
        rows.append(("android: old APK installs", PASS, f"{PKG} {name} ({code})"))
        dev.start()
        text = wait_log(dev, old_name)
        if log_version(text) != old_name:
            raise CheckFail(f"the old app's log does not name {old_name} (first line: {(text or '')[:120]!r})")
        rows.append(("android: old app starts", PASS, f"its log names Razdor {old_name}"))

        # data, the way a player's is made: the game's settings and a file in its saves folder. The app made its
        # folder on the first start, so it is stopped, the data is pushed and given to its uid, and it is started
        # again (the game reads its settings at start).
        dev.force_stop()
        data_dir = scratch / "data"
        data_dir.mkdir(exist_ok=True)
        (data_dir / "audio.json").write_bytes(audio_bytes())
        (data_dir / "carry-test.txt").write_bytes(SAVE_FILE_TEXT)
        dev.push(data_dir / "audio.json", REMOTE_AUDIO)
        dev.push(data_dir / "carry-test.txt", REMOTE_SAVE_FILE)
        uid = dev.app_uid()
        dev.root()
        dev.give_to_app(REMOTE_SAVES, uid)
        rows.append(("android: data made (saves/audio.json, a saves file)", PASS,
                     f"pushed with adb, given to the app's uid {uid}"))
        dev.start()

        time.sleep(RUN_SECONDS)
        old_pid = dev.pid()
        if old_pid is None:
            raise CheckFail(f"{old_name} is not running after {RUN_SECONDS} s")
        old_log = dev.cat(REMOTE_LOG) or ""
        old_logcat = dev.logcat()
        (scratch / "old-run.log").write_text(old_log, encoding="utf-8")
        (scratch / "old-logcat.txt").write_text(old_logcat, encoding="utf-8")
        old_crashes = log_panics(old_log) + logcat_panics(old_logcat, old_pid)
        old_sigs = {signature(ln) for ln in old_crashes}
        rows.append((f"android: v{old_name} runs {RUN_SECONDS} s", PASS if not old_crashes else INFO,
                     "no crash line" if not old_crashes else f"its own crash lines: {old_crashes[0][:140]}"))
        old_audio = dev.cat(REMOTE_AUDIO)
        result, detail = audio_check(old_audio)
        if result == PASS and denied_lines(old_log):
            result, detail = FAIL, f"the app cannot write it: {denied_lines(old_log)[0][:160]}"
        if result != PASS:
            raise ToolError(f"the old app did not read the test's saves/audio.json (the test cannot run): {detail}")
        rows.append((f"android: v{old_name} reads the test's saves/audio.json", PASS, detail))
        dev.force_stop()
        # the data must be on the device before the emulator stops: flushed, and read back as pushed
        dev.shell("sync", 60)
        if dev.cat(REMOTE_SAVE_FILE) != SAVE_FILE_TEXT.decode("utf-8"):
            raise CheckFail("the test's saves file is not on the device before the restart (the push did not hold)")
        rows.append(("android: test data on the device before the restart", PASS, "the saves file reads back as pushed"))
        stop_emulator(dev, emus.pop(), scratch)
        wait_for(lambda: not dev.listed(), 60, "the emulator's shutdown")
        secs = boot(2)
        rows.append(("android: emulator restarted for the new version (a fresh GL session)", PASS, f"{secs} s"))
        dev.root()

        # the new APK over the old one
        dev.install(new_apk)
        name, code = dev.version()
        if name != new_name:
            raise CheckFail(f"after the install the package is {name!r}, want {new_name}")
        rows.append(("android: new APK installs over the old (adb install -r)", PASS, f"{name} ({code})"))
        dev.start()
        text = wait_log(dev, new_name)
        new_pid = dev.pid()
        if log_version(text) != new_name:
            raise CheckFail(f"the updated app's log does not name {new_name} (first line: {(text or '')[:120]!r})")
        rows.append(("android: updated app starts", PASS, f"its log names Razdor {new_name}"))
        time.sleep(RUN_SECONDS)
        new_pid = dev.pid()
        if new_pid is None:
            raise CheckFail(f"{"v" + new_name} is not running after {RUN_SECONDS} s")
        new_log = dev.cat(REMOTE_LOG) or ""
        new_logcat = dev.logcat()
        (scratch / "new-run.log").write_text(new_log, encoding="utf-8")
        (scratch / "new-logcat.txt").write_text(new_logcat, encoding="utf-8")
        new_crashes = log_panics(new_log) + logcat_panics(new_logcat, new_pid)
        new_only = [ln for ln in new_crashes if signature(ln) not in old_sigs]
        if new_only:
            raise CheckFail(f"v{new_name} shows a crash the old version does not: {new_only[0][:200]}")
        if new_crashes:
            rows.append(("android: crash lines of the new version", INFO,
                         f"the same {len(new_crashes)} line(s) as the old version on this emulator "
                         f"(an emulator limit, not the update): {new_crashes[0][:120]}"))
        else:
            rows.append(("android: no crash in the log or logcat", PASS, f"pid {new_pid}"))

        # the data after the update
        save_now = dev.cat(REMOTE_SAVE_FILE)
        if save_now is None or save_now.encode("utf-8") != SAVE_FILE_TEXT:
            _, listing = dev.shell(f"ls -la {APP_FILES} {APP_FILES}/saves", 30)
            raise CheckFail("the saves folder's test file changed or is gone after the update; the folder now: "
                            + " | ".join(ln for ln in listing.splitlines() if ln.strip())[:400])
        rows.append(("android: saves folder file kept", PASS, "unchanged"))
        audio_now = dev.cat(REMOTE_AUDIO)
        result, detail = audio_check(audio_now)
        new_denied = denied_lines(new_log)
        if result == PASS and new_denied:
            result, detail = FAIL, f"the new version cannot write it: {new_denied[0][:160]}"
        rows.append(("android: settings kept (saves/audio.json)", result, detail))
        if result == FAIL:
            raise CheckFail(detail)
    finally:
        for proc in list(emus):
            try:
                if dev.listed():
                    dev.kill_emulator()
            except ToolError:
                pass
            uc.stop(proc)
        uc.stop(xvfb)
        if not opts.keep:
            shutil.rmtree(avd_home, ignore_errors=True)
            shutil.rmtree(emu_home, ignore_errors=True)


# ---------------------------------------------------------------- the whole check

def run(opts, rows, logs):
    if opts.to and opts.new_apk:
        raise ToolError("give --to (the published APK) or --new-apk (a local one), not both")
    frm = uc.parse_version(opts.frm)
    to = uc.parse_version(opts.to) if opts.to else None
    pub = uc.load_published()
    if to is not None:
        newest = max(pub)
        if to != newest:
            raise ToolError(f"{uc.vtag(to)} is not the newest published release ({uc.vtag(newest)})")
    if frm not in pub:
        raise ToolError(f"{uc.vtag(frm)} is not a published release")
    if to is not None and frm >= to:
        raise ToolError(f"--from {uc.vtag(frm)} must be older than --to {uc.vtag(to)}")
    tag = uc.vtag(to) if to else "pre-tag"
    scratch = uc.pick_scratch(opts.scratch or os.environ.get("RAZDOR_SCRATCH"), f"android-{uc.vtag(frm)}-{tag}")
    old_apk = scratch / "old" / f"razdor-{uc.vnum(frm)}.apk"
    old_apk = uc.fetch_release_file(pub[frm], f"razdor-{uc.vnum(frm)}.apk", old_apk)
    rows.append((f"android: {uc.vtag(frm)} APK", PASS, "sha256 matches its SHA256SUMS line"))
    bt = uc.build_tools(sdk_root(opts.sdk))
    if to is not None:
        new_apk = uc.fetch_release_file(pub[to], f"razdor-{uc.vnum(to)}.apk", scratch / "new" / f"razdor-{uc.vnum(to)}.apk")
        rows.append((f"android: {uc.vtag(to)} APK", PASS, "sha256 matches its SHA256SUMS line"))
        new_label, new_name = uc.vtag(to), uc.vnum(to)
    else:
        new_apk = Path(opts.new_apk).absolute()
        if not new_apk.is_file():
            raise ToolError(f"--new-apk {new_apk} is not a file")
        new_name = uc.badging(bt / "aapt2", new_apk)["vname"]
        if not new_name:
            raise ToolError(f"{new_apk.name} has no versionName")
        new_label = f"local build {new_name}"
    before = len(rows)
    uc.check_apk_pair(old_apk, new_apk, uc.vtag(frm), new_label, bt, rows, want_vname=new_name)
    if to is None:
        for i in range(before, len(rows)):
            if rows[i][0] == "android: same signing certificate":
                rows[i] = ("android: same signing certificate", INFO,
                           "a local build is not signed with the release key: the install decides")
    if opts.static_only:
        rows.append(("android: emulator", INFO, "skipped (--static-only)"))
        return
    emulator_run(old_apk, new_apk, uc.vnum(frm), new_name, scratch, opts, rows, logs)
    if not opts.keep:
        shutil.rmtree(scratch / "old", ignore_errors=True)
        shutil.rmtree(scratch / "new", ignore_errors=True)
        shutil.rmtree(scratch / "data", ignore_errors=True)


def main(argv=None):
    ap = argparse.ArgumentParser(description="Android update check: the old APK takes the new one over, its data survives.")
    ap.add_argument("--from", dest="frm", required=True, metavar="vX.Y.Z", help="the old release, e.g. v1.5.3")
    ap.add_argument("--to", metavar="vX.Y.Z", help="the new release (the newest published; after the release)")
    ap.add_argument("--new-apk", metavar="PATH", help="a local APK as the new one (before the tag)")
    ap.add_argument("--static-only", action="store_true", help="the APK rules only, no emulator")
    ap.add_argument("--sdk", help="the Android SDK (default: $ANDROID_HOME, else the scratch SDK)")
    ap.add_argument("--avd-src", default=str(DEFAULT_AVD), help="the folder with the AVD (default: the scratch AVD)")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT, help="the emulator's console port (even, 5554-5584)")
    ap.add_argument("--gpu", default=GPU, help="the emulator's -gpu mode (default: host, which Mesa draws on the private "
                    "Xvfb; swiftshader_indirect crashed the emulator here)")
    ap.add_argument("--scratch", help="scratch folder (default: $RAZDOR_SCRATCH, else the update-check scratch)")
    ap.add_argument("--keep", action="store_true", help="keep the AVD copy and the emulator's home")
    opts = ap.parse_args(argv)
    if not (opts.to or opts.new_apk):
        ap.error("give --to (after the release) or --new-apk (before the tag)")
    if opts.port % 2 or not (5554 <= opts.port <= 5584):
        ap.error("--port must be an even number from 5554 to 5584")

    rows, logs = [], []
    try:
        run(opts, rows, logs)
    except CheckFail as e:
        rows.append(("android update check", FAIL, str(e)))
    except ToolError as e:
        rows.append(("android update check", ERROR, str(e)))

    results = {r for _, r, _ in rows}
    code = EXIT_FAIL if FAIL in results else EXIT_ERROR if ERROR in results else EXIT_OK
    width = max([len(c) for c, _, _ in rows] + [5])
    print("Android update check")
    print(f"{'check':<{width}}  {'result':<6}  detail")
    for check, result, detail in rows:
        print(f"{check:<{width}}  {result:<6}  {detail}")
    for label, path in logs:
        if code != EXIT_OK and Path(path).is_file():
            print(f"--- {label} log tail ({path}) ---\n{uc.tail(path)}\n---", file=sys.stderr)
    print(f"exit {code}")
    return code


if __name__ == "__main__":
    sys.exit(main())
