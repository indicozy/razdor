#!/usr/bin/env python3
"""Publishes the hub catalog to GitVerse (stdlib only; run with `python3 -I`).

  python3 -I tools/gitverse/publish_hub.py --catalog-dir DIR [--tag catalog-YYYY-MM-DD]
      [--no-pages] [--replace] [--keep 2] [--dry-run] [--wait SECONDS]
  python3 -I tools/gitverse/publish_hub.py --unpublish TAG

DIR holds index.json (tools/community-pack/build.py's output) and the flat files it names.

Where each file goes (by its extension; the catalog's names are `<sha256 first 16>.<ext>`):
  - an archive (.zip, .gz, .tgz, .7z, .apk) that is a file of an entry (a mod's zip, an archive map):
    a release asset on `razdor-community` (GitVerse takes these); the index names it by its address;
  - anything else (.dtm maps and campaigns, every picture: previews, map pictures, the ad's picture)
    goes to the Pages site: a file at its catalog path (relative, unchanged in the index), a picture
    under previews/. GitVerse refuses .dtm as a release asset; Pages refuses only .zip, .exe and .py;
  - a file that neither can take (a refused name, an archive on Pages, a clash with the site's own
    files) stops the run before anything is created: the list is printed.

The Pages site (the docs/ folder of razdor-community) must stay well under GitHub's and GitVerse's
500 MB: above 400 MB this warns, at 500 MB it stops. The run also writes the GitVerse index.json into
DIR/gitverse/ (the site's layout, so the push copies it), pushes it with the hub page
(tools/community-pack/web/) and waits until Pages serves it, then deletes the catalog releases older
than the newest two. The game archive is not uploaded: it is the release `game-cu-1.2` already there;
the game keeps its GitVerse address and the GitHub one is its mirror.

Resumable: DIR/gitverse-state.json keeps each uploaded file's address; a run that stopped midway goes
on with the same command.
"""

import argparse
import copy
import datetime
import json
import os
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402

GAME_URL = "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5"
PREVIEWS = "previews/"
STATE = "gitverse-state.json"
STAGE = "gitverse"
ARCHIVE_EXT = frozenset({"zip", "gz", "tgz", "7z", "apk"})   # release assets (GitVerse takes these)
PAGES_REFUSED_EXT = frozenset({"zip", "exe", "py"})          # Pages refuses these
RESERVED_SITE = frozenset({"index.json", "releases.json", "index.html", "hub.js", "hub.css", ".nojekyll"})
MB = 1024 * 1024
PAGES_WARN = 400 * MB
PAGES_LIMIT = 500 * MB


class HubError(gv.GitVerseError):
    pass


def load_catalog(folder):
    path = os.path.join(folder, "index.json")
    with open(path, encoding="utf-8") as f:
        cat = json.load(f)
    if not isinstance(cat, dict) or cat.get("format") != 1 or not isinstance(cat.get("entries"), list):
        raise HubError(f"{path}: not a format 1 catalog")
    return cat


def ext_of(name):
    return name.rpartition(".")[2].lower() if "." in name else ""


def refs(cat):
    """[(name, role)] of the catalog's files: role "file" (an entry's file) or "picture" (a preview, a map
    picture, the ad's picture). Remote addresses are not files."""
    out = set()

    def add(rel, role):
        if not isinstance(rel, str) or not rel or rel.startswith("https://"):
            return
        if "/" in rel or "\\" in rel or rel.startswith("."):
            raise HubError(f"{rel}: the catalog's files are flat (a plain name)")
        out.add((rel, role))

    for e in cat["entries"]:
        for f in e.get("files") or []:
            add(f.get("path"), "file")
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                add(ref.get("path"), "picture")
    ad = cat.get("ad")
    if isinstance(ad, dict):
        add(ad.get("image"), "picture")
    return sorted(out)


def pages_problem(name):
    """Why Pages (as measured: refuses .zip, .exe, .py) cannot take this file, or None."""
    if not gv.NAME_RE.match(name):
        return "letters, digits, _, - and . only, at most 255 characters"
    ext = ext_of(name)
    if not ext:
        return "no extension"
    if ext in PAGES_REFUSED_EXT:
        return f"Pages refuses .{ext} files"
    if name in RESERVED_SITE:
        return "the name is the site's own file"
    return None


def destination(name, role):
    """('release' or 'pages', problem or None) for a catalog file of this role."""
    if role == "file" and ext_of(name) in ARCHIVE_EXT:
        return "release", gv.name_problem(name)
    return "pages", pages_problem(name)


def plan(cat):
    """{'release': [names], 'pages': {site path: name}, 'problems': [(name, why)]}. A picture's site path is
    under previews/, a file's is its name at the site root."""
    release, site, problems = [], {}, []
    for name, role in refs(cat):
        dest, problem = destination(name, role)
        if problem:
            problems.append((name, problem))
        elif dest == "release":
            release.append(name)
        else:
            site[name if role == "file" else PREVIEWS + name] = name
    return {"release": sorted(set(release)), "pages": site, "problems": problems}


def pages_size_verdict(total):
    """'ok', 'warn' (above 400 MB) or 'stop' (500 MB or more) for the Pages site's total bytes."""
    if total >= PAGES_LIMIT:
        return "stop"
    if total > PAGES_WARN:
        return "warn"
    return "ok"


def check_local(folder, cat, names, sha_of):
    """Each named file must be in the folder with the size and SHA-256 the catalog gives it."""
    want = {}
    for e in cat["entries"]:
        for ref in list(e.get("files") or []) + [e.get("preview"), e.get("picture")]:
            if isinstance(ref, dict) and ref.get("path") in names:
                want[ref["path"]] = (ref.get("size"), ref.get("sha256"))
    ad = cat.get("ad")
    if isinstance(ad, dict) and ad.get("image") in names:
        want[ad["image"]] = (ad.get("size"), ad.get("sha256"))
    for name in sorted(names):
        path = os.path.join(folder, name)
        if not os.path.isfile(path):
            raise HubError(f"{name} is named by index.json but missing from {folder}")
        size, digest = want.get(name, (None, None))
        if size is not None and os.path.getsize(path) != size:
            raise HubError(f"{name}: {os.path.getsize(path)} bytes, index.json says {size}")
        if digest and sha_of(path) != str(digest).lower():
            raise HubError(f"{name}: SHA-256 differs from index.json")
        if os.path.getsize(path) > gv.ASSET_LIMIT and ext_of(name) in ARCHIVE_EXT:
            raise HubError(f"{name}: over 100 MB, GitVerse's limit for a release asset")


def gitverse_index(cat, urls):
    """The GitVerse copy of the catalog: an archive file's attachment address (urls: name -> address), a
    Pages file's path unchanged (relative to index.json), pictures under previews/, the game's GitVerse
    address with the GitHub one as a mirror."""
    out = copy.deepcopy(cat)

    def file_path(rel):
        if isinstance(rel, str) and rel.startswith("https://"):
            return rel
        if ext_of(rel) in ARCHIVE_EXT:
            return urls[rel]
        return rel

    for e in out["entries"]:
        for f in e.get("files") or []:
            f["path"] = file_path(f["path"])
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                ref["path"] = PREVIEWS + os.path.basename(ref["path"])
    ad = out.get("ad")
    if isinstance(ad, dict):
        ad["image"] = PREVIEWS + os.path.basename(ad["image"])
    game = out.get("game")
    if isinstance(game, dict):
        mirrors = []
        original = game.get("url", "")
        if original.startswith("https://") and original != GAME_URL:
            mirrors.append(original)
        for m in game.get("mirrors") or []:
            if m not in mirrors and m != GAME_URL:
                mirrors.append(m)
        game["url"] = GAME_URL
        game["mirrors"] = mirrors
    check_index(out)
    return out


def check_index(idx):
    """A file is an address when it is an archive (a release asset), else a plain name at the site root;
    a picture is under previews/."""
    for e in idx["entries"]:
        for f in e.get("files") or []:
            p = f["path"]
            if p.startswith("https://"):
                continue
            if ext_of(p) in ARCHIVE_EXT or "/" in p:
                raise HubError(f"entry {e.get('id')}: {p} must be an address (an archive) or a plain site name")
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict) and not ref["path"].startswith(PREVIEWS):
                raise HubError(f"entry {e.get('id')}: {key} is not under {PREVIEWS}")
    ad = idx.get("ad")
    if isinstance(ad, dict) and not ad["image"].startswith(PREVIEWS):
        raise HubError(f"the ad's picture is not under {PREVIEWS}")


def write_stage(folder, idx, site):
    """DIR/gitverse/ in the site's own layout: index.json, the Pages files at their paths, the pictures in
    previews/. Returns the folder."""
    stage = os.path.join(folder, STAGE)
    if os.path.exists(stage):
        shutil.rmtree(stage)
    os.makedirs(stage)
    with open(os.path.join(stage, "index.json"), "w", encoding="utf-8") as f:
        json.dump(idx, f, ensure_ascii=False, indent=2)
        f.write("\n")
    for rel, name in sorted(site.items()):
        dst = os.path.join(stage, rel)
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copyfile(os.path.join(folder, name), dst)
    return stage


def stage_files(stage):
    """{site path: bytes} of every file the stage holds."""
    out = {}
    for root, _, names in os.walk(stage):
        for n in names:
            full = os.path.join(root, n)
            rel = os.path.relpath(full, stage).replace(os.sep, "/")
            with open(full, "rb") as f:
                out[rel] = f.read()
    return out


def load_state(folder):
    path = os.path.join(folder, STATE)
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            return json.load(f)
    return {}


def save_state(folder, state):
    gv.write_json_atomic(os.path.join(folder, STATE), state)


def publish(args):
    folder = os.path.abspath(args.catalog_dir)
    cat = load_catalog(folder)
    state = load_state(folder)
    tag = args.tag or state.get("tag") or "catalog-" + datetime.date.today().isoformat()
    pl = plan(cat)
    if pl["problems"]:
        for name, why in pl["problems"]:
            print(f"refused: {name}: {why}")
        print("STOP: these files have no place (neither a release asset nor the Pages site); "
              "nothing was created. The catalog's files must be changed first (not done here).")
        return 2
    uploads, site = pl["release"], pl["pages"]
    cache = {}

    def sha_of(path):
        if path not in cache:
            cache[path] = gv.sha256_file(path)
        return cache[path]

    check_local(folder, cat, set(uploads) | set(site.values()), sha_of)
    release_bytes = sum(os.path.getsize(os.path.join(folder, n)) for n in uploads)
    # the Pages site: the files once each, the hub page, and the index (the source's size stands in for
    # the GitVerse copy, which differs only in its addresses)
    site_bytes = sum(os.path.getsize(os.path.join(folder, n)) for n in set(site.values()))
    site_bytes += sum(len(data) for data in pages.web_files().values())
    site_bytes += os.path.getsize(os.path.join(folder, "index.json"))
    verdict = pages_size_verdict(site_bytes)
    print(f"{tag}: {len(uploads)} release assets ({release_bytes / MB:.1f} MB), "
          f"{len(site)} Pages files ({site_bytes / MB:.1f} MB), {len(cat['entries'])} entries")
    if verdict == "stop":
        print(f"STOP: the Pages site would be {site_bytes / MB:.0f} MB (the limit is 500 MB)")
        return 2
    if verdict == "warn":
        print(f"WARNING: the Pages site is {site_bytes / MB:.0f} MB (above 400 MB; the limit is 500 MB)")

    if args.dry_run:
        urls = {n: "https://gitverse.ru/api/attachments/DRY-RUN" for n in uploads}
        idx = gitverse_index(cat, urls)
        write_stage(folder, idx, site)
        print("dry run: nothing sent; the GitVerse index is in", os.path.join(folder, STAGE, "index.json"))
        return 0

    client = gv.Client()
    reg = gv.load_registry()
    if state.get("tag") == tag and state.get("release_id"):
        rid = state["release_id"]
        print(f"resuming {tag} (release id {rid}; {len(state.get('uploaded', {}))} files already up)")
    else:
        existing = client.release_by_tag(gv.OWNER, gv.HUB_REPO, tag)
        if existing:
            if not args.replace:
                raise HubError(f"the release {tag} exists on {gv.HUB_REPO} (id {existing.get('id')}): "
                               "--replace publishes it again, or pick another --tag")
            client.delete_release(gv.OWNER, gv.HUB_REPO, existing["id"])
            gv.unregister(reg, gv.HUB_REPO, tag)
            print(f"replaced the release {tag}")
        rel = client.create_release(gv.OWNER, gv.HUB_REPO, tag, "main", tag, "", False)
        rid = rel["id"]
        state = {"tag": tag, "release_id": rid, "uploaded": {}}
        save_state(folder, state)
        gv.register(reg, gv.HUB_REPO, tag, rid, "catalog", False, release_bytes)
        gv.save_registry(reg)
        print(f"created the release {tag} (id {rid})")

    done = state.setdefault("uploaded", {})
    for name in uploads:
        if name in done:
            continue
        path = os.path.join(folder, name)
        asset = client.upload_asset(gv.OWNER, gv.HUB_REPO, rid, name, path)
        done[name] = {"url": asset["browser_download_url"], "size": os.path.getsize(path),
                      "sha256": sha_of(path)}
        save_state(folder, state)
        print(f"uploaded {name}")
    for name in uploads:
        rec = done[name]
        if not rec.get("verified"):
            gv.fetch_exact(rec["url"], rec["size"], rec["sha256"])
            rec["verified"] = True
            save_state(folder, state)
            print(f"verified {name}")

    urls = {n: done[n]["url"] for n in uploads}
    idx = gitverse_index(cat, urls)
    stage = write_stage(folder, idx, site)
    with open(os.path.join(stage, "index.json"), "rb") as f:
        index_bytes = f.read()
    print(f"GitVerse index: {os.path.join(stage, 'index.json')}")
    if args.no_pages:
        print("Pages not pushed (--no-pages); the catalog's release is up")
        return 0

    site_files = stage_files(stage)
    site_files.update(pages.web_files())
    with pages.Pages(gv.HUB_REPO) as pg:
        pg.sync_site(site_files)
        pushed = pg.commit_push(f"Catalog {tag}")
        print(f"Pages: {'pushed ' + pushed if pushed else 'unchanged'}")
        sample = sorted(p for p in site if p in site_files)[:1]
        expected = {"docs/index.json": index_bytes}
        for rel in sample:
            expected["docs/" + rel] = site_files[rel]
        waited = pg.wait_served(expected, wait=args.wait)
        print(f"Pages serves the new catalog (waited {waited:.0f} s)")

    for entry in gv.select_prune_catalogs(reg["releases"], keep=args.keep):
        try:
            if entry.get("id"):
                client.delete_release(gv.OWNER, gv.HUB_REPO, entry["id"])
            else:
                client.delete_release_by_tag(gv.OWNER, gv.HUB_REPO, entry["tag"])
        except gv.ApiError as e:
            if e.status != 404:
                raise
        gv.unregister(reg, gv.HUB_REPO, entry["tag"])
        print(f"deleted the older catalog release {entry['tag']}")
    gv.save_registry(reg)
    total_mb = sum(r.get("bytes", 0) for r in reg["releases"]) / MB
    print(f"release files this script made: {total_mb:.0f} MB (budget 4 GB for both repos)")
    print(f"published {tag}: https://gitverse.ru/{gv.OWNER}/{gv.HUB_REPO}/releases")
    return 0


def unpublish(tag):
    """Deletes the release `tag` of razdor-community (its files go with it) and its registry entry."""
    client = gv.Client()
    reg = gv.load_registry()
    try:
        client.delete_release_by_tag(gv.OWNER, gv.HUB_REPO, tag)
        print(f"deleted the release {tag} on {gv.HUB_REPO}")
    except gv.ApiError as e:
        if e.status != 404:
            raise
        print(f"no release {tag} on {gv.HUB_REPO}")
    gv.unregister(reg, gv.HUB_REPO, tag)
    gv.save_registry(reg)
    return 0


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--unpublish", metavar="TAG", help="delete the catalog release TAG (no other work)")
    ap.add_argument("--catalog-dir", help="the folder with index.json and its files")
    ap.add_argument("--tag", help="the release tag (default: the state file's, else catalog-YYYY-MM-DD)")
    ap.add_argument("--no-pages", action="store_true", help="upload only: no Pages push, no cleanup")
    ap.add_argument("--replace", action="store_true", help="delete the existing release of the tag first")
    ap.add_argument("--keep", type=int, default=2, help="catalog releases kept (default 2)")
    ap.add_argument("--wait", type=int, default=pages.WAIT_DEFAULT, help="seconds to wait for Pages")
    ap.add_argument("--dry-run", action="store_true", help="check, route and write the GitVerse index; no network")
    args = ap.parse_args(argv)
    try:
        if args.unpublish:
            return unpublish(args.unpublish)
        if not args.catalog_dir:
            ap.error("--catalog-dir is required")
        return publish(args)
    except gv.GitVerseError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
