"""Checks for the VPS deploy files: the sudo rule of the razdor user and the root setup script.

Run from the repository root:  python3 -I server/deploy/test_deploy.py
Nothing here contacts a server. The visudo check runs only where visudo exists.
"""

import os
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))

# The complete list of commands the razdor user may run as root (sudoers-razdor). Exact lines only.
EXPECTED_COMMANDS = {
    # the API service, its image, nginx checks and certbot (as before)
    "/usr/bin/systemctl start razdor-api",
    "/usr/bin/systemctl stop razdor-api",
    "/usr/bin/systemctl restart razdor-api",
    "/usr/bin/systemctl daemon-reload",
    "/usr/bin/systemctl reload nginx",
    "/usr/bin/podman load",
    "/usr/bin/podman images",
    "/usr/bin/podman ps",
    "/usr/bin/podman image prune -f",
    "/usr/sbin/nginx -t",
    "/usr/bin/certbot certonly --webroot -w /var/www/letsencrypt -d razdor.indicozy.dev",
    "/usr/bin/certbot renew",
    "/usr/bin/certbot renew --dry-run",
    # the Razdor host binaries (routine updates)
    "/usr/bin/install -m 0755 -o root -g root /home/razdor/stage/razdor-mirror /usr/local/bin/razdor-mirror",
    "/usr/bin/install -m 0755 -o root -g root /home/razdor/stage/razdor-server /usr/local/bin/razdor-server",
    # the mirror's sync and the GeoIP table units
    "/usr/bin/systemctl start razdor-mirror-sync.service",
    "/usr/bin/systemctl start --no-block razdor-mirror-sync.service",
    "/usr/bin/systemctl stop razdor-mirror-sync.service",
    "/usr/bin/systemctl restart razdor-mirror-sync.service",
    "/usr/bin/systemctl start razdor-mirror-sync.timer",
    "/usr/bin/systemctl stop razdor-mirror-sync.timer",
    "/usr/bin/systemctl restart razdor-mirror-sync.timer",
    "/usr/bin/systemctl start razdor-geo-update.service",
    "/usr/bin/systemctl start --no-block razdor-geo-update.service",
    "/usr/bin/systemctl stop razdor-geo-update.service",
    "/usr/bin/systemctl restart razdor-geo-update.service",
    "/usr/bin/systemctl start razdor-geo-update.timer",
    "/usr/bin/systemctl stop razdor-geo-update.timer",
    "/usr/bin/systemctl restart razdor-geo-update.timer",
}
EXPECTED_USER_LINE = "razdor ALL=(root) NOPASSWD: RZ_SVC, RZ_IMG, RZ_WEB, RZ_BIN, RZ_MIRROR"

# Files the root script installs from the staging folder that are not in this folder (built binaries, the image).
BUILT = {"razdor-mirror", "razdor-server", "razdor-api.tar"}


def read(name):
    with open(os.path.join(HERE, name), encoding="utf-8") as fh:
        return fh.read()


def sudoers_text():
    return read("sudoers-razdor")


def sudoers_commands(text):
    """Every command of every Cmnd_Alias, with the continuation lines joined."""
    joined = text.replace("\\\n", " ")
    commands = []
    for line in joined.splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        m = re.match(r"Cmnd_Alias\s+\w+\s*=\s*(.*)$", s)
        if m:
            commands.extend(c.strip() for c in m.group(1).split(","))
    return commands


def sudoers_lines(text):
    """Non-comment lines after the continuation lines are joined."""
    joined = text.replace("\\\n", " ")
    return [l.strip() for l in joined.splitlines() if l.strip() and not l.strip().startswith("#")]


class SudoersRule(unittest.TestCase):
    def test_no_wildcards_in_any_command(self):
        for command in sudoers_commands(sudoers_text()):
            for ch in "*?[]!\"":
                self.assertNotIn(ch, command, "wildcard or quote in: " + command)

    def test_commands_are_exactly_the_allowed_list(self):
        commands = sudoers_commands(sudoers_text())
        self.assertEqual(len(commands), len(set(commands)), "a command is listed twice")
        self.assertEqual(set(commands), EXPECTED_COMMANDS)

    def test_only_aliases_and_the_user_line(self):
        for line in sudoers_lines(sudoers_text()):
            self.assertTrue(
                line.startswith("Cmnd_Alias ") or line == EXPECTED_USER_LINE,
                "unexpected line: " + line,
            )
        self.assertIn(EXPECTED_USER_LINE, sudoers_lines(sudoers_text()))

    def test_no_forbidden_targets(self):
        forbidden = ("/etc/nginx", "/etc/systemd", "/etc/sudoers", "useradd", "passwd", "firewall",
                     "dnf", "yum", "polkit", "/etc/polkit", "visudo", "/bin/sh", "/bin/bash")
        for command in sudoers_commands(sudoers_text()):
            for word in forbidden:
                self.assertNotIn(word, command, "forbidden for razdor: " + command)

    def test_vps_user_writes_the_same_text(self):
        src = read("vps-user.sh")
        marker = "cat > /etc/sudoers.d/razdor <<'EOF'\n"
        start = src.index(marker) + len(marker)
        end = src.index("\nEOF\n", start) + 1
        self.assertEqual(src[start:end], sudoers_text())


class RootSetupOrder(unittest.TestCase):
    def setUp(self):
        self.text = read("root-setup-2026-11-24.sh")

    def test_sudoers_is_checked_before_it_is_moved_into_place(self):
        self.assertLess(self.text.index("visudo -cf /etc/sudoers.d/.razdor.new"),
                        self.text.index("mv -f /etc/sudoers.d/.razdor.new /etc/sudoers.d/razdor"))

    def test_nginx_is_tested_before_it_is_reloaded(self):
        section = self.text[self.text.index("step \"nginx:"):self.text.index("step \"API image")]
        self.assertLess(section.index("nginx -t"), section.index("systemctl reload nginx"))

    def test_polkit_rule_gets_the_razdor_user(self):
        self.assertIn('subject\\.user == "deploy"', self.text)
        polkit = read("razdor-mirror-sync.polkit.rules")
        self.assertIn('subject.user == "deploy"', polkit)
        replaced = re.sub(r'subject\.user == "deploy"', 'subject.user == "razdor"', polkit)
        self.assertIn('subject.user == "razdor"', replaced)

    def test_every_staged_file_exists_in_this_folder(self):
        m = re.search(r"for f in (.*?); do", self.text.replace("\\\n", " "), re.S)
        self.assertIsNotNone(m)
        for name in m.group(1).split():
            if name in BUILT:
                continue
            self.assertTrue(os.path.isfile(os.path.join(HERE, name)), "missing in server/deploy: " + name)


class ScriptSyntax(unittest.TestCase):
    def _check(self, shell, name):
        exe = shutil.which(shell)
        if exe is None:
            self.skipTest(shell + " not installed")
        result = subprocess.run([exe, "-n", os.path.join(HERE, name)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_root_setup_parses_as_sh(self):
        self._check("sh", "root-setup-2026-11-24.sh")

    def test_vps_user_parses_as_sh(self):
        self._check("sh", "vps-user.sh")

    def test_stage_upload_parses_as_bash(self):
        self._check("bash", "stage-upload.sh")

    def test_visudo_accepts_the_sudoers_file(self):
        exe = shutil.which("visudo")
        if exe is None:
            self.skipTest("visudo not installed")
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "razdor")
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(sudoers_text())
            result = subprocess.run([exe, "-cf", path], capture_output=True, text=True)
            if "ermission" in result.stderr or "ermission" in result.stdout:
                self.skipTest("visudo needs root to read the file")
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
