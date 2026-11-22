#!/usr/bin/env python3
"""Publishes the hub catalog to GitVerse (stdlib only; run with `python3 -I`).

  python3 -I tools/gitverse/publish_hub.py --catalog-dir DIR [--tag catalog-YYYY-MM-DD]
      [--no-pages] [--replace] [--keep 2] [--dry-run] [--wait SECONDS]
  python3 -I tools/gitverse/publish_hub.py --unpublish TAG
  python3 -I tools/gitverse/publish_hub.py --catalog-dir DIR --layout pages [--host gitverse|github] [--dry-run]

The pages layout (--layout pages, the owner's decision 2026-11-22): every hub file and the game archive are
bare Pages files `files/<sha256>.dat` on the chosen host's site (`gitverse`: docs/ of main; `github`: the root of
gh-pages), the pictures under previews/, and that host's index-v2.json names them by absolute address. No
release is made or removed, so the release copies stay as the fallback. Its routing is in publish_pages().

DIR holds index.json (tools/community-pack/build.py's output, the bare layout) and the flat files it names.

The zipped layout (the owner's decision, 2026-11-13; apps from 1.5.2 read it):
  - every file of an entry that is not an accepted archive (a map or campaign `.dtm`, any other file) is
    packed in a zip of its own, `<name>.zip` (the zip holds that one file under its catalog name), and that
    zip is a release asset of `catalog-YYYY-MM-DD` on `razdor-community`;
  - an archive file that is accepted as it is (a mod's `.zip`, `.gz`, `.tgz`, `.7z`, `.apk`) is a release
    asset as it is;
  - the pictures (previews, map pictures, the ad's picture) are the only files on the Pages site, under
    previews/ (GitVerse's Pages refuses .zip, .exe and .py; a picture is none of them). A picture the catalog
    names by an https address (the ad's picture on GitHub) is downloaded first, checked against the catalog's
    size and SHA-256, and staged under previews/ like the others; the index never names a file that is not
    staged (checked before anything is pushed);
  - the GitVerse catalog is `index-v2.json` on the Pages site (the hub page reads it first; the bare
    `index.json` is not written there). Each file names its release asset by its attachment address; a
    wrapped file has `wrapped: "zip"`, `member` (its name in the zip), and the zip's `archive_size` and
    `archive_sha256`; its `size` and `sha256` stay the file's own.
  - a file that neither can take (a name GitVerse or Pages refuses, a picture that is a zip) stops the run
    before anything is created: the list is printed.

The Pages site (the docs/ folder of razdor-community) must stay well under GitHub's and GitVerse's
500 MB: above 400 MB this warns, at 500 MB it stops. The run also writes the GitVerse index into
DIR/gitverse/ (the site's layout, so the push copies it), pushes it with the hub page
(tools/community-pack/web/) and waits until Pages serves it, then deletes the catalog releases older
than the newest two. The game archive is not uploaded: it is the release `game-cu-1.2` already there;
the game keeps its GitVerse address and the GitHub one is its mirror.

Resumable: DIR/gitverse-state.json keeps each uploaded asset's address; the zips are kept in
DIR/gitverse-zips/ (rebuilt the same way each run: a fixed zip date, so the same bytes). A run that
stopped midway goes on with the same command.
"""

import argparse
import copy
import datetime
import json
import os
import re
import shutil
import sys
import urllib.parse
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402

GAME_URL = "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5"
GAME_GITHUB_URL = "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip"
# The pages layout (the owner's decision, 2026-11-22): every hub file and the game archive are Pages files,
# served as they are (no zip) under files/<sha256>.dat, on both hosts; the release copies stay as they are.
LAYOUTS = ("zipped", "pages")
HOSTS = ("gitverse", "github")
PAGES_FILES = "files/"
PAGES_EXT = "dat"
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
REMOTE_GAME = "remote-game"  # the game archive, fetched here during publishing (checked, never in git)
GITVERSE_BASE = pages.HOSTS["gitverse"]["base"].rstrip("/")
GITHUB_BASE = pages.HOSTS["github"]["base"].rstrip("/")
PREVIEWS = "previews/"
STATE = "gitverse-state.json"
STAGE = "gitverse"
ZIPS = "gitverse-zips"
REMOTE = "remote-pictures"   # the https pictures the index rewrites, fetched here during publishing
INDEX_V2 = "index-v2.json"
ARCHIVE_EXT = frozenset({"zip", "gz", "tgz", "7z", "apk"})   # release assets as they are (GitVerse takes these)
PAGES_REFUSED_EXT = frozenset({"zip", "exe", "py"})          # Pages refuses these
RESERVED_SITE = frozenset({"index-v2.json", "index.json", "releases.json", "index.html", "hub.js", "hub.css",
                           ".nojekyll"})
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
    """Why Pages (as measured: refuses .zip, .exe, .py) cannot take this picture, or None."""
    if not gv.NAME_RE.match(name):
        return "letters, digits, _, - and . only, at most 255 characters"
    if not ext_of(name):
        return "no extension"
    if ext_of(name) in PAGES_REFUSED_EXT:
        return f"Pages refuses .{ext_of(name)} files"
    if name in RESERVED_SITE:
        return "the name is the site's own file"
    return None


def destination(name, role):
    """('release', 'wrap' or 'pages', problem or None) for a catalog file of this role. An archive file is a
    release asset as it is; any other file is wrapped in `<name>.zip` (a release asset); a picture is a
    Pages file."""
    if role == "picture":
        return "pages", pages_problem(name)
    if ext_of(name) in ARCHIVE_EXT:
        return "release", gv.name_problem(name)
    return "wrap", gv.name_problem(name + ".zip")


def plan(cat):
    """{'release': [names as they are], 'wrap': [names to zip], 'pages': {site path: name},
    'problems': [(name, why)]}. A picture's site path is under previews/."""
    release, wrap, site, problems = [], [], {}, []
    for name, role in refs(cat):
        dest, problem = destination(name, role)
        if problem:
            problems.append((name, problem))
        elif dest == "release":
            release.append(name)
        elif dest == "wrap":
            wrap.append(name)
        else:
            site[PREVIEWS + name] = name
    return {"release": sorted(release), "wrap": sorted(wrap), "pages": site, "problems": problems}


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


def build_zip(src, name, dst):
    """A zip of the file `src` holding it as `name` (the catalog name), with a fixed date, so the same
    file gives the same bytes every run."""
    tmp = dst + ".tmp"
    with open(src, "rb") as f:
        data = f.read()
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = 0o644 << 16
        z.writestr(info, data, compresslevel=9)
    os.replace(tmp, dst)


def build_zips(folder, names):
    """{catalog name: {'file': zip name, 'path': where it is, 'size', 'sha256'}} for the wrapped files."""
    zipdir = os.path.join(folder, ZIPS)
    os.makedirs(zipdir, exist_ok=True)
    out = {}
    for name in names:
        zname = name + ".zip"
        path = os.path.join(zipdir, zname)
        build_zip(os.path.join(folder, name), name, path)
        if os.path.getsize(path) > gv.ASSET_LIMIT:
            raise HubError(f"{zname}: over 100 MB, GitVerse's limit for a release asset")
        out[name] = {"file": zname, "path": path, "size": os.path.getsize(path), "sha256": gv.sha256_file(path)}
    return out


def remote_pictures(cat):
    """The pictures the catalog names by an https address (the ad's picture, a preview or a picture given as
    an address): [{'url', 'name', 'size', 'sha256'}], one per address. The index rewrites each one under
    previews/<name>, so its bytes are fetched and checked during publishing."""
    out = {}

    def add(url, size, sha, where):
        if not isinstance(url, str) or not url.startswith("https://"):
            return
        name = urllib.parse.urlsplit(url).path.rpartition("/")[2]
        problem = pages_problem(name)
        if problem:
            raise HubError(f"{where}: {url}: {problem} (the index rewrites it under {PREVIEWS})")
        if not isinstance(size, int) or size < 0 or not str(sha or "").strip():
            raise HubError(f"{where}: {url}: the catalog gives no size or SHA-256 for it")
        rec = {"url": url, "name": name, "size": size, "sha256": str(sha).lower()}
        if url in out:
            if out[url] != rec:
                raise HubError(f"{url}: named twice with different sizes or sums")
            return
        for other in out.values():
            if other["name"] == name:
                raise HubError(f"{url} and {other['url']}: both would be {PREVIEWS}{name}")
        out[url] = rec

    for e in cat["entries"]:
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                add(ref.get("path"), ref.get("size"), ref.get("sha256"), f"entry {e.get('id')}")
    ad = cat.get("ad")
    if isinstance(ad, dict):
        add(ad.get("image"), ad.get("size"), ad.get("sha256"), "the ad")
    return sorted(out.values(), key=lambda r: r["name"])


def fetch_remote_pictures(folder, cat, local_site):
    """Downloads the https pictures into DIR/remote-pictures/ (a copy that already matches is kept) and checks
    each against the catalog's size and SHA-256. Returns {site path: DIR-relative name} for the Pages site.
    A name that a local picture of the site also has is a stop."""
    got = {}
    for rec in remote_pictures(cat):
        site_path = PREVIEWS + rec["name"]
        if site_path in local_site:
            raise HubError(f"{rec['url']}: its name {rec['name']} is also a local picture")
        dst = os.path.join(folder, REMOTE, rec["name"])
        if not (os.path.isfile(dst) and os.path.getsize(dst) == rec["size"] and gv.sha256_file(dst) == rec["sha256"]):
            data = gv.fetch_bytes(rec["url"])
            if len(data) != rec["size"]:
                raise HubError(f"{rec['url']}: {len(data)} bytes, the catalog says {rec['size']}")
            if gv.sha256_bytes(data) != rec["sha256"]:
                raise HubError(f"{rec['url']}: SHA-256 differs from the catalog's")
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            with open(dst + ".tmp", "wb") as f:
                f.write(data)
            os.replace(dst + ".tmp", dst)
        got[site_path] = REMOTE + "/" + rec["name"]
        print(f"picture {rec['url']}: {rec['size']} bytes, SHA-256 as the catalog says")
    return got


def relative_paths(node):
    """Every relative path the index names: a `path` or `image` value that is not an address."""
    if isinstance(node, dict):
        for k, v in node.items():
            if k in ("path", "image") and isinstance(v, str):
                if not v.startswith(("https://", "http://")):
                    yield v
            else:
                yield from relative_paths(v)
    elif isinstance(node, list):
        for v in node:
            yield from relative_paths(v)


def check_stage(stage, idx):
    """Stops (HubError) unless every relative path the index names is a file of the staged site."""
    named = sorted(set(relative_paths(idx)))
    missing = [p for p in named if ".." in p.split("/") or not os.path.isfile(os.path.join(stage, *p.split("/")))]
    if missing:
        raise HubError(f"index-v2.json names {', '.join(missing)}, not in the staged site: nothing is pushed")
    return named


def gitverse_index(cat, links):
    """The GitVerse copy of the catalog (index-v2.json). links: catalog name -> {'url', 'size', 'sha256'} of
    the release asset that carries it (the zip of a wrapped file, the file itself for an archive). A
    wrapped file keeps its own size and SHA-256 and names its zip (`archive_size`, `archive_sha256`) and its
    member (its catalog name). A picture is under previews/, relative; the game keeps its GitVerse address
    with the GitHub one as a mirror."""
    out = copy.deepcopy(cat)
    for e in out["entries"]:
        for f in e.get("files") or []:
            name = f["path"]
            if name.startswith("https://"):
                continue
            ln = links[name]
            f["path"] = ln["url"]
            if ext_of(name) not in ARCHIVE_EXT:
                f["wrapped"] = "zip"
                f["member"] = name
                f["archive_size"] = ln["size"]
                f["archive_sha256"] = ln["sha256"]
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
    """Every file is an address (its release asset); a wrapped file has its zip's fields and no others are
    written; a picture is under previews/."""
    for e in idx["entries"]:
        for f in e.get("files") or []:
            if not str(f.get("path", "")).startswith("https://"):
                raise HubError(f"entry {e.get('id')}: {f.get('path')} must be an address (its release asset)")
            if "wrapped" in f:
                ok = (f["wrapped"] == "zip" and f.get("member") and isinstance(f.get("archive_size"), int)
                      and len(str(f.get("archive_sha256", ""))) == 64)
                if not ok:
                    raise HubError(f"entry {e.get('id')}: {f.get('member')}: the zip's fields are wrong")
            elif any(k in f for k in ("member", "archive_size", "archive_sha256")):
                raise HubError(f"entry {e.get('id')}: the zip's fields without \"wrapped\"")
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict) and not ref["path"].startswith(PREVIEWS):
                raise HubError(f"entry {e.get('id')}: {key} is not under {PREVIEWS}")
    ad = idx.get("ad")
    if isinstance(ad, dict) and not ad["image"].startswith(PREVIEWS):
        raise HubError(f"the ad's picture is not under {PREVIEWS}")


def write_stage(folder, idx, site, name=STAGE):
    """DIR/<name> (default gitverse/) in the site's own layout: index-v2.json, the pictures in previews/ (and
    the pages layout's files in files/). Returns the folder."""
    stage = os.path.join(folder, name)
    if os.path.exists(stage):
        shutil.rmtree(stage)
    os.makedirs(stage)
    with open(os.path.join(stage, INDEX_V2), "w", encoding="utf-8") as f:
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


def file_shas(cat):
    """{catalog file name: its SHA-256 (lower case)} for the files that are local (not addresses)."""
    out = {}
    for e in cat["entries"]:
        for f in e.get("files") or []:
            p = f.get("path")
            if not isinstance(p, str) or p.startswith("https://"):
                continue
            digest = str(f.get("sha256", "")).lower()
            if out.get(p, digest) != digest:
                raise HubError(f"{p}: named twice with different SHA-256s")
            out[p] = digest
    return out


def pages_site_path(sha):
    """The Pages file of a hub file or the game, by its SHA-256: `files/<sha256>.dat`. The name is no `.zip`,
    `.exe` or `.py` (Pages refuses those), and the bytes are the file itself, so an app that reads a bare
    catalog entry downloads it as it is."""
    digest = str(sha or "").lower()
    if not SHA_RE.match(digest):
        raise HubError(f"{sha!r}: not a SHA-256")
    return PAGES_FILES + digest + "." + PAGES_EXT


def pages_routing(cat):
    """{'files': {catalog name: site path}, 'site': {site path: name in DIR}, 'problems': [(name, why)]}.
    A hub file is a Pages file under files/<sha256>.dat (two names with the same bytes share one); a picture
    is under previews/ as in the zipped layout."""
    shas = file_shas(cat)
    files, site, problems = {}, {}, []
    for name, role in refs(cat):
        if role == "file":
            try:
                where = pages_site_path(shas.get(name))
            except HubError as e:
                problems.append((name, str(e)))
                continue
            files[name] = where
            site.setdefault(where, name)
        else:
            why = pages_problem(name)
            if why:
                problems.append((name, why))
            else:
                site[PREVIEWS + name] = name
    return {"files": files, "site": site, "problems": problems}


def fetch_checked(folder, local, url, size, sha):
    """Downloads the https `url` to DIR/local unless a copy there already has the catalog's size and SHA-256;
    a download is checked against them before it is kept. Returns the path."""
    if not str(url).startswith("https://"):
        raise HubError(f"{url}: not an https address")
    dst = os.path.join(folder, local)
    digest = str(sha).lower()
    if os.path.isfile(dst) and os.path.getsize(dst) == size and gv.sha256_file(dst) == digest:
        return dst
    data = gv.fetch_bytes(url)
    if len(data) != size:
        raise HubError(f"{url}: {len(data)} bytes, the catalog says {size}")
    if gv.sha256_bytes(data) != digest:
        raise HubError(f"{url}: SHA-256 differs from the catalog's")
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    with open(dst + ".tmp", "wb") as f:
        f.write(data)
    os.replace(dst + ".tmp", dst)
    print(f"fetched {url}: {size} bytes, SHA-256 as the catalog says")
    return dst


def fetch_game(folder, cat):
    """The game archive (the catalog's `game`): (its Pages site path, its path under DIR)."""
    g = cat.get("game")
    if not isinstance(g, dict) or not isinstance(g.get("name"), str) or not gv.NAME_RE.match(g["name"]):
        raise HubError("the catalog's game has no usable file name")
    fetch_checked(folder, os.path.join(REMOTE_GAME, g["name"]), g.get("url"), g.get("size"), g.get("sha256"))
    return pages_site_path(g.get("sha256")), REMOTE_GAME + "/" + g["name"]


def pages_index(cat, base, game_mirrors):
    """The Pages copy of the catalog for the host at `base`: a file's address is `base/files/<sha256>.dat` (its
    own size and SHA-256; bare, no wrapping); pictures are relative under previews/; the game's address is its
    Pages file, with `game_mirrors` after it."""
    out = copy.deepcopy(cat)
    for e in out["entries"]:
        for f in e.get("files") or []:
            if not f["path"].startswith("https://"):
                f["path"] = base + "/" + pages_site_path(f["sha256"])
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                ref["path"] = PREVIEWS + os.path.basename(ref["path"])
    ad = out.get("ad")
    if isinstance(ad, dict):
        ad["image"] = PREVIEWS + os.path.basename(ad["image"])
    game = out.get("game")
    if isinstance(game, dict):
        url = base + "/" + pages_site_path(game["sha256"])
        mirrors = []
        for m in game_mirrors:
            if m != url and m not in mirrors:
                mirrors.append(m)
        game["url"] = url
        game["mirrors"] = mirrors
    check_index(out)
    check_pages_index(out, base)
    return out


def check_pages_index(idx, base):
    """Every hub file and the game is a bare Pages file under `base/files/`: no zip fields."""
    want = base + "/" + PAGES_FILES
    for e in idx["entries"]:
        for f in e.get("files") or []:
            p = str(f.get("path", ""))
            if not p.startswith(want) or not p.endswith("." + PAGES_EXT):
                raise HubError(f"entry {e.get('id')}: {f.get('name')}: not a Pages file under {want}")
            if any(k in f for k in ("wrapped", "member", "archive_size", "archive_sha256")):
                raise HubError(f"entry {e.get('id')}: {f.get('name')}: the pages layout has no zip fields")
    game = idx.get("game")
    if isinstance(game, dict) and not str(game.get("url", "")).startswith(want):
        raise HubError("the game's address is not a Pages file")


def check_pages_stage(stage, idx, base, site):
    """Every Pages address the index names is a staged file, and every site file is staged."""
    named = set()
    for e in idx["entries"]:
        for f in e.get("files") or []:
            named.add(str(f["path"])[len(base) + 1:])
    if isinstance(idx.get("game"), dict):
        named.add(str(idx["game"]["url"])[len(base) + 1:])
    missing = sorted(n for n in named if n not in site)
    if missing:
        raise HubError(f"the index names {', '.join(missing)}, which the site does not hold: nothing is pushed")
    for rel in sorted(site):
        if not os.path.isfile(os.path.join(stage, *rel.split("/"))):
            raise HubError(f"{rel} is not in the staged site: nothing is pushed")


def publish_pages(args, folder, cat, host):
    """The pages layout: every hub file, the pictures and the game archive are Pages files of `host` (GitVerse's
    site, or GitHub's gh-pages site), and that host's catalog copy names them. No release is made or removed:
    the release copies stay as they are. A dry run checks the routing and the sizes, and writes the index."""
    base = GITVERSE_BASE if host == "gitverse" else GITHUB_BASE
    rt = pages_routing(cat)
    if rt["problems"]:
        for name, why in rt["problems"]:
            print(f"refused: {name}: {why}")
        print("STOP: these files have no place on the Pages site; nothing was fetched or pushed.")
        return 2
    site = dict(rt["site"])
    game_where, game_local = fetch_game(folder, cat)
    site[game_where] = game_local
    site.update(fetch_remote_pictures(folder, cat, site))
    names = {n for n in set(rt["files"]) | set(site.values()) if "/" not in n}
    check_local(folder, cat, names, gv.sha256_file)
    sizes = {where: os.path.getsize(os.path.join(folder, local)) for where, local in site.items()}
    files_bytes = sum(s for w, s in sizes.items() if w.startswith(PAGES_FILES) and w != game_where)
    game_bytes = sizes[game_where]
    pics_bytes = sum(s for w, s in sizes.items() if w.startswith(PREVIEWS))
    web = pages.web_files()
    if host == "gitverse":
        mirrors = [GAME_URL, GAME_GITHUB_URL]
    else:
        mirrors = [GITVERSE_BASE + "/" + game_where, GAME_URL]
    idx = pages_index(cat, base, mirrors)
    stage = write_stage(folder, idx, site, name="pages-" + host)
    check_stage(stage, idx)
    check_pages_stage(stage, idx, base, site)
    index_size = os.path.getsize(os.path.join(stage, INDEX_V2))
    site_bytes = sum(sizes.values()) + sum(len(d) for d in web.values()) + index_size
    verdict = pages_size_verdict(site_bytes)
    print(f"{host} (pages layout): {len(rt['files'])} hub files as Pages files under {PAGES_FILES} "
          f"({files_bytes / MB:.1f} MB), {sum(1 for w in sizes if w.startswith(PREVIEWS))} pictures "
          f"({pics_bytes / MB:.1f} MB), the game ({game_bytes / MB:.1f} MB); the site {site_bytes / MB:.1f} MB "
          f"with the hub page and the index; {len(cat['entries'])} entries")
    if verdict == "stop":
        print(f"STOP: the Pages site would be {site_bytes / MB:.0f} MB (the limit is 500 MB)")
        return 2
    if verdict == "warn":
        print(f"WARNING: the Pages site is {site_bytes / MB:.0f} MB (above 400 MB; the limit is 500 MB)")
    if args.dry_run:
        print(f"dry run: nothing sent; the {host} index is in {os.path.join(stage, INDEX_V2)}")
        return 0
    if args.no_pages:
        print(f"Pages not pushed (--no-pages); the {host} site is staged in {stage}")
        return 0
    for name, data in web.items():
        with open(os.path.join(stage, name), "wb") as f:
            f.write(data)
    with open(os.path.join(stage, INDEX_V2), "rb") as f:
        index_bytes = f.read()
    sample = min(site, key=lambda w: sizes[w])
    with pages.Pages(gv.HUB_REPO, host=host) as pg:
        pg.sync_site_dir(stage)
        pushed = pg.commit_push(f"Hub catalog, pages layout ({datetime.date.today().isoformat()})")
        print(f"{host} Pages: {'pushed ' + pushed if pushed else 'unchanged'}")
        with open(os.path.join(stage, sample), "rb") as f:
            sample_bytes = f.read()
        expected = {pg._in_repo(INDEX_V2): index_bytes, pg._in_repo(sample): sample_bytes}
        waited = pg.wait_served(expected, wait=args.wait)
        print(f"{host} Pages serves the new index and a sample file (waited {waited:.0f} s)")
    print(f"published the {host} pages layout: {base}/{INDEX_V2}")
    return 0


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
    layout = getattr(args, "layout", "zipped")   # the zipped layout's callers build their own args
    host = getattr(args, "host", "gitverse")
    if layout == "pages":
        return publish_pages(args, folder, cat, host)
    if host != "gitverse":
        raise HubError("--host github is for the pages layout only (the zipped layout is GitVerse's release)")
    state = load_state(folder)
    tag = args.tag or state.get("tag") or "catalog-" + datetime.date.today().isoformat()
    pl = plan(cat)
    if pl["problems"]:
        for name, why in pl["problems"]:
            print(f"refused: {name}: {why}")
        print("STOP: these files have no place (neither a release asset nor the Pages site); "
              "nothing was created. The catalog's files must be changed first (not done here).")
        return 2
    release, wrap, site = pl["release"], pl["wrap"], dict(pl["pages"])
    site.update(fetch_remote_pictures(folder, cat, site))
    cache = {}

    def sha_of(path):
        if path not in cache:
            cache[path] = gv.sha256_file(path)
        return cache[path]

    check_local(folder, cat, set(release) | set(wrap) | set(site.values()), sha_of)
    for name in release:
        if os.path.getsize(os.path.join(folder, name)) > gv.ASSET_LIMIT:
            raise HubError(f"{name}: over 100 MB, GitVerse's limit for a release asset")
    zips = build_zips(folder, wrap)

    # each release asset: its name on GitVerse, where it is, its size and SHA-256
    assets = []
    for name in release:
        path = os.path.join(folder, name)
        assets.append({"name": name, "catalog": name, "path": path, "size": os.path.getsize(path), "sha256": sha_of(path)})
    for name in wrap:
        z = zips[name]
        assets.append({"name": z["file"], "catalog": name, "path": z["path"], "size": z["size"], "sha256": z["sha256"]})
    release_bytes = sum(a["size"] for a in assets)

    # the Pages site: the pictures once each, the hub page, and the index (its source's size stands in for the
    # GitVerse copy, which differs only in its addresses)
    site_bytes = sum(os.path.getsize(os.path.join(folder, n)) for n in set(site.values()))
    site_bytes += sum(len(data) for data in pages.web_files().values())
    site_bytes += os.path.getsize(os.path.join(folder, "index.json"))
    verdict = pages_size_verdict(site_bytes)
    print(f"{tag}: {len(release)} release assets as they are, {len(wrap)} files zipped "
          f"({release_bytes / MB:.1f} MB in the release), {len(site)} Pages pictures "
          f"({site_bytes / MB:.1f} MB), {len(cat['entries'])} entries")
    if verdict == "stop":
        print(f"STOP: the Pages site would be {site_bytes / MB:.0f} MB (the limit is 500 MB)")
        return 2
    if verdict == "warn":
        print(f"WARNING: the Pages site is {site_bytes / MB:.0f} MB (above 400 MB; the limit is 500 MB)")

    if args.dry_run:
        links = {a["catalog"]: {"url": "https://gitverse.ru/api/attachments/DRY-RUN", "size": a["size"],
                                "sha256": a["sha256"]} for a in assets}
        idx = gitverse_index(cat, links)
        stage = write_stage(folder, idx, site)
        check_stage(stage, idx)
        print("dry run: nothing sent; the GitVerse index is in", os.path.join(folder, STAGE, INDEX_V2))
        return 0

    client = gv.Client()
    reg = gv.load_registry()
    if state.get("tag") == tag and state.get("release_id"):
        rid = state["release_id"]
        print(f"resuming {tag} (release id {rid}; {len(state.get('uploaded', {}))} assets already up)")
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
    for a in assets:
        if a["name"] in done:
            if done[a["name"]]["sha256"] != a["sha256"]:
                raise HubError(f"{a['name']} changed since it was uploaded: --replace starts the release again")
            continue
        asset = client.upload_asset(gv.OWNER, gv.HUB_REPO, rid, a["name"], a["path"])
        done[a["name"]] = {"url": asset["browser_download_url"], "size": a["size"], "sha256": a["sha256"]}
        save_state(folder, state)
        print(f"uploaded {a['name']}")
    for a in assets:
        rec = done[a["name"]]
        if not rec.get("verified"):
            gv.fetch_exact(rec["url"], rec["size"], rec["sha256"])
            rec["verified"] = True
            save_state(folder, state)
            print(f"verified {a['name']}")

    links = {a["catalog"]: {"url": done[a["name"]]["url"], "size": a["size"], "sha256": a["sha256"]} for a in assets}
    idx = gitverse_index(cat, links)
    stage = write_stage(folder, idx, site)
    check_stage(stage, idx)
    with open(os.path.join(stage, INDEX_V2), "rb") as f:
        index_bytes = f.read()
    print(f"GitVerse index: {os.path.join(stage, INDEX_V2)}")
    if args.no_pages:
        print("Pages not pushed (--no-pages); the catalog's release is up")
        return 0

    site_files = stage_files(stage)
    site_files.update(pages.web_files())
    with pages.Pages(gv.HUB_REPO) as pg:
        pg.sync_site(site_files)
        pushed = pg.commit_push(f"Catalog {tag}")
        print(f"Pages: {'pushed ' + pushed if pushed else 'unchanged'}")
        expected = {"docs/" + INDEX_V2: index_bytes}
        fetched = [rel for rel in sorted(site) if site[rel].startswith(REMOTE + "/")]
        for rel in sorted(site)[:1] + fetched:
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
    ap.add_argument("--dry-run", action="store_true",
                    help="check, zip, route and write the GitVerse index; nothing sent (the https pictures are read)")
    ap.add_argument("--layout", choices=LAYOUTS, default="zipped",
                    help="zipped (default): the release catalog of the 2026-11-13 design; pages: every hub file and "
                         "the game as Pages files under files/ (no release is made or removed)")
    ap.add_argument("--host", choices=HOSTS, default="gitverse",
                    help="pages layout only: the Pages site to write (gitverse: docs/ of main; github: gh-pages)")
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
