#!/usr/bin/env python3
"""Publishes a Razdor release to GitVerse (stdlib only; run with `python3 -I`).

  python3 -I tools/gitverse/publish_release.py --tag vX.Y.Z --notes NOTES.md FILES...
      [--prerelease] [--target COMMIT-ISH] [--replace] [--keep 2] [--no-feed] [--dry-run]
  python3 -I tools/gitverse/publish_release.py --unpublish TAG [--git-tag]
  python3 -I tools/gitverse/publish_release.py --push-source vX.Y.Z
      (the release commit to GitVerse's main, and the tag)

What it does with the release files (the names the GitHub pipeline gives them):
  - packs what GitVerse refuses (.exe, names without an extension) into zips, the programs under
    their unversioned names with mode 0755: Razdor-X.Y.Z-windows.zip (Razdor.exe, Razdor-translate.exe),
    razdor-X.Y.Z-linux.zip (razdor, razdor-translate), razdor-X.Y.Z-macos.zip (razdor-macos,
    razdor-translate-macos);
  - passes the rest as they are (razdor-X.Y.Z.apk, razdor-ads-X.Y.Z.zip, ...); the iOS .ipa is GitHub-only;
  - skips the unversioned copies (Razdor.exe, razdor, razdor.apk, ...): the feed names them;
  - skips Razdor-win7.exe (the Windows 7 program): GitVerse refuses .exe, and the Windows 7 zip
    (razdor-X.Y.Z-windows7.zip, passed as it is) carries it;
  - writes SHA256SUMS.txt (the uploaded files, and the programs inside the zips and under their
    versioned and old names, which the updater looks up);
  - creates the release (refused when it exists, unless --replace), uploads with retries, verifies
    each upload by an anonymous download in parts and its SHA-256;
  - updates releases.json on the razdor-community Pages (the update feed: the newest two non-
    pre-release entries, in the GitHub API's release shape) and waits until Pages serves it;
  - deletes the GitVerse releases this script made, older than the newest two (pre-releases: no
    feed, no cleanup).

Pre-releases (--prerelease) do not touch the feed or the cleanup. A tag with a suffix (v1.2.3-x) is
refused without --prerelease.

The packed files go to $RAZDOR_GITVERSE_WORK (default /mnt/ai_models/razdor-scratch/gitverse-release/TAG)
and are removed at the end (--keep-work keeps them).
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
WORK_ROOT = os.environ.get("RAZDOR_GITVERSE_WORK") or "/mnt/ai_models/razdor-scratch/gitverse-release"
TAG_RE = re.compile(r"^v(\d+\.\d+\.\d+)(-[A-Za-z0-9.]+)?$")
VERSION_IN_NAME = re.compile(r"-(\d+\.\d+\.\d+)(?=[-.])")
FEED_PATH = "docs/releases.json"
SUMS_NAMES = ("SHA256SUMS", "SHA256SUMS.txt")

# Per platform: the zip, the versioned program and its unversioned name inside the zip, the translator.
PROGRAMS = {
    "windows": {"zip": "Razdor-{v}-windows.zip", "prog": "Razdor-{v}.exe", "inner": "Razdor.exe",
                "tr_prog": "Razdor-translate-{v}.exe", "tr_inner": "Razdor-translate.exe"},
    "linux": {"zip": "razdor-{v}-linux.zip", "prog": "razdor-{v}-linux", "inner": "razdor",
              "tr_prog": "razdor-translate-{v}-linux", "tr_inner": "razdor-translate"},
    "macos": {"zip": "razdor-{v}-macos.zip", "prog": "razdor-{v}-macos", "inner": "razdor-macos",
              "tr_prog": "razdor-translate-{v}-macos", "tr_inner": "razdor-translate-macos"},
}
# Files uploaded as they are: versioned name -> the old fixed name the feed also lists.
DIRECT = {"razdor-{v}.apk": "razdor.apk"}
# The Windows 7 program: a loose .exe GitVerse refuses; its zip, razdor-{v}-windows7.zip, is uploaded as it is.
WIN7_PROGRAM = "Razdor-win7.exe"
# GitHub-only files: GitVerse refuses .ipa (measured 2026-11-13), so they are left out with a warning.
GITHUB_ONLY = {"razdor-{v}.ipa": "razdor.ipa"}


class PlanError(gv.GitVerseError):
    pass


def version_of(tag):
    m = TAG_RE.match(tag or "")
    if not m:
        raise PlanError(f"{tag!r} is not a release tag (vX.Y.Z, or vX.Y.Z-suffix for a pre-release)")
    return m.group(1)


def read_notes(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def plan(paths, v):
    """Which file is what: {'groups': zipped programs, 'direct': {name: path}, 'skipped': [(name, why)],
    'warnings': [str]}. Raises PlanError for a name GitVerse refuses that is not a program."""
    by_name = {}
    for p in paths:
        name = os.path.basename(p)
        if name in by_name:
            raise PlanError(f"{name} is given twice")
        if not os.path.isfile(p):
            raise PlanError(f"{p} is not a file")
        by_name[name] = p
    prog_names = {}
    for plat, spec in PROGRAMS.items():
        prog_names[spec["prog"].format(v=v)] = (plat, "prog")
        prog_names[spec["tr_prog"].format(v=v)] = (plat, "tr")
    direct_names = {tmpl.format(v=v): fixed for tmpl, fixed in DIRECT.items()}
    github_only = {tmpl.format(v=v): fixed for tmpl, fixed in GITHUB_ONLY.items()}
    dup_names = set()
    for spec in PROGRAMS.values():
        dup_names.update({spec["inner"], spec["tr_inner"]})
    dup_names.update(DIRECT.values())
    dup_names.update(GITHUB_ONLY.values())

    found, direct, skipped, warnings = {}, {}, [], []
    for name in sorted(by_name):
        path = by_name[name]
        if name in prog_names:
            plat, kind = prog_names[name]
            found.setdefault(plat, {})[kind] = path
        elif name in direct_names:
            direct[name] = path
        elif name in github_only:
            warnings.append(f"{name} is not mirrored: GitVerse refuses it (the GitHub release has it)")
        elif name in SUMS_NAMES:
            skipped.append((name, "made here"))
        elif name == WIN7_PROGRAM:
            skipped.append((name, "GitVerse refuses .exe: the Windows 7 zip carries it"))
        elif name in dup_names:
            skipped.append((name, "an unversioned copy: the feed names it, the versioned file is uploaded"))
        else:
            problem = gv.name_problem(name)
            if problem:
                raise PlanError(f"{name}: {problem}; pack it or rename it")
            other = VERSION_IN_NAME.search(name)
            if other and other.group(1) != v:
                raise PlanError(f"{name} is for version {other.group(1)}, the tag is {v}")
            direct[name] = path

    groups = []
    for plat, spec in PROGRAMS.items():
        got = found.get(plat, {})
        if "prog" not in got:
            if "tr" in got:
                warnings.append(f"{spec['tr_prog'].format(v=v)} has no program beside it: not packed")
            else:
                warnings.append(f"no {plat} program ({spec['prog'].format(v=v)}) in this release")
            continue
        groups.append({"platform": plat, "zip": spec["zip"].format(v=v), "prog_name": spec["prog"].format(v=v),
                       "inner": spec["inner"], "prog": got["prog"],
                       "tr_inner": spec["tr_inner"] if "tr" in got else None, "tr": got.get("tr")})
    if not groups and not direct:
        raise PlanError("nothing to publish")
    return {"groups": groups, "direct": direct, "skipped": skipped, "warnings": warnings}


def _zip_time(path):
    return time.gmtime(max(os.path.getmtime(path), 315532800))[:6]


def pack(groups, workdir):
    """Writes each platform's zip into workdir (programs under their unversioned names, mode 0755).
    Returns {zip name: path}."""
    out = {}
    for g in groups:
        members = [(g["inner"], g["prog"])]
        if g["tr"]:
            members.append((g["tr_inner"], g["tr"]))
        path = os.path.join(workdir, g["zip"])
        with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
            for inner, src in members:
                info = zipfile.ZipInfo(inner, date_time=_zip_time(src))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = (0o100755 & 0xFFFF) << 16
                with z.open(info, "w") as dst, open(src, "rb") as f:
                    shutil.copyfileobj(f, dst, gv.CHUNK)
        out[g["zip"]] = path
    return out


def sums_lines(plan_, v, zips, shas):
    """{name: sha256} for SHA256SUMS.txt: the uploaded files, the programs under the names the updater
    looks up (the zip member, the versioned and the old name), and the translators under their names."""
    lines = {}
    for name, path in zips.items():
        lines[name] = shas(path)
    for name, path in plan_["direct"].items():
        lines[name] = shas(path)
    for g in plan_["groups"]:
        ph = shas(g["prog"])
        lines[g["inner"]] = ph
        lines[g["prog_name"]] = ph
        if g["tr"]:
            lines[g["tr_inner"]] = shas(g["tr"])
    for tmpl, alias in DIRECT.items():
        vname = tmpl.format(v=v)
        if vname in plan_["direct"]:
            lines[alias] = lines[vname]
    return lines


def sums_text(lines):
    return "".join(f"{lines[name]}  {name}\n" for name in sorted(lines))


def feed_entry(tag, v, prerelease, body, html_url, groups, direct_urls, zip_urls, sums_url):
    """One release in the GitHub API's shape, with the zipped programs as archive/member entries."""
    assets = []
    for g in groups:
        for name in (g["prog_name"], g["inner"]):
            assets.append({"name": name, "browser_download_url": zip_urls[g["zip"]],
                           "archive": "zip", "member": g["inner"]})
    for tmpl, alias in DIRECT.items():
        vname = tmpl.format(v=v)
        if vname in direct_urls:
            for name in (vname, alias):
                assets.append({"name": name, "browser_download_url": direct_urls[vname]})
    for name in SUMS_NAMES:
        assets.append({"name": name, "browser_download_url": sums_url})
    return {"tag_name": tag, "name": f"Раздор {v}", "html_url": html_url, "draft": False,
            "prerelease": bool(prerelease), "body": body, "assets": assets}


def merge_feed(prev, entry, keep=2):
    """The feed after `entry` (a non-pre-release): newest first, the newest `keep` non-pre-releases."""
    rest = [e for e in (prev or []) if e.get("tag_name") != entry["tag_name"]]
    ok = [e for e in rest + [entry]
          if not e.get("draft") and not e.get("prerelease") and gv.version_key(e.get("tag_name"))]
    ok.sort(key=lambda e: gv.version_key(e["tag_name"]), reverse=True)
    return ok[:keep]


def feed_bytes(feed):
    return (json.dumps(feed, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def target_for(tag, given):
    if given:
        return given
    r = subprocess.run(["git", "rev-parse", "--verify", "--quiet", tag + "^{commit}"],
                       cwd=REPO_ROOT, capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 and r.stdout.strip() else "main"


def delete_release_entry(client, entry):
    try:
        if entry.get("id"):
            client.delete_release(gv.OWNER, entry["repo"], entry["id"])
        else:
            client.delete_release_by_tag(gv.OWNER, entry["repo"], entry["tag"])
    except gv.ApiError as e:
        if e.status != 404:
            raise
    print(f"deleted the GitVerse release {entry['tag']} ({entry['repo']})")


def prune(client, reg):
    for entry in gv.select_prune(reg["releases"], keep=2):
        delete_release_entry(client, entry)
        gv.unregister(reg, entry["repo"], entry["tag"])
    gv.save_registry(reg)
    total = sum(r.get("bytes", 0) for r in reg["releases"])
    print(f"release files this script made: {total / 1024 ** 2:.0f} MB (the GitVerse budget is 4 GB "
          f"for both repos, the game archive included)")


def publish(args):
    v = version_of(args.tag)
    suffix = TAG_RE.match(args.tag).group(2)
    if suffix and not args.prerelease:
        raise PlanError(f"{args.tag} has a suffix: a pre-release, pass --prerelease")
    prerelease = bool(args.prerelease)
    notes = read_notes(args.notes)
    work = os.path.join(WORK_ROOT, args.tag)
    if os.path.exists(work):
        shutil.rmtree(work)
    os.makedirs(work)

    p = plan(args.files, v)
    for name, why in p["skipped"]:
        print(f"skipped {name}: {why}")
    for w in p["warnings"]:
        print(f"warning: {w}")
    zips = pack(p["groups"], work)
    cache = {}

    def shas(path):
        if path not in cache:
            cache[path] = gv.sha256_file(path)
        return cache[path]

    lines = sums_lines(p, v, zips, shas)
    sums_path = os.path.join(work, "SHA256SUMS.txt")
    with open(sums_path, "w", encoding="utf-8") as f:
        f.write(sums_text(lines))
    uploads = [(g["zip"], zips[g["zip"]]) for g in p["groups"]]
    uploads += list(p["direct"].items())
    uploads.append(("SHA256SUMS.txt", sums_path))
    sizes = {name: gv.check_size(path) for name, path in uploads}
    total = sum(sizes.values())
    print(f"{len(uploads)} files, {total / 1024 ** 2:.1f} MB for {args.tag}:")
    for name, path in uploads:
        print(f"  {name}  {sizes[name]} bytes  sha256 {shas(path)}")
    print("SHA256SUMS.txt:")
    print(sums_text(lines), end="")
    if args.dry_run:
        print("dry run: nothing sent")
        shutil.rmtree(work)
        return 0

    client = gv.Client()
    reg = gv.load_registry()
    existing = client.release_by_tag(gv.OWNER, gv.APP_REPO, args.tag)
    if existing:
        if not args.replace:
            raise PlanError(f"the release {args.tag} exists on GitVerse (id {existing.get('id')}): "
                            "--replace publishes it again")
        client.delete_release(gv.OWNER, gv.APP_REPO, existing["id"])
        gv.unregister(reg, gv.APP_REPO, args.tag)
        print(f"replaced the release {args.tag} (its files are gone)")
    target = target_for(args.tag, args.target)
    release = client.create_release(gv.OWNER, gv.APP_REPO, args.tag, target, f"Раздор {v}", notes, prerelease)
    rid = release["id"]
    gv.register(reg, gv.APP_REPO, args.tag, rid, "app", prerelease, total)
    gv.save_registry(reg)
    print(f"created the release {args.tag} (id {rid}, on {target})")

    urls = {}
    for name, path in uploads:
        asset = client.upload_asset(gv.OWNER, gv.APP_REPO, rid, name, path)
        urls[name] = asset["browser_download_url"]
        print(f"uploaded {name}")
    for name, path in uploads:
        gv.fetch_exact(urls[name], sizes[name], shas(path))
        print(f"verified {name} (anonymous, parts, sha256)")

    html_url = release.get("html_url") or f"{gv.HOST}/{gv.OWNER}/{gv.APP_REPO}/releases/tag/{args.tag}"
    # A pre-release, or --no-feed, leaves the feed and the cleanup alone.
    if prerelease or args.no_feed:
        print("update feed not changed (pre-release or --no-feed)")
    else:
        zip_urls = {g["zip"]: urls[g["zip"]] for g in p["groups"]}
        direct_urls = {name: urls[name] for name in p["direct"]}
        entry = feed_entry(args.tag, v, prerelease, notes, html_url, p["groups"], direct_urls, zip_urls,
                           urls["SHA256SUMS.txt"])
        with pages.Pages(gv.HUB_REPO) as pg:
            prev_raw = pg.read(FEED_PATH)
            prev = json.loads(prev_raw) if prev_raw else []
            data = feed_bytes(merge_feed(prev, entry, keep=2))
            pg.write(FEED_PATH, data)
            pushed = pg.commit_push(f"Update feed: {args.tag}")
            print(f"feed: {'pushed ' + pushed if pushed else 'unchanged'}")
            waited = pg.wait_served({FEED_PATH: data}, wait=args.wait)
            print(f"feed served by Pages (waited {waited:.0f} s)")
        prune(client, reg)
    if not args.keep_work:
        shutil.rmtree(work)
    print(f"published {args.tag}: {html_url}")
    return 0


def unpublish(args):
    client = gv.Client()
    reg = gv.load_registry()
    tag = args.unpublish
    rel = client.release_by_tag(gv.OWNER, gv.APP_REPO, tag)
    if rel:
        client.delete_release(gv.OWNER, gv.APP_REPO, rel["id"])
        print(f"deleted the GitVerse release {tag}")
    else:
        print(f"no GitVerse release {tag}")
    entry = next((r for r in reg["releases"] if r["repo"] == gv.APP_REPO and r["tag"] == tag), None)
    if (entry and not entry.get("prerelease")) or (not entry and gv.version_key(tag)):
        if gv.version_key(tag):
            with pages.Pages(gv.HUB_REPO) as pg:
                prev_raw = pg.read(FEED_PATH)
                prev = json.loads(prev_raw) if prev_raw else []
                feed = [e for e in prev if e.get("tag_name") != tag]
                if len(feed) != len(prev):
                    data = feed_bytes(feed)
                    pg.write(FEED_PATH, data)
                    pg.commit_push(f"Update feed: without {tag}")
                    pg.wait_served({FEED_PATH: data}, wait=args.wait)
                    print(f"removed {tag} from the update feed")
    gv.unregister(reg, gv.APP_REPO, tag)
    gv.save_registry(reg)
    if args.git_tag:
        pages.delete_git_tag(gv.APP_REPO, tag)
        print(f"deleted the git tag {tag} on GitVerse")
    return 0


def push_source(tag):
    """Pushes the release commit of `tag` to GitVerse's main, and the tag (the release skill's step: the
    commit as main and the tag, by URL). The token is in the push URL only; errors are redacted."""
    version_of(tag)
    token = gv.load_token()
    r = subprocess.run(["git", "rev-parse", "--verify", "--quiet", tag + "^{commit}"], cwd=REPO_ROOT,
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise PlanError(f"no local tag {tag}: tag the release commit first")
    sha = r.stdout.strip()
    url = f"https://{gv.OWNER}:{token}@" + pages.PLAIN_URL.format(repo=gv.APP_REPO)[len("https://"):]
    r = subprocess.run(["git", "push", url, f"{sha}:refs/heads/main", tag], cwd=REPO_ROOT,
                       capture_output=True, text=True)
    if r.returncode != 0:
        raise PlanError("the push to GitVerse failed: " + gv.redact((r.stderr or r.stdout).strip(), token))
    print(f"pushed {sha[:8]} to GitVerse main, and the tag {tag}")
    return 0


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("files", nargs="*", help="the release files (names as the GitHub pipeline gives them)")
    ap.add_argument("--tag", help="the release tag, vX.Y.Z (or vX.Y.Z-suffix with --prerelease)")
    ap.add_argument("--notes", help="a file with the release notes (Markdown); the feed's body too")
    ap.add_argument("--prerelease", action="store_true", help="a pre-release: no feed, no cleanup")
    ap.add_argument("--target", help="the commit-ish the tag points at (default: the tag's commit here, else main)")
    ap.add_argument("--replace", action="store_true", help="delete the existing release of the tag first")
    ap.add_argument("--keep", type=int, default=2, help="app releases kept on GitVerse (default 2)")
    ap.add_argument("--no-feed", action="store_true", help="GitVerse release only: no feed, no cleanup")
    ap.add_argument("--wait", type=int, default=pages.WAIT_DEFAULT, help="seconds to wait for Pages")
    ap.add_argument("--dry-run", action="store_true", help="pack and print the plan and sums; no network")
    ap.add_argument("--keep-work", action="store_true", help="keep the packed files")
    ap.add_argument("--unpublish", metavar="TAG", help="delete the GitVerse release of TAG (and its feed entry)")
    ap.add_argument("--git-tag", action="store_true", help="with --unpublish: also delete the git tag on GitVerse")
    ap.add_argument("--push-source", metavar="TAG", help="push the release commit of TAG and the tag to GitVerse")
    args = ap.parse_args(argv)
    try:
        if args.push_source:
            return push_source(args.push_source)
        if args.unpublish:
            return unpublish(args)
        if not args.tag or not args.notes:
            ap.error("--tag and --notes are required")
        if not args.files:
            ap.error("no files given")
        return publish(args)
    except gv.GitVerseError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
