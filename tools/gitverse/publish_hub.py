#!/usr/bin/env python3
"""Publishes the hub catalog to GitVerse (stdlib only; run with `python3 -I`).

  python3 -I tools/gitverse/publish_hub.py --catalog-dir DIR [--tag catalog-YYYY-MM-DD]
      [--no-pages] [--replace] [--keep 2] [--dry-run] [--wait SECONDS]
  python3 -I tools/gitverse/publish_hub.py --unpublish TAG

DIR holds index.json (tools/community-pack/build.py's output) and the flat files it names.

What it does:
  - checks every file of the catalog (its size and SHA-256 against index.json); a file whose name
    GitVerse refuses (.exe, .bin, no extension, a name over 255 characters) stops the run, listed;
  - creates the release `catalog-YYYY-MM-DD` on razdor-community and uploads the entries' files and
    the ad's picture as release assets (resumable: DIR/gitverse-state.json keeps name -> address),
    each verified anonymously by parts and SHA-256;
  - writes the GitVerse index.json into DIR/gitverse/: each uploaded file's path becomes its attachment
    address; previews and map pictures stay relative and move to previews/ (served from Pages next to
    index.json); the game keeps its GitVerse address with the GitHub address as a mirror;
  - pushes index.json, previews/ and the hub page (tools/community-pack/web/) to the razdor-community
    Pages (the docs/ folder; `--no-pages` skips it), waits until Pages serves the new index.json;
  - deletes the catalog releases older than the newest two (after the Pages push).

The game archive is not uploaded: it is the release `game-cu-1.2` already on GitVerse.
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


class HubError(gv.GitVerseError):
    pass


def load_catalog(folder):
    path = os.path.join(folder, "index.json")
    with open(path, encoding="utf-8") as f:
        cat = json.load(f)
    if not isinstance(cat, dict) or cat.get("format") != 1 or not isinstance(cat.get("entries"), list):
        raise HubError(f"{path}: not a format 1 catalog")
    return cat


def refs(cat):
    """(uploads, pictures): {file name: relative path} for the files that go to the release (the entries'
    files and the ad's picture) and for the previews and map pictures (served from Pages)."""
    uploads, pictures = {}, {}

    def add(store, rel):
        if not isinstance(rel, str) or not rel or rel.startswith("https://"):
            return
        if "/" in rel or "\\" in rel or rel.startswith("."):
            raise HubError(f"{rel}: the catalog's files are flat (a plain name)")
        store.setdefault(rel, rel)

    for e in cat["entries"]:
        for f in e.get("files") or []:
            add(uploads, f.get("path"))
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                add(pictures, ref.get("path"))
    ad = cat.get("ad")
    if isinstance(ad, dict):
        add(uploads, ad.get("image"))
    return uploads, pictures


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
        gv.check_size(path)


def gitverse_index(cat, urls):
    """The GitVerse copy of the catalog: attachment addresses for the uploaded files, previews/ for the
    pictures, the game's GitVerse address with the GitHub one as a mirror."""
    out = copy.deepcopy(cat)

    def address(rel):
        if isinstance(rel, str) and rel.startswith("https://"):
            return rel
        return urls[rel]

    for e in out["entries"]:
        for f in e.get("files") or []:
            f["path"] = address(f["path"])
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                ref["path"] = PREVIEWS + os.path.basename(ref["path"])
    ad = out.get("ad")
    if isinstance(ad, dict):
        ad["image"] = address(ad["image"])
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
    for e in idx["entries"]:
        for f in e.get("files") or []:
            if not f["path"].startswith("https://"):
                raise HubError(f"entry {e.get('id')}: a file path is not an address: {f['path']}")
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict) and not ref["path"].startswith(PREVIEWS):
                raise HubError(f"entry {e.get('id')}: {key} is not under {PREVIEWS}")


def write_stage(folder, idx, pictures):
    stage = os.path.join(folder, STAGE)
    if os.path.exists(stage):
        shutil.rmtree(stage)
    os.makedirs(os.path.join(stage, "previews"))
    with open(os.path.join(stage, "index.json"), "w", encoding="utf-8") as f:
        json.dump(idx, f, ensure_ascii=False, indent=2)
        f.write("\n")
    for name in pictures:
        shutil.copyfile(os.path.join(folder, name), os.path.join(stage, "previews", name))
    return stage


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
    uploads, pictures = refs(cat)
    refused = [(n, gv.name_problem(n)) for n in sorted(uploads) if gv.name_problem(n)]
    if refused:
        for n, why in refused:
            print(f"refused name: {n}: {why}")
        print("STOP: GitVerse refuses these names; the catalog's files must be renamed (not done here)")
        return 2
    cache = {}

    def sha_of(path):
        if path not in cache:
            cache[path] = gv.sha256_file(path)
        return cache[path]

    check_local(folder, cat, set(uploads) | set(pictures), sha_of)
    total = sum(os.path.getsize(os.path.join(folder, n)) for n in uploads)
    print(f"{tag}: {len(uploads)} files to upload ({total / 1024 ** 2:.1f} MB), "
          f"{len(pictures)} pictures for Pages, {len(cat['entries'])} entries")
    if args.dry_run:
        urls = {n: "https://gitverse.ru/api/attachments/DRY-RUN" for n in uploads}
        idx = gitverse_index(cat, urls)
        write_stage(folder, idx, pictures)
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
        gv.register(reg, gv.HUB_REPO, tag, rid, "catalog", False, total)
        gv.save_registry(reg)
        print(f"created the release {tag} (id {rid})")

    done = state.setdefault("uploaded", {})
    for name in sorted(uploads):
        if name in done:
            continue
        asset = client.upload_asset(gv.OWNER, gv.HUB_REPO, rid, name, os.path.join(folder, name))
        done[name] = {"url": asset["browser_download_url"], "size": os.path.getsize(os.path.join(folder, name)),
                      "sha256": sha_of(os.path.join(folder, name))}
        save_state(folder, state)
        print(f"uploaded {name}")
    for name in sorted(uploads):
        rec = done[name]
        if not rec.get("verified"):
            gv.fetch_exact(rec["url"], rec["size"], rec["sha256"])
            rec["verified"] = True
            save_state(folder, state)
            print(f"verified {name}")

    urls = {n: done[n]["url"] for n in uploads}
    idx = gitverse_index(cat, urls)
    stage = write_stage(folder, idx, pictures)
    with open(os.path.join(stage, "index.json"), "rb") as f:
        index_bytes = f.read()
    print(f"GitVerse index: {os.path.join(stage, 'index.json')}")
    if args.no_pages:
        print("Pages not pushed (--no-pages); the catalog's release is up")
        return 0

    with pages.Pages(gv.HUB_REPO) as pg:
        pg.clear("docs/previews")
        pg.write("docs/index.json", index_bytes)
        for name in pictures:
            with open(os.path.join(stage, "previews", name), "rb") as f:
                pg.write("docs/previews/" + name, f.read())
        for name, data in pages.web_files().items():
            pg.write("docs/" + name, data)
        pushed = pg.commit_push(f"Catalog {tag}")
        print(f"Pages: {'pushed ' + pushed if pushed else 'unchanged'}")
        expected = {"docs/index.json": index_bytes}
        if pictures:
            sample = sorted(pictures)[0]
            with open(os.path.join(stage, "previews", sample), "rb") as f:
                expected["docs/previews/" + sample] = f.read()
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
    total_mb = sum(r.get("bytes", 0) for r in reg["releases"]) / 1024 ** 2
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
    ap.add_argument("--dry-run", action="store_true", help="check and write the GitVerse index; no network")
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
