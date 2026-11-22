"""Hermetic tests of hub_candidate_check.py: the matching of a candidate's files, the judge of a run's log,
and the server's Range replies. No Xvfb, no program, no network (the server is plain HTTP on 127.0.0.1).

    python3 -I -m unittest discover -s tools/release -p 'test_*.py'
"""

import copy
import hashlib
import http.client
import json
import os
import shutil
import sys
import tempfile
import threading
import unittest
import zipfile
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import hub_candidate_check as hc  # noqa: E402

GAME_TEXT = b"game bytes " * 500
MAP_TEXT = b"map bytes " * 40
PREVIEW = b"\x89PNG-not-really" * 4
GAME_URL = "https://indicozy.gitverse.site/razdor-community/files/" + hashlib.sha256(GAME_TEXT).hexdigest() + ".dat"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def candidate(folder):
    """A made-up candidate: one wrapped map, one picture, a game on Pages, with the dry run's folders."""
    root = Path(folder)
    (root / "gitverse-zips").mkdir(parents=True)
    (root / "gitverse" / "files").mkdir(parents=True)
    (root / "gitverse" / "previews").mkdir(parents=True)
    (root / "remote-game").mkdir()
    zip_path = root / "gitverse-zips" / "Sample.DTm.zip"
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("Sample.DTm", MAP_TEXT)
    zip_bytes = zip_path.read_bytes()
    (root / "gitverse" / "files" / (sha(GAME_TEXT) + ".dat")).write_bytes(GAME_TEXT)
    (root / "gitverse" / "previews" / "sample.png").write_bytes(PREVIEW)
    index = {
        "format": 1,
        "entries": [{
            "id": "sample-map", "kind": "map", "title": {"ru": "Образец", "en": ""}, "author": "a", "version": "1",
            "preview": {"path": "previews/sample.png", "name": "sample.png", "size": len(PREVIEW), "sha256": sha(PREVIEW)},
            "files": [{
                "path": "https://gitverse.ru/api/attachments/0001", "name": "Sample.DTm", "lang": "ru",
                "size": len(MAP_TEXT), "sha256": sha(MAP_TEXT), "wrapped": "zip", "member": "Sample.DTm",
                "archive_size": len(zip_bytes), "archive_sha256": sha(zip_bytes),
                "mirrors": ["https://github.com/indicozy/razdor-community/releases/latest/download/Sample.DTm"],
            }],
        }],
        "game": {"url": GAME_URL, "mirrors": ["https://gitverse.ru/api/attachments/0002"],
                 "size": len(GAME_TEXT), "sha256": sha(GAME_TEXT), "name": "game.zip"},
    }
    # the bare file the GitHub mirror names sits in the catalog's own folder
    (root / "Sample.DTm").write_bytes(MAP_TEXT)
    (root / "remote-game" / "game.zip").write_bytes(GAME_TEXT)
    return index


class CandidateTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="hub-candidate-test-")
        self.index = candidate(self.tmp)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def test_every_address_is_matched_and_rewritten_local(self):
        plan = hc.build_plan(self.index, self.tmp, 4443)
        new = plan.new_index
        for url in hc.https_fetched(new):
            self.assertTrue(url.startswith("https://127.0.0.1:4443/"), url)
        self.assertTrue(any("zip Sample.DTm" in line for line in plan.checked))

    def test_the_zip_member_is_checked_against_the_file_not_the_zip(self):
        bad = copy.deepcopy(self.index)
        bad["entries"][0]["files"][0]["sha256"] = "0" * 64
        with self.assertRaises(hc.CheckFail):
            hc.build_plan(bad, self.tmp, 4443)

    def test_a_missing_game_file_refuses_the_candidate(self):
        shutil.rmtree(Path(self.tmp) / "gitverse" / "files")
        shutil.rmtree(Path(self.tmp) / "remote-game")
        with self.assertRaises(hc.CheckFail):
            hc.build_plan(self.index, self.tmp, 4443)

    def test_a_wrong_game_sha_refuses_the_candidate(self):
        bad = copy.deepcopy(self.index)
        bad["game"]["sha256"] = "1" * 64
        with self.assertRaises(hc.CheckFail):
            hc.build_plan(bad, self.tmp, 4443)

    def test_the_ad_link_is_not_fetched_and_is_left_alone(self):
        idx = copy.deepcopy(self.index)
        idx["ad"] = {"id": "x", "version": 1, "image": "previews/sample.png", "size": len(PREVIEW),
                     "sha256": sha(PREVIEW), "url": "https://example.invalid/ad", "caption_ru": "a", "caption_en": "b"}
        plan = hc.build_plan(idx, self.tmp, 4443)
        self.assertNotIn("https://example.invalid/ad", hc.https_fetched(plan.new_index))


class JudgeTest(unittest.TestCase):
    LOG = (
        "game download: catalog read, game in it: yes\n"
        "game download: the catalog offers game.zip (1024 bytes)\n"
        "game download: ended: installed\n"
    )
    INDEX = {"game": {"size": 1024, "url": GAME_URL}}
    ACCESS = [
        {"path": "/cand/index-v2.json", "status": 200, "range": None},
        {"path": "/cand/h/indicozy.gitverse.site/razdor-community/files/" + "a" * 64 + ".dat", "status": 206,
         "range": "bytes=0-99"},
    ]

    def test_a_good_run_passes(self):
        out = hc.judge(self.LOG, copy.deepcopy(self.ACCESS), {"game": {"size": 1024, "url": "https://x/" + "a" * 64 + ".dat"}}, 1024)
        self.assertTrue(out["passed"], out)

    def test_no_catalog_line_fails(self):
        out = hc.judge(self.LOG.replace("in it: yes", "in it: no"), copy.deepcopy(self.ACCESS), self.INDEX, 1024)
        self.assertFalse(out["passed"])

    def test_an_offer_of_another_size_fails(self):
        out = hc.judge(self.LOG.replace("1024 bytes", "999 bytes"), copy.deepcopy(self.ACCESS), self.INDEX, 1024)
        self.assertFalse(out["passed"])
        self.assertIn("not the catalog's", out["why"])

    def test_a_failed_download_fails(self):
        out = hc.judge(self.LOG.replace("installed", "sha256 does not match"), copy.deepcopy(self.ACCESS), self.INDEX, 1024)
        self.assertFalse(out["passed"])

    def test_no_game_request_fails(self):
        out = hc.judge(self.LOG, [self.ACCESS[0]], self.INDEX, 1024)
        self.assertFalse(out["passed"])
        self.assertIn("game from the server", out["why"])


class RangeTest(unittest.TestCase):
    """The server answers Range with 206 for a valid range, 416 for one past the end, and the whole file otherwise."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp(prefix="hub-range-test-")
        cls.body = bytes(range(256)) * 40
        cls.file = Path(cls.tmp) / "f.bin"
        cls.file.write_bytes(cls.body)
        state = hc.State(b"{}", {f"{hc.ROUTE}/f.bin": (cls.file, len(cls.body), sha(cls.body))})
        handler = type("H", (hc.Handler,), {"state": state})
        cls.server = hc.Server(("127.0.0.1", 0), handler)
        cls.port = cls.server.server_address[1]
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def get(self, headers=None):
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        conn.request("GET", f"{hc.ROUTE}/f.bin", headers=headers or {})
        resp = conn.getresponse()
        data = resp.read()
        conn.close()
        return resp.status, resp.getheader("Content-Range"), data

    def test_a_full_get_is_200(self):
        status, _, data = self.get()
        self.assertEqual(status, 200)
        self.assertEqual(data, self.body)

    def test_a_range_is_206_with_its_bytes(self):
        status, cr, data = self.get({"Range": "bytes=10-19"})
        self.assertEqual(status, 206)
        self.assertEqual(cr, f"bytes 10-19/{len(self.body)}")
        self.assertEqual(data, self.body[10:20])

    def test_an_open_range_runs_to_the_end(self):
        status, _, data = self.get({"Range": f"bytes={len(self.body) - 5}-"})
        self.assertEqual(status, 206)
        self.assertEqual(data, self.body[-5:])

    def test_a_range_past_the_end_is_416(self):
        status, _, _ = self.get({"Range": f"bytes={len(self.body) + 10}-"})
        self.assertEqual(status, 416)


if __name__ == "__main__":
    unittest.main()
