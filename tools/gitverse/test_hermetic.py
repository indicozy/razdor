"""The guards of testkit: each one refuses what a test must not do, and lets through what it may (loopback,
git in the temp folder). A refused call raises before anything is sent, run or written."""

import os
import pwd
import shutil
import socket
import subprocess
import tempfile
import unittest

from testkit import HermeticTestCase, HermeticViolation, SYSTEM_TMP, inside_root  # noqa: E402

import gitverse as gv  # noqa: E402
import pages  # noqa: E402
import publish_release as pr  # noqa: E402
import publish_site as ps  # noqa: E402


def real_home():
    """The account's home from the password file (HOME is the test's own folder during a test)."""
    return pwd.getpwuid(os.getuid()).pw_dir


class GuardTests(HermeticTestCase):
    def test_the_environment_points_into_the_test_folder(self):
        self.assertTrue(inside_root(os.path.expanduser("~")))
        self.assertTrue(inside_root(gv.state_dir()))
        self.assertTrue(inside_root(pages.SCRATCH))
        self.assertTrue(inside_root(pr.WORK_ROOT))
        self.assertTrue(inside_root(ps.SCRATCH))
        self.assertTrue(inside_root(tempfile.gettempdir()))
        self.assertNotIn("GITVERSE_TOKEN", os.environ)

    def test_the_token_is_not_read(self):
        with self.assertRaises(HermeticViolation):
            gv.load_token()
        with self.assertRaises(HermeticViolation):
            gv.Client()  # no token given: the default loads it

    def test_a_real_gitverse_request_is_refused(self):
        with self.assertRaises(HermeticViolation):
            gv.Client(token="fake").request("GET", "/repos/indicozy/razdor/releases")
        with self.assertRaises(HermeticViolation):
            gv.anon_get("https://gitverse.ru/indicozy/razdor/releases/download/v1/x.zip")

    def test_a_raw_socket_to_a_real_address_is_refused(self):
        with socket.socket() as s:
            with self.assertRaises(HermeticViolation):
                s.connect(("93.184.216.34", 443))  # refused before any packet is sent

    def test_loopback_still_works(self):
        with socket.socket() as server:
            server.bind(("127.0.0.1", 0))
            server.listen(1)
            with socket.create_connection(server.getsockname(), timeout=5) as client:
                self.assertEqual(client.getpeername()[0], "127.0.0.1")

    def test_a_real_registry_path_is_refused(self):
        real = os.path.join(real_home(), ".local", "state", "razdor-gitverse", "registry.json")
        with self.assertRaises(HermeticViolation):
            gv.load_registry(real)  # a read: nothing is changed even if the guard were gone
        outside = tempfile.mkdtemp(dir=SYSTEM_TMP)  # a folder outside the test root, removed below
        try:
            with self.assertRaises(HermeticViolation):
                gv.save_registry({"releases": []}, os.path.join(outside, "registry.json"))
            self.assertFalse(os.path.exists(os.path.join(outside, "registry.json")))
        finally:
            shutil.rmtree(outside, ignore_errors=True)

    def test_a_real_clone_folder_is_refused_before_anything_is_removed(self):
        real = "/mnt/ai_models/razdor-scratch/gitverse-pages"
        with self.assertRaises(HermeticViolation):
            pages.Pages(gv.HUB_REPO, token="fake", scratch=real)

    def test_only_git_runs(self):
        for argv in (["curl", "--version"], ["gh", "auth", "status"], ["npm", "ci"]):
            with self.assertRaises(HermeticViolation):
                subprocess.run(argv, capture_output=True)
        # os.system is probed on purpose: a literal command, refused before any shell is started.
        with self.assertRaises(HermeticViolation):
            os.system("true")  # noqa: S605 - the guard refuses it; no input reaches the shell

    def test_git_with_a_remote_address_is_refused(self):
        with self.assertRaises(HermeticViolation):
            subprocess.run(["git", "clone", "https://example.invalid/x.git", "x"], capture_output=True)
        with self.assertRaises(HermeticViolation):
            subprocess.run(["git", "push", "git@example.invalid:x.git", "main"], capture_output=True)

    def test_git_outside_the_test_folder_is_refused(self):
        with self.assertRaises(HermeticViolation):
            subprocess.run(["git", "status"], cwd=gv.HERE, capture_output=True)  # the real repository
        with self.assertRaises(HermeticViolation):
            subprocess.run(["git", "-C", real_home(), "status"], capture_output=True)

    def test_git_on_a_local_repository_in_the_test_folder_runs(self):
        folder = os.path.join(tempfile.gettempdir(), "repo")
        os.makedirs(folder)
        subprocess.run(["git", "init", "-q", "-b", "main", folder], check=True, capture_output=True)
        out = subprocess.run(["git", "-C", folder, "rev-parse", "--is-inside-work-tree"],
                             check=True, capture_output=True, text=True)
        self.assertEqual(out.stdout.strip(), "true")


if __name__ == "__main__":
    unittest.main()
