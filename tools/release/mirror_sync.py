#!/usr/bin/env python3
"""Starts the own download mirror's sync and waits until it has the new state (the owner's decision, 2026-11-24).

The owner's server (`razdor.indicozy.dev/hub/`) pulls the release files from GitHub on a timer (every 15
minutes, `razdor-mirror` in `server/`). After a release, or a rollback, the release skill does not wait for that
timer: it starts the sync over ssh, then polls `status.json` until the state is there:

    python3 -I tools/release/mirror_sync.py --tag v1.7.0                 # the tag is in the mirror's app list
    python3 -I tools/release/mirror_sync.py --tag v1.5.3 --gone v1.5.4   # a rollback: the bad tag is gone

The file `status.json` is `{"synced_at", "app": [tags], "catalog", "config_sha256"}`. A poll counts only when
`synced_at` is newer than the one read before the start and the tags are as asked. When a finished run does not
have the tag yet, the sync is started again (a start while a run is going does nothing).

The ssh host is `--ssh`, else `$RAZDOR_MIRROR_SSH`, else the line `RAZDOR_MIRROR_SSH=` of the repo's `.env`. The
command run there is `systemctl start razdor-mirror-sync.service` (the ssh login must be allowed to start that
unit; nothing here uses sudo). Only a public GET reads the status (no token).

Exit codes: 0 the mirror has the state, 1 it did not get it within the timeout (the sync failed, or it is slow:
check `journalctl -u razdor-mirror-sync` on the server), 2 the tool could not run (no ssh host, ssh failed, a bad
input).
"""

import argparse
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import update_check as uc  # noqa: E402

STATUS_URL = "https://razdor.indicozy.dev/hub/status.json"
UNIT = "razdor-mirror-sync.service"
SSH_KEY = "RAZDOR_MIRROR_SSH"
TIMEOUT = 1800  # seconds: a release is about 313 MiB from GitHub, a catalog 357 MiB; the timer runs every 15 min
POLL = 20  # seconds between two reads of status.json
EXIT_OK, EXIT_TIMEOUT, EXIT_ERROR = 0, 1, 2


class MirrorError(Exception):
    """The tool could not run (ssh, a bad status file, no host): exit 2."""


class MirrorTimeout(Exception):
    """The mirror did not get the state in time: exit 1."""


def parse_status(raw):
    """The status file as the sync writes it. Raises MirrorError on anything else (a web page, a half file)."""
    try:
        st = json.loads(raw.decode("utf-8") if isinstance(raw, bytes) else raw)
    except (UnicodeDecodeError, ValueError) as e:
        raise MirrorError(f"status.json is not JSON: {e}") from e
    if not isinstance(st, dict):
        raise MirrorError("status.json is not an object")
    app = st.get("app")
    if not isinstance(app, list) or not all(isinstance(t, str) for t in app):
        raise MirrorError("status.json: app must be a list of tags")
    if not isinstance(st.get("synced_at"), str) or not st["synced_at"]:
        raise MirrorError("status.json: synced_at is missing")
    return st


def fetch_status(get=None):
    """The status file, read now (the address is fetched with a query so no cache answers it)."""
    get = get or (lambda url: uc.http_get(url, timeout=20))
    return parse_status(get(f"{STATUS_URL}?t={int(time.time())}"))


def read_env_value(key, path=None):
    """The value of `key` in the repo's `.env` (quotes stripped), or None. Never printed."""
    path = path or os.path.join(ROOT, ".env")
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except OSError:
        return None
    for line in lines:
        name, sep, value = line.partition("=")
        if sep and name.strip() == key:
            return value.strip().strip('"').strip("'") or None
    return None


def ssh_host(given=None, env=None, env_file=None):
    env = os.environ if env is None else env
    host = (given or env.get(SSH_KEY, "") or read_env_value(SSH_KEY, env_file) or "").strip()
    if not host:
        raise MirrorError(f"no ssh host for the own mirror: pass --ssh or set {SSH_KEY} (in .env)")
    if host.startswith("-") or any(c.isspace() for c in host):
        raise MirrorError("the ssh host is not a plain host name")
    return host


def trigger(host, runner=None):
    """Starts the sync on the server (a run already going makes this a no-op)."""
    argv = ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=20", "--", host, f"systemctl start {UNIT}"]
    run = runner or (lambda a: subprocess.run(a, capture_output=True, text=True, timeout=120))
    try:
        r = run(argv)
    except (OSError, subprocess.SubprocessError) as e:
        raise MirrorError(f"cannot run ssh: {e}") from e
    if r.returncode != 0:
        raise MirrorError(f"ssh start of {UNIT} failed: {(r.stderr or r.stdout or '').strip()[-300:]}")


def safe_synced(fetch):
    try:
        return fetch().get("synced_at")
    except (MirrorError, uc.ToolError):
        return None


def sync_and_wait(has_new, host=None, timeout=TIMEOUT, interval=POLL, fetch=None, runner=None,
                  clock=time.monotonic, sleep=time.sleep, out=print):
    """Starts the sync and returns the status once `has_new(status)` holds on a run that finished after the start.
    Raises MirrorTimeout when it does not hold in time, MirrorError when the tool cannot run."""
    fetch = fetch or fetch_status
    target = ssh_host(host)
    before = safe_synced(fetch)
    trigger(target, runner)
    out(f"mirror: sync started on the own server (status before: {before or 'unreadable'})")
    seen = before
    deadline = clock() + timeout
    last = "no status read yet"
    while True:
        try:
            st = fetch()
        except (MirrorError, uc.ToolError) as e:
            last = str(e)
        else:
            last = f"synced_at {st['synced_at']}, app {', '.join(st['app'][:3]) or 'none'}"
            if st["synced_at"] != seen:
                seen = st["synced_at"]
                if st["synced_at"] != before and has_new(st):
                    out(f"mirror: complete ({last})")
                    return st
                trigger(target, runner)  # a finished run without the new state: start again
                out(f"mirror: a run finished without the new state ({last}); started again")
        if clock() >= deadline:
            raise MirrorTimeout(f"not complete after {timeout} s: {last}")
        sleep(interval)


def release_check(tag, gone=None):
    """The state a release or a rollback needs: `tag` in the app list, `gone` (a bad tag) not in it."""
    def has_new(st):
        return tag in st["app"] and (gone is None or gone not in st["app"])
    return has_new


def main(argv=None):
    ap = argparse.ArgumentParser(description="Start the own download mirror's sync and wait until it has the release.")
    ap.add_argument("--tag", required=True, metavar="vX.Y.Z", help="the release the mirror must list (newest kept)")
    ap.add_argument("--gone", metavar="vX.Y.Z", help="a rolled-back release the mirror must no longer list")
    ap.add_argument("--ssh", help=f"the server's ssh host (default ${SSH_KEY}, else .env)")
    ap.add_argument("--timeout", type=int, default=TIMEOUT, help=f"seconds to wait (default {TIMEOUT})")
    a = ap.parse_args(argv)
    try:
        uc.parse_version(a.tag)
        if a.gone:
            uc.parse_version(a.gone)
        sync_and_wait(release_check(a.tag, a.gone), host=a.ssh, timeout=a.timeout)
    except MirrorTimeout as e:
        print(f"mirror: {e}. The own mirror does not have the state yet: check the server's sync log.", file=sys.stderr)
        return EXIT_TIMEOUT
    except (MirrorError, uc.ToolError, ValueError) as e:
        print(f"mirror: {e}", file=sys.stderr)
        return EXIT_ERROR
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
