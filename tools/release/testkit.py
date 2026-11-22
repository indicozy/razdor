"""The hermetic base for the release tools' tests (run: `python3 -I -m unittest discover -s tools/release`).

Every test gets its own temp folder, and no test reaches the network: a socket connection raises
`HermeticViolation`. The tests inject fakes (a release list, a stats CSV, a fake `adb`) instead.
"""

import shutil
import socket
import tempfile
import unittest
import unittest.mock


class HermeticViolation(AssertionError):
    """A test tried to leave the machine or to run something it must not."""


def _no_connect(self, address, *a, **kw):
    raise HermeticViolation(f"a test tried to connect to {address!r}")


class HermeticTestCase(unittest.TestCase):
    def setUp(self):
        super().setUp()
        self.root = tempfile.mkdtemp(prefix="release-tools-test-")
        self.addCleanup(shutil.rmtree, self.root, True)
        patcher = unittest.mock.patch.object(socket.socket, "connect", _no_connect)
        patcher.start()
        self.addCleanup(patcher.stop)

