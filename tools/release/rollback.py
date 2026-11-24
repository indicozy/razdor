#!/usr/bin/env python3
"""Rolls a bad Razdor release back without a new release: the update feeds stop offering it.

    python3 -I tools/release/rollback.py --to v1.5.3              # dry run (the default): prints the plan
    python3 -I tools/release/rollback.py --to v1.5.3 --bad v1.5.4 --apply

What the updaters read (the app's update check, `src/update.rs`): the feed's list of releases, newest
first, drafts and pre-releases left out, and an offer only for a release newer than the program. So a
release is withdrawn by two changes, both done here:

1. GitHub: the bad release becomes a pre-release (`gh release edit vBAD --prerelease`). The API list the
   app reads skips it, so the newest published release is vTO again. Its files stay downloadable.
2. The own feed (`razdor.indicozy.dev/hub/pages/razdor-community/releases.json`, `UPDATE_FEED` in
   `src/mirror.rs`) is made by the own mirror from GitHub's list, so the bad release leaves it at the next
   sync (`mirror_sync.py`, below). Nothing is written to any other host.

The config (`data/remote/config.json`, served at both Pages sites) needs no change for a rollback: it names
the feed, not a release. The tool only reads it and warns when it names the bad tag, or when its
`update_feed` does not list the feed this tool rewrites. A change to it is made in the repo file and
goes out with the next push to `main` (`pages.yml`; the own mirror copies it), after the owner's yes.

What a rollback does NOT do: a player already on vBAD is not moved back (the updaters offer only newer
releases, never an older one). Those players get the next fixed release; say so to the owner.

Dry run by default: it reads GitHub, the feed and the live config (anonymous reads), and writes nothing.
`--apply` makes the two changes, then reads them back and prints what it changed. Then it starts the own download
mirror's sync over ssh and waits until its `status.json` lists vTO and no longer lists vBAD (`mirror_sync.py`; the
own feed is generated from GitHub's list, so the bad release leaves it at that sync). A mirror that does not confirm
in time is reported with exit 2 and the command to run by hand. It refuses (exit 1,
nothing changed) when a blocker is found: vBAD is not the newest published release, vTO is not published
or not older than vBAD, the feed does not list vTO, or the repo's config and the live one differ (the
Pages push writes docs/config.json from the repo file, so it would overwrite the live config).

Exit codes: 0 the plan is clear (dry run) or applied and verified, 1 a blocker (nothing changed), 2 the tool
could not run (network, gh, a bad input).
"""

import argparse
import difflib
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, HERE)

import mirror_sync as ms  # noqa: E402
import update_check as uc  # noqa: E402

REPO = uc.REPO
CONFIG_SRC = os.path.join(ROOT, "data", "remote", "config.json")
MIRROR_SRC = os.path.join(ROOT, "src", "mirror.rs")
REMOTE_CONFIG_SRC = os.path.join(ROOT, "src", "remote_config.rs")
EXIT_OK, EXIT_BLOCKED, EXIT_ERROR = 0, 1, 2


class RollbackError(Exception):
    """The tool could not run (exit 2)."""


def rust_urls(path, const):
    """The https literals of a `pub const NAME: [&str; N] = [...]` in a Rust file (the built-in lists)."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    m = re.search(rf"pub const {const}\s*:\s*\[&str;\s*\d+\]\s*=\s*\[(.*?)\];", text, re.S)
    if not m:
        raise RollbackError(f"{const} not found in {path}")
    urls = re.findall(r'"(https://[^"]+)"', m.group(1))
    if not urls:
        raise RollbackError(f"{const} in {path} has no https address")
    return urls


def feed_url_default():
    """The feed address this tool reads: the own mirror's copy of the API list (`UPDATE_FEED`). The mirror
    makes it from GitHub's list, so the check reads what the players' second address serves."""
    return next(u for u in rust_urls(MIRROR_SRC, "UPDATE_FEED") if "razdor.indicozy.dev" in u)


def config_urls_default():
    return rust_urls(REMOTE_CONFIG_SRC, "ADDRESSES")


def version_of(tag):
    m = uc.VERSION_RE.match(tag or "")
    return tuple(int(x) for x in m.groups()) if m else None


def feed_bytes(feed):
    return (json.dumps(feed, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def new_feed(feed, to, bad):
    """The feed without the bad release and anything newer than `to` (pre-releases and drafts stay out)."""
    kept = []
    for e in feed:
        v = version_of(e.get("tag_name"))
        if v is None or e.get("draft") or e.get("prerelease"):
            continue
        if v <= to and e.get("tag_name") != bad:
            kept.append(e)
    kept.sort(key=lambda e: version_of(e["tag_name"]), reverse=True)
    return kept[:2]


def config_mentions(config, tags):
    """The JSON paths of the config whose string value names one of the tags (e.g. a link to the bad release)."""
    hits = []

    def walk(node, path):
        if isinstance(node, dict):
            for k, v in node.items():
                walk(v, f"{path}.{k}" if path else k)
        elif isinstance(node, list):
            for i, v in enumerate(node):
                walk(v, f"{path}[{i}]")
        elif isinstance(node, str):
            for t in tags:
                if t and t in node:
                    hits.append(path)
    walk(config, "")
    return hits


def plan(to, bad, releases, feed, feed_url, live_config, repo_config, config_feeds):
    """The plan as data. `releases`: the GitHub release list (API shape); `feed`: the feed's entries
    (None when it could not be read); `live_config`/`repo_config`: parsed JSON or None; `config_feeds`:
    the update_feed the config names (a list, or None when the live config is unknown).

    Returns a dict: `blockers` (strings), `warnings`, `gh` (argv or None), `feed_before`, `feed_after`
    (lists of tags), `feed_new` (the new feed's entries, or None), `to`, `bad`."""
    out = {"blockers": [], "warnings": [], "gh": None, "feed_before": None, "feed_after": None,
           "feed_new": None, "to": to, "bad": bad}
    published = uc.published_map(releases)
    if not published:
        out["blockers"].append("no published release on GitHub")
        return out
    newest = max(published)
    bad_v = version_of(bad)
    if bad_v is None or to is None:
        out["blockers"].append("versions must be vX.Y.Z")
        return out
    if not bad_v > to:
        out["blockers"].append(f"{uc.vtag(to)} is not older than {bad}")
    if to not in published:
        out["blockers"].append(f"{uc.vtag(to)} is not a published release (a draft or a pre-release is no rollback target)")
    if bad_v not in published:
        if any(version_of(r.get("tag_name")) == bad_v and r.get("prerelease") for r in releases):
            out["warnings"].append(f"{bad} is already a pre-release on GitHub: the GitHub step is skipped")
        else:
            out["blockers"].append(f"{bad} is not a published release")
    elif bad_v != newest:
        out["blockers"].append(f"{bad} is not the newest published release ({uc.vtag(newest)} is): "
                               "a rollback withdraws the newest one only")
    if bad_v in published:
        out["gh"] = ["gh", "release", "edit", bad, "--repo", REPO, "--prerelease"]
    if feed is None:
        out["blockers"].append("the update feed could not be read")
    else:
        tags = [e.get("tag_name") for e in feed]
        out["feed_before"] = tags
        if bad not in tags:
            out["warnings"].append(f"the feed does not list {bad}: nothing to withdraw there")
        if uc.vtag(to) not in tags:
            out["blockers"].append(f"the feed does not list {uc.vtag(to)}: its entry (its asset addresses) "
                                   "can only be rebuilt by publish_release.py; see the release skill")
        fn = new_feed(feed, to, bad)
        out["feed_new"] = fn
        out["feed_after"] = [e["tag_name"] for e in fn]
    if live_config is None:
        out["warnings"].append("the live config could not be read: its addresses and links are not checked")
    else:
        hits = config_mentions(live_config, [bad])
        if hits:
            out["warnings"].append(f"the live config names {bad} at {', '.join(hits)}: change data/remote/config.json "
                                   "(it needs no change for the feed itself)")
        if config_feeds is not None and feed_url not in config_feeds:
            out["warnings"].append(f"the live config's update_feed does not list {feed_url}: clients that read "
                                   "that config do not see this rollback; add the feed to data/remote/config.json")
        if repo_config is None:
            out["warnings"].append("data/remote/config.json could not be read")
        elif repo_config != live_config:
            out["blockers"].append("data/remote/config.json and the live config differ: a Pages push would write the "
                                   "repo's config over the live one; publish or fix the repo file first")
    return out


def describe(p, feed_url):
    lines = [f"Rollback plan: {p['bad']} withdrawn, {uc.vtag(p['to'])} the newest again"]
    if p["blockers"]:
        lines.append("BLOCKED (nothing would change):")
        lines += [f"  - {b}" for b in p["blockers"]]
    for w in p["warnings"]:
        lines.append(f"warning: {w}")
    if p["gh"]:
        lines.append("1. GitHub: the bad release becomes a pre-release (the API list skips it):")
        lines.append("     GH_TOKEN=$(gh auth token -u indicozy) " + " ".join(p["gh"]))
    if p["feed_new"] is not None:
        lines.append(f"2. Own feed {feed_url} (the own mirror makes it from GitHub's list; nothing is written here):")
        lines.append(f"     entries {p['feed_before']} -> {p['feed_after']}")
        lines.append("     (the diff shows the tag names; each kept entry keeps its own asset list)")
        before = feed_bytes([{"tag_name": t} for t in p["feed_before"]]).decode().splitlines()
        after = feed_bytes([{"tag_name": t} for t in p["feed_after"]]).decode().splitlines()
        lines += ["     " + ln for ln in difflib.unified_diff(before, after, "now", "after", lineterm="", n=1)][2:]
        lines.append("     then a commit on razdor-community main and a wait until the address serves it")
    lines.append("3. config.json: no change made by a rollback (see the warnings above, if any)")
    lines.append("Clients already on the bad release are not moved back: they get the next fixed release.")
    return "\n".join(lines)


def read_live(url):
    try:
        return uc.http_get(url, timeout=30)
    except uc.ToolError:
        return None


def gh_env():
    try:
        r = subprocess.run(["gh", "auth", "token", "-u", "indicozy"], capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.SubprocessError) as e:
        raise RollbackError(f"cannot run gh: {e}") from e
    token = r.stdout.strip()
    if r.returncode != 0 or not token:
        raise RollbackError("gh has no token for indicozy (gh auth login -u indicozy)")
    env = dict(os.environ)
    env["GH_TOKEN"] = token
    return env


def apply(p, feed_url, releases_reader=None, gh_runner=None):
    """Makes the changes and reads them back. Returns the lines to print."""
    out = []
    if p["gh"]:
        runner = gh_runner or (lambda argv: subprocess.run(argv, env=gh_env(), capture_output=True, text=True, timeout=120))
        r = runner(p["gh"])
        if r.returncode != 0:
            raise RollbackError(f"gh release edit failed: {(r.stderr or r.stdout).strip()[-300:]}")
        out.append(f"github: {p['bad']} is a pre-release now")
    read = releases_reader or (lambda: uc.release_list())
    after = uc.published_map(read())
    newest = max(after) if after else None
    out.append(f"check: the newest published release is now {uc.vtag(newest) if newest else 'none'}")
    if newest != p["to"]:
        raise RollbackError(f"after the change the newest published release is {newest}, not {uc.vtag(p['to'])}")
    return out


def own_mirror(to, bad):
    """After the GitHub flip: starts the own download mirror's sync and waits until its status lists `to` and no
    longer lists `bad` (the sync drops a pre-release from the own feed). Raises mirror_sync's errors."""
    st = ms.sync_and_wait(ms.release_check(to, gone=bad))
    return f"the own mirror lists {to} and not {bad} (synced {st['synced_at']})"


def load_inputs(env, to, feed_url):
    """The inputs as the tool reads them: each one from `env` when a test gives it (None included: an
    unreadable input), else from GitHub, the own mirror and the repo."""
    def given(key):
        return key in env

    releases = env["releases"] if given("releases") else uc.release_list()
    if given("feed"):
        feed = env["feed"]
    else:
        raw = read_live(feed_url)
        feed = json.loads(raw) if raw else None
    if given("live_config"):
        live_config = env["live_config"]
    else:
        live_config = None
        for url in config_urls_default():
            raw = read_live(url)
            if raw:
                try:
                    live_config = json.loads(raw)
                    break
                except ValueError:
                    continue
    if given("repo_config"):
        repo_config = env["repo_config"]
    else:
        with open(CONFIG_SRC, encoding="utf-8") as f:
            repo_config = json.load(f)
    config_feeds = live_config.get("update_feed") if isinstance(live_config, dict) else None
    return releases, feed, live_config, repo_config, config_feeds


def main(argv=None, env=None):
    """`env` (tests only) replaces inputs: keys releases, feed, feed_url, live_config, repo_config,
    releases_reader, gh_runner, mirror (a function () -> str that runs the own mirror's
    trigger; tests pass a fake, the real one needs the ssh host). A key given as None means that input is unreadable."""
    env = env or {}
    ap = argparse.ArgumentParser(description="Roll a bad release back: the update feeds stop offering it.")
    ap.add_argument("--to", required=True, metavar="vX.Y.Z", help="the release to make the newest again (the previous one)")
    ap.add_argument("--bad", metavar="vX.Y.Z", help="the release to withdraw (default: the newest published one)")
    ap.add_argument("--apply", action="store_true", help="make the changes (default: a dry run that changes nothing)")
    a = ap.parse_args(argv)
    try:
        to = uc.parse_version(a.to)
        feed_url = env["feed_url"] if "feed_url" in env else feed_url_default()
        releases, feed, live_config, repo_config, config_feeds = load_inputs(env, to, feed_url)
        published = uc.published_map(releases)
        if not published:
            raise RollbackError("no published release on GitHub")
        bad_v = uc.parse_version(a.bad) if a.bad else max(published)
        p = plan(to, uc.vtag(bad_v), releases, feed, feed_url, live_config, repo_config, config_feeds)
    except (RollbackError, uc.ToolError, ValueError, OSError) as e:
        print(f"rollback: {e}", file=sys.stderr)
        return EXIT_ERROR
    print(describe(p, feed_url))
    if p["blockers"]:
        print("nothing changed.", file=sys.stderr)
        return EXIT_BLOCKED
    if not a.apply:
        print("\ndry run: nothing changed. Run with --apply to make these changes (the owner's yes first).")
        return EXIT_OK
    try:
        lines = apply(p, feed_url, env.get("releases_reader"), env.get("gh_runner"))
    except RollbackError as e:
        print(f"rollback: {e}", file=sys.stderr)
        return EXIT_ERROR
    except Exception as e:  # a Pages or network error: say what may have changed
        print(f"rollback: {e}. Check the feed and the GitHub release by hand (see the release skill's Rollback).",
              file=sys.stderr)
        return EXIT_ERROR
    print("\nApplied:")
    print("\n".join("  " + ln for ln in lines))
    mirror = env.get("mirror") or (lambda: own_mirror(uc.vtag(p["to"]), p["bad"]))
    try:
        print("  " + mirror())
    except (ms.MirrorError, ms.MirrorTimeout, uc.ToolError, ValueError) as e:
        print(f"rollback: applied on GitHub, but the own mirror did not confirm it: {e}. "
              f"Run `python3 -I tools/release/mirror_sync.py --tag {uc.vtag(p['to'])} --gone {p['bad']}` "
              "when it is fixed, and tell the owner.", file=sys.stderr)
        return EXIT_ERROR
    return EXIT_OK


if __name__ == "__main__":
    sys.exit(main())
