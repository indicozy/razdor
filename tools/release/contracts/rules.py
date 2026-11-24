"""The parser rules of the old Razdor releases, for every file the publishers write.

Each check is what a released program does with the file, taken from its source at the tag named
in the comment (`git show <tag>:src/hub.rs` and so on). A check raises ContractError with the reason
the program would refuse the file. Stdlib only; no network.

Versions and what they read:
- v1.4.1, v1.5.0: the GitHub API's release list (`parse_releases`), and the flat catalog `index.json`
  of the GitHub hub release. They never read the own mirror's feed, the config or `index-v2.json`.
- v1.5.2, v1.5.3, v1.5.4 (the same hub.rs, update.rs, mirror.rs and remote_config.rs in all three):
  `releases.json` (the update feed), `config.json` (schema 1), `index-v2.json` and `index.json` of the
  hub (absolute https addresses or paths in the hub folder).

Nothing here denies unknown fields: the Rust structs carry no `deny_unknown_fields` in any tag, so a
field a version does not know is ignored. The test suite checks that ignoring with mutations.
"""

import json
import re
import struct

MAX_ARCHIVE_DOWNLOAD = 256 << 20   # hub.rs: MAX_ARCHIVE_DOWNLOAD (v1.5.2 .. v1.5.4)
MAX_AD_BYTES = 2 << 20              # hub.rs: MAX_AD_BYTES
MAX_CAPTION = 200                   # hub.rs: MAX_CAPTION
PREVIEW_MAX_W = 480                 # hub.rs: PREVIEW_MAX_W (v1.5.2)
PREVIEW_MAX_H = 270                 # hub.rs: PREVIEW_MAX_H (v1.5.2)
U32_MAX = (1 << 32) - 1
U64_MAX = (1 << 64) - 1
KINDS = ("map", "campaign", "mod")  # hub.rs: enum Kind, rename_all lowercase
STATUSES = ("full", "partial", "broken", "unknown")  # enum Status, rename_all lowercase
SAFE_ID = re.compile(r"^[A-Za-z0-9_-]{1,64}$")       # hub.rs: safe_id
SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")
SUMS_LINE = re.compile(r"^[0-9a-f]{64}  \S.*$")       # sha256sum's text form, two spaces
FORBIDDEN_NAME_CHARS = set('/\\<>:"|?*')
WINDOWS_DEVICES = ("CON", "PRN", "AUX", "NUL")


class ContractError(Exception):
    """The program would refuse this file."""


# --------------------------------------------------------------------------- serde-like field checks

def _where(where, key):
    return f"{where}.{key}" if where else key


def _is_int(v):
    return isinstance(v, int) and not isinstance(v, bool)


def check_str(v, where):
    if not isinstance(v, str):
        raise ContractError(f"{where}: a string is needed, got {type(v).__name__}")


def check_bool(v, where):
    if not isinstance(v, bool):
        raise ContractError(f"{where}: a bool is needed, got {type(v).__name__}")


def check_u64(v, where):
    if not _is_int(v) or not 0 <= v <= U64_MAX:
        raise ContractError(f"{where}: an unsigned 64-bit integer is needed, got {v!r}")


def check_u32(v, where):
    if not _is_int(v) or not 0 <= v <= U32_MAX:
        raise ContractError(f"{where}: an unsigned 32-bit integer is needed, got {v!r}")


def check_list_of(check):
    def inner(v, where):
        if not isinstance(v, list):
            raise ContractError(f"{where}: a list is needed, got {type(v).__name__}")
        for i, x in enumerate(v):
            check(x, f"{where}[{i}]")
    return inner


def check_obj(v, where):
    if not isinstance(v, dict):
        raise ContractError(f"{where}: an object is needed, got {type(v).__name__}")


def one_of(values, what):
    """A closed serde enum (rename_all lowercase): any other string is refused."""
    def check(v, where):
        if v not in values:
            raise ContractError(f"{where}: {v!r} is not a {what}")
    return check


def field(obj, key, check, where, required=False, nullable=False):
    """serde's rule for one field: a missing field takes its #[serde(default)] (or fails when required);
    a present field must have the type (null only for an Option, `nullable`)."""
    w = _where(where, key)
    if key not in obj:
        if required:
            raise ContractError(f"{w}: missing")
        return None
    v = obj[key]
    if v is None:
        if nullable:
            return None
        raise ContractError(f"{w}: null is not allowed here")
    check(v, w)
    return v


# --------------------------------------------------------------------------- update feed (releases.json)

def _api_asset(a, where, v152):
    check_obj(a, where)
    field(a, "name", check_str, where, required=True)
    field(a, "browser_download_url", check_str, where, required=True)
    if v152:
        # update.rs (v1.5.2): `archive` and `member` are Option<String>, absent for a plain asset.
        field(a, "archive", check_str, where, nullable=True)
        field(a, "member", check_str, where, nullable=True)


def _api_release(r, where, v152):
    check_obj(r, where)
    field(r, "tag_name", check_str, where, required=True)
    field(r, "html_url", check_str, where, required=True)
    field(r, "draft", check_bool, where)
    field(r, "prerelease", check_bool, where)
    field(r, "body", check_str, where, nullable=True)
    assets = field(r, "assets", check_list_of(lambda a, w: None), where) or []
    for i, a in enumerate(assets):
        _api_asset(a, f"{where}.assets[{i}]", v152)


def _newest_asset_names(releases):
    """The names of the newest non-draft, non-prerelease release's assets (what parse_releases takes)."""
    best = None
    for r in releases:
        if r.get("draft") or r.get("prerelease"):
            continue
        m = re.match(r"^v(\d+)\.(\d+)\.(\d+)$", str(r.get("tag_name", "")))
        if not m:
            continue
        key = tuple(int(x) for x in m.groups())
        if best is None or key > best[0]:
            best = (key, r)
    if best is None:
        return None, []
    return best[1], [a.get("name") for a in best[1].get("assets") or []]


def check_feed_v141(data):
    """v1.4.1 and v1.5.0 (update.rs `ApiRelease`, `ApiAsset`): the GitHub API's shape. Both need the
    newest release to carry `SHA256SUMS` exactly (`url("SHA256SUMS")`), not `.txt`."""
    if not isinstance(data, list):
        raise ContractError("releases: the reply is a list of releases")
    for i, r in enumerate(data):
        _api_release(r, f"releases[{i}]", v152=False)
    newest, names = _newest_asset_names(data)
    if newest is not None and "SHA256SUMS" not in names:
        raise ContractError(f"{newest.get('tag_name')}: no asset named SHA256SUMS (v1.4.1 and v1.5.0 need that name)")


def check_feed_v152(data):
    """v1.5.2 .. v1.5.4 (update.rs `ApiRelease`, `ApiAsset` with `archive` and `member`; the sums asset
    is SHA256SUMS or SHA256SUMS.txt; the program is the versioned name, the fixed name, or a zip whose
    member is the fixed name)."""
    if not isinstance(data, list):
        raise ContractError("releases: the reply is a list of releases")
    for i, r in enumerate(data):
        _api_release(r, f"releases[{i}]", v152=True)
    newest, names = _newest_asset_names(data)
    if newest is None:
        return
    if not ("SHA256SUMS" in names or "SHA256SUMS.txt" in names):
        raise ContractError(f"{newest.get('tag_name')}: no SHA256SUMS or SHA256SUMS.txt")
    version = str(newest["tag_name"])[1:]
    program_ok = any(n in names for n in (f"razdor-{version}-linux", "razdor")) or any(
        a.get("archive") == "zip" and a.get("member") == "razdor" for a in newest.get("assets") or [])
    if not program_ok:
        raise ContractError(f"{newest['tag_name']}: no Linux program (razdor-{version}-linux, razdor, or a zip member razdor)")


# --------------------------------------------------------------------------- remote config (config.json)

def good_address(a):
    """remote_config.rs `good_address`: https, longer than the scheme, no whitespace, control or backslash."""
    return (isinstance(a, str) and len(a) > len("https://") and a.startswith("https://")
            and not any(c.isspace() or ord(c) < 32 or ord(c) == 127 or c == "\\" for c in a))


def check_config_v152(data):
    """v1.5.2 .. v1.5.4 (remote_config.rs `Raw`, `RawGame`). `schema` is a u32 and must be 1 (any other
    schema is the whole file ignored). The lists are non-empty https addresses (a bad one is dropped by
    the program; a list left with none is the built-in one), the game has a file name, a 64-digit SHA-256
    and a size up to MAX_ARCHIVE_DOWNLOAD. Unknown fields are ignored."""
    if not isinstance(data, dict):
        raise ContractError("config: the file is a JSON object")
    field(data, "schema", check_u32, "config", required=True)
    if data["schema"] != 1:
        raise ContractError(f"config: schema {data['schema']!r} (this program reads schema 1)")
    for key in ("update_feed", "hub"):
        lst = field(data, key, check_list_of(check_str), "config") or []
        if not lst or not all(good_address(a) for a in lst):
            raise ContractError(f"config.{key}: needs a non-empty list of https addresses")
    game = field(data, "game", check_obj, "config", nullable=True)
    if game is None:
        raise ContractError("config.game: missing (the game is needed by the publishers' checks)")
    links = field(game, "links", check_list_of(check_str), "config.game") or []
    if not links or not all(good_address(a) for a in links):
        raise ContractError("config.game.links: needs a non-empty list of https addresses")
    field(game, "size", check_u64, "config.game", required=True)
    field(game, "sha256", check_str, "config.game", required=True)
    field(game, "name", check_str, "config.game", required=True)
    if not 0 < game["size"] <= MAX_ARCHIVE_DOWNLOAD:
        raise ContractError(f"config.game.size {game['size']} is not a sane archive size")
    if not SHA256.match(game["sha256"]):
        raise ContractError("config.game.sha256 is not 64 hex digits")
    if "/" in game["name"] or not safe_name(game["name"]):
        raise ContractError(f"config.game.name {game['name']!r} is not a safe file name")


# --------------------------------------------------------------------------- hub catalog (index.json, index-v2.json)

def safe_name(name):
    """hub.rs `safe_name`: a Windows-safe file name."""
    if not name or len(name.encode("utf-8")) > 255 or name in (".", ".."):
        return False
    if name.startswith(" ") or name.endswith(" ") or name.endswith("."):
        return False
    if any(ord(c) < 32 or ord(c) == 127 or c in FORBIDDEN_NAME_CHARS for c in name):
        return False
    stem = name.split(".")[0].upper()
    numbered = len(stem) == 4 and stem[:3] in ("COM", "LPT") and stem[3] in "123456789"
    return stem not in WINDOWS_DEVICES and not numbered


def safe_rel_name(name):
    return bool(name) and all(safe_name(p) for p in name.split("/"))


def safe_location(path):
    """hub.rs `safe_location`: an https address (no whitespace or backslash) or a relative path inside the hub."""
    if path.startswith("https://"):
        return len(path) > len("https://") and not any(c.isspace() or c == "\\" for c in path)
    return bool(path) and not path.startswith("/") and "\\" not in path and all(p not in ("", "..") for p in path.split("/"))


def check_text(v, where):
    """hub.rs `Text`: {ru: string (required), en: string (default "")}."""
    check_obj(v, where)
    field(v, "ru", check_str, where, required=True)
    field(v, "en", check_str, where)


def check_wrapped(f, where, is_mod):
    """hub.rs `check_wrapped` (v1.5.2): a wrapped file names its zip's fields; a mod's zip is not wrapped."""
    if "wrapped" not in f or f["wrapped"] is None:
        if any(f.get(k) is not None for k in ("member", "archive_size", "archive_sha256")):
            raise ContractError(f"{where}: the zip's fields need \"wrapped\"")
        return
    if f["wrapped"] != "zip":
        raise ContractError(f"{where}: wrapped {f['wrapped']!r} is not a zip")
    if f.get("archive") is True:
        raise ContractError(f"{where}: a mod's zip is not wrapped again")
    if not isinstance(f.get("member"), str) or not safe_name(f["member"]):
        raise ContractError(f"{where}: member is not a safe file name")
    size = f.get("archive_size")
    if not _is_int(size) or not 0 < size <= MAX_ARCHIVE_DOWNLOAD:
        raise ContractError(f"{where}: archive_size is missing or too big")
    if not isinstance(f.get("archive_sha256"), str) or not SHA256.match(f["archive_sha256"]):
        raise ContractError(f"{where}: archive_sha256 is not 64 hex digits")


def check_file(f, where, kind):
    check_obj(f, where)
    field(f, "path", check_str, where, required=True)
    field(f, "name", check_str, where, required=True)
    field(f, "lang", check_str, where, nullable=True)
    field(f, "size", check_u64, where, required=True)
    field(f, "sha256", check_str, where, required=True)
    field(f, "archive", check_bool, where)
    field(f, "wrapped", check_str, where, nullable=True)
    field(f, "member", check_str, where, nullable=True)
    field(f, "archive_size", check_u64, where, nullable=True)
    field(f, "archive_sha256", check_str, where, nullable=True)
    name_ok = safe_rel_name(f["name"]) if kind == "mod" else safe_name(f["name"])
    if not name_ok:
        raise ContractError(f"{where}: bad file name {f['name']!r}")
    if not safe_location(f["path"]):
        raise ContractError(f"{where}: bad path {f['path']!r}")
    if not SHA256.match(f["sha256"]):
        raise ContractError(f"{where}: the SHA-256 is not 64 hex digits")
    if f.get("archive") and kind != "mod":
        raise ContractError(f"{where}: only a mod may be an archive")
    check_wrapped(f, where, kind == "mod")


def check_picture_ref(p, where):
    check_obj(p, where)
    field(p, "path", check_str, where, required=True)
    field(p, "name", check_str, where, required=True)
    field(p, "size", check_u64, where, required=True)
    field(p, "sha256", check_str, where, required=True)
    if not safe_location(p["path"]):
        raise ContractError(f"{where}: bad path {p['path']!r}")
    if not SHA256.match(p["sha256"]):
        raise ContractError(f"{where}: the SHA-256 is not 64 hex digits")


def check_entry(e, where):
    check_obj(e, where)
    field(e, "id", check_str, where, required=True)
    field(e, "kind", one_of(KINDS, "kind"), where, required=True)
    field(e, "title", check_text, where, required=True)
    field(e, "author", check_str, where, required=True)
    field(e, "version", check_str, where, required=True)
    field(e, "description", check_text, where)
    field(e, "about", check_text, where)
    field(e, "languages", check_list_of(check_str), where)
    field(e, "machine_translated", check_bool, where)
    field(e, "requires", check_list_of(check_str), where)
    field(e, "source_url", check_str, where)
    field(e, "added", check_str, where)
    field(e, "status", one_of(STATUSES, "status"), where)
    field(e, "status_note", check_text, where)
    if e.get("preview") is not None:
        check_picture_ref(e["preview"], f"{where}.preview")
    if e.get("picture") is not None:
        check_picture_ref(e["picture"], f"{where}.picture")
    field(e, "base", check_str, where)
    rating = field(e, "rating", check_obj, where, nullable=True)
    if rating is not None:
        field(rating, "up", check_u32, f"{where}.rating", required=True)
        field(rating, "down", check_u32, f"{where}.rating", required=True)
    files = field(e, "files", check_list_of(lambda v, w: None), where, required=True)
    for i, f in enumerate(files):
        check_file(f, f"{where}.files[{i}]", e["kind"])
    if not SAFE_ID.match(e["id"]):
        raise ContractError(f"{where}: bad id {e['id']!r}")
    for r in e.get("requires") or []:
        if not SAFE_ID.match(r):
            raise ContractError(f"{where}: bad required id {r!r}")


def check_game(g, where):
    """hub.rs `GameRef` and `checked`: url (https, or a path in the hub), mirrors (default []),
    size, sha256, name. A mirror that is not https is dropped by the program, not refused."""
    check_obj(g, where)
    field(g, "url", check_str, where, required=True)
    field(g, "mirrors", check_list_of(check_str), where)
    field(g, "size", check_u64, where, required=True)
    field(g, "sha256", check_str, where, required=True)
    field(g, "name", check_str, where, required=True)
    if g["url"].startswith("http://"):
        raise ContractError(f"{where}: the address must be https")
    if not safe_location(g["url"]):
        raise ContractError(f"{where}: bad address {g['url']!r}")
    if not safe_name(g["name"]):
        raise ContractError(f"{where}: bad file name {g['name']!r}")
    if not SHA256.match(g["sha256"]):
        raise ContractError(f"{where}: the SHA-256 is not 64 hex digits")
    if not 0 < g["size"] <= MAX_ARCHIVE_DOWNLOAD:
        raise ContractError(f"{where}: bad size {g['size']}")


def check_ad(a, where):
    """hub.rs `AdRef` and `checked`: id, version (u32), image, size, sha256, url, caption_ru, caption_en."""
    check_obj(a, where)
    field(a, "id", check_str, where, required=True)
    field(a, "version", check_u32, where, required=True)
    field(a, "image", check_str, where, required=True)
    field(a, "size", check_u64, where, required=True)
    field(a, "sha256", check_str, where, required=True)
    field(a, "url", check_str, where, required=True)
    field(a, "caption_ru", check_str, where, required=True)
    field(a, "caption_en", check_str, where, required=True)
    if not SAFE_ID.match(a["id"]):
        raise ContractError(f"{where}: bad id {a['id']!r}")
    if not safe_location(a["image"]) or a["image"].startswith("http://"):
        raise ContractError(f"{where}: bad picture address {a['image']!r}")
    if not SHA256.match(a["sha256"]):
        raise ContractError(f"{where}: the SHA-256 is not 64 hex digits")
    if not 0 < a["size"] <= MAX_AD_BYTES:
        raise ContractError(f"{where}: bad size {a['size']}")
    url = a["url"]
    if not (url.startswith("https://") or url.startswith("http://")) or len(url) <= 8 or any(c.isspace() or ord(c) < 32 for c in url):
        raise ContractError(f"{where}: bad link {url!r}")
    for key in ("caption_ru", "caption_en"):
        if len(a[key]) > MAX_CAPTION or any(c in "\r\n" or ord(c) < 32 for c in a[key]):
            raise ContractError(f"{where}: bad {key}")


def check_catalog_v152(data):
    """v1.5.2 .. v1.5.4 hub catalog (index-v2.json and index.json): format 1, the entries, and the
    game and ad entries (the program refuses the whole catalog on a bad game or ad)."""
    check_catalog_common(data, level="v152")


def check_catalog_v141(data):
    """v1.4.1 and v1.5.0 (GitHub's flat index.json): format 1 and the entries. Their Catalog has no
    `wrapped` fields; the game and ad entries are ignored by v1.4.1 and read by v1.5.0. The check
    here is the stricter 1.5.2 set minus the wrapped fields, which the flat catalog never has."""
    check_catalog_common(data, level="v141")


def check_catalog_common(data, level):
    check_obj(data, "catalog")
    field(data, "format", check_u32, "catalog", required=True)
    if data["format"] != 1:
        raise ContractError(f"catalog: format {data['format']} is not known")
    entries = field(data, "entries", check_list_of(lambda v, w: None), "catalog") or []
    for i, e in enumerate(entries):
        check_entry(e, f"catalog.entries[{i}]")
        if level == "v141" and any(k in f for f in e["files"] for k in ("wrapped", "member")):
            raise ContractError(f"catalog.entries[{i}]: the flat catalog has no wrapped files")
    if level == "v152":
        if data.get("game") is not None:
            check_game(data["game"], "catalog.game")
        if data.get("ad") is not None:
            check_ad(data["ad"], "catalog.ad")
    if level == "v141" and data.get("game") is not None:
        check_game(data["game"], "catalog.game")


# --------------------------------------------------------------------------- sums file and preview

def check_sums(text):
    """update.rs `sum_for` and the publishers' SHA256SUMS: one `<64 hex>  <name>` line per file."""
    lines = [l for l in text.splitlines() if l.strip()]
    if not lines:
        raise ContractError("SHA256SUMS: no lines")
    for line in lines:
        if not SUMS_LINE.match(line):
            raise ContractError(f"SHA256SUMS: bad line {line[:80]!r}")


def check_preview_png(data):
    """hub.rs `preview_ok`: a PNG with a size of 1..480 x 1..270."""
    if len(data) < 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        raise ContractError("preview: not a PNG")
    w, h = struct.unpack(">II", data[16:24])
    if not (1 <= w <= PREVIEW_MAX_W and 1 <= h <= PREVIEW_MAX_H):
        raise ContractError(f"preview: {w}x{h} is not within {PREVIEW_MAX_W}x{PREVIEW_MAX_H}")


def load_json(path):
    with open(path, "rb") as f:
        return json.loads(f.read().decode("utf-8"))


# The rule for each published file, by the versions that read it. A check of one file runs its rules.
RULES = {
    "releases.json": {"v1.4.1": check_feed_v141, "v1.5.0": check_feed_v141,
                      "v1.5.2": check_feed_v152, "v1.5.3": check_feed_v152, "v1.5.4": check_feed_v152},
    "config.json": {"v1.5.2": check_config_v152, "v1.5.3": check_config_v152, "v1.5.4": check_config_v152},
    "index-v2.json": {"v1.5.2": check_catalog_v152, "v1.5.3": check_catalog_v152, "v1.5.4": check_catalog_v152},
    "index.json": {"v1.4.1": check_catalog_v141, "v1.5.0": check_catalog_v141,
                   "v1.5.2": check_catalog_v152, "v1.5.3": check_catalog_v152, "v1.5.4": check_catalog_v152},
    # The own mirror's copies (2026-11-24): the same files with the own addresses, read by 1.5.2 .. 1.5.4 when
    # the mirror serves them (the config's and catalog's own addresses; the feed's own copy of the API list).
    "config-own.json": {"v1.5.2": check_config_v152, "v1.5.3": check_config_v152, "v1.5.4": check_config_v152},
    "index-own.json": {"v1.5.2": check_catalog_v152, "v1.5.3": check_catalog_v152, "v1.5.4": check_catalog_v152},
    "releases-own.json": {"v1.5.2": check_feed_v152, "v1.5.3": check_feed_v152, "v1.5.4": check_feed_v152},
}
