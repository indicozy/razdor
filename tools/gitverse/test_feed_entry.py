"""The GitVerse update feed entry for a release: links to the own mirror, the checks, the feed (no network)."""

import contextlib
import io
import json
import os
import sys
import unittest
import unittest.mock

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402
import publish_feed_entry as pf  # noqa: E402

V = "1.6.1"
TAG = "v" + V
OWN = f"https://razdor.indicozy.dev/hub/indicozy/razdor/releases/download/{TAG}/"
GITHUB = f"https://github.com/indicozy/razdor/releases/download/{TAG}/"
ALL_NAMES = [
    f"razdor-{V}-linux", f"Razdor-{V}.exe", f"razdor-{V}-macos", f"razdor-{V}.apk",
    "Razdor-win7.exe", f"razdor-{V}-windows7.zip", f"razdor-{V}.ipa",
    "SHA256SUMS", "SHA256SUMS.txt", f"razdor-translate-{V}-linux", "razdor-ads-" + V + ".zip",
]
SUMS_TEXT = "".join(f"{'a' * 64}  {n}\n" for n in (
    f"razdor-{V}-linux", f"Razdor-{V}.exe", f"razdor-{V}-macos", f"razdor-{V}.apk", "Razdor-win7.exe"))


def github_release(names=ALL_NAMES, draft=False, prerelease=False):
    return {"name": "Раздор " + V, "html_url": f"https://github.com/indicozy/razdor/releases/tag/{TAG}",
            "body": "Notes\n", "draft": draft, "prerelease": prerelease,
            "assets": [{"name": n} for n in names]}


def old_entry(tag, body="old"):
    return {"tag_name": tag, "name": "Раздор " + tag[1:], "html_url": "", "draft": False,
            "prerelease": False, "body": body, "assets": []}


class FakePages:
    """Stands in for pages.Pages: a feed held in memory, and the calls the script makes."""

    def __init__(self, feed=None):
        self.files = {}
        if feed is not None:
            self.files[pf.FEED_PATH] = json.dumps(feed).encode("utf-8")
        self.writes = []
        self.commits = []
        self.waited = []

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def read(self, rel):
        return self.files.get(rel)

    def write(self, rel, data):
        self.writes.append((rel, data))
        self.files[rel] = data

    def commit_push(self, message):
        self.commits.append(message)
        return "abcd1234"

    def wait_served(self, expected, **kw):
        self.waited.append(expected)
        return 0.0

    def feed(self):
        return json.loads(self.files[pf.FEED_PATH].decode("utf-8"))


class FeedEntryTest(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self.heads = {}
        self.sums = SUMS_TEXT
        self.release = github_release()
        patches = [
            unittest.mock.patch.object(pf, "fetch_github_release", lambda tag: self.release),
            unittest.mock.patch.object(pf, "head_status", lambda url: self.heads.get(url, 200)),
            unittest.mock.patch.object(pf, "fetch_text", lambda url: self.sums),
        ]
        for p in patches:
            p.start()
            self.addCleanup(p.stop)
        self.fake = None

    def use_pages(self, feed=None):
        self.fake = FakePages(feed)
        p = unittest.mock.patch.object(pages, "Pages", lambda *a, **k: self.fake)
        p.start()
        self.addCleanup(p.stop)
        return self.fake

    def run_main(self, argv):
        out = io.StringIO()
        err = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                code = pf.main(argv)
            except SystemExit as e:
                code = e.code
        return code, out.getvalue(), err.getvalue()

    def assets_by_name(self, feed_entry=None):
        entry = feed_entry or pf.build_entry(TAG, self.release)[0]
        return {a["name"]: a["browser_download_url"] for a in entry["assets"]}

    # the entry: links

    def test_versioned_and_legacy_names_point_at_the_own_mirror(self):
        urls = self.assets_by_name()
        for name in (f"razdor-{V}-linux", f"Razdor-{V}.exe", f"razdor-{V}-macos", f"razdor-{V}.apk"):
            self.assertEqual(urls[name], OWN + name)
        self.assertEqual(urls["razdor"], OWN + f"razdor-{V}-linux")
        self.assertEqual(urls["Razdor.exe"], OWN + f"Razdor-{V}.exe")
        self.assertEqual(urls["razdor-macos"], OWN + f"razdor-{V}-macos")
        self.assertEqual(urls["razdor.apk"], OWN + f"razdor-{V}.apk")
        self.assertEqual(urls["Razdor-win7.exe"], OWN + "Razdor-win7.exe")
        self.assertEqual(urls[f"razdor-{V}-windows7.zip"], OWN + f"razdor-{V}-windows7.zip")

    def test_ipa_points_at_github_and_sums_at_the_mirror(self):
        urls = self.assets_by_name()
        self.assertEqual(urls[f"razdor-{V}.ipa"], GITHUB + f"razdor-{V}.ipa")
        self.assertEqual(urls["razdor.ipa"], GITHUB + f"razdor-{V}.ipa")
        self.assertEqual(urls["SHA256SUMS"], OWN + "SHA256SUMS")
        self.assertEqual(urls["SHA256SUMS.txt"], OWN + "SHA256SUMS")

    def test_entry_has_the_api_shape_and_leaves_out_other_files(self):
        entry = pf.build_entry(TAG, self.release)[0]
        self.assertEqual(set(entry), {"tag_name", "name", "html_url", "draft", "prerelease", "body", "assets"})
        self.assertEqual((entry["tag_name"], entry["draft"], entry["prerelease"]), (TAG, False, False))
        self.assertEqual(entry["body"], "Notes\n")
        names = {a["name"] for a in entry["assets"]}
        self.assertNotIn(f"razdor-translate-{V}-linux", names)
        self.assertNotIn("razdor-ads-" + V + ".zip", names)

    def test_a_missing_versioned_file_is_skipped_with_a_warning(self):
        self.release = github_release([n for n in ALL_NAMES if n != f"razdor-{V}-macos"])
        entry, versioned, checks = pf.build_entry(TAG, self.release)
        names = {a["name"] for a in entry["assets"]}
        self.assertNotIn(f"razdor-{V}-macos", names)
        self.assertNotIn("razdor-macos", names)
        self.assertNotIn(OWN + f"razdor-{V}-macos", checks)

    def test_a_draft_or_pre_release_is_refused(self):
        self.use_pages([])
        for kwargs in ({"draft": True}, {"prerelease": True}):
            self.release = github_release(**kwargs)
            code, _, _ = self.run_main(["--tag", TAG])
            self.assertEqual(code, 2)
        self.assertEqual(self.fake.writes, [])

    def test_a_bad_tag_is_refused(self):
        self.use_pages([])
        code, _, _ = self.run_main(["--tag", "1.6.1"])
        self.assertEqual(code, 2)

    # the checks

    def test_a_missing_own_file_aborts_without_writing(self):
        fake = self.use_pages([old_entry("v1.6.0")])
        self.heads[OWN + f"razdor-{V}-linux"] = 404
        code, _, err = self.run_main(["--tag", TAG])
        self.assertEqual(code, 1)
        self.assertIn("HTTP 404", err)
        self.assertEqual(fake.writes, [])
        self.assertEqual(fake.commits, [])

    def test_sums_that_do_not_list_a_program_abort_without_writing(self):
        fake = self.use_pages([])
        self.sums = "".join(f"{'b' * 64}  {n}\n" for n in (f"razdor-{V}-linux", f"Razdor-{V}.exe"))
        code, _, err = self.run_main(["--tag", TAG])
        self.assertEqual(code, 1)
        self.assertIn(f"does not list razdor-{V}-macos", err)
        self.assertEqual(fake.writes, [])

    def test_dry_run_prints_the_entry_and_pushes_nothing(self):
        fake = self.use_pages([old_entry("v1.6.0"), old_entry("v1.5.2")])
        code, out, _ = self.run_main(["--tag", TAG, "--dry-run"])
        self.assertEqual(code, 0)
        self.assertIn("the feed would hold: v1.6.1, v1.6.0", out)
        self.assertEqual(fake.commits, [])
        self.assertEqual(fake.writes, [])

    # the feed

    def test_publish_writes_the_feed_and_waits_for_it(self):
        fake = self.use_pages([old_entry("v1.6.0")])
        code, _, _ = self.run_main(["--tag", TAG])
        self.assertEqual(code, 0)
        self.assertEqual(fake.commits, [f"Update feed: {TAG} (files from the own mirror)"])
        feed = fake.feed()
        self.assertEqual([e["tag_name"] for e in feed], [TAG, "v1.6.0"])
        self.assertEqual(fake.waited[0][pf.FEED_PATH], fake.files[pf.FEED_PATH])
        self.assertTrue(fake.files[pf.FEED_PATH].endswith(b"\n"))
        self.assertIn(b'\n  "tag_name"', fake.files[pf.FEED_PATH])  # indent=1

    def test_the_same_tag_is_replaced(self):
        feed = pf.feed_after([old_entry(TAG, body="stale"), old_entry("v1.6.0")], entry=pf.build_entry(
            TAG, self.release)[0])
        self.assertEqual([e["tag_name"] for e in feed], [TAG, "v1.6.0"])
        self.assertEqual(feed[0]["body"], "Notes\n")

    def test_keeps_the_newest_two_by_version(self):
        prev = [old_entry("v1.5.2"), old_entry("v1.6.0"), old_entry("v1.4.9")]
        feed = pf.feed_after(prev, entry=pf.build_entry(TAG, self.release)[0])
        self.assertEqual([e["tag_name"] for e in feed], [TAG, "v1.6.0"])

    def test_drop_removes_one_tag(self):
        fake = self.use_pages([old_entry(TAG), old_entry("v1.6.0")])
        code, _, _ = self.run_main(["--drop", TAG])
        self.assertEqual(code, 0)
        self.assertEqual([e["tag_name"] for e in fake.feed()], ["v1.6.0"])
        self.assertEqual(fake.commits, [f"Update feed: without {TAG}"])

    def test_drop_refuses_to_empty_the_feed(self):
        fake = self.use_pages([old_entry(TAG)])
        code, _, _ = self.run_main(["--drop", TAG])
        self.assertEqual(code, 2)
        self.assertEqual(fake.writes, [])

    def test_drop_of_an_absent_tag_changes_nothing(self):
        fake = self.use_pages([old_entry("v1.6.0")])
        code, out, _ = self.run_main(["--drop", "v9.9.9"])
        self.assertEqual(code, 0)
        self.assertIn("nothing to do", out)
        self.assertEqual(fake.writes, [])

    def test_tag_and_drop_together_are_refused(self):
        self.use_pages([])
        with self.assertRaises(SystemExit):
            with contextlib.redirect_stderr(io.StringIO()):
                pf.main(["--tag", TAG, "--drop", TAG])


if __name__ == "__main__":
    unittest.main()
