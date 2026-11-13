#!/usr/bin/env python3
"""GitVerse client shared by the publish scripts (stdlib only; run with `python3 -I`).

What it does:
  - reads the token (GITVERSE_TOKEN, else the repo's .env; the main checkout's .env for a
    worktree), never prints it;
  - calls the API (releases: create, get by tag, delete by id or tag; assets: upload as a
    streamed multipart body, delete);
  - retries 429, 5xx and connection errors with backoff (Retry-After honoured);
  - fetches files anonymously in byte ranges, each part checked for its exact length and
    retried, then the whole file checked for size and SHA-256 (a part can come back short);
  - keeps a registry of the releases these scripts made, in the state folder outside git
    (`RAZDOR_GITVERSE_STATE`, default ~/.local/state/razdor-gitverse/registry.json), so the
    old-release cleanup never touches a release it did not make.

GitVerse facts used here (measured 2026-11-13, see docs/specs/2026-11-13-gitverse-mirrors.md):
  - GET /repos/{o}/{r}/releases (the list) answered 404 on 2026-11-13 and 200 later that day: not
    relied on (the registry names what these scripts made); ids come from the create responses;
  - HEAD on an attachment answers 405: anonymous downloads use GET with Range;
  - asset names: .exe, .bin and names without an extension are refused;
  - 100 MB a file, 4 GB of release files per account (both repos together).
"""

import concurrent.futures
import copy
import hashlib
import http.client
import json
import math
import mimetypes
import os
import re
import secrets
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.gitverse.ru"
HOST = "https://gitverse.ru"
OWNER = "indicozy"
APP_REPO = "razdor"
HUB_REPO = "razdor-community"
ACCEPT = "application/vnd.gitverse.object+json;version=1"
USER_AGENT = "razdor-publish/1"
ASSET_LIMIT = 100 * 1024 * 1024
RELEASE_BUDGET = 4 * 1024 ** 3
PARTS = 8
PART_MIN = 256 * 1024
TIMEOUT = 300   # an upload or a whole small reply
STALL = 60      # a download read that gets no bytes for this long is a failure
TRIES = 5
RETRY_STATUS = frozenset({429, 500, 502, 503, 504})
# An upload that GitVerse answers 400 VALIDATION_ERROR to is tried again this many times, after these
# waits (seen once in 2026-11-13's run after 525 uploads; an immediate retry went through).
UPLOAD_400_WAITS = (2, 4)
CHUNK = 64 * 1024
# The extensions GitVerse took in the measurement of 2026-11-13 (upload tests on a throwaway release,
# with the right and with a generic Content-Type alike: the extension decides). Anything else is refused
# before a release is made: .exe, .bin, .dtm, .json, .ini, .lit, .ogg, .wav, .mp3, .webp, .ttf, .cfg,
# .dll, .xml, .ipa, a name without an extension, and .DTm.
GV_EXT_OK = frozenset({"apk", "zip", "gz", "tgz", "7z", "appimage", "png", "jpg", "jpeg", "pdf", "md", "txt", "csv"})
NAME_RE = re.compile(r"^[A-Za-z0-9_.-]{1,255}$")
SEMVER_TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")

HERE = os.path.dirname(os.path.abspath(__file__))


class GitVerseError(Exception):
    pass


class ApiError(GitVerseError):
    def __init__(self, status, detail, retry_after=None):
        super().__init__(f"HTTP {status}: {detail}")
        self.status = status
        self.retry_after = retry_after


class ShortRead(OSError):
    """A download part that came back with the wrong length (transient: retried)."""


class VerifyError(GitVerseError):
    """A file that does not match its size or SHA-256 (not retried: the bytes are wrong)."""


def note(msg):
    print(msg, file=sys.stderr, flush=True)


# ---------------------------------------------------------------- token and state

def _git(args, cwd):
    r = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else ""


def _env_files():
    paths = []
    top = _git(["rev-parse", "--show-toplevel"], HERE)
    if top:
        paths.append(os.path.join(top, ".env"))
    common = _git(["rev-parse", "--git-common-dir"], HERE)
    if common:
        # A worktree's common git dir is <main checkout>/.git: its parent holds the .env.
        main = os.path.dirname(os.path.abspath(os.path.join(HERE, common)))
        if os.path.join(main, ".env") not in paths:
            paths.append(os.path.join(main, ".env"))
    return paths


def env_value(name, path):
    """The value of `name` in a dotenv file (quotes stripped), or ''. Missing file: ''."""
    try:
        with open(path, encoding="utf-8") as f:
            lines = f.read().splitlines()
    except OSError:
        return ""
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export "):]
        key, sep, value = line.partition("=")
        if sep and key.strip() == name:
            value = value.strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            return value
    return ""


def load_token():
    tok = os.environ.get("GITVERSE_TOKEN", "").strip()
    if not tok:
        for path in _env_files():
            tok = env_value("GITVERSE_TOKEN", path)
            if tok:
                break
    if not tok:
        raise GitVerseError("GITVERSE_TOKEN is not set (the environment, or the repo's .env)")
    return tok


def redact(text, token):
    text = str(text)
    return text.replace(token, "***") if token else text


def state_dir():
    d = os.environ.get("RAZDOR_GITVERSE_STATE") or os.path.join(
        os.path.expanduser("~"), ".local", "state", "razdor-gitverse")
    os.makedirs(d, exist_ok=True)
    return d


def write_json_atomic(path, obj):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(obj, f, ensure_ascii=False, indent=2)
        f.write("\n")
    os.replace(tmp, path)


def load_registry(path=None):
    path = path or os.path.join(state_dir(), "registry.json")
    if not os.path.exists(path):
        return {"releases": []}
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def save_registry(reg, path=None):
    write_json_atomic(path or os.path.join(state_dir(), "registry.json"), reg)


def register(reg, repo, tag, release_id, kind, prerelease, nbytes):
    """Adds (or replaces) a release this script made. Returns the entry."""
    reg["releases"] = [r for r in reg["releases"] if not (r["repo"] == repo and r["tag"] == tag)]
    entry = {"repo": repo, "tag": tag, "id": release_id, "kind": kind, "prerelease": bool(prerelease),
             "created": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "bytes": int(nbytes)}
    reg["releases"].append(entry)
    return entry


def unregister(reg, repo, tag):
    reg["releases"] = [r for r in reg["releases"] if not (r["repo"] == repo and r["tag"] == tag)]


def version_key(tag):
    m = SEMVER_TAG.match(tag or "")
    return tuple(int(x) for x in m.groups()) if m else None


def select_prune(entries, keep):
    """The registry entries to delete: app releases (vX.Y.Z, not pre-releases) beyond the newest
    `keep` by version. Entries of other kinds, and pre-releases, are never selected."""
    apps = [e for e in entries if e.get("kind") == "app" and not e.get("prerelease") and version_key(e["tag"])]
    apps.sort(key=lambda e: version_key(e["tag"]), reverse=True)
    return apps[keep:]


def select_prune_catalogs(entries, keep):
    """The catalog releases beyond the newest `keep` (by creation time)."""
    cats = [e for e in entries if e.get("kind") == "catalog"]
    cats.sort(key=lambda e: e.get("created", ""), reverse=True)
    return cats[keep:]


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def name_problem(name):
    """Why GitVerse (as measured, GV_EXT_OK) would refuse this asset name, or None."""
    if not NAME_RE.match(name or ""):
        return "letters, digits, _, - and . only, at most 255 characters"
    stem, dot, ext = name.rpartition(".")
    if not dot or not stem or not ext:
        return "the name needs a stem and an extension"
    if ext.lower() not in GV_EXT_OK:
        return f"GitVerse refuses .{ext} files (accepted: {', '.join(sorted(GV_EXT_OK))})"
    return None


# ---------------------------------------------------------------- retries

def transient(exc):
    if isinstance(exc, ApiError):
        return exc.status in RETRY_STATUS or exc.status >= 500
    return isinstance(exc, (OSError, http.client.HTTPException))


def retrying(what, fn, tries=TRIES, sleep=time.sleep):
    """Calls fn() until it returns; transient failures wait and retry (2, 4, 8 … s, or Retry-After)."""
    attempt = 0
    while True:
        attempt += 1
        try:
            return fn()
        except Exception as e:  # noqa: BLE001 - classified below
            if attempt >= tries or not transient(e):
                raise
            wait = e.retry_after if isinstance(e, ApiError) and e.retry_after else min(60, 2 ** attempt)
            note(f"{what}: {e}; retry {attempt}/{tries - 1} in {wait:g} s")
            sleep(wait)


# ---------------------------------------------------------------- multipart body

class Multipart:
    """A multipart/form-data body that streams its files: read(n) like a file, so a large asset is
    never held whole in memory. Build a new one for every attempt (the file is read again)."""

    def __init__(self, parts):
        """parts: (field, filename, content_type, source), source = bytes or a file path."""
        self.boundary = "razdor" + secrets.token_hex(16)
        self.content_type = f"multipart/form-data; boundary={self.boundary}"
        self._parts = []
        self.length = 0
        for field, filename, ctype, src in parts:
            head = (f'--{self.boundary}\r\nContent-Disposition: form-data; name="{field}"; '
                    f'filename="{filename}"\r\nContent-Type: {ctype}\r\n\r\n').encode("utf-8")
            size = len(src) if isinstance(src, bytes) else os.path.getsize(src)
            self._parts.append((head, src))
            self.length += len(head) + size + len(b"\r\n")
        self._tail = f"--{self.boundary}--\r\n".encode("ascii")
        self.length += len(self._tail)
        self._gen = self._chunks()
        self._buf = bytearray()

    def _chunks(self):
        for head, src in self._parts:
            yield head
            if isinstance(src, bytes):
                yield src
            else:
                with open(src, "rb") as f:
                    while True:
                        block = f.read(CHUNK)
                        if not block:
                            break
                        yield block
            yield b"\r\n"
        yield self._tail

    def read(self, n=-1):
        if n is None or n < 0:
            rest = b"".join(self._gen)
            out = bytes(self._buf) + rest
            self._buf = bytearray()
            return out
        while len(self._buf) < n:
            try:
                self._buf += next(self._gen)
            except StopIteration:
                break
        out = bytes(self._buf[:n])
        del self._buf[:n]
        return out

    def to_bytes(self):
        """The whole body (tests and small bodies only)."""
        return b"".join(self._chunks())


def asset_content_type(name):
    return mimetypes.guess_type(name)[0] or "application/octet-stream"


# ---------------------------------------------------------------- the API client

class Client:
    def __init__(self, token=None, api=API, sleep=time.sleep):
        self.token = token or load_token()
        self.api = api.rstrip("/")
        self.sleep = sleep   # the upload's 400 waits (a test gives a fake that records them)

    def _open(self, req):
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
                return r.status, r.headers, r.read()
        except urllib.error.HTTPError as e:
            try:
                body = e.read()[:400].decode("utf-8", "replace")
            except OSError:
                body = ""
            finally:
                e.close()
            ra = e.headers.get("Retry-After") if e.headers else None
            retry_after = float(ra) if ra and ra.replace(".", "", 1).isdigit() else None
            raise ApiError(e.code, redact(body, self.token), retry_after) from None

    def request(self, method, path, *, query=None, body=None, ctype=None, auth=True, tries=TRIES):
        """One API call with retries. `body` is None, bytes, or a callable returning a fresh
        Multipart (or bytes) for every attempt. Returns (status, headers, bytes)."""
        url = self.api + path + ("?" + urllib.parse.urlencode(query) if query else "")

        def once():
            headers = {"User-Agent": USER_AGENT}
            if auth:
                headers["Authorization"] = "Bearer " + self.token
                headers["Accept"] = ACCEPT
            data = None
            if callable(body):
                data = body()
            elif body is not None:
                data = body
            if isinstance(data, Multipart):
                headers["Content-Type"] = data.content_type
                headers["Content-Length"] = str(data.length)
            elif isinstance(data, bytes):
                headers["Content-Length"] = str(len(data))
                if ctype:
                    headers["Content-Type"] = ctype
            req = urllib.request.Request(url, data=data, headers=headers, method=method)
            return self._open(req)

        return retrying(f"{method} {path}", once, tries)

    def _json(self, method, path, **kw):
        _, _, data = self.request(method, path, **kw)
        return json.loads(data) if data else None

    def create_release(self, owner, repo, tag, target, name, body, prerelease):
        payload = {"tag_name": tag, "target_commitish": target, "name": name, "body": body,
                   "prerelease": bool(prerelease), "draft": False, "is_authorized_only": False}
        return self._json("POST", f"/repos/{owner}/{repo}/releases",
                          body=json.dumps(payload).encode("utf-8"), ctype="application/json")

    def release_by_tag(self, owner, repo, tag):
        """The release of `tag`, or None when there is none (GET releases/tags/{tag} answers 404)."""
        try:
            return self._json("GET", f"/repos/{owner}/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}")
        except ApiError as e:
            if e.status == 404:
                return None
            raise

    def delete_release(self, owner, repo, release_id):
        self.request("DELETE", f"/repos/{owner}/{repo}/releases/{int(release_id)}")

    def delete_release_by_tag(self, owner, repo, tag):
        self.request("DELETE", f"/repos/{owner}/{repo}/releases/tags/{urllib.parse.quote(tag, safe='')}")

    def upload_asset(self, owner, repo, release_id, name, path):
        """Uploads `path` as asset `name`. Returns the API's asset (browser_download_url, id)."""
        problem = name_problem(name)
        if problem:
            raise GitVerseError(f"{name}: {problem}")

        def body():
            return Multipart([("attachment", name, asset_content_type(name), path)])

        route = f"/repos/{owner}/{repo}/releases/{int(release_id)}/assets"
        for attempt in range(len(UPLOAD_400_WAITS) + 1):
            try:
                return self._json("POST", route, query={"name": name}, body=body)
            except ApiError as e:
                if e.status != 400 or attempt >= len(UPLOAD_400_WAITS):
                    raise
                # A 400 is not retried anywhere else; only an upload gets these short retries.
                note(f"upload {name}: {e}; retry {attempt + 1}/{len(UPLOAD_400_WAITS)} in {UPLOAD_400_WAITS[attempt]} s")
                self.sleep(UPLOAD_400_WAITS[attempt])

    def delete_asset(self, owner, repo, release_id, asset_id):
        self.request("DELETE", f"/repos/{owner}/{repo}/releases/{int(release_id)}/assets/{int(asset_id)}")


# ---------------------------------------------------------------- anonymous downloads

def anon_get(url, start=None, end=None, tries=TRIES):
    """GET without the token, optionally the byte range start..end (both inclusive)."""

    def once():
        headers = {"User-Agent": USER_AGENT}
        if start is not None:
            headers["Range"] = f"bytes={start}-{end}"
        req = urllib.request.Request(url, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=STALL) as r:
                return r.status, r.headers, r.read()
        except urllib.error.HTTPError as e:
            e.close()
            raise ApiError(e.code, f"GET {url}") from None

    return retrying(f"GET {url}", once, tries)


def fetch_bytes(url, tries=TRIES):
    status, _, data = anon_get(url, tries=tries)
    if status != 200:
        raise GitVerseError(f"GET {url}: HTTP {status}")
    return data


def fetch_exact(url, size, sha256, parts=PARTS, tries=TRIES):
    """The file at `url`, fetched anonymously in up to `parts` byte ranges at once. Each range must
    come back with its exact length (a short one is retried); the whole must have `size` bytes and
    `sha256`. Raises VerifyError (or the last transport error) when it does not."""
    size = int(size)
    if size == 0:
        data = fetch_bytes(url, tries)
        if data:
            raise VerifyError(f"{url}: expected an empty file, got {len(data)} bytes")
        return
    n = max(1, min(parts, math.ceil(size / PART_MIN)))
    step = math.ceil(size / n)
    spans = [(i * step, min(size, (i + 1) * step) - 1) for i in range(n) if i * step < size]

    def part(span):
        a, b = span

        def once():
            status, headers, data = anon_get(url, a, b, tries=1)
            cr = headers.get("Content-Range") if headers else None
            if status == 206 and cr and cr != f"bytes {a}-{b}/{size}":
                raise VerifyError(f"{url}: range {a}-{b} answered {cr} (the file changed?)")
            if status == 200 and len(data) == size:
                data = data[a:b + 1]          # a server that ignored Range sent the whole file
            if status not in (200, 206):
                raise ShortRead(f"{url}: range {a}-{b} answered HTTP {status}")
            if len(data) != b - a + 1:
                raise ShortRead(f"{url}: range {a}-{b} came back with {len(data)} of {b - a + 1} bytes")
            return data

        return retrying(f"part {a}-{b}", once, tries)

    with concurrent.futures.ThreadPoolExecutor(max_workers=len(spans)) as ex:
        chunks = list(ex.map(part, spans))
    blob = b"".join(chunks)
    if len(blob) != size:
        raise VerifyError(f"{url}: {len(blob)} bytes, expected {size}")
    if sha256_bytes(blob) != sha256.lower():
        raise VerifyError(f"{url}: SHA-256 differs from the local file")


def check_size(path):
    size = os.path.getsize(path)
    if size > ASSET_LIMIT:
        raise GitVerseError(f"{os.path.basename(path)} is {size} bytes: GitVerse takes 100 MB a file")
    return size
