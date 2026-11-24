"""Checks for the VPS deploy files: the sudo rule of the razdor user and the root setup script.

Run from the repository root:  python3 -I server/deploy/test_deploy.py
Nothing here contacts a server. The visudo check runs only where visudo exists.
"""

import os
import re
import shutil
import subprocess
import sys
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
    # the Razdor host binaries (routine updates): the root wrapper, fixed arguments only
    "/usr/local/sbin/razdor-install-bin mirror",
    "/usr/local/sbin/razdor-install-bin server",
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


def code_lines(text):
    """The script without its comment lines (the comments name the paths on purpose)."""
    return "\n".join(l for l in text.splitlines() if not l.lstrip().startswith("#"))


def unit_names():
    return sorted(n for n in os.listdir(HERE) if n.endswith((".service", ".timer")))


def exec_lines(text):
    """(prefix, command) of every Exec* line of a unit; the prefix holds the systemd flags (+, !, -, @, :)."""
    out = []
    for line in text.splitlines():
        m = re.match(r"\s*Exec(?:Start|StartPre|StartPost|Stop|Reload|Condition)=(.*)$", line)
        if m:
            value = m.group(1).strip()
            prefix = re.match(r"[@\-:+!]*", value).group(0)
            out.append((prefix, value[len(prefix):]))
    return out


def unit_user(text):
    users = re.findall(r"^\s*User=(\S+)", text, re.M)
    return users[-1] if users else "root"


class RootRunCode(unittest.TestCase):
    def test_no_root_run_unit_executes_a_razdor_binary_or_a_home_file(self):
        for name in unit_names():
            text = read(name)
            for prefix, command in exec_lines(text):
                root = unit_user(text) == "root" or "+" in prefix or "!" in prefix
                if not root:
                    continue
                self.assertNotIn("/usr/local/bin/razdor-", command, name + ": root runs " + command)
                self.assertNotIn("/home/razdor", command, name + ": root runs " + command)

    def test_geo_unit_runs_unprivileged_with_fixed_root_steps(self):
        text = read("razdor-geo-update.service")
        self.assertIn("User=razdor-geo", text)
        self.assertIn("Group=razdor-geo", text)
        root_steps = [c for p, c in exec_lines(text) if "+" in p]
        self.assertEqual(root_steps, ["/usr/local/sbin/razdor-geo-apply", "/usr/bin/systemctl restart razdor-api"])
        for prefix, command in exec_lines(text):
            if "+" not in prefix:
                self.assertEqual(command, "/usr/local/sbin/razdor-geo-update")

    def test_helpers_run_as_root_do_not_execute_razdor_binaries(self):
        for name in ("geo-apply.sh", "razdor-install-bin.sh"):
            self.assertNotIn("/usr/local/bin/razdor", code_lines(read(name)), name)

    def test_unprivileged_geo_script_has_no_root_steps(self):
        text = code_lines(read("geo-update.sh"))
        for word in ("systemctl", "nginx -", "-o 10001", "-o root"):
            self.assertNotIn(word, text)

    def test_geo_helper_tests_nginx_before_reload_and_keeps_the_old_include(self):
        text = read("geo-apply.sh")
        self.assertLess(text.index("nginx -t"), text.index("systemctl reload nginx"))
        self.assertIn('cp -p "$NGINX_CONF" "$WORK/old.conf"', text)

    def test_geo_helper_range_form_rejects_config_injection(self):
        text = read("geo-apply.sh")
        m = re.search(r"^RANGE_RE='(.*)'$", text, re.M)
        self.assertIsNotNone(m)
        pattern = re.compile(m.group(1))
        self.assertTrue(pattern.match("1.0.0.0-1.0.0.255 AU;"))
        self.assertTrue(pattern.match("203.0.113.0-203.0.113.255 ZZ;"))
        for bad in ("1.0.0.0/24 AU;", "1.0.0.0-1.0.0.255 AU; } server {", "include /etc/shadow;", "}", "1.0.0.0-1.0.0.255 AU;;"):
            self.assertIsNone(pattern.match(bad), bad)


class StageLists(unittest.TestCase):
    def test_root_setup_preflight_list_is_the_upload_list_plus_the_built_files(self):
        setup = read("root-setup-2026-11-24.sh").replace("\\\n", " ")
        preflight = re.search(r"for f in (.*?); do", setup, re.S).group(1).split()
        upload = read("stage-upload.sh").replace("\\\n", " ")
        upload_loop = re.search(r"for f in (.*?); do\n  cp", upload, re.S).group(1).split()
        self.assertEqual(sorted(n for n in preflight if n not in BUILT), sorted(upload_loop))


class InstallWrapperStatic(unittest.TestCase):
    def test_sudoers_allows_only_the_wrapper_with_fixed_arguments(self):
        commands = sudoers_commands(sudoers_text())
        wrapper = [c for c in commands if "razdor-install-bin" in c]
        self.assertEqual(sorted(wrapper), ["/usr/local/sbin/razdor-install-bin mirror",
                                           "/usr/local/sbin/razdor-install-bin server"])
        for c in commands:
            self.assertNotEqual(c.split()[0], "/usr/bin/install", c)
            self.assertNotIn("/home/razdor", c, c)

    def test_wrapper_takes_only_mirror_or_server_and_checks_the_copy(self):
        text = read("razdor-install-bin.sh")
        self.assertIn("mirror|server)", text)
        self.assertIn("cp -P --", text)
        self.assertIn("7f454c46", text)
        self.assertIn("-L \"$STAGE\"", text)


WRAPPER = os.path.join(HERE, "razdor-install-bin.sh")


@unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0,
                 "the wrapper's test overrides are ignored as root; these tests run unprivileged")
class InstallWrapperRuns(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp(prefix="razdor-wrapper-test-")
        self.stage = os.path.join(self.tmp, "stage")
        self.work = os.path.join(self.tmp, "work")
        self.dest = os.path.join(self.tmp, "dest")
        for d in (self.stage, self.work, self.dest):
            os.mkdir(d)

    def tearDown(self):
        shutil.rmtree(self.tmp, ignore_errors=True)

    def run_wrapper(self, *args):
        env = {"PATH": "/usr/bin:/bin", "RZ_INSTALL_STAGE": self.stage,
               "RZ_INSTALL_DEST": self.dest, "RZ_INSTALL_WORK": self.work}
        return subprocess.run(["sh", WRAPPER, *args], env=env, capture_output=True, text=True)

    def dest_is_empty(self):
        return os.listdir(self.dest) == []

    def test_wrong_arguments_exit_2(self):
        for args in ((), ("foo",), ("mirror", "extra"), ("../mirror",)):
            r = self.run_wrapper(*args)
            self.assertEqual(r.returncode, 2, args)
            self.assertTrue(self.dest_is_empty())

    def test_symlinked_staging_folder_is_refused(self):
        real = os.path.join(self.tmp, "real")
        os.mkdir(real)
        with open(os.path.join(real, "razdor-mirror"), "wb") as fh:
            fh.write(b"\x7fELF" + b"\0" * 60)
        os.rmdir(self.stage)
        os.symlink(real, self.stage)
        r = self.run_wrapper("mirror")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("staging folder", r.stderr)
        self.assertTrue(self.dest_is_empty())

    def test_symlinked_staged_file_is_refused(self):
        secret = os.path.join(self.tmp, "secret")
        with open(secret, "w", encoding="utf-8") as fh:
            fh.write("root:!locked:::\n")
        os.symlink(secret, os.path.join(self.stage, "razdor-mirror"))
        r = self.run_wrapper("mirror")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("is a symlink", r.stderr)
        self.assertTrue(self.dest_is_empty())

    def test_non_elf_file_is_refused(self):
        with open(os.path.join(self.stage, "razdor-server"), "w", encoding="utf-8") as fh:
            fh.write("#!/bin/sh\necho hi\n")
        r = self.run_wrapper("server")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("not an ELF file", r.stderr)
        self.assertTrue(self.dest_is_empty())

    def test_empty_file_is_refused(self):
        open(os.path.join(self.stage, "razdor-mirror"), "wb").close()
        r = self.run_wrapper("mirror")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("has size 0", r.stderr)
        self.assertTrue(self.dest_is_empty())

    def test_missing_file_is_refused(self):
        r = self.run_wrapper("mirror")
        self.assertEqual(r.returncode, 1, r.stderr)
        self.assertIn("is not in the staging folder", r.stderr)
        self.assertTrue(self.dest_is_empty())

    def test_real_elf_is_installed_under_the_fixed_name(self):
        with open(sys.executable, "rb") as fh:
            if fh.read(4) != b"\x7fELF":
                self.skipTest("the test needs an ELF file to copy (sys.executable is not one)")
        for target, name in (("mirror", "razdor-mirror"), ("server", "razdor-server")):
            src = os.path.join(self.stage, name)
            shutil.copyfile(sys.executable, src)
            r = self.run_wrapper(target)
            self.assertEqual(r.returncode, 0, r.stderr)
            installed = os.path.join(self.dest, name)
            self.assertTrue(os.path.isfile(installed) and not os.path.islink(installed))
            self.assertEqual(os.stat(installed).st_mode & 0o777, 0o755)
            with open(installed, "rb") as a, open(src, "rb") as b:
                self.assertEqual(a.read(), b.read())
            self.assertIn("sha256", r.stdout)
            os.remove(src)


if __name__ == "__main__":
    unittest.main()
