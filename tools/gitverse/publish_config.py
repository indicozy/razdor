#!/usr/bin/env python3
"""Publishes the remote config to the razdor-community GitVerse Pages, and nothing else.

The source is `data/remote/config.json`, the same bytes the GitHub Pages site serves (pages.yml copies
it there). The push goes to `docs/config.json` of razdor-community through `pages.Pages`, which writes
that file on every push, and the script waits until the anonymous address serves exactly these bytes
(`https://indicozy.gitverse.site/razdor-community/config.json`). The app reads the file from both sites
(`src/remote_config.rs`); a change needs no app release.

The file is checked first, as the app reads it: schema 1, non-empty lists of https addresses, a game
with a file name, a SHA-256 and a size. A failed check pushes nothing.

Usage:
  python3 -I tools/gitverse/publish_config.py              # check, push, wait until served
  python3 -I tools/gitverse/publish_config.py --dry-run    # check only, no network
"""

import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402

SCHEMA = 1
MAX_GAME_BYTES = 256 << 20  # the app's MAX_ARCHIVE_DOWNLOAD
BAD_ADDRESS_CHARS = re.compile(r"[\s\\\x00-\x1f\x7f]")
SHA256 = re.compile(r"[0-9a-fA-F]{64}")


def https_address(a):
    return isinstance(a, str) and a.startswith("https://") and len(a) > len("https://") and not BAD_ADDRESS_CHARS.search(a)


def check(raw):
    """The config's shape as the app reads it. Returns the parsed config; raises gv.GitVerseError
    with the reason when the app would not take it."""
    try:
        cfg = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, ValueError) as e:
        raise gv.GitVerseError(f"config.json is not JSON: {e}")
    if not isinstance(cfg, dict) or cfg.get("schema") != SCHEMA:
        raise gv.GitVerseError(f"config.json must have \"schema\": {SCHEMA}")
    for key in ("update_feed", "hub"):
        addresses = cfg.get(key)
        if not isinstance(addresses, list) or not addresses or not all(https_address(a) for a in addresses):
            raise gv.GitVerseError(f"config.json: {key} must be a non-empty list of https addresses")
    game = cfg.get("game")
    if not isinstance(game, dict):
        raise gv.GitVerseError("config.json: game is missing")
    links = game.get("links")
    if not isinstance(links, list) or not links or not all(https_address(a) for a in links):
        raise gv.GitVerseError("config.json: game.links must be a non-empty list of https addresses")
    size = game.get("size")
    if not isinstance(size, int) or isinstance(size, bool) or not 0 < size <= MAX_GAME_BYTES:
        raise gv.GitVerseError(f"config.json: game.size {size!r} is not a sane archive size")
    if not isinstance(game.get("sha256"), str) or not SHA256.fullmatch(game["sha256"]):
        raise gv.GitVerseError("config.json: game.sha256 is not 64 hex digits")
    name = game.get("name")
    if not isinstance(name, str) or not name or "/" in name or name in (".", ".."):
        raise gv.GitVerseError(f"config.json: game.name {name!r} is not a file name")
    return cfg


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dry-run", action="store_true", help="check the file only; no network")
    ap.add_argument("--wait", type=int, default=pages.WAIT_DEFAULT, help="seconds to wait for Pages")
    args = ap.parse_args(argv)
    try:
        raw = pages.config_bytes()
        cfg = check(raw)
        print(f"config: schema {cfg['schema']}, {len(cfg['update_feed'])} feed addresses, "
              f"{len(cfg['hub'])} hub folders, game {cfg['game']['name']}")
        if args.dry_run:
            print("dry run: nothing pushed")
            return 0
        with pages.Pages(gv.HUB_REPO) as pg:
            pushed = pg.commit_push("Remote config: data/remote/config.json")
            print(f"config: {'pushed ' + pushed if pushed else 'unchanged'}")
            waited = pg.wait_served({"docs/config.json": raw}, wait=args.wait)
            print(f"config.json served by Pages (waited {waited:.0f} s)")
        return 0
    except gv.GitVerseError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
