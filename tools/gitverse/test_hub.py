"""The hub catalog's routing (release or Pages), its GitVerse copy, the Pages site size and the web files
(no network)."""

import json
import os
import re
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402
import publish_hub as ph  # noqa: E402

GH_GAME = "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/game.zip"
ATT = "https://gitverse.ru/api/attachments/"


def catalog():
    """A small catalog in the real shape: a .dtm map, a zip mod, pictures, an ad, the game."""
    return {
        "format": 1,
        "game": {"url": GH_GAME, "size": 10, "sha256": "ab" * 32, "name": "game.zip"},
        "ad": {"id": "thorny", "version": 1, "image": "aaaaaaaaaaaaaaaa.png", "size": 5, "sha256": "cd" * 32,
               "url": "https://store.example/x", "caption": {"ru": "Реклама", "en": "Ad"}},
        "entries": [
            {
                "id": "map-1",
                "kind": "map",
                "title": {"ru": "Карта"},
                "files": [{"path": "bbbbbbbbbbbbbbbb.dtm", "name": "Map.DTm", "size": 3, "sha256": "ef" * 32,
                           "archive": False}],
                "preview": {"path": "cccccccccccccccc.png", "name": "preview.png", "size": 4,
                            "sha256": "01" * 32, "archive": False},
                "picture": {"path": "dddddddddddddddd.png", "name": "picture.png", "size": 4,
                            "sha256": "02" * 32, "archive": False},
            },
            {
                "id": "mod-x",
                "kind": "mod",
                "files": [{"path": "eeeeeeeeeeeeeeee.zip", "name": "mod.zip", "size": 9, "sha256": "03" * 32,
                           "archive": True}],
            },
        ],
    }


def urls_for(cat):
    """The addresses the upload would give: one per release asset."""
    return {n: f"{ATT}{n[:4]}-uuid" for n, _ in ph.refs(cat) if ph.destination(n, "file")[0] == "release"}


class RoutingTests(unittest.TestCase):
    def test_archives_are_release_assets_everything_else_is_pages(self):
        self.assertEqual(ph.destination("x.zip", "file"), ("release", None))
        self.assertEqual(ph.destination("x.tgz", "file"), ("release", None))
        self.assertEqual(ph.destination("x.apk", "file"), ("release", None))
        self.assertEqual(ph.destination("x.dtm", "file"), ("pages", None))
        self.assertEqual(ph.destination("x.DTm", "file"), ("pages", None))
        self.assertEqual(ph.destination("x.png", "picture"), ("pages", None))
        self.assertEqual(ph.destination("x.png", "file"), ("pages", None))

    def test_a_file_neither_can_take_is_a_problem(self):
        self.assertIsNotNone(ph.destination("x.zip", "picture")[1])   # a zip on Pages: refused
        self.assertIsNotNone(ph.destination("x.exe", "file")[1])      # Pages refuses .exe
        self.assertIsNotNone(ph.destination("x.py", "file")[1])       # Pages refuses .py
        self.assertIsNotNone(ph.destination("noext", "file")[1])      # no extension
        self.assertIsNotNone(ph.destination("with space.dtm", "file")[1])
        self.assertIsNotNone(ph.destination("index.json", "file")[1])  # the site's own file

    def test_refs_list_each_file_once_by_role(self):
        self.assertEqual(ph.refs(catalog()), [
            ("aaaaaaaaaaaaaaaa.png", "picture"),   # the ad's picture
            ("bbbbbbbbbbbbbbbb.dtm", "file"),
            ("cccccccccccccccc.png", "picture"),
            ("dddddddddddddddd.png", "picture"),
            ("eeeeeeeeeeeeeeee.zip", "file"),
        ])

    def test_plan_splits_release_from_pages(self):
        pl = ph.plan(catalog())
        self.assertEqual(pl["problems"], [])
        self.assertEqual(pl["release"], ["eeeeeeeeeeeeeeee.zip"])
        self.assertEqual(pl["pages"], {
            "bbbbbbbbbbbbbbbb.dtm": "bbbbbbbbbbbbbbbb.dtm",
            "previews/cccccccccccccccc.png": "cccccccccccccccc.png",
            "previews/dddddddddddddddd.png": "dddddddddddddddd.png",
            "previews/aaaaaaaaaaaaaaaa.png": "aaaaaaaaaaaaaaaa.png",
        })

    def test_plan_reports_problems_instead_of_routing_them(self):
        cat = catalog()
        cat["entries"][0]["files"][0]["path"] = "Setup.exe"
        pl = ph.plan(cat)
        self.assertEqual([n for n, _ in pl["problems"]], ["Setup.exe"])
        self.assertNotIn("Setup.exe", pl["pages"])

    def test_pages_size_verdicts(self):
        self.assertEqual(ph.pages_size_verdict(399 * ph.MB), "ok")
        self.assertEqual(ph.pages_size_verdict(401 * ph.MB), "warn")
        self.assertEqual(ph.pages_size_verdict(499 * ph.MB), "warn")
        self.assertEqual(ph.pages_size_verdict(500 * ph.MB), "stop")


class IndexTests(unittest.TestCase):
    def test_archive_becomes_attachment_dtm_stays_relative_pictures_under_previews(self):
        cat = catalog()
        idx = ph.gitverse_index(cat, urls_for(cat))
        e = idx["entries"][0]
        self.assertEqual(e["files"][0]["path"], "bbbbbbbbbbbbbbbb.dtm")
        self.assertEqual(e["preview"]["path"], "previews/cccccccccccccccc.png")
        self.assertEqual(e["picture"]["path"], "previews/dddddddddddddddd.png")
        mod = idx["entries"][1]["files"][0]["path"]
        self.assertTrue(mod.startswith(ATT))
        self.assertEqual(idx["ad"]["image"], "previews/aaaaaaaaaaaaaaaa.png")
        # the input is not changed
        self.assertEqual(cat["entries"][0]["files"][0]["path"], "bbbbbbbbbbbbbbbb.dtm")

    def test_game_keeps_gitverse_address_and_mirrors_the_github_one(self):
        idx = ph.gitverse_index(catalog(), urls_for(catalog()))
        self.assertEqual(idx["game"]["url"], ph.GAME_URL)
        self.assertEqual(idx["game"]["mirrors"], [GH_GAME])
        self.assertEqual(idx["game"]["sha256"], "ab" * 32)

    def test_game_mirrors_do_not_repeat(self):
        cat = catalog()
        cat["game"]["url"] = ph.GAME_URL
        cat["game"]["mirrors"] = [GH_GAME, GH_GAME]
        idx = ph.gitverse_index(cat, urls_for(cat))
        self.assertEqual(idx["game"]["mirrors"], [GH_GAME])

    def test_catalog_without_game_or_ad_still_rewrites(self):
        cat = catalog()
        del cat["game"]
        del cat["ad"]
        idx = ph.gitverse_index(cat, urls_for(cat))
        self.assertNotIn("game", idx)
        self.assertNotIn("ad", idx)

    def test_check_index_refuses_a_zip_that_is_not_an_address(self):
        with self.assertRaises(ph.HubError):
            ph.check_index({"entries": [{"id": "x", "files": [{"path": "a.zip"}]}]})
        ph.check_index({"entries": [{"id": "x", "files": [{"path": "a.dtm"}]}]})
        cat = catalog()
        ph.check_index(ph.gitverse_index(cat, urls_for(cat)))

    def test_refs_refuse_a_path_with_folders(self):
        cat = catalog()
        cat["entries"][0]["files"][0]["path"] = "maps/x.dtm"
        with self.assertRaises(ph.HubError):
            ph.refs(cat)


class StageTests(unittest.TestCase):
    def test_stage_is_the_site_layout(self):
        with tempfile.TemporaryDirectory() as d:
            cat = catalog()
            for name in ("cccccccccccccccc.png", "dddddddddddddddd.png", "aaaaaaaaaaaaaaaa.png",
                         "bbbbbbbbbbbbbbbb.dtm"):
                with open(os.path.join(d, name), "wb") as f:
                    f.write(b"x" + name.encode())
            pl = ph.plan(cat)
            idx = ph.gitverse_index(cat, urls_for(cat))
            stage = ph.write_stage(d, idx, pl["pages"])
            with open(os.path.join(stage, "index.json"), encoding="utf-8") as f:
                self.assertEqual(json.load(f)["entries"][0]["files"][0]["path"], "bbbbbbbbbbbbbbbb.dtm")
            self.assertTrue(os.path.isfile(os.path.join(stage, "bbbbbbbbbbbbbbbb.dtm")))
            self.assertTrue(os.path.isfile(os.path.join(stage, "previews", "cccccccccccccccc.png")))
            files = ph.stage_files(stage)
            self.assertEqual(sorted(files), ["bbbbbbbbbbbbbbbb.dtm", "index.json",
                                             "previews/aaaaaaaaaaaaaaaa.png", "previews/cccccccccccccccc.png",
                                             "previews/dddddddddddddddd.png"])

    def test_registry_prune_for_catalogs(self):
        reg = [{"tag": f"catalog-2026-11-0{i}", "kind": "catalog", "created": f"2026-11-0{i}T00:00:00"}
               for i in range(1, 4)]
        gone = [e["tag"] for e in gv.select_prune_catalogs(reg, keep=2)]
        self.assertEqual(gone, ["catalog-2026-11-01"])


class SyncTests(unittest.TestCase):
    def test_sync_site_removes_old_catalog_files_and_keeps_the_feed(self):
        import subprocess
        with tempfile.TemporaryDirectory() as d:
            seed = os.path.join(d, "seed")
            os.makedirs(os.path.join(seed, "docs", "previews"))
            for rel, data in (("docs/old.dtm", b"old"), ("docs/previews/old.png", b"old"),
                              ("docs/releases.json", b"[]\n"), ("docs/.nojekyll", b"")):
                with open(os.path.join(seed, rel), "wb") as f:
                    f.write(data)
            env = ["-c", "user.name=t", "-c", "user.email=t@example.invalid"]
            subprocess.run(["git", *env, "init", "-q", "-b", "main", seed], check=True, capture_output=True)
            subprocess.run(["git", *env, "-C", seed, "add", "-A"], check=True, capture_output=True)
            subprocess.run(["git", *env, "-C", seed, "commit", "-q", "-m", "seed"], check=True, capture_output=True)
            bare = os.path.join(d, "remote.git")
            subprocess.run(["git", "init", "-q", "--bare", bare], check=True, capture_output=True)
            subprocess.run(["git", "-C", seed, "push", "-q", bare, "main"], check=True, capture_output=True)
            pg = pages.Pages(gv.HUB_REPO, remote=bare, scratch=os.path.join(d, "scratch"))
            pg.sync_site({"index.json": b"{}", "new.dtm": b"new", "previews/new.png": b"png",
                          "index.html": b"<html>", "hub.js": b"", "hub.css": b""})
            self.assertFalse(os.path.exists(os.path.join(pg.dir, "docs", "old.dtm")))
            self.assertFalse(os.path.exists(os.path.join(pg.dir, "docs", "previews", "old.png")))
            self.assertEqual(pg.read("docs/releases.json"), b"[]\n")
            self.assertTrue(os.path.exists(os.path.join(pg.dir, "docs", ".nojekyll")))
            self.assertEqual(pg.read("docs/new.dtm"), b"new")


class WebTests(unittest.TestCase):
    def test_web_files_are_the_hub_page_without_a_files_base(self):
        files = pages.web_files()
        self.assertEqual(sorted(files), ["hub.css", "hub.js", "index.html"])
        live = re.sub(rb"(?s)<!--.*?-->", b"", files["index.html"])  # the example in the comment is not a meta
        self.assertNotIn(b'name="razdor-files-base"', live)

    def test_an_active_files_base_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            web = os.path.join(d, "web")
            os.makedirs(web)
            for name in ("index.html", "hub.js", "hub.css"):
                with open(os.path.join(web, name), "w", encoding="utf-8") as f:
                    f.write('<meta name="razdor-files-base" content="https://x/">' if name == "index.html" else "")
            saved = pages.WEB_DIR
            pages.WEB_DIR = web
            try:
                with self.assertRaises(pages.PagesError):
                    pages.web_files()
            finally:
                pages.WEB_DIR = saved


if __name__ == "__main__":
    unittest.main()
