"""The hermetic base for the GitVerse tool tests (standard library only).

Every test class derives from HermeticTestCase. For each test it:

- gives the test one fresh temp root: tempfile.tempdir points into it, the working directory is in it,
  and HOME, XDG_STATE_HOME, the registry folder, the release work folder and the scratch clones
  (pages, release, site) all point into it;
- removes GITVERSE_TOKEN from the environment, and the token loader (gitverse.load_token) raises
  instead of reading the environment or the repo's .env;
- refuses a TCP connection to any address that is not loopback (socket.connect), a urllib request to
  any host that is not loopback (urllib.request.urlopen), any program but git (subprocess.Popen,
  os.system), a git command with a remote address (a URL or git@ address) or outside the temp root,
  a registry file outside the temp root (load_registry, save_registry) and a Pages clone outside it;
- keeps the output the scripts print during the test: it is shown only when the test fails, under a
  line that says it came from fake GitVerse calls.

Loopback stays allowed: the tests run a local HTTP server and local bare repositories as stand-ins
for GitVerse and Pages. Fakes (gv.Client, gv.fetch_bytes, pages.Pages(remote=...)) are injected per test.
A guard that refuses raises HermeticViolation (an AssertionError), so the test fails and nothing
retries or swallows it.
"""

import contextlib
import io
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
import urllib.parse
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402
import publish_release as pr  # noqa: E402
import publish_site as ps  # noqa: E402

LOOPBACK = frozenset({"127.0.0.1", "localhost", "::1"})
ALLOWED_PROGRAMS = frozenset({"git"})
SYSTEM_TMP = tempfile.gettempdir()  # before any test points tempfile into its own root

# The real originals, kept before any patch is applied.
_REAL_POPEN = subprocess.Popen
_REAL_URLOPEN = urllib.request.urlopen
_REAL_CONNECT = socket.socket.connect
_REAL_CONNECT_EX = socket.socket.connect_ex
_REAL_LOAD_REGISTRY = gv.load_registry
_REAL_SAVE_REGISTRY = gv.save_registry
_REAL_CLONE = pages.Pages._clone

_root = None  # the temp root of the running test; None outside a test


class HermeticViolation(AssertionError):
    """A test tried to reach the network, the real token, a real registry or a real clone."""


def inside_root(path):
    """True when `path` is the running test's temp root or inside it (False outside a test)."""
    if _root is None:
        return False
    real = os.path.realpath(os.fspath(path))
    base = os.path.realpath(_root)
    return real == base or real.startswith(base + os.sep)


def _require_inside(path, what):
    if not inside_root(path):
        raise HermeticViolation(f"{what} is outside this test's temp folder: {os.fspath(path)}")


def _no_token(*args, **kwargs):
    raise HermeticViolation("a test asked for GITVERSE_TOKEN: give the fake client or Pages a token= instead")


def _guard_load_registry(path=None):
    path = path if path is not None else os.path.join(gv.state_dir(), "registry.json")
    _require_inside(path, "the registry")
    return _REAL_LOAD_REGISTRY(path)


def _guard_save_registry(reg, path=None):
    path = path if path is not None else os.path.join(gv.state_dir(), "registry.json")
    _require_inside(path, "the registry")
    return _REAL_SAVE_REGISTRY(reg, path)


def _guard_clone(self):
    _require_inside(self.scratch, "a Pages clone's scratch folder")
    return _REAL_CLONE(self)


def _host_of(address):
    if isinstance(address, (str, bytes)):
        return None  # a Unix socket path is local
    return address[0]


def _guard_connect(self, address):
    host = _host_of(address)
    if host is not None and host not in LOOPBACK:
        raise HermeticViolation(f"a test opened a TCP connection to {host}")
    return _REAL_CONNECT(self, address)


def _guard_connect_ex(self, address):
    host = _host_of(address)
    if host is not None and host not in LOOPBACK:
        raise HermeticViolation(f"a test opened a TCP connection to {host}")
    return _REAL_CONNECT_EX(self, address)


def _guard_urlopen(url, *args, **kwargs):
    target = url.full_url if isinstance(url, urllib.request.Request) else str(url)
    host = urllib.parse.urlsplit(target).hostname
    if host not in LOOPBACK:
        raise HermeticViolation(f"a test requested a non-loopback address ({host})")
    return _REAL_URLOPEN(url, *args, **kwargs)


def check_command(args, cwd=None, shell=False):
    """Refuses a subprocess that a test must not run. Only git, with no remote address, in the temp root."""
    if shell:
        raise HermeticViolation("a test ran a shell command")
    argv = [args] if isinstance(args, (str, bytes, os.PathLike)) else list(args)
    if not argv:
        raise HermeticViolation("a test ran an empty command")
    program = os.path.basename(os.fsdecode(argv[0]))
    if program not in ALLOWED_PROGRAMS:
        raise HermeticViolation(f"a test ran {program!r}: only git, on local repositories, is allowed")
    for i, arg in enumerate(argv[1:], start=1):
        text = os.fsdecode(arg)
        if "://" in text or text.startswith("git@"):
            # The text is not printed: a push or clone URL carries a token.
            raise HermeticViolation("a test gave git a remote address")
        if text in ("-C", "--git-dir") and i + 1 < len(argv):
            _require_inside(os.fsdecode(argv[i + 1]), "git's repository")
    where = cwd if cwd is not None else os.getcwd()
    _require_inside(os.fspath(where), "git's working folder")


class _GuardedPopen(_REAL_POPEN):
    def __init__(self, args, *positional, **kwargs):
        if positional:
            raise HermeticViolation("a test passed positional Popen arguments (not checked)")
        check_command(args, kwargs.get("cwd"), kwargs.get("shell", False))
        super().__init__(args, **kwargs)


def _no_system(command):
    raise HermeticViolation("a test called os.system")


class HermeticTestCase(unittest.TestCase):
    """The base of every GitVerse tool test. A subclass that defines setUp calls super().setUp() first."""

    def setUp(self):
        super().setUp()
        global _root
        root = tempfile.mkdtemp(prefix="gitverse-test-")
        self.addCleanup(shutil.rmtree, root, True)  # runs last
        previous_root, previous_tmp = _root, tempfile.tempdir
        _root = root
        tempfile.tempdir = root

        def restore():
            global _root
            _root = previous_root
            tempfile.tempdir = previous_tmp

        self.addCleanup(restore)
        cwd = os.getcwd()
        os.chdir(root)
        self.addCleanup(os.chdir, cwd)  # runs before the rmtree

        env = {
            "HOME": os.path.join(root, "home"),
            "XDG_STATE_HOME": os.path.join(root, "xdg-state"),
            "RAZDOR_GITVERSE_STATE": os.path.join(root, "state"),
            "RAZDOR_GITVERSE_SCRATCH": os.path.join(root, "scratch"),
            "RAZDOR_GITVERSE_WORK": os.path.join(root, "work"),
            "RAZDOR_GV_SITE_SCRATCH": os.path.join(root, "site-scratch"),
        }
        patches = [
            unittest.mock.patch.dict(os.environ, env),
            unittest.mock.patch.object(gv, "load_token", _no_token),
            unittest.mock.patch.object(gv, "load_registry", _guard_load_registry),
            unittest.mock.patch.object(gv, "save_registry", _guard_save_registry),
            unittest.mock.patch.object(pages, "SCRATCH", os.path.join(root, "scratch")),
            unittest.mock.patch.object(pr, "WORK_ROOT", os.path.join(root, "work")),
            unittest.mock.patch.object(ps, "SCRATCH", os.path.join(root, "site-scratch")),
            unittest.mock.patch.object(pages.Pages, "_clone", _guard_clone),
            unittest.mock.patch.object(urllib.request, "urlopen", _guard_urlopen),
            unittest.mock.patch.object(socket.socket, "connect", _guard_connect),
            unittest.mock.patch.object(socket.socket, "connect_ex", _guard_connect_ex),
            unittest.mock.patch.object(subprocess, "Popen", _GuardedPopen),
            unittest.mock.patch.object(os, "system", _no_system),
        ]
        for patch in patches:
            patch.start()
            self.addCleanup(patch.stop)
        os.environ.pop("GITVERSE_TOKEN", None)  # restored with the rest of os.environ at cleanup

    def run(self, result=None):
        if result is None:
            result = self.defaultTestResult()
        failed_before = len(result.failures) + len(result.errors)
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured), contextlib.redirect_stderr(captured):
            super().run(result)
        if len(result.failures) + len(result.errors) > failed_before and captured.getvalue():
            sys.__stderr__.write(f"--- output of {self.id()} (fake GitVerse calls, not live; the test failed) ---\n"
                                 f"{captured.getvalue()}--- end of output ---\n")
        return result
