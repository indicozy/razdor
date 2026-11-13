"""The hub catalog's GitVerse copy and the Pages web files (no network)."""

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


def catalog():
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
    uploads, _ = ph.refs(cat)
    return {n: f"https://gitverse.ru/api/attachments/{n[:4]}-uuid" for n in uploads}


class IndexTests(unittest.TestCase):
    def test_refs_split_release_files_from_pages_pictures(self):
        uploads, pictures = ph.refs(catalog())
        self.assertEqual(sorted(uploads), ["aaaaaaaaaaaaaaaa.png", "bbbbbbbbbbbbbbbb.dtm", "eeeeeeeeeeeeeeee.zip"])
        self.assertEqual(sorted(pictures), ["cccccccccccccccc.png", "dddddddddddddddd.png"])

    def test_files_become_attachment_addresses_previews_stay_relative_under_previews(self):
        cat = catalog()
        idx = ph.gitverse_index(cat, urls_for(cat))
        e = idx["entries"][0]
        self.assertTrue(e["files"][0]["path"].startswith("https://gitverse.ru/api/attachments/"))
        self.assertEqual(e["preview"]["path"], "previews/cccccccccccccccc.png")
        self.assertEqual(e["picture"]["path"], "previews/dddddddddddddddd.png")
        self.assertTrue(idx["entries"][1]["files"][0]["path"].startswith("https://gitverse.ru/api/attachments/"))
        self.assertTrue(idx["ad"]["image"].startswith("https://gitverse.ru/api/attachments/"))
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

    def test_check_index_refuses_a_relative_file(self):
        with self.assertRaises(ph.HubError):
            ph.check_index({"entries": [{"id": "x", "files": [{"path": "a.zip"}]}]})
        cat = catalog()
        ph.check_index(ph.gitverse_index(cat, urls_for(cat)))

    def test_refs_refuse_a_path_with_folders(self):
        cat = catalog()
        cat["entries"][0]["files"][0]["path"] = "maps/x.dtm"
        with self.assertRaises(ph.HubError):
            ph.refs(cat)

    def test_refused_names_are_listed_by_the_name_rule(self):
        cat = catalog()
        cat["entries"][0]["files"][0]["path"] = "Setup.exe"
        uploads, _ = ph.refs(cat)
        self.assertIsNotNone(gv.name_problem("Setup.exe"))
        self.assertIn("Setup.exe", uploads)
        cat["entries"][0]["files"][0]["path"] = "noext"
        uploads, _ = ph.refs(cat)
        self.assertIsNotNone(gv.name_problem("noext"))

    def test_stage_writes_index_and_previews(self):
        with tempfile.TemporaryDirectory() as d:
            cat = catalog()
            for name in ("cccccccccccccccc.png", "dddddddddddddddd.png"):
                with open(os.path.join(d, name), "wb") as f:
                    f.write(b"png!")
            idx = ph.gitverse_index(cat, urls_for(cat))
            stage = ph.write_stage(d, idx, ["cccccccccccccccc.png", "dddddddddddddddd.png"])
            with open(os.path.join(stage, "index.json"), encoding="utf-8") as f:
                self.assertEqual(json.load(f)["entries"][0]["preview"]["path"], "previews/cccccccccccccccc.png")
            self.assertTrue(os.path.isfile(os.path.join(stage, "previews", "cccccccccccccccc.png")))

    def test_registry_prune_for_catalogs(self):
        reg = [{"tag": f"catalog-2026-11-0{i}", "kind": "catalog", "created": f"2026-11-0{i}T00:00:00"}
               for i in range(1, 4)]
        gone = [e["tag"] for e in gv.select_prune_catalogs(reg, keep=2)]
        self.assertEqual(gone, ["catalog-2026-11-01"])


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
