"""Hermetic tests of android_update_check.py: the parsers, the emulator's command and environment,
the AVD copy, and the whole emulator run with a fake device (no adb, no emulator, no network)."""

import argparse
import contextlib
import io
import json
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import android_update_check as au  # noqa: E402
import update_check as uc  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402


class ParserTests(unittest.TestCase):
    def test_version_from_dumpsys(self):
        text = "Packages:\n  Package [io.github.indicozy.razdor] (abc):\n    versionCode=1 minSdk=26 targetSdk=36\n    versionName=1.5.4\n"
        self.assertEqual(au.parse_dumpsys_version(text), ("1.5.4", 1))

    def test_no_package_no_version(self):
        self.assertEqual(au.parse_dumpsys_version("Unable to find package"), (None, None))

    def test_pid(self):
        self.assertEqual(au.parse_pid("4774\n"), 4774)
        self.assertEqual(au.parse_pid(""), None)
        self.assertEqual(au.parse_pid(None), None)

    def test_log_version_is_the_first_version_line(self):
        text = "[   0.002] Razdor 1.5.4 (26b47dd), made by @indicozy, android x86_64\n[ 0.1] Razdor 1.5.3 (x)\n"
        self.assertEqual(au.log_version(text), "1.5.4")
        self.assertIsNone(au.log_version("[ 0.1] something else\n"))

    def test_panics_in_the_app_log(self):
        text = "ok\nthread 'main' panicked at src/x.rs:1\nfine\n"
        self.assertEqual(len(au.log_panics(text)), 1)
        self.assertEqual(au.log_panics("all fine\n"), [])

    def test_logcat_crashes_only_for_its_pid(self):
        logcat = (
            "11-22 11:00:00.000  500  500 E AndroidRuntime: FATAL EXCEPTION: main\n"
            "11-22 11:00:01.000  900  901 E AndroidRuntime: FATAL EXCEPTION: other\n"
            "11-22 11:00:02.000  900  901 E SAPP    : panicked at egl.rs:209:5\n"
        )
        got = au.logcat_panics(logcat, 500)
        self.assertEqual(len(got), 1)
        self.assertIn("FATAL EXCEPTION: main", got[0])
        self.assertEqual(len(au.logcat_panics(logcat, 900)), 2)

    def test_the_same_crash_has_the_same_signature_in_two_runs(self):
        a = "11-22 11:26:15.862  4519  4540 E SAPP    : panicked at egl.rs:209:5:"
        b = "11-22 12:01:03.100  7001  7002 E SAPP    : panicked at egl.rs:209:5:"
        self.assertEqual(au.signature(a), au.signature(b))
        self.assertIn("panicked at egl.rs:209:5", au.signature(a))


class SettingsTests(unittest.TestCase):
    before = json.dumps(au.SETTINGS_TEST).encode()

    def test_unchanged(self):
        self.assertEqual(au.settings_check(self.before, self.before)[0], uc.PASS)

    def test_rewritten_with_the_marker_kept(self):
        after = json.dumps({"language": "ru", "carry_test": 1, "volume": 3}).encode()
        self.assertEqual(au.settings_check(self.before, after)[0], uc.PASS)

    def test_marker_lost(self):
        after = json.dumps({"language": "ru"}).encode()
        self.assertEqual(au.settings_check(self.before, after)[0], uc.FAIL)

    def test_not_json(self):
        self.assertEqual(au.settings_check(self.before, b"\x00\x01")[0], uc.FAIL)

    def test_missing_file(self):
        self.assertEqual(au.settings_check(self.before, b"")[0], uc.FAIL)


class CommandTests(unittest.TestCase):
    def test_command_is_pinned_and_headless(self):
        argv = au.emulator_command("/sdk", "/avd", "razdor", 5554, "8-19")
        self.assertEqual(argv[:6], ["taskset", "-c", "8-19", "nice", "-n", "19"])
        self.assertEqual(argv[6], os.path.join("/sdk", "emulator", "emulator"))
        for flag in ("-no-window", "-no-audio", "-no-snapshot"):
            self.assertIn(flag, argv)
        self.assertEqual(argv[argv.index("-gpu") + 1], "host")
        self.assertEqual(argv[argv.index("-feature") + 1], "-Vulkan")
        self.assertEqual(argv[argv.index("-port") + 1], "5554")
        self.assertNotIn("-read-only", argv)

    def test_environment_uses_only_the_private_display(self):
        base = {"PATH": "/bin", "DISPLAY": ":0", "RAZDOR_DT_DIR": "/game", "XAUTHORITY": "/x", "WAYLAND_DISPLAY": "w"}
        env = au.emulator_env("/sdk", Path("/avd"), Path("/home"), ":97", base)
        self.assertEqual(env["DISPLAY"], ":97")
        self.assertNotIn("RAZDOR_DT_DIR", env)
        self.assertNotIn("XAUTHORITY", env)
        self.assertNotIn("WAYLAND_DISPLAY", env)
        self.assertEqual(env["ANDROID_AVD_HOME"], "/avd")
        self.assertEqual(env["__GLX_VENDOR_LIBRARY_NAME"], "mesa")
        self.assertEqual(env["PATH"], "/bin")

    def test_avd_ini_points_at_the_copy(self):
        text = "avd.ini.encoding=UTF-8\npath=/old/razdor.avd\ntarget=android-34\n"
        out = au.avd_ini_text(text, "/new/razdor.avd")
        self.assertIn("path=/new/razdor.avd", out.splitlines())
        self.assertIn("target=android-34", out.splitlines())


class FakeDevice:
    """Stands in for au.Device: a clean run where the new version starts and the data survives."""

    calls = []
    instances = []

    def __init__(self, adb, serial, runner=None):
        self.serial = serial
        self.version_name = None
        self.files = {}
        self.new_log_version = None
        FakeDevice.instances.append(self)

    def listed(self):
        return False

    def booted(self):
        return True

    def uninstall(self):
        FakeDevice.calls.append("uninstall")

    def install(self, apk, timeout=300):
        FakeDevice.calls.append(("install", Path(apk).name))
        self.version_name = FakeDevice.next_version.pop(0)

    def version(self):
        return (self.version_name, 1)

    def start(self):
        FakeDevice.calls.append("start")
        self.log_version = self.version_name

    def cat(self, remote):
        if remote == au.REMOTE_LOG:
            return f"[ 0.0] Razdor {self.version_name} (abc), made by @indicozy, android x86_64\n"
        return self.files.get(remote)

    def pid(self):
        return 4242

    def push(self, local, remote):
        self.files[remote] = Path(local).read_bytes().decode("utf-8")

    def force_stop(self):
        FakeDevice.calls.append("force-stop")

    def logcat(self):
        return ""

    def kill_emulator(self):
        FakeDevice.calls.append("kill")

    def shell(self, cmd, timeout=30):
        FakeDevice.calls.append(("shell", cmd))
        return 0, ""


class FakeProc:
    pid = 0

    def poll(self):
        return 0

    @property
    def returncode(self):
        return 0


class EmulatorRunTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        FakeDevice.calls = []
        FakeDevice.instances = []
        FakeDevice.next_version = ["1.5.3", "1.5.4"]
        root = Path(self.root)
        self.avd_src = root / "src"
        (self.avd_src / "razdor.avd").mkdir(parents=True)
        (self.avd_src / "razdor.avd" / "config.ini").write_text("image.sysdir.1=x\n")
        (self.avd_src / "razdor.ini").write_text("path=/elsewhere/razdor.avd\ntarget=android-34\n")
        self.scratch = root / "scratch"
        self.scratch.mkdir()
        self.old_apk = root / "old.apk"
        self.old_apk.write_bytes(b"old")
        self.new_apk = root / "new.apk"
        self.new_apk.write_bytes(b"new")
        self.opts = argparse.Namespace(sdk="/sdk", avd_src=str(self.avd_src), port=5554, gpu="host",
                                       keep=False, scratch=str(self.scratch))
        patches = [
            mock.patch.object(au, "Device", FakeDevice),
            mock.patch.object(au, "sdk_root", lambda given: "/sdk"),
            mock.patch.object(au, "cpus_for_run", lambda: "8-19"),
            mock.patch.object(au, "LOG_SECONDS", 0),
            mock.patch.object(au.uc, "start_xvfb", lambda log: (FakeProc(), ":97")),
            mock.patch.object(au.subprocess, "Popen", lambda *a, **k: FakeProc()),
            mock.patch.object(au.time, "sleep", lambda s: None),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)

    def test_a_clean_update_passes_every_row(self):
        rows, logs = [], []
        with mock.patch.object(au, "RUN_SECONDS", 0):
            au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, logs)
        results = {name: res for name, res, _ in rows}
        self.assertIn(("install", "old.apk"), FakeDevice.calls)
        self.assertIn(("install", "new.apk"), FakeDevice.calls)
        self.assertEqual(FakeDevice.calls.count("force-stop"), 1)
        self.assertEqual(results["android: updated app starts"], uc.PASS)
        self.assertEqual(results["android: settings kept"], uc.PASS)
        self.assertEqual(results["android: saves folder file kept"], uc.PASS)
        self.assertNotIn(uc.FAIL, results.values())
        self.assertIn("kill", FakeDevice.calls)

    def test_the_new_version_not_installed_fails(self):
        FakeDevice.next_version = ["1.5.3", "1.5.3"]
        rows, logs = [], []
        with mock.patch.object(au, "RUN_SECONDS", 0):
            with self.assertRaises(uc.CheckFail):
                au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, logs)

    def test_the_new_app_logging_the_old_version_fails(self):
        class OldLog(FakeDevice):
            def cat(self, remote):
                if remote == au.REMOTE_LOG:
                    return "[ 0.0] Razdor 1.5.3 (abc)\n"
                return self.files.get(remote)

        with mock.patch.object(au, "Device", OldLog):
            rows = []
            with mock.patch.object(au, "RUN_SECONDS", 0), self.assertRaises(uc.CheckFail):
                au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, [])

    def test_a_marker_lost_from_settings_fails(self):
        class Lossy(FakeDevice):
            def cat(self, remote):
                if remote == au.REMOTE_SETTINGS:
                    return '{"language": "en"}'
                return super().cat(remote)

        with mock.patch.object(au, "Device", Lossy):
            rows = []
            with mock.patch.object(au, "RUN_SECONDS", 0), self.assertRaises(uc.CheckFail):
                au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, [])

    def test_a_crash_only_the_new_version_shows_fails(self):
        class Crashy(FakeDevice):
            def logcat(self):
                if self.version_name == "1.5.4":
                    return "11-22 11:00:00.000  4242  4300 E SAPP    : panicked at new.rs:1:1\n"
                return ""

        with mock.patch.object(au, "Device", Crashy):
            rows = []
            with mock.patch.object(au, "RUN_SECONDS", 0), self.assertRaises(uc.CheckFail):
                au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, [])

    def test_a_crash_both_versions_show_is_an_emulator_limit(self):
        class Shared(FakeDevice):
            def logcat(self):
                return "11-22 11:00:00.000  4242  4300 E SAPP    : panicked at egl.rs:209:5\n"

        with mock.patch.object(au, "Device", Shared):
            rows = []
            with mock.patch.object(au, "RUN_SECONDS", 0):
                au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, [])
        results = {name: res for name, res, _ in rows}
        self.assertEqual(results["android: crash lines of the new version"], uc.INFO)
        self.assertNotIn(uc.FAIL, results.values())

    def test_a_device_already_on_the_port_is_an_error(self):
        class Busy(FakeDevice):
            def listed(self):
                return True

        with mock.patch.object(au, "Device", Busy):
            with self.assertRaises(uc.ToolError):
                au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, [], [])

    def test_the_avd_copy_is_removed_at_the_end(self):
        rows = []
        with mock.patch.object(au, "RUN_SECONDS", 0):
            au.emulator_run(self.old_apk, self.new_apk, "1.5.3", "1.5.4", self.scratch, self.opts, rows, [])
        self.assertFalse((self.scratch / "avd").exists())

    def test_copy_avd_writes_a_copy_with_its_own_path(self):
        dst = self.scratch / "copy"
        au.copy_avd(self.avd_src, dst)
        self.assertTrue((dst / "razdor.avd" / "config.ini").is_file())
        ini = (dst / "razdor.ini").read_text()
        self.assertIn(f"path={dst / 'razdor.avd'}", ini)

    def test_copy_avd_missing_is_an_error(self):
        with self.assertRaises(uc.ToolError):
            au.copy_avd(self.scratch / "none", self.scratch / "copy")


class DeviceTests(HermeticTestCase):
    def test_install_success_and_failure(self):
        ok = au.Device("adb", "emulator-5554", runner=lambda argv, t: (0, "Performing Streamed Install\nSuccess\n", ""))
        ok.install("x.apk")
        bad = au.Device("adb", "emulator-5554", runner=lambda argv, t: (1, "", "INSTALL_FAILED_UPDATE_INCOMPATIBLE"))
        with self.assertRaises(uc.CheckFail):
            bad.install("x.apk")

    def test_listed_reads_adb_devices(self):
        dev = au.Device("adb", "emulator-5554", runner=lambda argv, t: (0, "List of devices attached\nemulator-5554\tdevice\n", ""))
        self.assertTrue(dev.listed())
        none = au.Device("adb", "emulator-5554", runner=lambda argv, t: (0, "List of devices attached\n\n", ""))
        self.assertFalse(none.listed())

    def test_the_serial_goes_to_every_call(self):
        seen = []

        def runner(argv, t):
            seen.append(argv)
            return 0, "1\n", ""

        au.Device("adb", "emulator-5560", runner=runner).booted()
        self.assertEqual(seen[0][:3], ["adb", "-s", "emulator-5560"])


class ReadTests(HermeticTestCase):
    def test_cat_pulls_the_bytes_and_keeps_lf(self):
        content = b"carry-over test\n"

        def runner(argv, t):
            if argv[3] == "pull":
                Path(argv[5]).write_bytes(content)
                return 0, "1 file pulled", ""
            raise AssertionError("cat must not go through the shell (a terminal turns LF into CRLF)")

        dev = au.Device("adb", "emulator-5554", runner=runner)
        self.assertEqual(dev.cat("/x/carry-test.txt"), "carry-over test\n")

    def test_cat_of_a_missing_file_is_none(self):
        dev = au.Device("adb", "emulator-5554", runner=lambda argv, t: (1, "", "remote object does not exist"))
        self.assertIsNone(dev.cat("/x/none.txt"))


class CpuTests(HermeticTestCase):
    def test_cpus_fall_back_to_8_19(self):
        with mock.patch.object(au.subprocess, "run", side_effect=OSError("no tool")):
            self.assertEqual(au.cpus_for_run(), "8-19")

    def test_cpus_come_from_ai_resources(self):
        done = mock.Mock(returncode=0, stdout="0-19\n")
        with mock.patch.object(au.subprocess, "run", return_value=done):
            self.assertEqual(au.cpus_for_run(), "0-19")


class MainTests(HermeticTestCase):
    def test_needs_to_or_new_apk(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            au.main(["--from", "v1.5.3"])

    def test_port_must_be_an_even_emulator_port(self):
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            au.main(["--from", "v1.5.3", "--to", "v1.5.4", "--port", "5555"])


if __name__ == "__main__":
    unittest.main()
