#!/usr/bin/env python3
"""Hub candidate check: do the published old releases read a candidate catalog, and get its game?

Run before any catalog or feed publish (the release skill, step 6a; the community-pack skill, step 6a):

    python3 -I tools/release/hub_candidate_check.py \\
        --catalog-dir <the publish_hub dry run's folder, e.g. .../catalog-src> [--versions v1.5.2 v1.5.3 v1.5.4]

The candidate is the dry run's output: `gitverse/index-v2.json` with the files (`gitverse-zips/`,
`gitverse/previews/`, `gitverse/files/`, `remote-game/`, the catalog's own folder). Nothing is published:
the check serves the candidate from a local HTTPS server on 127.0.0.1, with the addresses of the index
rewritten to that server.

What it does:
1. Maps every https address of the index to a local route and finds each file by its size and SHA-256
   (a wrapped file's zip by the archive fields, then its member by the file's own size and SHA-256). Any
   address without a matching file stops the run before anything is served or launched.
2. Serves the rewritten index and the files over TLS (threaded, Range answered with 206 or 200, never a
   wrong partial), logs every request.
3. Runs each old Linux release (downloaded from its GitHub release and checked against SHA256SUMS) on a
   private Xvfb (never :0), with RAZDOR_HUB pointing at the server, fresh HOME and XDG folders, and
   `updates: off` in its saves. The app's own CA trust comes from CURL_CA_BUNDLE (curl reads it; the app
   calls the system curl).
4. Reads the program's log. With no install, the game download starts by itself at launch: the check
   passes when the log says the catalog was read with a game in it, offers the catalog's game (name and
   size), ends with `installed`, and the game's bytes came from the server (access log).

Not covered (needs the Hub window, or an app hook): downloading a map, a campaign and a mod. The file
rules for them are checked here only on the bytes the server serves (sizes, SHA-256 of the file and of
its zip member), not through the app. An app hook that would close that gap is named in the report of
the test run (see tools/release/README.md).

Exit codes: 0 every version passed; 1 a version failed; 2 the check could not run (input, network, tool).
Stdlib only; uses update_check's download, Xvfb and launch helpers.
"""

import argparse
import copy
import hashlib
import http.server
import json
import os
import re
import shutil
import socketserver
import ssl
import subprocess
import sys
import threading
import time
import urllib.parse
import zipfile
from pathlib import Path

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import update_check as uc  # noqa: E402

REPO_RELEASES = "https://github.com/indicozy/razdor/releases/download"
DEFAULT_VERSIONS = ("v1.5.2", "v1.5.3", "v1.5.4")
DEFAULT_WORK = "/mnt/ai_models/razdor-scratch/release-checks2/hub-candidate"
ROUTE = "/cand"
INDEX_NAME = "index-v2.json"
GAME_LINE = "game download: catalog read, game in it: yes"
OFFER = re.compile(r"game download: the catalog offers (.+) \((\d+) bytes\)")
ENDED = re.compile(r"game download: ended: (.+)")
RUN_TIMEOUT = 420   # seconds to wait for the launch-time game download of one version
POLL = 2
CLOSED_PORT = 9     # the discard port: a proxy that refuses, so no outside address answers


class CheckFail(Exception):
    """The candidate or a version does not pass. The message is the reason, shown in the report."""


class ToolFail(Exception):
    """The check could not run (input, network, tool)."""


def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


class Finder:
    """Finds a candidate file by its size and SHA-256 in a few folders (each file hashed at most once)."""

    def __init__(self, folders):
        self.folders = [Path(f) for f in folders]
        self.cache = {}

    def find(self, size, sha):
        sha = sha.lower()
        for folder in self.folders:
            if not folder.is_dir():
                continue
            for p in sorted(folder.iterdir()):
                if not p.is_file() or p.stat().st_size != size:
                    continue
                if p not in self.cache:
                    self.cache[p] = sha256_of(p)
                if self.cache[p] == sha:
                    return p
        return None


def zip_member(zip_path, member, size, sha):
    """The member of a zip, checked against its size and SHA-256 (the file's own, not the zip's)."""
    with zipfile.ZipFile(zip_path) as z:
        data = z.read(member)
    if len(data) != size or hashlib.sha256(data).hexdigest() != sha.lower():
        raise CheckFail(f"{zip_path.name}: member {member} does not match the catalog's size or SHA-256")


def https_urls(index):
    """Every address the app fetches, with the expected (size, sha256) of what it returns, and the
    kind (for the folders to search). The ad's link is not fetched (it opens in a browser): skipped."""
    out = []
    for e in index.get("entries", []):
        for f in e.get("files") or []:
            if f["path"].startswith("https://"):
                if f.get("wrapped") == "zip":
                    out.append((f["path"], f["archive_size"], f["archive_sha256"], "zip", f))
                else:
                    out.append((f["path"], f["size"], f["sha256"], "file", f))
                for m in f.get("mirrors") or []:
                    out.append((m, f["size"], f["sha256"], "file", f))
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict) and ref["path"].startswith("https://"):
                out.append((ref["path"], ref["size"], ref["sha256"], "picture", ref))
    game = index.get("game")
    if isinstance(game, dict):
        for u in [game["url"]] + list(game.get("mirrors") or []):
            out.append((u, game["size"], game["sha256"], "game", game))
    ad = index.get("ad")
    if isinstance(ad, dict) and ad["image"].startswith("https://"):
        out.append((ad["image"], ad["size"], ad["sha256"], "picture", ad))
    return out


def route_for(url):
    """The local path a remote address is served at: /cand/h/<host>/<path>."""
    u = urllib.parse.urlsplit(url)
    return f"{ROUTE}/h/{u.netloc}{u.path}"


class Plan:
    def __init__(self, index, cat_dir, port):
        self.index = copy.deepcopy(index)
        self.cat = Path(cat_dir)
        self.base = f"https://127.0.0.1:{port}{ROUTE}"
        self.routes = {}     # local route -> (local file, size, sha256)
        self.rewritten = {}  # original address -> new address
        self.checked = []    # human lines: what was matched

    def serve(self, url, local, size, sha):
        route = route_for(url)
        self.routes[route] = (local, size, sha.lower())
        new = f"https://127.0.0.1:{self.port}{route}"
        self.rewritten[url] = new
        return new


def build_plan(index, cat_dir, port):
    """Rewrites the index's addresses and matches each address with its local file. Raises CheckFail."""
    cat = Path(cat_dir)
    finder = Finder([cat / "gitverse-zips", cat / "gitverse" / "files", cat / "remote-game",
                     cat / "gitverse" / "previews", cat / "remote-pictures", cat])
    plan = Plan(index, cat_dir, port)
    plan.port = port
    urls = https_urls(index)
    # the entries' own addresses first (wrapped zips, archives, pictures, the game), then the mirrors
    for url, size, sha, kind, ref in urls:
        if url in plan.rewritten:
            continue
        if kind == "zip":
            local = finder.find(size, sha)
            if local is None:
                raise CheckFail(f"no file in the candidate for the zip of {ref['name']} ({url})")
            m = ref.get("member")
            zip_member(local, m, ref["size"], ref["sha256"])
            plan.checked.append(f"zip {ref['name']}: member {m} matches size and sha256")
        else:
            local = finder.find(size, sha)
            if local is None:
                raise CheckFail(f"no file in the candidate for {kind} {url} ({size} bytes)")
        plan.serve(url, local, size, sha)
    # relative pictures of the index: previews/<name>, served under the base address
    for e in plan.index.get("entries", []):
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict) and not ref["path"].startswith("https://"):
                serve_relative(plan, finder, ref)
    ad = plan.index.get("ad")
    if isinstance(ad, dict) and not ad["image"].startswith("https://"):
        serve_relative(plan, finder, ad, key="image")
    # every address the app would fetch, in the rewritten index, must be the local server's
    plan.new_index = rewrite(index, plan)
    for path in https_fetched(plan.new_index):
        if not path.startswith(f"https://127.0.0.1:{port}/"):
            raise CheckFail(f"an address the app would fetch is not local: {path}")
    return plan


def serve_relative(plan, finder, ref, key="path"):
    rel = ref[key]
    local = finder.find(ref["size"], ref["sha256"])
    if local is None:
        raise CheckFail(f"no picture in the candidate for {rel} ({ref['size']} bytes)")
    route = f"{ROUTE}/{rel}"
    plan.routes[route] = (local, ref["size"], ref["sha256"].lower())
    plan.checked.append(f"picture {rel}: size and sha256 match")


def https_fetched(index):
    """The addresses the app fetches from an index (a relative picture is not listed)."""
    out = []
    for e in index.get("entries", []):
        for f in e.get("files") or []:
            out.append(f["path"])
            out.extend(f.get("mirrors") or [])
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict):
                out.append(ref["path"])
    game = index.get("game")
    if isinstance(game, dict):
        out.append(game["url"])
        out.extend(game.get("mirrors") or [])
    ad = index.get("ad")
    if isinstance(ad, dict):
        out.append(ad["image"])
    return [u for u in out if u.startswith("https://")]


def rewrite(index, plan):
    """The index with its addresses replaced by the local ones (relative pictures stay relative)."""
    new = copy.deepcopy(index)

    def swap(u):
        return plan.rewritten.get(u, u)

    for e in new.get("entries", []):
        for f in e.get("files") or []:
            f["path"] = swap(f["path"])
            if f.get("mirrors"):
                f["mirrors"] = [swap(m) for m in f["mirrors"]]
        for key in ("preview", "picture"):
            ref = e.get(key)
            if isinstance(ref, dict) and ref["path"].startswith("https://"):
                ref["path"] = swap(ref["path"])
    game = new.get("game")
    if isinstance(game, dict):
        game["url"] = swap(game["url"])
        game["mirrors"] = [swap(m) for m in game.get("mirrors") or []]
    ad = new.get("ad")
    if isinstance(ad, dict) and ad["image"].startswith("https://"):
        ad["image"] = swap(ad["image"])
    return new


class State:
    def __init__(self, index_bytes, routes):
        self.index_bytes = index_bytes
        self.routes = routes
        self.lock = threading.Lock()
        self.access = []


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.0"
    state = None

    def log_message(self, fmt, *args):
        pass

    def do_GET(self):
        st = self.state
        path = urllib.parse.urlsplit(self.path).path
        rng = self.headers.get("Range")
        if path == f"{ROUTE}/{INDEX_NAME}":
            body = st.index_bytes
            self.send_full(body, 200, path, rng)
            return
        item = st.routes.get(path)
        if item is None:
            self.send_error(404)
            self.record(path, 404, rng)
            return
        local, size, _ = item
        start, end = 0, size - 1
        status = 200
        m = re.fullmatch(r"bytes=(\d+)-(\d*)", rng or "")
        if m:
            a = int(m.group(1))
            b = int(m.group(2)) if m.group(2) else size - 1
            b = min(b, size - 1)
            if a > b or a >= size:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                self.record(path, 416, rng)
                return
            start, end, status = a, b, 206
        length = end - start + 1
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        with open(local, "rb") as f:
            f.seek(start)
            remaining = length
            while remaining:
                chunk = f.read(min(1 << 20, remaining))
                if not chunk:
                    break
                self.wfile.write(chunk)
                remaining -= len(chunk)
        self.record(path, status, rng)

    def send_full(self, body, status, path, rng):
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        self.record(path, status, rng)

    def record(self, path, status, rng):
        with self.state.lock:
            self.state.access.append({"t": time.time(), "path": path, "status": status, "range": rng})


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True


def start_server(state, cert, key):
    handler = type("H", (Handler,), {"state": state})
    server = Server(("127.0.0.1", 0), handler)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(cert, key)
    server.socket = ctx.wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, server.server_address[1]


def make_cert(folder):
    """A self-signed certificate for 127.0.0.1, made in the scratch folder (never in the repo)."""
    key, cert = folder / "key.pem", folder / "cert.pem"
    proc = subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(key), "-out", str(cert),
         "-days", "2", "-subj", "/CN=127.0.0.1", "-addext", "subjectAltName=IP:127.0.0.1"],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    if proc.returncode != 0:
        raise ToolFail(f"openssl could not make the certificate: {proc.stderr.strip()}")
    return cert, key


def download_version(tag, dest_dir):
    """The version's Linux program, checked against the release's SHA256SUMS line."""
    v = tag.lstrip("v")
    name = f"razdor-{v}-linux"
    sums = dest_dir / "SHA256SUMS"
    try:
        uc.download(f"{REPO_RELEASES}/{tag}/SHA256SUMS", sums)
        uc.download(f"{REPO_RELEASES}/{tag}/{name}", dest_dir / name)
    except Exception as e:  # network or HTTP error
        raise ToolFail(f"{tag}: cannot download {name} ({e})") from e
    want = None
    for line in sums.read_text(encoding="utf-8").splitlines():
        parts = line.split(None, 1)
        if len(parts) == 2 and parts[1].strip().lstrip("*") == name:
            want = parts[0].lower()
    if want is None:
        raise ToolFail(f"{tag}: SHA256SUMS has no {name}")
    prog = dest_dir / name
    prog.chmod(0o755)
    if sha256_of(prog) != want:
        raise CheckFail(f"{tag}: {name} does not match its SHA256SUMS line")
    return prog


def read_log(path):
    try:
        return Path(path).read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return ""


def run_version(tag, prog, work, port, cert, index, game_size, state, timeout):
    """One old program against the candidate: returns a result dict (passed or not, and why)."""
    base = work / "run" / tag
    shutil.rmtree(base, ignore_errors=True)
    for sub in ("home", "xdg/data", "xdg/config", "xdg/cache", "xdg/state", "run", "downloads", "saves"):
        (base / sub).mkdir(parents=True, exist_ok=True)
    (base / "saves" / "audio.json").write_text(json.dumps({"updates": "off"}), encoding="utf-8")
    bindir = base / "bin"
    bindir.mkdir()
    local_prog = bindir / "razdor"
    shutil.copy(prog, local_prog)
    local_prog.chmod(0o755)
    log = base / "razdor.log"
    result = {"version": tag, "passed": False, "why": ""}
    xvfb = None
    app = None
    before = len(state.access)
    try:
        xvfb, display = uc.start_xvfb(base / "xvfb.log")
        env = uc.carry_env(display, base, log)
        env.update({
            "RAZDOR_HUB": f"https://127.0.0.1:{port}{ROUTE}",
            "CURL_CA_BUNDLE": str(cert),
            "SSL_CERT_FILE": str(cert),
            # Nothing but the local server is reached: every other address goes to a closed local port,
            # so the program's own remote config fetch fails at once and it keeps its built-in lists.
            "https_proxy": f"http://127.0.0.1:{CLOSED_PORT}",
            "HTTPS_PROXY": f"http://127.0.0.1:{CLOSED_PORT}",
            "NO_PROXY": "127.0.0.1,localhost",
            "no_proxy": "127.0.0.1,localhost",
        })
        app = uc.start_game(local_prog, env, base / "stdout.log")
        t0 = time.monotonic()
        text = ""
        while time.monotonic() - t0 < timeout:
            text = read_log(log)
            if ENDED.search(text) or app.poll() is not None:
                break
            time.sleep(POLL)
        text = read_log(log)
        result["log_lines"] = [l for l in text.splitlines() if "game download" in l or "hub" in l.lower()][:40]
        result["panics"] = text.count("panicked")
        result.update(judge(text, state.access[before:], index, game_size))
    except (uc.ToolError, OSError) as e:
        result["why"] = f"could not run: {e}"
    finally:
        if app is not None and app.poll() is None:
            uc.stop(app)
        if xvfb is not None:
            uc.stop(xvfb)
    return result


def judge(text, access, index, game_size):
    """The checks on one version's run: the catalog read, the game offered and installed, the bytes served."""
    out = {"catalog_read": GAME_LINE in text}
    offer = OFFER.search(text)
    out["offered"] = offer.group(1) if offer else None
    out["offered_size"] = int(offer.group(2)) if offer else None
    ended = ENDED.search(text)
    out["ended"] = ended.group(1).strip() if ended else None
    game = index.get("game") or {}
    game_route = [a for a in access if a["path"].startswith(f"{ROUTE}/h/") and a["status"] in (200, 206)
                  and a["path"].endswith(os.path.basename(urllib.parse.urlsplit(game.get("url", "")).path))]
    out["game_requests"] = len(game_route)
    out["game_partial"] = sum(1 for a in game_route if a["status"] == 206)
    out["index_requests"] = sum(1 for a in access if a["path"] == f"{ROUTE}/{INDEX_NAME}" and a["status"] == 200)
    why = []
    if not out["catalog_read"]:
        why.append("the log does not say the catalog was read with a game in it")
    if out["offered"] is None:
        why.append("the log does not say the catalog offers the game")
    elif out["offered_size"] != game.get("size"):
        why.append(f"the offer's size {out['offered_size']} is not the catalog's {game.get('size')}")
    if out["ended"] is None:
        why.append("the game download did not end within the wait")
    elif not (out["ended"] == "installed" or out["ended"].startswith("patched")):
        why.append(f"the game download ended with: {out['ended']}")
    if out["index_requests"] < 1:
        why.append("the program did not ask the server for the index")
    if out["game_requests"] < 1:
        why.append("the program did not fetch the game from the server")
    out["passed"] = not why
    out["why"] = "; ".join(why)
    return out


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--catalog-dir", required=True, help="the publish_hub dry run's folder (holds gitverse/index-v2.json)")
    ap.add_argument("--index", help="the index to check (default: CATALOG/gitverse/index-v2.json)")
    ap.add_argument("--versions", nargs="+", default=list(DEFAULT_VERSIONS))
    ap.add_argument("--work", default=DEFAULT_WORK, help="scratch folder (removed at the end unless --keep)")
    ap.add_argument("--timeout", type=int, default=RUN_TIMEOUT, help="seconds per version for the game download")
    ap.add_argument("--keep", action="store_true", help="keep the scratch folder (its logs)")
    ap.add_argument("--plan-only", action="store_true", help="match and serve-check the candidate; no program is run")
    args = ap.parse_args(argv)
    work = Path(args.work)
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    index_path = Path(args.index or Path(args.catalog_dir) / "gitverse" / INDEX_NAME)
    try:
        index = json.loads(index_path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        print(f"error: cannot read {index_path}: {e}", file=sys.stderr)
        return 2
    if index.get("format") != 1 or not isinstance(index.get("entries"), list):
        print(f"error: {index_path} is not a format 1 catalog", file=sys.stderr)
        return 2
    cert, key = make_cert(work)
    # the candidate is matched and served at a port chosen now; the server starts before the matching
    state = State(b"", {})
    server, port = start_server(state, str(cert), str(key))
    results = []
    code = 0
    try:
        try:
            plan = build_plan(index, args.catalog_dir, port)
        except CheckFail as e:
            print(f"candidate refused: {e}")
            return 1
        new_index = plan.new_index
        for line in plan.checked:
            print("match:", line)
        state.index_bytes = (json.dumps(new_index, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
        state.routes.update(plan.routes)
        (work / INDEX_NAME).write_bytes(state.index_bytes)
        if args.plan_only:
            print(f"plan: {len(plan.routes)} routes, {len(plan.rewritten)} remote addresses rewritten, all local")
            return 0
        game_size = index["game"]["size"] if isinstance(index.get("game"), dict) else None
        for tag in args.versions:
            try:
                bin_dir = work / "bin" / tag
                bin_dir.mkdir(parents=True, exist_ok=True)
                prog = download_version(tag, bin_dir)
                res = run_version(tag, prog, work, port, cert, new_index, game_size, state, args.timeout)
            except (ToolFail, CheckFail) as e:
                res = {"version": tag, "passed": False, "why": str(e)}
            results.append(res)
            verdict = "PASS" if res.get("passed") else "FAIL"
            print(f"{tag}: {verdict}" + (f" ({res.get('why')})" if res.get("why") else ""))
            print(f"  catalog read: {res.get('catalog_read')}, offered: {res.get('offered')} ({res.get('offered_size')} bytes), "
                  f"ended: {res.get('ended')}, game requests: {res.get('game_requests')} ({res.get('game_partial')} partial), "
                  f"index requests: {res.get('index_requests')}, panics: {res.get('panics')}")
            if not res.get("passed"):
                code = 1
    finally:
        server.shutdown()
        server.server_close()
    (work / "result.json").write_text(json.dumps(results, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if not args.keep:
        # the programs, the runs' folders, the certificate and the rewritten index go; the result stays
        for name in ("bin", "run", "cert.pem", "key.pem", INDEX_NAME):
            target = work / name
            if target.is_dir():
                shutil.rmtree(target, ignore_errors=True)
            elif target.exists():
                target.unlink()
    if all(r.get("passed") for r in results) and results:
        print("candidate: every version reads the catalog and gets the game")
    else:
        code = max(code, 1)
    return code


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except ToolFail as e:
        print(f"error: {e}", file=sys.stderr)
        sys.exit(2)
