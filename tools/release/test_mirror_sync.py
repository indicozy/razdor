"""Hermetic tests of mirror_sync.py (the own mirror's trigger and poll) and of update_check's own feed:
fake status reads, a fake ssh runner and a fake clock. No network, no ssh."""

import contextlib
import io
import json
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import mirror_sync as ms  # noqa: E402
import update_check as uc  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402


def status(synced, app):
    return json.dumps({"synced_at": synced, "app": app, "catalog": "catalog-2026-11-22-3",
                       "config_sha256": "0" * 64}).encode("utf-8")


class FakeClock:
    def __init__(self):
        self.now = 0.0

    def clock(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class FakeRunner:
    """Records each ssh argv; returns the given return code."""
    def __init__(self, code=0, stderr=""):
        self.calls = []
        self.code = code
        self.stderr = stderr

    def __call__(self, argv):
        self.calls.append(argv)
        return type("R", (), {"returncode": self.code, "stdout": "", "stderr": self.stderr})()


def reads(*items):
    """A status reader that gives each item in turn (raw bytes, or an exception to raise), then repeats the last."""
    queue = list(items)

    def fetch():
        item = queue.pop(0) if len(queue) > 1 else queue[0]
        if isinstance(item, Exception):
            raise item
        return ms.parse_status(item)
    return fetch


class StatusTests(HermeticTestCase):
    def test_a_good_status_parses(self):
        st = ms.parse_status(status("2026-11-24T10:00:00Z", ["v1.6.0"]))
        self.assertEqual(st["app"], ["v1.6.0"])

    def test_a_web_page_or_a_bad_status_is_refused(self):
        bad = (b"<html>", b"[]", status("", ["v1"]), json.dumps({"synced_at": "x", "app": "v1"}).encode())
        for raw in bad:
            with self.subTest(raw=raw[:20]):
                with self.assertRaises(ms.MirrorError):
                    ms.parse_status(raw)

    def test_the_host_comes_from_the_flag_then_the_environment_then_the_env_file(self):
        self.assertEqual(ms.ssh_host("box", env={}, env_file=os.devnull), "box")
        self.assertEqual(ms.ssh_host(None, env={ms.SSH_KEY: "envbox"}, env_file=os.devnull), "envbox")
        path = os.path.join(self.root, ".env")
        with open(path, "w", encoding="utf-8") as f:
            f.write(f'OTHER=1\n{ms.SSH_KEY}="filebox"\n')
        self.assertEqual(ms.ssh_host(None, env={}, env_file=path), "filebox")
        with self.assertRaises(ms.MirrorError):
            ms.ssh_host(None, env={}, env_file=os.devnull)
        with self.assertRaises(ms.MirrorError):
            ms.ssh_host("bad host", env={}, env_file=os.devnull)

    def test_the_own_feed_is_read_as_one_list_with_its_own_addresses(self):
        own = "https://razdor.indicozy.dev/hub/indicozy/razdor/releases/download/v1.6.0/razdor-1.6.0-linux"
        feed = [{"tag_name": "v1.6.0", "draft": False, "prerelease": False,
                 "assets": [{"name": "razdor-1.6.0-linux", "browser_download_url": own}]}]
        seen = []

        def get(url):
            seen.append(url)
            return json.dumps(feed)
        pub = uc.load_published(get, feed="own")
        self.assertEqual(seen, [uc.OWN_FEED])
        self.assertEqual(max(pub), (1, 6, 0))
        self.assertEqual(uc.asset_url(pub[(1, 6, 0)], "razdor-1.6.0-linux"), own)
        with self.assertRaises(uc.ToolError):
            uc.release_list(lambda url: "{}", feed="own")


class TriggerTests(HermeticTestCase):
    def test_the_trigger_starts_the_unit_over_ssh(self):
        runner = FakeRunner()
        ms.trigger("box", runner)
        argv = runner.calls[0]
        self.assertEqual(argv[0], "ssh")
        self.assertIn("box", argv)
        self.assertEqual(argv[-1], f"systemctl start {ms.UNIT}")

    def test_a_failed_ssh_is_a_tool_error(self):
        with self.assertRaises(ms.MirrorError):
            ms.trigger("box", FakeRunner(code=255, stderr="denied"))

        def boom(argv):
            raise OSError("no ssh")
        with self.assertRaises(ms.MirrorError):
            ms.trigger("box", boom)


class WaitTests(HermeticTestCase):
    def wait(self, fetch, runner, has_new, timeout=100):
        clock = FakeClock()
        logs = []
        st = ms.sync_and_wait(has_new, host="box", timeout=timeout, interval=10, fetch=fetch, runner=runner,
                              clock=clock.clock, sleep=clock.sleep, out=logs.append)
        return st, clock, logs

    def test_it_returns_once_a_run_after_the_start_has_the_tag(self):
        runner = FakeRunner()
        fetch = reads(status("1", ["v1.5.0"]), status("1", ["v1.5.0"]), status("2", ["v1.6.0", "v1.5.0"]))
        st, _, _ = self.wait(fetch, runner, ms.release_check("v1.6.0"))
        self.assertEqual(st["synced_at"], "2")
        self.assertEqual(len(runner.calls), 1)

    def test_a_finished_run_without_the_tag_starts_the_sync_again(self):
        runner = FakeRunner()
        fetch = reads(status("1", ["v1.5.0"]), status("2", ["v1.5.0"]), status("3", ["v1.6.0"]))
        st, _, logs = self.wait(fetch, runner, ms.release_check("v1.6.0"))
        self.assertEqual(len(runner.calls), 2)
        self.assertTrue(any("started again" in line for line in logs))
        self.assertEqual(st["synced_at"], "3")

    def test_a_rollback_waits_for_the_bad_tag_to_leave(self):
        runner = FakeRunner()
        fetch = reads(status("1", ["v1.5.4", "v1.5.3"]), status("2", ["v1.5.4", "v1.5.3"]),
                      status("3", ["v1.5.3"]))
        st, _, _ = self.wait(fetch, runner, ms.release_check("v1.5.3", gone="v1.5.4"))
        self.assertEqual(st["app"], ["v1.5.3"])

    def test_an_unreadable_status_is_retried_until_the_timeout(self):
        runner = FakeRunner()
        fetch = reads(ms.MirrorError("down"))
        with self.assertRaises(ms.MirrorTimeout) as cm:
            self.wait(fetch, runner, ms.release_check("v1.6.0"), timeout=50)
        self.assertIn("down", str(cm.exception))

    def test_a_status_that_never_gets_the_tag_times_out(self):
        runner = FakeRunner()
        fetch = reads(status("1", ["v1.5.0"]), status("2", ["v1.5.0"]))
        with self.assertRaises(ms.MirrorTimeout):
            self.wait(fetch, runner, ms.release_check("v1.6.0"), timeout=60)


class MainTests(HermeticTestCase):
    def test_a_bad_version_exits_2(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            code = ms.main(["--tag", "latest", "--ssh", "box"])
        self.assertEqual(code, 2)
        self.assertIn("not a version", err.getvalue())


if __name__ == "__main__":
    unittest.main()
