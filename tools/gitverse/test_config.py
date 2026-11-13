"""The remote config's publishing: the shipped file passes the check, the check refuses what the app
would refuse, and a Pages push writes docs/config.json (against a local bare repository, no network)."""

import copy
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402
import pages  # noqa: E402
import publish_config  # noqa: E402


def git(*args, cwd):
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                   cwd=cwd, check=True, capture_output=True)


def good():
    return {
        "schema": 1,
        "update_feed": ["https://feed.example/releases.json", "https://api.example/releases"],
        "hub": ["https://hub.example/razdor", "https://github.example/latest/download"],
        "game": {"links": ["https://gitverse.example/game.zip"], "size": 1234,
                 "sha256": "ab" * 32, "name": "game.zip"},
    }


def raw_of(cfg):
    return json.dumps(cfg).encode("utf-8")


class CheckTests(HermeticTestCase):
    def test_the_shipped_config_passes(self):
        cfg = publish_config.check(pages.config_bytes())
        self.assertEqual(cfg["schema"], 1)
        self.assertTrue(cfg["update_feed"][0].startswith("https://indicozy.gitverse.site/"))

    def test_a_good_config_passes(self):
        self.assertEqual(publish_config.check(raw_of(good()))["game"]["name"], "game.zip")

    def test_refusals(self):
        cases = {
            "a newer schema": lambda c: c.update({"schema": 2}),
            "a plain address": lambda c: c.update({"hub": ["http://plain.example/x"]}),
            "an empty list": lambda c: c.update({"update_feed": []}),
            "a space in an address": lambda c: c.update({"hub": ["https://sp ace.example/x"]}),
            "a short SHA-256": lambda c: c["game"].update({"sha256": "abc"}),
            "a size of zero": lambda c: c["game"].update({"size": 0}),
            "a file name with a slash": lambda c: c["game"].update({"name": "x/game.zip"}),
            "no game": lambda c: c.pop("game"),
        }
        for why, change in cases.items():
            cfg = copy.deepcopy(good())
            change(cfg)
            with self.subTest(why), self.assertRaises(gv.GitVerseError):
                publish_config.check(raw_of(cfg))

    def test_a_web_page_or_bad_json_is_refused(self):
        with self.assertRaises(gv.GitVerseError):
            publish_config.check(b"<!doctype html><html>not found</html>")
        with self.assertRaises(gv.GitVerseError):
            publish_config.check(b"{")

    def test_unknown_fields_pass(self):
        cfg = good()
        cfg["news"] = {"x": 1}
        cfg["game"]["extra"] = True
        self.assertEqual(publish_config.check(raw_of(cfg))["schema"], 1)


class PagesConfigTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        root = self._tmp.name
        self.bare = os.path.join(root, "remote.git")
        seed = os.path.join(root, "seed")
        self.scratch = os.path.join(root, "scratch")
        os.makedirs(os.path.join(seed, "docs"))
        git("init", "-q", "-b", "main", cwd=seed)
        with open(os.path.join(seed, "docs", "releases.json"), "w") as f:
            f.write("[]\n")
        git("add", "-A", cwd=seed)
        git("commit", "-q", "-m", "seed", cwd=seed)
        git("init", "-q", "--bare", self.bare, cwd=root)
        git("push", "-q", self.bare, "main", cwd=seed)

    def tearDown(self):
        self._tmp.cleanup()

    def test_a_push_writes_the_config_once(self):
        pg = pages.Pages(remote=self.bare, scratch=self.scratch)
        try:
            self.assertIsNotNone(pg.commit_push("Remote config: test"))
            self.assertEqual(pg.read("docs/config.json"), pages.config_bytes())
            self.assertIsNone(pg.commit_push("Remote config: again"), "the same bytes: no commit")
        finally:
            pg.__exit__(None, None, None)

    def test_the_feed_and_the_config_survive_a_site_sync(self):
        pg = pages.Pages(remote=self.bare, scratch=self.scratch)
        pg.sync_site({"index-v2.json": b"{}"})
        pg.commit_push("Site: test")
        self.assertIsNotNone(pg.read("docs/config.json"))
        self.assertIsNotNone(pg.read("docs/releases.json"))


if __name__ == "__main__":
    unittest.main()
