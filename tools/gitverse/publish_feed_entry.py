#!/usr/bin/env python3
"""Adds a Razdor release to the GitVerse update feed, with links to the own mirror (stdlib only; run with
`python3 -I`).

  python3 -I tools/gitverse/publish_feed_entry.py --tag vX.Y.Z [--dry-run]
  python3 -I tools/gitverse/publish_feed_entry.py --drop vBAD [--dry-run]

The owner's decision (2026-11-25): Razdor 1.6.0 copies still read the feed
https://indicozy.gitverse.site/razdor-community/releases.json (docs/releases.json on main of GitVerse's
razdor-community, served by Pages). No files go to GitVerse (its downloads throttle): the download links
of each entry point at the own mirror, razdor.indicozy.dev, and the IPA at GitHub.

--tag: the release's GitHub API entry (name, body, assets) becomes the feed entry. Its assets are the
versioned program files on the own mirror, the old fixed names (razdor, Razdor.exe, razdor-macos,
razdor.apk) pointing at the same versioned URLs, Razdor-win7.exe and the Windows 7 zip as they are, the
IPA on GitHub, and SHA256SUMS(.txt) on the mirror. Before anything is written, every mirror link is checked
(HEAD answers 200) and the mirror's SHA256SUMS lists each versioned program. Any failure exits 1 and
writes nothing (the own mirror is not synced yet). The feed keeps the newest two entries.

--drop: removes that tag's entry (a rollback). Refused when it would leave the feed empty.
--dry-run: prints the entry and the resulting tags; nothing is pushed (the feed is still read).

Exit codes: 0 done (or nothing to do), 1 a failed check or fetch, 2 a refused release (a draft, a
pre-release, a bad tag) or a refused drop.
The token is read by pages.Pages (gitverse.load_token) and never printed.
"""

import argparse
import json
import os
import sys
import urllib.error
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402

FEED_PATH = "docs/releases.json"
GH_API = "https://api.github.com/repos/" + gv.OWNER + "/" + gv.APP_REPO + "/releases/tags/{tag}"
OWN = "https://razdor.indicozy.dev/hub/" + gv.OWNER + "/" + gv.APP_REPO + "/releases/download/{tag}/"
GITHUB_DL = "https://github.com/" + gv.OWNER + "/" + gv.APP_REPO + "/releases/download/{tag}/"
KEEP = 2
TIMEOUT = 60
UA = gv.USER_AGENT

# The versioned program files, each with its old fixed name (the feed names both).
PROGRAMS = (
    ("razdor-{v}-linux", "razdor"),
    ("Razdor-{v}.exe", "Razdor.exe"),
    ("razdor-{v}-macos", "razdor-macos"),
    ("razdor-{v}.apk", "razdor.apk"),
)
# Files listed on the own mirror as they are (not versioned programs, so not in the SHA256SUMS check).
EXTRA_OWN = ("Razdor-win7.exe", "razdor-{v}-windows7.zip")
IPA = ("razdor-{v}.ipa", "razdor.ipa")  # GitHub only: the own mirror has no IPA
SUMS = ("SHA256SUMS", "SHA256SUMS.txt")


def warn(msg):
    print(f"warning: {msg}", file=sys.stderr, flush=True)


def die(msg, code):
    print(f"error: {msg}", file=sys.stderr, flush=True)
    raise SystemExit(code)


def version_of(tag):
    m = gv.SEMVER_TAG.match(tag or "")
    if not m:
        die(f"{tag!r} is not a release tag (vX.Y.Z)", 2)
    return ".".join(m.groups())


def fetch_github_release(tag):
    """The GitHub API's release object for `tag` (a public repo: no token)."""
    req = urllib.request.Request(GH_API.format(tag=tag), headers={
        "Accept": "application/vnd.github+json", "User-Agent": UA})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            return json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        raise gv.GitVerseError(f"GitHub release {tag}: HTTP {e.code}")


def head_status(url):
    """The HTTP status of a HEAD request, redirects followed; 0 when the request fails."""
    req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": UA})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return e.code
    except (urllib.error.URLError, OSError):
        return 0


def fetch_text(url):
    """The body of an anonymous GET as text."""
    return gv.fetch_bytes(url, tries=2).decode("utf-8", "replace")


def sums_names(text):
    """The file names listed in a `sha256  name` file (a leading `*` on a name marks binary mode)."""
    names = set()
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2:
            names.add(parts[1].strip().lstrip("*"))
    return names


def build_entry(tag, release):
    """The feed entry (the GitHub API shape), the versioned program names on the mirror, and every own URL
    used. Exits 2 for a draft or a pre-release."""
    v = version_of(tag)
    if release.get("draft") or release.get("prerelease"):
        die(f"{tag} is a draft or a pre-release: not added to the feed", 2)
    have = {a.get("name") for a in release.get("assets", [])}
    own = OWN.format(tag=tag)
    assets = []
    versioned = []  # the versioned program names on the own mirror (checked against its SHA256SUMS)
    checks = []     # every own URL used (checked with HEAD)

    def add(name, url):
        assets.append({"name": name, "browser_download_url": url})

    for vtmpl, legacy in PROGRAMS:
        vname = vtmpl.format(v=v)
        if vname not in have:
            warn(f"{tag}: {vname} is not in the GitHub release: skipped")
            continue
        add(vname, own + vname)
        add(legacy, own + vname)
        versioned.append(vname)
        checks.append(own + vname)
    for tmpl in EXTRA_OWN:
        name = tmpl.format(v=v)
        if name not in have:
            warn(f"{tag}: {name} is not in the GitHub release: skipped")
            continue
        add(name, own + name)
        checks.append(own + name)
    ipa_v, ipa_legacy = (t.format(v=v) for t in IPA)
    if ipa_v in have:
        add(ipa_v, GITHUB_DL.format(tag=tag) + ipa_v)
        add(ipa_legacy, GITHUB_DL.format(tag=tag) + ipa_v)
    else:
        warn(f"{tag}: {ipa_v} is not in the GitHub release: skipped")
    for name in SUMS:
        add(name, own + "SHA256SUMS")
    checks.append(own + "SHA256SUMS")
    entry = {"tag_name": tag, "name": release.get("name") or f"Раздор {v}",
             "html_url": release.get("html_url", ""), "draft": False, "prerelease": False,
             "body": release.get("body") or "", "assets": assets}
    return entry, versioned, sorted(set(checks))


def verify(tag, versioned, checks):
    """Every own URL answers 200, and the mirror's SHA256SUMS lists each versioned program. Returns the
    problems found (empty when all is well)."""
    problems = []
    for url in checks:
        status = head_status(url)
        if status != 200:
            problems.append(f"HEAD {url}: HTTP {status}")
    if problems:
        return problems
    try:
        listed = sums_names(fetch_text(OWN.format(tag=tag) + "SHA256SUMS"))
    except gv.GitVerseError as e:
        return [f"SHA256SUMS on the mirror: {e}"]
    for name in versioned:
        if name not in listed:
            problems.append(f"SHA256SUMS on the mirror does not list {name}")
    return problems


def feed_after(prev, entry=None, drop=None):
    """The feed with `entry` added (its tag replaced), or with the tag `drop` removed. Newest first; with an
    entry, the newest KEEP only."""
    tag = entry["tag_name"] if entry else drop
    items = ([entry] if entry else []) + [e for e in prev if e.get("tag_name") != tag]
    items.sort(key=lambda e: gv.version_key(e.get("tag_name")) or (0, 0, 0), reverse=True)
    return items[:KEEP] if entry else items


def feed_bytes(feed):
    return (json.dumps(feed, ensure_ascii=False, indent=1) + "\n").encode("utf-8")


def read_feed(pg):
    raw = pg.read(FEED_PATH)
    return json.loads(raw.decode("utf-8")) if raw else []


def tags_of(feed):
    return ", ".join(str(e.get("tag_name")) for e in feed)


def publish(args):
    tag = args.tag
    version_of(tag)
    release = fetch_github_release(tag)
    entry, versioned, checks = build_entry(tag, release)
    problems = verify(tag, versioned, checks)
    if problems:
        for p in problems:
            print(f"failed: {p}", file=sys.stderr)
        die("the own mirror is not ready for this release: nothing written "
            "(run again once it lists the files)", 1)
    with pages.Pages() as pg:
        feed = feed_after(read_feed(pg), entry=entry)
        if args.dry_run:
            print(json.dumps(entry, ensure_ascii=False, indent=1))
            print(f"the feed would hold: {tags_of(feed)}")
            print("dry run: nothing pushed")
            return 0
        data = feed_bytes(feed)
        pg.write(FEED_PATH, data)
        pushed = pg.commit_push(f"Update feed: {tag} (files from the own mirror)")
        print(f"feed: {'pushed ' + pushed if pushed else 'unchanged'}")
        waited = pg.wait_served({FEED_PATH: data})
        print(f"feed served by Pages (waited {waited:.0f} s); the feed holds: {tags_of(feed)}")
    return 0


def drop(args):
    tag = args.drop
    version_of(tag)
    with pages.Pages() as pg:
        prev = read_feed(pg)
        if not any(e.get("tag_name") == tag for e in prev):
            print(f"{tag} is not in the feed: nothing to do")
            return 0
        feed = feed_after(prev, drop=tag)
        if not feed:
            die(f"dropping {tag} would leave the feed empty: refused", 2)
        if args.dry_run:
            print(f"dry run: the feed would hold: {tags_of(feed)}")
            return 0
        data = feed_bytes(feed)
        pg.write(FEED_PATH, data)
        pushed = pg.commit_push(f"Update feed: without {tag}")
        print(f"feed: {'pushed ' + pushed if pushed else 'unchanged'}")
        waited = pg.wait_served({FEED_PATH: data})
        print(f"feed served by Pages (waited {waited:.0f} s); the feed holds: {tags_of(feed)}")
    return 0


def main(argv):
    ap = argparse.ArgumentParser(description="Adds a release to the GitVerse update feed, or drops one.")
    ap.add_argument("--tag", help="the release to add to the feed (vX.Y.Z)")
    ap.add_argument("--drop", metavar="TAG", help="remove TAG from the feed (a rollback)")
    ap.add_argument("--dry-run", action="store_true", help="print the entry and the tags; push nothing")
    args = ap.parse_args(argv)
    if bool(args.tag) == bool(args.drop):
        ap.error("give exactly one of --tag or --drop")
    try:
        return drop(args) if args.drop else publish(args)
    except (pages.PagesError, gv.GitVerseError) as e:
        die(str(e), 1)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
