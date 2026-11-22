"""Hermetic tests of rollback.py: the plan, its blockers and warnings, the dry run (nothing called), and
the apply path against fakes (the GitHub edit, the Pages push). No network, no gh, no token."""

import contextlib
import io
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import rollback as rb  # noqa: E402
import update_check as uc  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402

FEED_URL = "https://indicozy.gitverse.site/razdor-community/releases.json"
CONFIG_FEEDS = [FEED_URL, "https://api.github.com/repos/indicozy/razdor/releases?per_page=100"]


def rel(tag, prerelease=False, draft=False):
    return {"tag_name": tag, "draft": draft, "prerelease": prerelease, "assets": []}


def feed_entry(tag):
    return {"tag_name": tag, "draft": False, "prerelease": False, "assets": []}


RELEASES = [rel("v1.5.4"), rel("v1.5.3"), rel("v1.5.0"), rel("v1.4.1")]
FEED = [feed_entry("v1.5.4"), feed_entry("v1.5.3")]
CONFIG = {"schema": 1, "update_feed": CONFIG_FEEDS, "hub": ["https://indicozy.gitverse.site/razdor-community"],
          "game": {"links": ["https://x"], "size": 1, "sha256": "0" * 64, "name": "a.zip"}}


class FakePages:
    """Stands in for tools/gitverse/pages.Pages: records what would be written and pushed."""
    log = []

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def write(self, rel_path, data):
        FakePages.log.append(("write", rel_path, data))

    def commit_push(self, message):
        FakePages.log.append(("push", message))
        return "abc12345"

    def wait_served(self, expected):
        FakePages.log.append(("served", sorted(expected)))
        return 0


def refuse(*a, **k):
    raise AssertionError("a dry run must not call this")


class PlanTests(unittest.TestCase):
    def plan(self, **over):
        args = dict(to=(1, 5, 3), bad="v1.5.4", releases=RELEASES, feed=FEED, feed_url=FEED_URL,
                    live_config=CONFIG, repo_config=CONFIG, config_feeds=CONFIG_FEEDS)
        args.update(over)
        return rb.plan(**args)

    def test_a_clear_rollback(self):
        p = self.plan()
        self.assertEqual(p["blockers"], [])
        self.assertEqual(p["warnings"], [])
        self.assertEqual(p["gh"], ["gh", "release", "edit", "v1.5.4", "--repo", "indicozy/razdor", "--prerelease"])
        self.assertEqual(p["feed_before"], ["v1.5.4", "v1.5.3"])
        self.assertEqual(p["feed_after"], ["v1.5.3"])

    def test_the_feed_keeps_the_target_and_two_at_most(self):
        feed = [feed_entry("v1.5.5"), feed_entry("v1.5.4"), feed_entry("v1.5.3"), feed_entry("v1.5.0")]
        got = rb.new_feed(feed, (1, 5, 3), "v1.5.4")
        self.assertEqual([e["tag_name"] for e in got], ["v1.5.3", "v1.5.0"])

    def test_pre_releases_and_drafts_never_enter_the_feed(self):
        feed = [feed_entry("v1.5.3"), dict(feed_entry("v1.5.2"), prerelease=True), dict(feed_entry("v1.5.1"), draft=True)]
        self.assertEqual([e["tag_name"] for e in rb.new_feed(feed, (1, 5, 3), "v1.5.4")], ["v1.5.3"])

    def test_the_target_must_be_older_than_the_bad_one(self):
        self.assertTrue(any("not older" in b for b in self.plan(to=(1, 5, 4))["blockers"]))

    def test_the_target_must_be_published(self):
        releases = [rel("v1.5.4"), rel("v1.5.3", prerelease=True)]
        self.assertTrue(any("not a published" in b for b in self.plan(releases=releases)["blockers"]))

    def test_the_bad_release_must_be_the_newest(self):
        releases = RELEASES + [rel("v1.5.5")]
        blockers = self.plan(releases=releases)["blockers"]
        self.assertTrue(any("not the newest" in b for b in blockers))

    def test_an_unreadable_feed_blocks(self):
        self.assertTrue(any("could not be read" in b for b in self.plan(feed=None)["blockers"]))

    def test_a_feed_without_the_target_blocks(self):
        self.assertTrue(any("does not list v1.5.3" in b for b in self.plan(feed=[feed_entry("v1.5.4")])["blockers"]))

    def test_repo_and_live_config_must_match(self):
        other = dict(CONFIG, hub=["https://elsewhere.example"])
        self.assertTrue(any("differ" in b for b in self.plan(repo_config=other)["blockers"]))

    def test_a_config_naming_the_bad_tag_warns(self):
        live = dict(CONFIG, game=dict(CONFIG["game"], name="razdor-v1.5.4.zip"))
        warns = self.plan(live_config=live, repo_config=live)["warnings"]
        self.assertTrue(any("names v1.5.4" in w for w in warns))

    def test_a_config_without_the_feed_warns(self):
        warns = self.plan(config_feeds=["https://api.github.com/repos/indicozy/razdor/releases"])["warnings"]
        self.assertTrue(any("does not list" in w for w in warns))

    def test_an_unreadable_live_config_warns_only(self):
        p = self.plan(live_config=None, config_feeds=None)
        self.assertEqual(p["blockers"], [])
        self.assertTrue(any("live config could not be read" in w for w in p["warnings"]))

    def test_a_bad_release_already_a_prerelease_skips_github(self):
        releases = [rel("v1.5.4", prerelease=True), rel("v1.5.3")]
        p = self.plan(releases=releases)
        self.assertIsNone(p["gh"])
        self.assertTrue(any("already a pre-release" in w for w in p["warnings"]))


class MainTests(HermeticTestCase):
    def env(self, **over):
        env = {"releases": RELEASES, "feed": FEED, "feed_url": FEED_URL, "live_config": CONFIG,
               "repo_config": CONFIG}
        env.update(over)
        return env

    def run_main(self, args, env):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = rb.main(args, env)
        return code, out.getvalue(), err.getvalue()

    def test_a_dry_run_changes_nothing_and_calls_nothing(self):
        code, out, _ = self.run_main(["--to", "v1.5.3"], self.env(gh_runner=refuse, pages_factory=refuse))
        self.assertEqual(code, 0)
        self.assertIn("dry run: nothing changed", out)
        self.assertIn("GH_TOKEN=$(gh auth token -u indicozy) gh release edit v1.5.4 --repo indicozy/razdor --prerelease", out)
        self.assertIn("entries ['v1.5.4', 'v1.5.3'] -> ['v1.5.3']", out)

    def test_a_blocked_plan_exits_1_and_says_nothing_changed(self):
        code, out, err = self.run_main(["--to", "v1.5.3"], self.env(feed=None, gh_runner=refuse, pages_factory=refuse))
        self.assertEqual(code, 1)
        self.assertIn("nothing changed", err)

    def test_apply_writes_the_feed_then_flips_github_then_checks(self):
        calls = []
        FakePages.log = []

        def gh(argv):
            calls.append(argv)

            class R:
                returncode = 0
                stdout = ""
                stderr = ""
            return R()

        after = [rel("v1.5.4", prerelease=True), rel("v1.5.3")] + RELEASES[2:]
        code, out, err = self.run_main(["--to", "v1.5.3", "--apply"], self.env(
            gh_runner=gh, pages_factory=FakePages, releases_reader=lambda: after))
        self.assertEqual(code, 0, err)
        self.assertEqual(FakePages.log[0][0], "write")
        self.assertEqual(FakePages.log[0][1], rb.FEED_PATH)
        self.assertEqual(FakePages.log[1][0], "push")
        self.assertEqual(FakePages.log[2][0], "served")
        self.assertEqual(calls, [["gh", "release", "edit", "v1.5.4", "--repo", "indicozy/razdor", "--prerelease"]])
        self.assertIn("newest published release is now v1.5.3", out)

    def test_apply_stops_when_the_check_still_sees_the_bad_release(self):
        FakePages.log = []
        code, out, err = self.run_main(["--to", "v1.5.3", "--apply"], self.env(
            gh_runner=lambda argv: type("R", (), {"returncode": 0, "stdout": "", "stderr": ""})(),
            pages_factory=FakePages, releases_reader=lambda: RELEASES))
        self.assertEqual(code, 2)
        self.assertIn("not v1.5.3", err)

    def test_a_blocked_apply_calls_nothing(self):
        FakePages.log = []
        code, _, _ = self.run_main(["--to", "v1.5.3", "--apply"], self.env(
            feed=[feed_entry("v1.5.4")], gh_runner=refuse, pages_factory=refuse))
        self.assertEqual(code, 1)
        self.assertEqual(FakePages.log, [])

    def test_a_gh_failure_is_an_error(self):
        code, _, err = self.run_main(["--to", "v1.5.3", "--apply"], self.env(
            pages_factory=FakePages, releases_reader=lambda: RELEASES,
            gh_runner=lambda argv: type("R", (), {"returncode": 1, "stdout": "", "stderr": "HTTP 403"})()))
        self.assertEqual(code, 2)
        self.assertIn("HTTP 403", err)

    def test_a_bad_version_is_an_error(self):
        code, _, err = self.run_main(["--to", "latest"], self.env())
        self.assertEqual(code, 2)


class RealSourceTests(unittest.TestCase):
    def test_the_feed_is_the_gitverse_copy_of_the_update_feed(self):
        self.assertEqual(rb.feed_url_default(), FEED_URL)

    def test_the_config_names_github_first_then_gitverse(self):
        urls = rb.config_urls_default()
        self.assertEqual(urls[0], "https://indicozy.github.io/razdor/config.json")
        self.assertEqual(urls[1], "https://indicozy.gitverse.site/razdor-community/config.json")
        self.assertEqual(len(urls), 2)

    def test_the_repo_config_is_readable(self):
        self.assertTrue(os.path.isfile(rb.CONFIG_SRC))


class MatchTests(unittest.TestCase):
    def test_config_mentions_walks_nested_values(self):
        cfg = {"a": ["x v1.5.4 y"], "b": {"c": "v1.5.4"}, "d": "v1.5.3"}
        self.assertEqual(sorted(rb.config_mentions(cfg, ["v1.5.4"])), ["a[0]", "b.c"])


if __name__ == "__main__":
    unittest.main()
