"""The whole release flow of publish_release.publish() with the GitVerse API faked and the Pages repo a
local bare repository: the feed pushed, the older release deleted, a pre-release leaving the feed alone."""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import unittest
import unittest.mock

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402
import publish_release as pr  # noqa: E402
from test_release import PAYLOAD  # noqa: E402


class FakeClient:
    """The API calls publish() makes, recorded. Releases live in a dict keyed by (repo, tag)."""

    releases = {}
    calls = []
    counter = [500]

    def __init__(self, token=None, api=None):
        pass

    def release_by_tag(self, owner, repo, tag):
        return FakeClient.releases.get((repo, tag))

    def create_release(self, owner, repo, tag, target, name, body, prerelease):
        FakeClient.counter[0] += 1
        rel = {"id": FakeClient.counter[0], "tag_name": tag, "html_url": f"https://gitverse.ru/{owner}/{repo}/releases/tag/{tag}"}
        FakeClient.releases[(repo, tag)] = rel
        FakeClient.calls.append(("create", repo, tag, bool(prerelease)))
        return rel

    def upload_asset(self, owner, repo, rid, name, path):
        FakeClient.calls.append(("upload", repo, name))
        return {"browser_download_url": f"https://gitverse.ru/api/attachments/{rid}-{name}", "id": 1}

    def delete_release(self, owner, repo, rid):
        FakeClient.calls.append(("delete", repo, rid))

    def delete_release_by_tag(self, owner, repo, tag):
        FakeClient.calls.append(("delete-tag", repo, tag))


def git(*args, cwd):
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                   cwd=cwd, check=True, capture_output=True)


def old_feed():
    def entry(tag):
        return {"tag_name": tag, "draft": False, "prerelease": False, "body": tag, "assets": []}

    return [entry("v1.5.1"), entry("v1.5.0")]


class PublishFlowTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = self._tmp.name
        self.bare = os.path.join(root, "remote.git")
        seed = os.path.join(root, "seed")
        os.makedirs(os.path.join(seed, "docs"))
        with open(os.path.join(seed, "docs", "releases.json"), "w", encoding="utf-8") as f:
            json.dump(old_feed(), f)
        git("init", "-q", "-b", "main", cwd=seed)
        git("add", "-A", cwd=seed)
        git("commit", "-q", "-m", "seed", cwd=seed)
        git("init", "-q", "--bare", self.bare, cwd=root)
        git("push", "-q", self.bare, "main", cwd=seed)
        self.scratch = os.path.join(root, "scratch")
        self.state = os.path.join(root, "state")
        os.makedirs(self.state)
        self.files = []
        fdir = os.path.join(root, "files")
        os.makedirs(fdir)
        for name in ("Razdor-1.5.2.exe", "razdor-1.5.2-linux", "razdor-1.5.2.apk", "razdor-1.5.2-macos",
                     "razdor-translate-1.5.2-linux", "Razdor.exe", "razdor", "razdor.apk"):
            p = os.path.join(fdir, name)
            with open(p, "wb") as f:
                f.write(PAYLOAD.get(name, b"bytes of " + name.encode()))
            self.files.append(p)
        notes = os.path.join(root, "notes.md")
        with open(notes, "w", encoding="utf-8") as f:
            f.write("\n### Исправлено\n- Тест.\n")
        self.notes = notes
        FakeClient.releases = {}
        FakeClient.calls = []
        # the registry knows two older releases, as the scripts would have made them
        import gitverse as g
        reg = {"releases": []}
        for tag, rid in (("v1.5.0", 10), ("v1.5.1", 11)):
            g.register(reg, "razdor", tag, rid, "app", False, 1)
        g.save_registry(reg, os.path.join(self.state, "registry.json"))
        FakeClient.releases[("razdor", "v1.5.0")] = {"id": 10, "tag_name": "v1.5.0"}
        FakeClient.releases[("razdor", "v1.5.1")] = {"id": 11, "tag_name": "v1.5.1"}
        self.patches = [
            unittest.mock.patch.object(gv, "Client", FakeClient),
            unittest.mock.patch.object(gv, "fetch_exact", lambda *a, **k: None),
            unittest.mock.patch.object(pages.Pages, "wait_served", lambda self, expected, **k: 0.0),
            unittest.mock.patch.object(pr, "WORK_ROOT", os.path.join(root, "work")),
            unittest.mock.patch.object(pr, "target_for", lambda tag, given: "main"),
            unittest.mock.patch.dict(os.environ, {"RAZDOR_GITVERSE_STATE": self.state}),
            unittest.mock.patch.object(pages, "SCRATCH", self.scratch),
        ]
        orig_init = pages.Pages.__init__

        def remote_init(this, repo=gv.HUB_REPO, token=None, remote=None, scratch=None):
            orig_init(this, repo, token=token, remote=self.bare, scratch=self.scratch)

        self.patches.append(unittest.mock.patch.object(pages.Pages, "__init__", remote_init))
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in reversed(self.patches):
            p.stop()
        self._tmp.cleanup()

    def args(self, tag, prerelease=False):
        return argparse.Namespace(files=self.files, tag=tag, notes=self.notes, prerelease=prerelease,
                                  target=None, replace=False, keep=2, no_feed=False, wait=0, dry_run=False,
                                  keep_work=False, unpublish=None, git_tag=False, push_source=None)

    def bare_feed(self):
        out = subprocess.run(["git", "--git-dir", self.bare, "show", "main:docs/releases.json"],
                             capture_output=True, text=True, check=True).stdout
        return json.loads(out)

    def test_stable_release_updates_feed_and_deletes_the_oldest(self):
        self.assertEqual(pr.publish(self.args("v1.5.2")), 0)
        feed = self.bare_feed()
        self.assertEqual([e["tag_name"] for e in feed], ["v1.5.2", "v1.5.1"])
        names = {a["name"]: a for a in feed[0]["assets"]}
        self.assertEqual(names["Razdor-1.5.2.exe"]["member"], "Razdor.exe")
        self.assertEqual(names["Razdor-1.5.2.exe"]["archive"], "zip")
        self.assertIn("razdor-1.5.2-linux.zip", names["razdor-1.5.2-linux"]["browser_download_url"])
        self.assertEqual(names["razdor.apk"]["browser_download_url"], names["razdor-1.5.2.apk"]["browser_download_url"])
        self.assertIn(("delete", "razdor", 10), FakeClient.calls)
        self.assertNotIn(("delete", "razdor", 11), FakeClient.calls)
        self.assertIn(("create", "razdor", "v1.5.2", False), FakeClient.calls)
        self.assertNotIn(("delete", "razdor", 500), FakeClient.calls)

    def test_prerelease_leaves_feed_and_cleanup_alone(self):
        before = self.bare_feed()
        self.assertEqual(pr.publish(self.args("v1.5.2-rc1", prerelease=True)), 0)
        self.assertEqual(self.bare_feed(), before)
        self.assertFalse([c for c in FakeClient.calls if c[0] in ("delete", "delete-tag")])

    def test_a_second_run_refuses_an_existing_release(self):
        self.assertEqual(pr.publish(self.args("v1.5.2")), 0)
        with self.assertRaises(pr.PlanError):
            pr.publish(self.args("v1.5.2"))

    def test_suffix_without_prerelease_is_refused(self):
        with self.assertRaises(pr.PlanError):
            pr.publish(self.args("v1.5.2-rc1"))


class HubFlowTests(unittest.TestCase):
    """publish_hub.publish() with the same fakes: the Pages site (index, previews, the hub page) is
    pushed, and the catalog releases beyond the newest two are deleted."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = self._tmp.name
        self.bare = os.path.join(root, "remote.git")
        seed = os.path.join(root, "seed")
        os.makedirs(os.path.join(seed, "docs"))
        with open(os.path.join(seed, "docs", "releases.json"), "w", encoding="utf-8") as f:
            f.write("[]\n")
        git("init", "-q", "-b", "main", cwd=seed)
        git("add", "-A", cwd=seed)
        git("commit", "-q", "-m", "seed", cwd=seed)
        git("init", "-q", "--bare", self.bare, cwd=root)
        git("push", "-q", self.bare, "main", cwd=seed)
        self.scratch = os.path.join(root, "scratch")
        self.state = os.path.join(root, "state")
        os.makedirs(self.state)
        self.cat = os.path.join(root, "cat")
        os.makedirs(self.cat)
        self.payloads = {"0123456789abcdef.dtm": b"map dtm", "fedcba9876543210.zip": b"mod zip",
                         "aaaaaaaaaaaaaaaa.png": b"\x89PNG one", "bbbbbbbbbbbbbbbb.png": b"\x89PNG two"}
        for name, data in self.payloads.items():
            with open(os.path.join(self.cat, name), "wb") as f:
                f.write(data)
        import hashlib

        def ref(name, extra=None):
            d = self.payloads[name]
            r = {"path": name, "size": len(d), "sha256": hashlib.sha256(d).hexdigest()}
            r.update(extra or {})
            return r

        index = {"format": 1,
                 "game": {"url": "https://github.com/x/game.zip", "size": 1, "sha256": "11" * 32, "name": "g.zip"},
                 "entries": [{"id": "map-1", "kind": "map", "title": {"ru": "Т"}, "author": "a", "version": "1",
                              "files": [ref("0123456789abcdef.dtm", {"name": "m.DTm", "archive": False})],
                              "preview": ref("aaaaaaaaaaaaaaaa.png", {"name": "preview.png", "archive": False})},
                             {"id": "mod-1", "kind": "mod", "title": {"ru": "М"}, "author": "a", "version": "1",
                              "files": [ref("fedcba9876543210.zip", {"name": "mod.zip", "archive": True})]}]}
        with open(os.path.join(self.cat, "index.json"), "w", encoding="utf-8") as f:
            json.dump(index, f)
        FakeClient.releases = {}
        FakeClient.calls = []
        import gitverse as g
        reg = {"releases": []}
        for tag, created in (("catalog-2026-11-01", "2026-11-01T00:00:00"),
                             ("catalog-2026-11-05", "2026-11-05T00:00:00")):
            g.register(reg, "razdor-community", tag, 20, "catalog", False, 1)
            reg["releases"][-1]["created"] = created
            FakeClient.releases[("razdor-community", tag)] = {"id": 20, "tag_name": tag}
        g.save_registry(reg, os.path.join(self.state, "registry.json"))
        orig_init = pages.Pages.__init__

        def remote_init(this, repo=gv.HUB_REPO, token=None, remote=None, scratch=None):
            orig_init(this, repo, token=token, remote=self.bare, scratch=self.scratch)

        self.patches = [
            unittest.mock.patch.object(gv, "Client", FakeClient),
            unittest.mock.patch.object(gv, "fetch_exact", lambda *a, **k: None),
            unittest.mock.patch.object(pages.Pages, "wait_served", lambda self, expected, **k: 0.0),
            unittest.mock.patch.object(pages.Pages, "__init__", remote_init),
            unittest.mock.patch.dict(os.environ, {"RAZDOR_GITVERSE_STATE": self.state}),
        ]
        for p in self.patches:
            p.start()

    def tearDown(self):
        for p in reversed(self.patches):
            p.stop()
        self._tmp.cleanup()

    def test_catalog_is_pushed_with_previews_and_web_page_and_old_catalogs_go(self):
        import publish_hub as ph
        args = argparse.Namespace(catalog_dir=self.cat, tag="catalog-2026-11-13", no_pages=False, replace=False,
                                  keep=2, wait=0, dry_run=False, unpublish=None)
        self.assertEqual(ph.publish(args), 0)

        def show(path):
            return subprocess.run(["git", "--git-dir", self.bare, "show", "main:" + path],
                                  capture_output=True, check=True).stdout

        idx = json.loads(show("docs/index.json"))
        # the map is a Pages file at its catalog path; the mod's zip is a release asset
        self.assertEqual(idx["entries"][0]["files"][0]["path"], "0123456789abcdef.dtm")
        self.assertEqual(show("docs/0123456789abcdef.dtm"), b"map dtm")
        self.assertTrue(idx["entries"][1]["files"][0]["path"].startswith("https://gitverse.ru/api/attachments/"))
        uploads = [c[2] for c in FakeClient.calls if c[0] == "upload"]
        self.assertEqual(uploads, ["fedcba9876543210.zip"])
        self.assertEqual(idx["entries"][0]["preview"]["path"], "previews/aaaaaaaaaaaaaaaa.png")
        self.assertEqual(idx["game"]["url"], ph.GAME_URL)
        self.assertEqual(idx["game"]["mirrors"], ["https://github.com/x/game.zip"])
        self.assertEqual(show("docs/previews/aaaaaaaaaaaaaaaa.png"), b"\x89PNG one")
        self.assertIn(b"<title>", show("docs/index.html"))
        self.assertEqual(show("docs/releases.json"), b"[]\n")  # the app feed is not touched by a catalog
        deleted = [c for c in FakeClient.calls if c[0] in ("delete", "delete-tag")]
        self.assertEqual(deleted, [("delete", "razdor-community", 20)])

    def test_a_file_with_no_place_stops_before_any_release_is_made(self):
        import publish_hub as ph
        with open(os.path.join(self.cat, "setup.exe"), "wb") as f:
            f.write(b"x")
        with open(os.path.join(self.cat, "index.json"), encoding="utf-8") as f:
            idx = json.load(f)
        idx["entries"][1]["files"][0]["path"] = "setup.exe"
        with open(os.path.join(self.cat, "index.json"), "w", encoding="utf-8") as f:
            json.dump(idx, f)
        FakeClient.calls = []
        args = argparse.Namespace(catalog_dir=self.cat, tag="catalog-2026-11-14", no_pages=False, replace=False,
                                  keep=2, wait=0, dry_run=False, unpublish=None)
        self.assertEqual(ph.publish(args), 2)
        self.assertFalse([c for c in FakeClient.calls if c[0] in ("create", "upload")])


if __name__ == "__main__":
    unittest.main()
