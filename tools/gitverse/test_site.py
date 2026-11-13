"""The website assembly, the push of the `site` branch, the waits and the live check of publish_site.py
(no network: a local bare repository, a fake client and fake fetches stand in for GitVerse)."""

import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
from testkit import HermeticTestCase  # noqa: E402
import publish_site as ps  # noqa: E402

PUBLIC = ps.PUBLIC


def tar_of(files):
    """A tar (bytes) of {name: bytes}, as git archive would make it."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w") as tf:
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tf.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def write_tree(root, files):
    for rel, data in files.items():
        path = os.path.join(root, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)


def git(*args, cwd):
    subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", *args],
                   cwd=cwd, check=True, capture_output=True)


class _Client:
    """A fake GitVerse client: `runs` is the list of workflow runs the API answers, `jobs` the jobs."""

    def __init__(self, runs_seq, jobs=()):
        self.runs_seq = list(runs_seq)
        self.jobs = list(jobs)
        self.calls = []

    def request(self, method, path, **kw):
        self.calls.append(path)
        if path.endswith("/actions/runs"):
            runs = self.runs_seq.pop(0) if len(self.runs_seq) > 1 else self.runs_seq[0]
            return 200, {}, json.dumps({"workflow_runs": runs}).encode()
        if path.endswith("/jobs"):
            return 200, {}, json.dumps({"jobs": self.jobs}).encode()
        raise AssertionError(path)


def run_of(sha, status, run_id=7):
    return {"id": run_id, "status": status, "commit_sha": sha, "ref": "refs/heads/site"}


class AssembleTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.root = self._tmp.name
        self.site = os.path.join(self.root, "site")
        os.makedirs(self.site)
        self.wiki_dist = os.path.join(self.root, "dist")
        write_tree(self.wiki_dist, {"index.html": b"<wiki>", "_astro/x.css": b"a{}"})
        self.config = os.path.join(self.root, "config.json")
        with open(self.config, "wb") as f:
            f.write(b'{"schema": 1}\n')
        self.hand = tar_of({
            "index.html": b"<hand>",
            ".nojekyll": b"",
            ".github/workflows/deploy.yml": b"x",
            "wiki/old.html": b"<old wiki>",
            "install.html": b"<install>",
        })

    def tearDown(self):
        self._tmp.cleanup()

    def test_assembles_the_pages_site(self):
        ps.assemble(self.site, self.wiki_dist, self.config, hand_made=self.hand)
        self.assertEqual(ps.read(os.path.join(self.site, "index.html")), b"<hand>")
        self.assertTrue(os.path.exists(os.path.join(self.site, ".nojekyll")))
        self.assertFalse(os.path.exists(os.path.join(self.site, ".github")))
        self.assertEqual(ps.read(os.path.join(self.site, "wiki", "index.html")), b"<wiki>")
        self.assertEqual(ps.read(os.path.join(self.site, "wiki", "_astro", "x.css")), b"a{}")
        self.assertFalse(os.path.exists(os.path.join(self.site, "wiki", "old.html")))
        self.assertEqual(ps.read(os.path.join(self.site, "config.json")), b'{"schema": 1}\n')

    def test_a_second_assembly_replaces_the_wiki(self):
        ps.assemble(self.site, self.wiki_dist, self.config, hand_made=self.hand)
        ps.reset_dir(self.site, self.root)
        ps.assemble(self.site, self.wiki_dist, self.config, hand_made=self.hand)
        self.assertTrue(os.path.exists(os.path.join(self.site, "wiki", "index.html")))
        self.assertFalse(os.path.exists(os.path.join(self.site, "wiki", "old.html")))

    def test_refuses_to_reset_outside_the_scratch(self):
        with self.assertRaises(ps.SiteError):
            ps.reset_dir(self.site, os.path.join(self.root, "elsewhere"))

    def test_the_hand_made_site_without_a_root_index_is_refused(self):
        with self.assertRaises(ps.SiteError):
            ps.assemble(self.site, self.wiki_dist, self.config, hand_made=tar_of({"a.txt": b"x"}))


class PushTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        root = self._tmp.name
        self.scratch = os.path.join(root, "scratch")
        os.makedirs(self.scratch)
        self.bare = os.path.join(root, "remote.git")
        seed = os.path.join(root, "seed")
        os.makedirs(seed)
        git("init", "-q", "-b", "main", cwd=seed)
        write_tree(seed, {"README.md": b"code\n"})
        git("add", "-A", cwd=seed)
        git("commit", "-q", "-m", "seed", cwd=seed)
        git("init", "-q", "--bare", self.bare, cwd=root)
        git("push", "-q", self.bare, "main", cwd=seed)
        self.site = os.path.join(self.scratch, "site")
        os.makedirs(self.site)
        write_tree(self.site, {"index.html": b"<one>", "wiki/index.html": b"<w1>"})
        self.workflow = ps.WORKFLOW_SRC

    def tearDown(self):
        self._tmp.cleanup()

    def _tree(self, ref, path):
        return subprocess.run(["git", "--git-dir", self.bare, "show", f"{ref}:{path}"],
                              capture_output=True, check=True).stdout

    def _names(self, ref):
        out = subprocess.run(["git", "--git-dir", self.bare, "ls-tree", "-r", "--name-only", ref],
                             capture_output=True, check=True, text=True).stdout
        return set(out.split())

    def test_pushes_the_site_branch_only(self):
        main_before = self._tree("main", "README.md")
        sha = ps.push_site(self.site, self.workflow, self.bare, self.scratch, "unused", "Site test")
        self.assertEqual(len(sha), 40)
        names = self._names("site")
        self.assertIn("site/index.html", names)
        self.assertIn("site/wiki/index.html", names)
        self.assertIn(ps.WORKFLOW_DEST, names)
        self.assertNotIn("index.html", names)  # the site is the folder site/, not the root
        self.assertEqual(self._tree("main", "README.md"), main_before)
        self.assertEqual(self._names("main"), {"README.md"})

    def test_a_second_push_replaces_the_branch(self):
        ps.push_site(self.site, self.workflow, self.bare, self.scratch, "unused", "first")
        write_tree(self.site, {"index.html": b"<two>"})
        os.remove(os.path.join(self.site, "wiki", "index.html"))
        sha2 = ps.push_site(self.site, self.workflow, self.bare, self.scratch, "unused", "second")
        self.assertEqual(self._tree("site", "site/index.html"), b"<two>")
        self.assertNotIn("site/wiki/index.html", self._names("site"))
        head = subprocess.run(["git", "--git-dir", self.bare, "rev-parse", "site"],
                              capture_output=True, check=True, text=True).stdout.strip()
        self.assertEqual(head, sha2)
        log = subprocess.run(["git", "--git-dir", self.bare, "rev-list", "--count", "site"],
                             capture_output=True, check=True, text=True).stdout.strip()
        self.assertEqual(log, "1")  # an orphan commit each time

    def test_the_workflow_file_is_the_template(self):
        ps.push_site(self.site, self.workflow, self.bare, self.scratch, "unused", "wf")
        self.assertEqual(self._tree("site", ps.WORKFLOW_DEST), ps.read(self.workflow))


class WaitTests(HermeticTestCase):
    def test_waits_for_the_new_bytes_past_a_not_found_page(self):
        page = "<html>Страница не найдена</html>".encode("utf-8")  # GitVerse's 200 not-found page
        served = [page, page, b"new"]
        calls = []

        def fetch(url):
            calls.append(url)
            return served.pop(0) if served else b"new"

        waited = ps.wait_bytes({"index.html": b"new"}, PUBLIC, wait=60, interval=0,
                               fetch=fetch, sleep=lambda s: None)
        self.assertGreaterEqual(waited, 0)
        self.assertEqual(calls[0], PUBLIC + "index.html")
        self.assertEqual(len(calls), 3)

    def test_gives_up_at_the_deadline(self):
        def fetch(url):
            return b"other"

        with self.assertRaises(ps.SiteError):
            ps.wait_bytes({"index.html": b"new"}, PUBLIC, wait=-1, interval=0,
                          fetch=fetch, sleep=lambda s: None)

    def test_a_failed_fetch_is_not_served_yet(self):
        def fetch(url):
            raise gv.GitVerseError("HTTP 503")

        with self.assertRaises(ps.SiteError):
            ps.wait_bytes({"config.json": b"x"}, PUBLIC, wait=-1, interval=0,
                          fetch=fetch, sleep=lambda s: None)

    def test_the_workflow_run_is_awaited_and_a_failure_is_named(self):
        sha = "a" * 40
        client = _Client([[], [run_of(sha, "waiting")], [run_of(sha, "success", 9)]])
        self.assertEqual(ps.wait_workflow(client, sha, wait=60, interval=0, sleep=lambda s: None), 9)

        bad = _Client([[run_of(sha, "failure", 5)]],
                      jobs=[{"name": "build", "status": "failure",
                             "steps": [{"name": "Run actions/upload-pages-artifact@v1.0.0",
                                        "status": "failure"}]}])
        with self.assertRaises(ps.SiteError) as cm:
            ps.wait_workflow(bad, sha, wait=60, interval=0, sleep=lambda s: None)
        self.assertIn("job build: failure", str(cm.exception))
        self.assertIn("upload-pages-artifact", str(cm.exception))

    def test_a_run_that_never_appears_times_out(self):
        client = _Client([[]])
        with self.assertRaises(ps.SiteError):
            ps.wait_workflow(client, "b" * 40, wait=-1, interval=0, sleep=lambda s: None)


class LiveCheckTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        self._tmp = tempfile.TemporaryDirectory()
        self.site = os.path.join(self._tmp.name, "site")
        self.files = {
            "index.html": b'<html><link rel="icon" href="favicon.ico"><a href="wiki/">w</a></html>',
            "favicon.ico": b"ico",
            "config.json": b"{}\n",
            "wiki/index.html": b'<html><link rel="stylesheet" href="/razdor/wiki/_astro/s.css">'
                               b'<script src="/razdor/wiki/_astro/p.js"></script></html>',
            "wiki/en/index.html": b"<html>en</html>",
            "wiki/start/install/index.html": b'<html><img src="../../img.png"></html>',
            "wiki/en/start/install/index.html": b"<html>en page</html>",
            "wiki/img.png": b"png",
            "wiki/_astro/s.css": b"@font-face{src:url(../fonts/f.woff2)}",
            "wiki/_astro/p.js": b"js",
            "wiki/fonts/f.woff2": b"font",
        }
        write_tree(self.site, self.files)

    def tearDown(self):
        self._tmp.cleanup()

    def served_from_build(self, overrides=None):
        table = {PUBLIC + rel: data for rel, data in self.files.items()}
        table[PUBLIC + "wiki/start/install/"] = self.files["wiki/start/install/index.html"]
        table[PUBLIC + "wiki/en/start/install/"] = self.files["wiki/en/start/install/index.html"]
        table[PUBLIC + "wiki/"] = self.files["wiki/index.html"]
        table[PUBLIC + "wiki/en/"] = self.files["wiki/en/index.html"]
        table[PUBLIC] = self.files["index.html"]
        table.update(overrides or {})

        def fetch(url):
            if url not in table:
                raise gv.GitVerseError(f"GET {url}: HTTP 404")
            return table[url]

        return fetch

    def test_a_matching_site_has_no_problems(self):
        self.assertEqual(ps.check_live(self.site, fetch=self.served_from_build()), [])

    def test_a_page_asset_that_differs_is_reported(self):
        bad = {PUBLIC + "wiki/_astro/p.js": b"<html>not found</html>"}
        problems = ps.check_live(self.site, fetch=self.served_from_build(bad))
        self.assertEqual(len(problems), 1)
        self.assertIn("wiki/_astro/p.js", problems[0])

    def test_a_css_font_is_checked_too(self):
        bad = {PUBLIC + "wiki/fonts/f.woff2": b"<html>not found</html>"}
        problems = ps.check_live(self.site, fetch=self.served_from_build(bad))
        self.assertTrue(any("wiki/fonts/f.woff2" in p for p in problems))

    def test_sample_pages_pick_one_ru_and_one_en_wiki_page(self):
        pages = ps.sample_pages(self.site)
        self.assertIn("wiki/index.html", pages)
        self.assertIn("wiki/en/index.html", pages)
        self.assertIn("wiki/start/install/index.html", pages)
        self.assertIn("wiki/en/start/install/index.html", pages)

    def test_references_and_urls(self):
        base = PUBLIC + "wiki/start/install/"
        refs = ps.page_refs(base, '<link href="/razdor/wiki/_astro/s.css"><img src="../img.png">'
                                  '<a href="x/">no</a><script src="https://other.example/x.js"></script>')
        self.assertIn(PUBLIC + "wiki/_astro/s.css", refs)
        self.assertIn(PUBLIC + "wiki/start/img.png", refs)  # ../ from wiki/start/install/
        self.assertFalse(any("x/" in r for r in refs))
        css = ps.css_refs(PUBLIC + "wiki/_astro/s.css", "@font-face{src:url('../fonts/f.woff2')}")
        self.assertEqual(css, {PUBLIC + "wiki/fonts/f.woff2"})
        self.assertEqual(ps.to_rel(PUBLIC), "index.html")
        self.assertEqual(ps.to_rel(PUBLIC + "wiki/en/?x=1#y"), "wiki/en/index.html")
        self.assertEqual(ps.to_rel("https://elsewhere.example/razdor/x"), None)

    def test_markers_name_the_config_and_the_hashed_css(self):
        marks = ps.markers(self.site)
        self.assertEqual(set(marks), {"config.json", "index.html", "wiki/index.html",
                                      "wiki/en/index.html", "wiki/_astro/s.css"})
        self.assertEqual(marks["config.json"], b"{}\n")


class WorkflowTemplateTests(HermeticTestCase):
    def setUp(self):
        super().setUp()
        with open(ps.WORKFLOW_SRC, encoding="utf-8") as f:
            self.text = f.read()

    def test_runs_on_the_github_free_runner_and_the_public_actions(self):
        self.assertIn("runs-on: ubuntu-latest", self.text)
        self.assertNotIn("ubuntu-22.04", self.text)
        self.assertIn("actions/upload-pages-artifact@v1.0.0", self.text)
        self.assertIn("actions/deploy-pages@v1.0.0", self.text)
        self.assertNotIn("uses: gitverse/", self.text)  # the private action names

    def test_the_artifact_is_the_site_folder_and_the_branch_triggers_it(self):
        self.assertIn("path: site", self.text)
        self.assertIn("branches: [site]", self.text)
        self.assertIn("workflow_dispatch", self.text)


class MiscTests(HermeticTestCase):
    def test_the_build_is_pinned_to_the_free_cpus(self):
        if shutil.which("taskset") and shutil.which("nice"):
            self.assertEqual(ps.pinned(["npm", "ci"]), ps.HEAVY + ["npm", "ci"])
            self.assertEqual(ps.HEAVY[-2:], ["-c", "8-19"])
        else:
            self.skipTest("taskset or nice is not installed")

    def test_the_wiki_build_output_is_the_dist_folder(self):
        self.assertTrue(ps.SRC_PATHS == ("wiki", "data/remote"))


if __name__ == "__main__":
    unittest.main()
