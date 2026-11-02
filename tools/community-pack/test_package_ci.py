#!/usr/bin/env python3
"""Tests of package_ci.py (stdlib unittest). Run: python3 -I tools/community-pack/test_package_ci.py

The razdor-pack binary is a fake shell script here (so the checks run without the game install); its
`check` and `sim` answers are set per test. Set RAZDOR_PACK_BIN to a real razdor-pack to run the
real `check` on the example package too.
"""

import contextlib
import io
import json
import os
import shutil
import stat
import sys
import tempfile
import unittest
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import package_ci  # noqa: E402

FAKE = """#!/bin/sh
if [ "$1" = "check" ]; then
  if [ "$FAKE_CHECK" = "error" ]; then
    echo '{"ok": false, "diagnostics": [{"severity": "error", "file": "units/a.toml", "line": 3, "message": "unknown field", "hint": null}]}'
    exit 1
  fi
  echo '{"ok": true, "diagnostics": []}'
  exit 0
fi
if [ "$1" = "sim" ]; then
  if [ "$FAKE_SIM" = "fail" ]; then echo "the sim panicked" >&2; exit 1; fi
  echo "[ 0.000] install: a log line before the answer"
  echo '[{"map": "first-light", "days": 30, "ok": true}]'
  exit 0
fi
exit 2
"""

MANIFEST = (
    'id = "{id}"\nversion = "{version}"\nformat = 1\ntitle = "t.title"\nauthor = "{author}"\n'
    'license = "{license}"\nbase = "community-update"\nrequires = []\nrules = "dt"\n'
)


class Case(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix="pkgci-")
        self.fake = os.path.join(self.dir, "razdor-pack")
        with open(self.fake, "w", encoding="utf-8") as f:
            f.write(FAKE)
        os.chmod(self.fake, os.stat(self.fake).st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        self.env = {k: os.environ.pop(k) for k in ("RAZDOR_DT_DIR", "FAKE_CHECK", "FAKE_SIM") if k in os.environ}

    def tearDown(self):
        shutil.rmtree(self.dir, ignore_errors=True)
        for k in ("RAZDOR_DT_DIR", "FAKE_CHECK", "FAKE_SIM"):
            os.environ.pop(k, None)
        os.environ.update(self.env)

    def pack(self, name="hello-0.1.0.rzpack", author="Razdor", license="MIT", members=None, id="hello", version="0.1.0"):
        path = os.path.join(self.dir, name)
        files = {
            "package.toml": MANIFEST.format(id=id, version=version, author=author, license=license),
            "texts/ru.toml": '[t]\ntitle = "Привет"\n',
            "texts/en.toml": '[t]\ntitle = "Hello"\n',
            "maps/first/terrain.png": "not really a picture",
        }
        files.update(members or {})
        with zipfile.ZipFile(path, "w") as z:
            for member, text in files.items():
                z.writestr(member, text)
        return path

    def run_check(self, packs, index=None, hashes=None, sim="auto", binary=None):
        argv = ["check", "--razdor-pack", binary or self.fake, "--sim", sim]
        if index is not None:
            path = os.path.join(self.dir, "index.json")
            with open(path, "w", encoding="utf-8") as f:
                json.dump({"format": 1, "entries": index}, f)
            argv += ["--index", path]
        if hashes is not None:
            path = os.path.join(self.dir, "dt-hashes.txt")
            with open(path, "w", encoding="utf-8") as f:
                f.write("\n".join(hashes) + "\n")
            argv += ["--hashes", path]
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = package_ci.main(argv + list(packs))
        return code, out.getvalue()


class PackageGate(Case):
    def test_a_good_package_passes_and_the_sim_is_skipped_without_an_install(self):
        code, text = self.run_check([self.pack()])
        self.assertEqual(code, 0, text)
        self.assertIn("pass: hello 0.1.0", text)
        self.assertIn("skipped (no install on this runner", text)
        self.assertIn("# Package check: 1 of 1 pass", text)

    def test_the_sim_runs_when_an_install_is_named(self):
        os.environ["RAZDOR_DT_DIR"] = "/somewhere"
        code, text = self.run_check([self.pack()])
        self.assertEqual(code, 0, text)
        self.assertIn("played: first-light 30 days", text)

    def test_a_sim_that_fails_refuses_the_package(self):
        os.environ["RAZDOR_DT_DIR"] = "/somewhere"
        os.environ["FAKE_SIM"] = "fail"
        code, text = self.run_check([self.pack()])
        self.assertEqual(code, 1)
        self.assertIn("FAIL: exit 1", text)

    def test_a_package_too_big_is_refused(self):
        saved = package_ci.MAX_BYTES
        package_ci.MAX_BYTES = 10
        try:
            code, text = self.run_check([self.pack()])
        finally:
            package_ci.MAX_BYTES = saved
        self.assertEqual(code, 1)
        self.assertIn("more than 50 MB", text)

    def test_a_package_with_a_game_file_is_refused(self):
        code, text = self.run_check([self.pack(members={"DiscordTimes.exe": "MZ"})])
        self.assertEqual(code, 1, text)
        self.assertIn("game files", text)
        self.assertIn("DiscordTimes.exe", text)

    def test_a_packed_file_that_is_a_file_of_the_install_is_refused(self):
        bytes_of_map = "pretend this is a game map"
        game_hash = package_ci.sha256_bytes(bytes_of_map.encode())
        path = self.pack(members={"maps/copied/terrain.png": bytes_of_map})
        code, text = self.run_check([path], hashes=[f"{game_hash}  Maps_Rus/first.dtm"])
        self.assertEqual(code, 1, text)
        self.assertIn("a file of the install", text)
        self.assertIn("maps/copied/terrain.png", text)

    def test_the_hash_list_lets_other_files_through(self):
        code, text = self.run_check([self.pack()], hashes=["0" * 64])
        self.assertEqual(code, 0, text)

    def test_a_razdor_pack_error_refuses_the_package(self):
        os.environ["FAKE_CHECK"] = "error"
        code, text = self.run_check([self.pack()])
        self.assertEqual(code, 1)
        self.assertIn("1 errors", text)
        self.assertIn("units/a.toml:3: unknown field", text)

    def test_a_licence_that_is_not_mit_is_refused(self):
        code, text = self.run_check([self.pack(license="GPL-3.0-only")])
        self.assertEqual(code, 1)
        self.assertIn("MIT", text)

    def test_a_version_from_another_author_is_refused(self):
        index = [{"id": "hello", "author": "Someone else", "version": "0.0.9"}]
        code, text = self.run_check([self.pack()], index=index)
        self.assertEqual(code, 1, text)
        self.assertIn("a new version comes from the same author", text)

    def test_a_new_version_from_the_same_author_passes(self):
        index = [{"id": "hello", "author": "Razdor", "version": "0.0.9"}]
        code, text = self.run_check([self.pack()], index=index)
        self.assertEqual(code, 0, text)
        self.assertIn("the same author as the catalog's", text)

    def test_a_new_id_passes_whoever_the_author_is(self):
        index = [{"id": "elves", "author": "Someone else", "version": "1.0.0"}]
        code, text = self.run_check([self.pack()], index=index)
        self.assertEqual(code, 0, text)

    def test_a_member_out_of_the_package_is_refused(self):
        code, text = self.run_check([self.pack(members={"../escape.txt": "x"})])
        self.assertEqual(code, 1)
        self.assertIn("not a path inside the package", text)

    def test_no_package_is_an_empty_pass(self):
        code, text = self.run_check([])
        self.assertEqual(code, 0)
        self.assertIn("no package in this pull request", text)

    def test_the_summary_file_gets_the_same_text(self):
        path = self.pack()
        summary = os.path.join(self.dir, "summary.md")
        code = package_ci.main(["check", "--razdor-pack", self.fake, "--sim", "no", "--summary", summary, path])
        self.assertEqual(code, 0)
        with open(summary, encoding="utf-8") as f:
            self.assertIn("## pass: hello 0.1.0", f.read())


class Hashes(Case):
    def test_hashes_lists_every_file_of_the_install_sorted(self):
        install = os.path.join(self.dir, "install")
        os.makedirs(os.path.join(install, "Maps_Rus"))
        with open(os.path.join(install, "Maps_Rus", "first.dtm"), "wb") as f:
            f.write(b"map")
        with open(os.path.join(install, "b.ini"), "wb") as f:
            f.write(b"ini")
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = package_ci.main(["hashes", install])
        self.assertEqual(code, 0)
        rows = [line.split("  ", 1) for line in out.getvalue().splitlines()]
        self.assertEqual([r[1] for r in rows], ["Maps_Rus/first.dtm", "b.ini"])
        self.assertEqual(rows[0][0], package_ci.sha256_bytes(b"map"))

    def test_the_hash_list_reads_back(self):
        path = os.path.join(self.dir, "list.txt")
        digest = package_ci.sha256_bytes(b"x")
        with open(path, "w", encoding="utf-8") as f:
            f.write(f"{digest}  a.ini\nnot a hash\n")
        self.assertEqual(package_ci.read_hashes(path), {digest})

    def test_a_missing_install_is_a_usage_problem(self):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(package_ci.main(["hashes", os.path.join(self.dir, "nope")]), 2)


@unittest.skipUnless(os.environ.get("RAZDOR_PACK_BIN"), "set RAZDOR_PACK_BIN to run the real razdor-pack check")
class RealTool(Case):
    def test_the_example_packs_and_passes_the_real_check(self):
        example = os.path.join(HERE, "..", "..", "examples", "packs", "hello-story")
        packed = os.path.join(self.dir, "hello-story-0.1.0.rzpack")
        import subprocess

        subprocess.run([os.environ["RAZDOR_PACK_BIN"], "pack", example, "-o", packed], check=True, capture_output=True)
        code, text = self.run_check([packed], sim="no", binary=os.environ["RAZDOR_PACK_BIN"])
        self.assertEqual(code, 0, text)


if __name__ == "__main__":
    unittest.main(verbosity=1)
