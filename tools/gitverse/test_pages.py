"""The Pages push (commit, no-op, push, wait until served) against a local bare repository and a
local server (no network)."""

import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402


def git(*args, cwd):
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                   cwd=cwd, check=True, capture_output=True)


class PagesRepoTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        root = self._tmp.name
        self.bare = os.path.join(root, "remote.git")
        self.seed = os.path.join(root, "seed")
        self.scratch = os.path.join(root, "scratch")
        os.makedirs(self.seed)
        git("init", "-q", "-b", "main", cwd=self.seed)
        os.makedirs(os.path.join(self.seed, "docs"))
        with open(os.path.join(self.seed, "docs", "releases.json"), "w") as f:
            f.write("[]\n")
        git("add", "-A", cwd=self.seed)
        git("commit", "-q", "-m", "seed", cwd=self.seed)
        git("init", "-q", "--bare", self.bare, cwd=root)
        git("push", "-q", self.bare, "main", cwd=self.seed)

    def tearDown(self):
        self._tmp.cleanup()

    def test_commit_and_push_then_no_change_is_a_no_op(self):
        pg = pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch)
        self.assertEqual(pg.read("docs/releases.json"), b"[]\n")
        self.assertIsNone(pg.read("docs/nope.json"))
        pg.write("docs/releases.json", b'[{"tag_name": "v1.0.0"}]\n')
        head = pg.commit_push("Update feed: v1.0.0")
        self.assertIsNotNone(head)
        out = subprocess.run(["git", "--git-dir", self.bare, "log", "-1", "--format=%an <%ae>|%s", "main"],
                             capture_output=True, text=True, check=True).stdout.strip()
        self.assertEqual(out, "indicozy <indicozy@gmail.com>|Update feed: v1.0.0")
        # the same bytes again: nothing to commit, nothing pushed
        pg2 = pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch)
        pg2.write("docs/releases.json", b'[{"tag_name": "v1.0.0"}]\n')
        self.assertIsNone(pg2.commit_push("again"))

    def test_clone_remote_is_left_tokenless(self):
        pg = pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch)
        url = subprocess.run(["git", "-C", pg.dir, "remote", "get-url", "origin"],
                             capture_output=True, text=True, check=True).stdout.strip()
        self.assertEqual(url, self.bare)

    def test_nojekyll_is_kept_and_created_when_missing(self):
        pg = pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch)
        pg.write("docs/index.json", b"{}")
        pg.commit_push("x")
        self.assertTrue(os.path.exists(os.path.join(pg.dir, "docs", ".nojekyll")))

    def test_context_manager_form_used_by_the_scripts(self):
        with pages.Pages("razdor-community", remote=self.bare, scratch=self.scratch) as pg:
            self.assertEqual(pg.read("docs/releases.json"), b"[]\n")


class Handler(http.server.BaseHTTPRequestHandler):
    """Answers the first `html_until` requests with a 200 "not found" HTML page, then `payload`."""
    payload = b"[]"
    html_until = 1
    count = [0]

    def log_message(self, *a):
        pass

    def do_GET(self):
        Handler.count[0] += 1
        body = b"<html>not found</html>" if Handler.count[0] <= Handler.html_until else Handler.payload
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


class WaitTests(unittest.TestCase):
    def setUp(self):
        self.httpd = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.httpd.server_address[1]}/"
        threading.Thread(target=self.httpd.serve_forever, daemon=True).start()
        self._saved = pages.PAGES_BASE
        pages.PAGES_BASE = self.base
        Handler.count = [0]
        Handler.html_until = 1
        Handler.payload = b'[{"tag_name": "v1.0.0"}]\n'

    def tearDown(self):
        pages.PAGES_BASE = self._saved
        self.httpd.shutdown()
        self.httpd.server_close()

    def test_waits_past_an_html_page_until_the_bytes_match(self):
        pg = object.__new__(pages.Pages)
        pg.repo = "razdor-community"
        waited = pages.Pages.wait_served(pg, {"docs/releases.json": Handler.payload}, wait=60,
                                         interval=0, sleep=lambda s: None)
        self.assertGreaterEqual(waited, 0)
        self.assertGreaterEqual(Handler.count[0], 2)
        json.loads(Handler.payload)

    def test_times_out_when_the_bytes_never_match(self):
        pg = object.__new__(pages.Pages)
        pg.repo = "razdor-community"
        Handler.html_until = 10 ** 9
        with self.assertRaises(pages.PagesError):
            pages.Pages.wait_served(pg, {"docs/releases.json": Handler.payload}, wait=0,
                                    interval=0, sleep=lambda s: None)


if __name__ == "__main__":
    unittest.main()
