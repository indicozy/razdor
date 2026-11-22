"""The pages layout (publish_hub.py --layout pages): every hub file and the game are bare Pages files under
files/<sha256>.dat on each host, the index names them, the Pages site of the GitHub host is the root of its
gh-pages branch. Hermetic: real bytes in a temp catalog, a local bare repository and a local server."""

import hashlib
import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import types
import unittest
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402
import pages  # noqa: E402
import publish_hub as ph  # noqa: E402

GV_BASE = "https://indicozy.gitverse.site/razdor-community"
GH_BASE = "https://indicozy.github.io/razdor-community"


def git(*args, cwd):
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                   cwd=cwd, check=True, capture_output=True)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def real_catalog(folder):
    """A catalog in the real shape with real bytes: a map, a mod's zip, two entries that share one map's
    bytes, a preview, an ad picture, and the game archive (already in DIR/remote-game, so nothing is fetched)."""
    blobs = {
        "map.dtm": b"DTM-map-bytes" * 50,
        "mod.zip": None,
        "pic.png": b"\x89PNG-preview",
        "ad.png": b"\x89PNG-ad",
    }
    mod_path = os.path.join(folder, "mod.zip")
    with zipfile.ZipFile(mod_path, "w") as z:
        z.writestr("pack/readme.txt", "mod")
    blobs["mod.zip"] = open(mod_path, "rb").read()
    game = b"GAME-ARCHIVE" * 100
    os.makedirs(os.path.join(folder, ph.REMOTE_GAME))
    with open(os.path.join(folder, ph.REMOTE_GAME, "game.zip"), "wb") as f:
        f.write(game)
    for name, data in blobs.items():
        with open(os.path.join(folder, name), "wb") as f:
            f.write(data)
    m = blobs["map.dtm"]
    cat = {
        "format": 1,
        "game": {"url": "https://github.example/game-cu.zip", "size": len(game), "sha256": sha(game),
                 "name": "game.zip"},
        "ad": {"id": "thorny", "version": 1, "image": "ad.png", "size": len(blobs["ad.png"]),
               "sha256": sha(blobs["ad.png"]), "url": "https://store.example/x", "caption_ru": "a"},
        "entries": [
            {"id": "map-1", "kind": "map", "title": {"ru": "Карта"},
             "files": [{"path": "map.dtm", "name": "Map.DTm", "size": len(m), "sha256": sha(m), "archive": False}],
             "preview": {"path": "pic.png", "name": "pic.png", "size": len(blobs["pic.png"]),
                         "sha256": sha(blobs["pic.png"])}},
            {"id": "map-2", "kind": "map", "title": {"ru": "Копия"},
             "files": [{"path": "map.dtm", "name": "Copy.DTm", "size": len(m), "sha256": sha(m), "archive": False}]},
            {"id": "mod-x", "kind": "mod", "title": {"ru": "Мод"},
             "files": [{"path": "mod.zip", "name": "mod.zip", "size": len(blobs["mod.zip"]),
                        "sha256": sha(blobs["mod.zip"]), "archive": True}]},
        ],
    }
    with open(os.path.join(folder, "index.json"), "w", encoding="utf-8") as f:
        json.dump(cat, f, ensure_ascii=False)
    return cat, game


class RoutingTests(HermeticTestCase):
    def test_site_path_is_the_sha_as_dat_and_never_a_zip(self):
        digest = "AB" * 32
        self.assertEqual(ph.pages_site_path(digest), "files/" + "ab" * 32 + ".dat")
        for bad in ("", "xyz", "ab" * 31, None):
            with self.assertRaises(ph.HubError):
                ph.pages_site_path(bad)

    def test_routing_puts_every_file_on_pages_and_shares_identical_bytes(self):
        with tempfile.TemporaryDirectory() as d:
            cat, _ = real_catalog(d)
            rt = ph.pages_routing(cat)
            self.assertEqual(rt["problems"], [])
            self.assertEqual(rt["files"]["mod.zip"], "files/" + sha(open(os.path.join(d, "mod.zip"), "rb").read()) + ".dat")
            self.assertIn("previews/pic.png", rt["site"])
            self.assertIn("previews/ad.png", rt["site"])
            files = [p for p in rt["site"] if p.startswith("files/")]
            self.assertEqual(len(files), 2, "the map is one site file for its two entries; the mod is the other")
            self.assertTrue(all(p.endswith(".dat") for p in files))

    def test_a_zip_picture_has_no_place_and_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            cat, _ = real_catalog(d)
            cat["entries"][0]["preview"]["path"] = "pic.zip"
            rt = ph.pages_routing(cat)
            self.assertTrue(any(name == "pic.zip" for name, _ in rt["problems"]))

    def test_two_names_with_different_sums_are_refused(self):
        with tempfile.TemporaryDirectory() as d:
            cat, _ = real_catalog(d)
            cat["entries"][1]["files"][0]["sha256"] = "00" * 32
            with self.assertRaises(ph.HubError):
                ph.file_shas(cat)


class IndexTests(HermeticTestCase):
    def test_index_names_bare_pages_files_on_the_host_it_is_for(self):
        with tempfile.TemporaryDirectory() as d:
            cat, game = real_catalog(d)
            idx = ph.pages_index(cat, GV_BASE, [ph.GAME_URL, ph.GAME_GITHUB_URL])
            for e in idx["entries"]:
                for f in e.get("files") or []:
                    self.assertTrue(f["path"].startswith(GV_BASE + "/files/"), f["path"])
                    self.assertFalse({"wrapped", "member", "archive_size", "archive_sha256"} & set(f))
                    self.assertEqual(f["path"], GV_BASE + "/files/" + f["sha256"] + ".dat")
            mod = idx["entries"][2]["files"][0]
            self.assertTrue(mod["archive"], "a mod's zip keeps its archive flag")
            self.assertEqual(idx["entries"][0]["preview"]["path"], "previews/pic.png")
            self.assertEqual(idx["ad"]["image"], "previews/ad.png")
            self.assertEqual(idx["game"]["url"], GV_BASE + "/files/" + sha(game) + ".dat")
            self.assertEqual(idx["game"]["mirrors"], [ph.GAME_URL, ph.GAME_GITHUB_URL])
            self.assertEqual(idx["game"]["size"], len(game))

    def test_the_github_copy_uses_its_own_host(self):
        with tempfile.TemporaryDirectory() as d:
            cat, game = real_catalog(d)
            idx = ph.pages_index(cat, GH_BASE, [GV_BASE + "/files/" + sha(game) + ".dat", ph.GAME_URL])
            self.assertTrue(all(f["path"].startswith(GH_BASE + "/files/") for e in idx["entries"]
                                for f in e.get("files") or []))
            self.assertEqual(idx["game"]["mirrors"][0], GV_BASE + "/files/" + sha(game) + ".dat")

    def test_a_wrapped_file_in_the_pages_index_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            cat, _ = real_catalog(d)
            idx = ph.pages_index(cat, GV_BASE, [])
            idx["entries"][0]["files"][0]["wrapped"] = "zip"
            with self.assertRaises(ph.HubError):
                ph.check_pages_index(idx, GV_BASE)


class PublishDryRunTests(HermeticTestCase):
    def args(self, folder, host):
        return types.SimpleNamespace(catalog_dir=folder, tag=None, dry_run=True, no_pages=False, wait=0,
                                     replace=False, keep=2, layout="pages", host=host)

    def test_dry_run_stages_every_file_and_writes_the_index_without_sending(self):
        with tempfile.TemporaryDirectory() as d:
            cat, game = real_catalog(d)
            for host, base in (("gitverse", GV_BASE), ("github", GH_BASE)):
                self.assertEqual(ph.publish(self.args(d, host)), 0)
                stage = os.path.join(d, "pages-" + host)
                with open(os.path.join(stage, "index-v2.json"), encoding="utf-8") as f:
                    idx = json.load(f)
                for e in idx["entries"]:
                    for f in e.get("files") or []:
                        rel = f["path"][len(base) + 1:]
                        with open(os.path.join(stage, *rel.split("/")), "rb") as src:
                            self.assertEqual(sha(src.read()), f["sha256"])
                self.assertTrue(os.path.isfile(os.path.join(stage, "files", sha(game) + ".dat")))
                self.assertTrue(os.path.isfile(os.path.join(stage, "previews", "ad.png")))
            self.assertFalse(os.path.exists(os.path.join(d, "gitverse-state.json")), "no release state written")


class FakePages:
    """Stands in for pages.Pages: records the calls of a publish, in order (nothing is cloned or pushed)."""
    calls = []

    def __init__(self, repo, host="gitverse"):
        self.host = host
        FakePages.calls.append(("open", host))

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def _in_repo(self, rel):
        return rel

    def sync_site_dir(self, src, remove=True, skip=()):
        FakePages.calls.append(("sync", remove, tuple(skip)))

    def commit_push(self, message):
        FakePages.calls.append(("commit", message))
        return "abc12345"

    def wait_served(self, expected, wait=0):
        FakePages.calls.append(("wait", sorted(expected)))
        return 0.0


class PublishOrderTests(HermeticTestCase):
    def test_the_files_are_served_before_the_index_moves(self):
        with tempfile.TemporaryDirectory() as d:
            real_catalog(d)
            saved = ph.pages.Pages
            ph.pages.Pages = FakePages
            FakePages.calls = []
            try:
                args = types.SimpleNamespace(catalog_dir=d, tag=None, dry_run=False, no_pages=False, wait=0,
                                             replace=False, keep=2, layout="pages", host="github")
                self.assertEqual(ph.publish(args), 0)
            finally:
                ph.pages.Pages = saved
            calls = FakePages.calls
            self.assertEqual(calls[0], ("open", "github"))
            self.assertEqual(calls[1], ("sync", False, (ph.INDEX_V2,)), "step 1 adds the files, not the index")
            self.assertEqual(calls[2][0], "commit")
            self.assertEqual(calls[3][0], "wait")
            samples = calls[3][1]
            self.assertIn("files/" + sha(b"GAME-ARCHIVE" * 100) + ".dat", samples)
            self.assertTrue(any(s.startswith("files/") for s in samples))
            self.assertTrue(any(s.startswith("previews/") for s in samples))
            self.assertEqual(calls[4], ("sync", True, ()), "step 2 moves the index and removes what it no longer names")
            self.assertEqual(calls[6], ("wait", [ph.INDEX_V2]), "the index is the last thing served")


class HostTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        root = self._tmp.name
        self.bare = os.path.join(root, "remote.git")
        seed = os.path.join(root, "seed")
        self.scratch = os.path.join(root, "scratch")
        os.makedirs(seed)
        git("init", "-q", "-b", "gh-pages", cwd=seed)
        with open(os.path.join(seed, "README"), "w") as f:
            f.write("seed\n")
        git("add", "-A", cwd=seed)
        git("commit", "-q", "-m", "seed", cwd=seed)
        git("init", "-q", "--bare", self.bare, cwd=root)
        git("push", "-q", self.bare, "gh-pages", cwd=seed)

    def tearDown(self):
        self._tmp.cleanup()
        super().tearDown()

    def test_github_host_writes_the_site_at_the_root_of_gh_pages(self):
        self.assertEqual(pages.HOSTS["github"]["branch"], "gh-pages")
        self.assertEqual(pages.HOSTS["github"]["site_dir"], "")
        with pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch, host="github") as pg:
            self.assertEqual(pg._in_repo("index-v2.json"), "index-v2.json")
            pg.sync_site({"index-v2.json": b"{}", "files/aa.dat": b"x"})
            head = pg.commit_push("Hub catalog test")
            self.assertIsNotNone(head)
        tree = subprocess.run(["git", "--git-dir", self.bare, "ls-tree", "-r", "--name-only", "gh-pages"],
                              capture_output=True, text=True, check=True).stdout.split()
        self.assertIn("index-v2.json", tree)
        self.assertIn("files/aa.dat", tree)
        self.assertIn("config.json", tree)
        self.assertIn(".nojekyll", tree)
        self.assertNotIn("docs/index-v2.json", tree)

    def test_sync_site_dir_copies_the_stage_and_removes_old_files(self):
        stage = os.path.join(self.scratch, "stage-src")
        os.makedirs(os.path.join(stage, "files"))
        with open(os.path.join(stage, "files", "new.dat"), "wb") as f:
            f.write(b"new")
        with pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch, host="github") as pg:
            pg.write("files/old.dat", b"old")
            pg.sync_site_dir(stage)
            self.assertFalse(os.path.exists(os.path.join(pg.dir, "files", "old.dat")))
            self.assertEqual(pg.read("files/new.dat"), b"new")

    def test_the_feed_copy_goes_to_the_github_root_as_releases_json(self):
        import publish_release as pr
        import unittest.mock as mock
        orig_init = pages.Pages.__init__

        def remote_init(this, repo=gv.HUB_REPO, token=None, remote=None, scratch=None, host="gitverse"):
            orig_init(this, repo, token=token, remote=self.bare, scratch=self.scratch, host=host)

        with mock.patch.object(pages.Pages, "__init__", remote_init), \
                mock.patch.object(pages.Pages, "wait_served", lambda self, expected, **k: 0.0):
            pr.push_feed_copies(b"[]\n", 0)
        tree = subprocess.run(["git", "--git-dir", self.bare, "ls-tree", "--name-only", "gh-pages"],
                              capture_output=True, text=True, check=True).stdout.split()
        self.assertIn("releases.json", tree)
        content = subprocess.run(["git", "--git-dir", self.bare, "show", "gh-pages:releases.json"],
                                 capture_output=True, text=True, check=True).stdout
        self.assertEqual(content, "[]\n")

    def test_two_step_sync_adds_first_then_removes_and_keeps_the_feed(self):
        stage = os.path.join(self.scratch, "stage-two")
        os.makedirs(os.path.join(stage, "files"))
        for rel, data in (("index-v2.json", b"new-index"), ("files/new.dat", b"new")):
            with open(os.path.join(stage, *rel.split("/")), "wb") as f:
                f.write(data)
        with pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch, host="github") as pg:
            pg.write("releases.json", b"[]")
            pg.write("index-v2.json", b"old-index")
            pg.write("files/old.dat", b"old")
            # step 1: additive, the live index stays
            pg.sync_site_dir(stage, remove=False, skip=("index-v2.json",))
            self.assertEqual(pg.read("index-v2.json"), b"old-index")
            self.assertEqual(pg.read("files/old.dat"), b"old")
            self.assertEqual(pg.read("files/new.dat"), b"new")
            # step 2: the index, and what it no longer names goes; the feed stays
            pg.sync_site_dir(stage)
            self.assertEqual(pg.read("index-v2.json"), b"new-index")
            self.assertIsNone(pg.read("files/old.dat"))
            self.assertEqual(pg.read("releases.json"), b"[]")

    def test_wait_uses_the_host_base_and_the_root_paths(self):
        class H(http.server.BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_GET(self):
                body = b'{"ok": 1}' if self.path == "/index-v2.json" else b"<html>not found</html>"
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        try:
            pg = object.__new__(pages.Pages)
            pg.repo = "razdor-community"
            pg.base = f"http://127.0.0.1:{httpd.server_address[1]}/"
            pg.site_dir = ""
            waited = pages.Pages.wait_served(pg, {"index-v2.json": b'{"ok": 1}'}, wait=5, interval=0,
                                             sleep=lambda s: None)
            self.assertGreaterEqual(waited, 0)
        finally:
            httpd.shutdown()
            httpd.server_close()


if __name__ == "__main__":
    unittest.main()
