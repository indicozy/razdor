#!/usr/bin/env python3
"""Builds the community catalog for the Razdor hub from the scratch data about community mods and maps.

The output is what the hub reads (`Catalog` in `src/hub.rs`): `index.json` plus the flat asset
files it names. The assets become the files of a GitHub release on indicozy/razdor-community.

- One entry per mod with status full or partial: a zip (`archive: true`) with the files that
  differ from its base. The base is Community Update, or the built folder of the mod it is
  made on (then the entry requires that mod). Programs (`.exe`, `.dll`), the uninstaller, saves
  and logs are left out. A file the base already has is refused, byte for byte. A mod's
  `Sounds/*.raw` (and each `.wav` that halves) go in as Ogg Vorbis under the same stem.
- One entry per map thread of the maps forum (its newest version's files only; a campaign when
  it has several map files; the maps of a mod the catalog carries, requiring it). `maps-le` is
  not read.
- One entry per archive map that no forum entry and no Community Update map already covers.
- A preview picture per entry, when ImageMagick or ffmpeg is installed.

Inputs (read only): under `--src`: `discord/threads/<forum>/<id>.json`, `mods/status.tsv`,
`mods/<slug>/mod.json` and `mods/<slug>/game*/`, and for the forum maps `maps/files.tsv`,
`maps/test.tsv` and `maps/by_mod.tsv` (`--no-forum-maps` leaves them out). Under
`--archive-maps`: `maps.tsv`, `report.tsv`, `raw/` and `nested/`. `--base` is the Community
Update install.

Outputs (`--out`, cleared of its own old files first): `index.json`, `<sha256 first 16 hex>.<ext>`
assets and `report.md`. Deterministic: the same inputs give the same bytes.

Run:      python3 -I tools/community-pack/build.py --src DIR --archive-maps DIR --base DIR --out DIR
Checks:   python3 -I tools/community-pack/build.py --selftest
"""

import argparse
import bz2
import csv
import datetime
import hashlib
import json
import math
import os
import re
import shutil
import string
import struct
import subprocess
import sys
import tempfile
import wave
import zipfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

GUILD = "1007626631059361792"
FORMAT = 1
FORUMS = ("mods", "maps", "maps-le")
CU_TITLE = "Community Update"
UNKNOWN_AUTHOR = "неизвестен"
TEXT_LIMIT = 1500
DTM_HEADER_SIZE = 0x12F  # src/dt/dtm.rs HEADER_SIZE
PREVIEW_BOX = "480x270"
# A mod's Sounds files go in as Ogg Vorbis (the engine plays a same-stem .ogg in place of the named file).
# `.raw` is headerless s16le mono 22050 Hz; `.wav` is read from its header. A `.wav` changes only when its
# ogg is under half its size. The encodes of one mod run in parallel; the CPU pin keeps them off the desktop.
OGG_QUALITY = "3"
SOUND_EXT = (".raw", ".wav")
ENCODE_WORKERS = 12
CPU_PIN = ["nice", "-n", "19", "taskset", "-c", "8-19"]
CHUNK = 1 << 20
ZIP_TIME = (1980, 1, 1, 0, 0, 0)
PROGRAM_EXT = (".exe", ".dll")
# The file types the game reads (the ones Community Update ships, checked against its install). A package
# and the rebuild comparison carry only these. Everything else is left out and counted by extension in
# report.md, so a game-read type that gets left out shows up there: the authors' notes and documents
# (.txt .doc .docx .pdf .htm .css .ico ...), archives inside a mod, screenshots, programs (.exe .dll), saves.
# The notes stay in their threads; the Hub shows the first post.
GAME_EXT = frozenset({".lit", ".ugs", ".bmp", ".tga", ".spi", ".wav", ".raw", ".dtm", ".ini"})
SKIP_DIRS = ("saves/", "logs/")
ASSET_RE = re.compile(r"^[0-9a-f]{16}\.(zip|dtm|png)$")
ID_RE = re.compile(r"^[A-Za-z0-9_-]{1,64}$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
TITLE_TAIL_RE = re.compile(r"\s*\[[^\[\]]*\]\s*$")
MENTION_RE = re.compile(r"<(@!?|@&|#)(\d+)>")
EMOJI_RE = re.compile(r"<a?:\w+:\d+>")
URL_RE = re.compile(r"https?://\S+")
AUTHOR_RE = re.compile(r"Автор[ы]?\s*:\s*\**\s*([^\n<(*]+)")
ARCHIVE_AUTHOR_RE = re.compile(r"^([^\s_\-]+)-.+$")

NOTE_PROGRAM = ("Мод также меняет программу игры (DiscordTimes.exe или ddraw.dll). "
                "Программа в пакет не входит, поэтому часть возможностей мода в Razdor может не работать.")
NOTE_PART = "Загружена только часть мода: остальные его пакеты были недоступны, поэтому каких-то файлов может не хватать."
NOTE_PARTIAL = "Мод работает не со всем, что есть в оригинале, поэтому часть возможностей в Razdor может не работать."

FILES_COLS = ["thread_id", "message_id", "file_name", "file_path", "sha256", "is_newest"]
TEST_COLS = ["thread_id", "file_name", "sha256", "slug", "result"]
BYMOD_COLS = ["thread_id", "mod_slug"]
MAPS_COLS = ["flat_name", "original_relative_path", "source", "sha256", "duplicate_of"]
REPORT_COLS = ["flat_name", "cu_vanilla_result", "evo_mod_result", "first_failure_line"]


class Fail(Exception):
    """A problem that stops the build; the message says what to fix."""


class Skip(Exception):
    """An item left out of the catalog; the message is its reason in report.md."""


# ---------------------------------------------------------------- files and hashes

def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(CHUNK), b""):
            h.update(chunk)
    return h.hexdigest()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def load_json(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def read_tsv(path, columns):
    """The rows of a tab-separated file; Fail when a column the reader needs is missing."""
    if not os.path.isfile(path):
        raise Fail(f"{path} is missing")
    # surrogateescape: a name written as raw archive bytes (see package_name) must still read
    with open(path, encoding="utf-8", errors="surrogateescape", newline="") as f:
        reader = csv.DictReader(f, delimiter="\t", quoting=csv.QUOTE_NONE)
        have = reader.fieldnames or []
        lack = [c for c in columns if c not in have]
        if lack:
            raise Fail(f"{path}: no column {', '.join(lack)} (the header is {', '.join(have)})")
        return list(reader)


def package_name(rel):
    """The name a file has in a package. A name that is not UTF-8 (the DOS names some mod zips
    carry, e.g. `Посыльный.dtm` in cp866) is read as cp866: the Russian DOS code page."""
    try:
        rel.encode("utf-8")
        return rel
    except UnicodeEncodeError:
        return os.fsencode(rel).decode("cp866")


class Tree:
    """An install folder: normalized relative path ('/'-separated, see package_name) -> absolute
    path (opened by that, so a raw name still reads); hashes on demand."""

    def __init__(self, root):
        if not os.path.isdir(root):
            raise Fail(f"{root} is not a folder")
        self.root = root
        self.paths = {}
        self.decoded = set()  # normalized names that were not UTF-8 on disk
        self._sha = {}
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames.sort()
            for fn in filenames:
                full = os.path.join(dirpath, fn)
                raw = os.path.relpath(full, root).replace(os.sep, "/")
                rel = package_name(raw)
                if rel != raw:
                    self.decoded.add(rel)
                self.paths[rel] = full

    def sha(self, rel):
        if rel not in self._sha:
            self._sha[rel] = sha256_file(self.paths[rel])
        return self._sha[rel]


def same_file(base, rel, full):
    """Whether the base has the file `rel` with the same bytes as `full` (a shared inode counts)."""
    other = base.paths.get(rel)
    if other is None:
        return False
    a, b = os.stat(full), os.stat(other)
    if (a.st_dev, a.st_ino) == (b.st_dev, b.st_ino):
        return True
    return a.st_size == b.st_size and sha256_file(full) == base.sha(rel)


def changed_files(game, base):
    """(rel, path) of every file of the `game` tree that is new or differs from the base (programs included)."""
    return [(rel, full) for rel, full in sorted(game.paths.items()) if not same_file(base, rel, full)]


ENGLISH_PREFIX = "eng"
RAZDOR_HEADER = "; Razdor translation:"


def is_translated_map(name):
    """Mirror of `is_translated_file` in src/translate/job.rs: `Name_<lang>.DTm` with a non-ASCII name
    and a lowercase language suffix (2 to 27 of a-z, 0-9, `-`). The translator writes these copies next
    to the originals, and the repo drops them from the map lists (dt/install.rs, the editor)."""
    stem = os.path.splitext(name)[0]
    if "_" not in stem:
        return False
    base, suffix = stem.rsplit("_", 1)
    return (2 <= len(suffix) <= 27 and all(c in string.ascii_lowercase + string.digits + "-" for c in suffix)
            and not base.isascii())


def razdor_prefixes(tree):
    """The data-file prefixes Razdor writes into an install (src/dt/english.rs): `Eng`, and the prefix of
    each `<Prefix>_Units.ini` whose first line is Razdor's header `; Razdor translation: <language>`."""
    out = {ENGLISH_PREFIX}
    for rel, full in tree.paths.items():
        m = re.match(r"^(.+)_Units\.ini$", rel.rsplit("/", 1)[-1], re.IGNORECASE)
        if not m or m.group(1).lower() == "rus":
            continue
        with open(full, encoding="utf-8", errors="replace") as f:
            first = f.readline().lstrip("﻿")
        if first.startswith(RAZDOR_HEADER):
            out.add(m.group(1).lower())
    return out


def razdor_made(rel, prefixes):
    """Whether a file is Razdor's own output written into an install, never part of the game or a mod:
    a translated map copy (is_translated_map) or a data file of a Razdor language (razdor_prefixes)."""
    low = rel.lower().rsplit("/", 1)[-1]
    if low.endswith(".dtm"):
        return is_translated_map(rel.rsplit("/", 1)[-1])
    return low.endswith(".ini") and any(low.startswith(p + "_") for p in prefixes)


def excluded(rel, prefixes=frozenset({ENGLISH_PREFIX})):
    """Files a package leaves out: any type not in GAME_EXT (programs, documents, archives, saves, logs),
    the uninstaller, crash logs, and Razdor's own translation output (razdor_made)."""
    low = rel.lower()
    name = low.rsplit("/", 1)[-1]
    return (os.path.splitext(name)[1] not in GAME_EXT or name.startswith("unins") or low.startswith(SKIP_DIRS)
            or "crashlog" in name or razdor_made(rel, prefixes))


def build_zip(members, zip_path):
    """A deflate (level 9) zip of (rel, path) members, with fixed times and modes: the same files give the same bytes."""
    with zipfile.ZipFile(zip_path, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for rel, full in sorted(members):
            with open(full, "rb") as f:
                data = f.read()
            info = zipfile.ZipInfo(rel, date_time=ZIP_TIME)
            info.create_system = 3
            info.external_attr = 0o644 << 16
            zf.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def package_ogg(rel):
    """Whether a package member is an encoded sound (an .ogg under Sounds/, see package_sounds). A mod's own
    .ogg files are not: `excluded` leaves them out of the members, so they never reach a package."""
    parts = rel.split("/")
    return len(parts) > 1 and parts[0].lower() == "sounds" and rel.lower().endswith(".ogg")


def ffmpeg_available():
    return shutil.which("ffmpeg") is not None


def ogg_encode(src_path, ext, cache_dir):
    """The Ogg Vorbis (quality 3, mono) of a sound file, cached as `<sha256 of its bytes>-q3.ogg` in cache_dir.
    A `.raw` is headerless s16le 22050 Hz mono; a `.wav` is read from its header. Encoded to a temp name, then
    renamed, so a cached file is always whole. Runs ffmpeg at the CPU pin (CPUs 8-19, nice 19)."""
    final = os.path.join(cache_dir, f"{sha256_file(src_path)}-q3.ogg")
    if os.path.isfile(final):
        return final
    os.makedirs(cache_dir, exist_ok=True)
    work = tempfile.mkdtemp(prefix=".enc-", dir=cache_dir)
    try:
        out = os.path.join(work, "out.ogg")
        argv = CPU_PIN + ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error"]
        if ext == ".raw":
            argv += ["-f", "s16le", "-ar", "22050", "-ac", "1"]
        argv += ["-i", src_path, "-c:a", "libvorbis", "-q:a", OGG_QUALITY, "-ac", "1",
                 "-map_metadata", "-1", "-fflags", "+bitexact", "-flags:a", "+bitexact", out]
        try:
            done = subprocess.run(argv, capture_output=True, encoding="utf-8", errors="replace")
        except OSError as e:
            raise Fail(f"cannot run ffmpeg (or the CPU pin, nice and taskset): {e}")
        if done.returncode != 0 or not os.path.isfile(out) or os.path.getsize(out) == 0:
            raise Fail(f"ffmpeg could not encode {src_path}: {done.stderr.strip()[-300:] or 'no output'}")
        os.replace(out, final)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return final


def package_sounds(slug, members, game, cache_dir, notes):
    """The members of a package with each `Sounds/` `.raw` (and each `.wav` whose ogg is under half its size)
    replaced by its Ogg Vorbis under the same folder and stem, extension `.ogg`. Returns (members, bytes saved,
    number replaced). Two members that would share an ogg name (ignoring case) keep their originals, and so
    does a member whose mod ships a same-stem `.ogg` itself; both are noted. The encodes run in parallel."""
    def ogg_key(rel):
        return os.path.splitext(rel)[0].lower() + ".ogg"

    shipped = {rel.lower() for rel in game.paths}
    cands = [i for i, (rel, _) in enumerate(members)
             if len(rel.split("/")) > 1 and rel.split("/")[0].lower() == "sounds"
             and os.path.splitext(rel)[1].lower() in SOUND_EXT]
    count = Counter(ogg_key(members[i][0]) for i in cands)
    todo = []
    for i in cands:
        rel = members[i][0]
        if count[ogg_key(rel)] > 1:
            notes.append(f"{slug}: {rel} and another Sounds file would share the name {ogg_key(rel)}; both are packed as they are.")
        elif ogg_key(rel) in shipped:
            notes.append(f"{slug}: the mod ships {ogg_key(rel)} itself; {rel} is packed as it is.")
        else:
            todo.append(i)
    with ThreadPoolExecutor(max_workers=ENCODE_WORKERS) as pool:
        futures = {i: pool.submit(ogg_encode, members[i][1], os.path.splitext(members[i][0])[1].lower(), cache_dir)
                   for i in todo}
        encoded = {i: f.result() for i, f in futures.items()}
    out, saved, replaced = list(members), 0, 0
    for i in todo:
        rel, full = members[i]
        src_size, ogg_size = os.path.getsize(full), os.path.getsize(encoded[i])
        if os.path.splitext(rel)[1].lower() == ".wav" and ogg_size * 2 >= src_size:
            continue  # the wav stays: its ogg is not under half of it
        out[i] = (os.path.splitext(rel)[0] + ".ogg", encoded[i])
        saved += src_size - ogg_size
        replaced += 1
    return out, saved, replaced


def verify_package(zip_path, base):
    """Refuses a package with an excluded file or with a file the base already has, byte for byte. An encoded
    sound (package_ogg) must be an Ogg file (`OggS`)."""
    prefixes = razdor_prefixes(base)
    with zipfile.ZipFile(zip_path) as zf:
        for info in zf.infolist():
            rel = info.filename
            if package_ogg(rel):
                if not zf.read(info).startswith(b"OggS"):
                    raise Fail(f"{zip_path}: {rel} is not an Ogg file")
            elif excluded(rel, prefixes):
                raise Fail(f"{zip_path}: {rel} must not be in a package")
            if rel in base.paths and sha256_bytes(zf.read(info)) == base.sha(rel):
                raise Fail(f"{zip_path}: {rel} is the base's own file, byte for byte; the package must not carry it")


def dtm_container_payload(data):
    """The payload of a `.DTm` in the `AIpf` container (src/dt/container.rs `decode`): the legacy layout is one
    bzip2 stream after 12 bytes (scramble mode 1 XORs byte i with i+1); the new layout is chunks of (u32 length,
    bzip2 stream), with `size div block + 1` chunks."""
    if data[1:2] == b"I":
        body = bytearray(data[12:])
        if data[7] == 1:
            for i in range(len(body)):
                body[i] ^= (i + 1) & 0xFF
        elif data[7] != 0:
            raise Fail(f"scramble mode {data[7]} cannot be read")
        return bz2.decompress(bytes(body))
    block = struct.unpack_from("<H", data, 4)[0] * 1024
    size = struct.unpack_from("<I", data, 8)[0]
    chunks = 1 if block == 0 else size // block + 1
    at, out = 12, bytearray()
    for _ in range(chunks):
        n = struct.unpack_from("<I", data, at)[0]
        out += bz2.decompress(data[at + 4:at + 4 + n])
        at += 4 + n
    return bytes(out)


def dtm_text(raw):
    """A string of the text section (src/dt/text.rs `decode`): UTF-8 when it is valid and has a character cp1251
    cannot hold, else cp1251."""
    if any(b >= 0x80 for b in raw):
        try:
            s = raw.decode("utf-8")
            if any(not _in_cp1251(c) for c in s):
                return s
        except UnicodeDecodeError:
            pass
    return raw.decode("cp1251", errors="replace")


def _in_cp1251(c):
    try:
        c.encode("cp1251")
        return True
    except UnicodeEncodeError:
        return False


def dtm_next_map(path):
    """The next-map name a `.DTm` holds: the fourth string at the header's text offset (src/dt/dtm.rs
    `parse_payload`; the title, description and campaign name come before it). None for a file that is not a map."""
    with open(path, "rb") as f:
        data = f.read()
    if len(data) >= 4 and data[0:1] == b"A" and data[2:3] == b"p" and data[3:4] == b"f":
        data = dtm_container_payload(data)
    if len(data) < DTM_HEADER_SIZE or data[9] < ord("4"):
        return None
    pos = struct.unpack_from("<I", data, 0x18)[0]
    strings = []
    for _ in range(4):
        end = data.find(b"\0", pos)
        end = len(data) if end < 0 else end
        strings.append(dtm_text(data[pos:end]) if pos < len(data) else "")
        pos = end + 1
    return strings[3]


def game_ref_key(ref):
    """The key a next-map field is looked up by, following the fixed `next_campaign_map` (src/ui/screens.rs): the
    field trimmed, the map extension `.dtm` removed whatever its case (Windows opens files ignoring case), lower
    case. Quotes are NOT stripped, so a field in quotes finds nothing."""
    s = ref.strip()
    if s.lower().endswith(".dtm"):
        s = s[:-4]
    return s.lower()


def game_name_key(name):
    """The key of a chapter file: its name without its extension, lower case (the game's `ScenarioEntry.file`)."""
    return os.path.splitext(name)[0].lower()


REASON_TEXT = {
    "quoted": "in quotes; the game does not find it",
    "extension": "the name has a different extension than the map's, so the game does not find it",
    "missing": "no chapter of the campaign has that name",
}


def unresolved_reason(ref, chapter_names):
    """Why the game does not find a next-map name (kinds in REASON_TEXT): a quote character in it, a chapter with
    the same stem but another extension, or no such chapter."""
    if '"' in ref:
        return "quoted"
    if any(game_name_key(n) == game_name_key(ref.strip()) for n in chapter_names):
        return "extension"
    return "missing"


def chain_position(links, chapter):
    """The 1-based place of `chapter` in the chain of chapters it belongs to (a chain starts at a chapter no other
    chapter names). Falls back to the campaign's size when no chain reaches it."""
    targets = {t for t in links.values() if t}
    for start in (n for n in links if n not in targets):
        seen, count, cur = set(), 0, start
        while cur is not None and cur not in seen:
            seen.add(cur)
            count += 1
            if cur == chapter:
                return count
            cur = links.get(cur)
    return len(links)


class Assets:
    """The asset files of the release: each named by its content, `<sha256 first 16>.<ext>`."""

    def __init__(self, out):
        self.out = out
        self.full = {}    # name -> full sha256
        self.sizes = {}   # name -> size in bytes
        self.owners = {}  # name -> entry ids that use it

    def add(self, path, ext, owner):
        digest = sha256_file(path)
        name = digest[:16] + ext
        if name in self.full and self.full[name] != digest:
            raise Fail(f"asset name clash on {name}")
        if name not in self.full:
            final = os.path.join(self.out, name)
            shutil.copyfile(path, final + ".part")
            os.replace(final + ".part", final)
            self.full[name] = digest
            self.sizes[name] = os.path.getsize(final)
        self.owners.setdefault(name, set()).add(owner)
        return {"path": name, "size": self.sizes[name], "sha256": digest}


# ---------------------------------------------------------------- text

def strip_markup(text, users):
    """The plain text of a Discord message: mentions become names (or go), emoji go, the bold,
    strike, spoiler and code marks go; links stay whole (they have underscores)."""
    out, pos = [], 0
    for m in URL_RE.finditer(text):
        out.append(_plain(text[pos:m.start()], users))
        out.append(m.group(0))
        pos = m.end()
    out.append(_plain(text[pos:], users))
    return "".join(out).strip()


def _plain(seg, users):
    def mention(m):
        return users.get(m.group(2), "") if m.group(1) in ("@", "@!") else ""

    seg = MENTION_RE.sub(mention, seg)
    seg = EMOJI_RE.sub("", seg)
    seg = re.sub(r"\*\*|__|~~|\|\||`|\*", "", seg)
    seg = re.sub(r"\(\s*\)", "", seg)
    seg = re.sub(r"[ \t]+", " ", seg)
    seg = re.sub(r" *\n *", "\n", seg)
    return re.sub(r"\n{3,}", "\n\n", seg)


def cut_text(text, limit=TEXT_LIMIT):
    """`text` cut to `limit` characters at its last sentence end; a hard cut at a word when there is none."""
    if len(text) <= limit:
        return text
    window = text[:limit]
    ends = [m.end() for m in re.finditer(r"[.!?…](?=\s|$)", window)]
    if ends:
        return window[:ends[-1]].rstrip()
    space = window.rfind(" ")
    return (window[:space] if space > 0 else window).rstrip()


def clean_title(name):
    """A thread's name without its trailing `[...]` bracket."""
    return TITLE_TAIL_RE.sub("", name).strip() or name.strip()


def clean_version(raw):
    """A version without the decoration (`** 2.0` is 2.0; a line of symbols is no version)."""
    return re.sub(r"[^\w .\-]", "", raw or "").strip()


def named_author(content, users):
    """The name a first post gives in its `Автор: …` line, or ''."""
    m = AUTHOR_RE.search(content)
    return strip_markup(m.group(1), users).strip() if m else ""


WIN_DEVICES = {"CON", "PRN", "AUX", "NUL"} | {f"COM{i}" for i in range(1, 10)} | {f"LPT{i}" for i in range(1, 10)}
BAD_NAME_CHARS = set('/\\<>:"|?*')


def hub_name_problem(name):
    """Why the hub cannot store this file name (None when it can). The hub keeps any Unicode
    name except these, so one name works on every platform."""
    try:
        size = len(name.encode("utf-8"))
    except UnicodeEncodeError:
        return "not valid Unicode"
    if not name or name in (".", ".."):
        return "empty, or . or .."
    if size > 255:
        return "over 255 UTF-8 bytes"
    if any(ord(c) < 32 or ord(c) == 127 for c in name):
        return "control character"
    bad = sorted(c for c in name if c in BAD_NAME_CHARS)
    if bad:
        return f"character {bad[0]!r}"
    if name != name.strip(" "):
        return "leading or trailing space"
    if name.endswith("."):
        return "trailing dot"
    if name.split(".")[0].upper() in WIN_DEVICES:
        return "Windows device name"
    return None


def file_date(path):
    """The date the file carries (its mtime, kept from the archive it was extracted from), YYYY-MM-DD."""
    return datetime.datetime.fromtimestamp(os.path.getmtime(path), datetime.timezone.utc).strftime("%Y-%m-%d")


def archive_author(source):
    """The author an archive's name gives (`Author-Title`), and the prefix to take off its maps' names."""
    m = ARCHIVE_AUTHOR_RE.match(source)
    if m:
        return m.group(1), m.group(1)
    return UNKNOWN_AUTHOR, ""


def archive_title(base, prefix):
    """A map's title from its file name: no `.final`, no `Author-` prefix, underscores as spaces."""
    stem = os.path.splitext(base)[0]
    if stem.lower().endswith(".final"):
        stem = stem[:-6]
    if prefix and stem.lower().startswith(prefix.lower() + "-"):
        stem = stem[len(prefix) + 1:]
    return " ".join(stem.replace("_", " ").split()) or stem


def thread_url(tid):
    return f"https://discord.com/channels/{GUILD}/{tid}"


# ---------------------------------------------------------------- threads

def load_threads(src):
    threads = {}
    for forum in FORUMS:
        d = os.path.join(src, "discord", "threads", forum)
        if not os.path.isdir(d):
            raise Fail(f"{d} is missing")
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".json"):
                t = load_json(os.path.join(d, fn))
                t["forum"] = forum
                threads[str(t["id"])] = t
    return threads


def user_names(threads):
    """Discord user id -> display name, from every message the crawl has."""
    names = {}
    for t in threads.values():
        for m in t.get("messages") or []:
            a = m.get("author") or {}
            name = a.get("global_name") or a.get("username")
            if a.get("id") and name:
                names.setdefault(str(a["id"]), name)
    return names


def first_post(t):
    msgs = t.get("messages") or []
    for m in msgs:
        if m.get("id") == t["id"]:
            return m
    return msgs[0] if msgs else None


def first_image(t):
    """The local file of the first image of the first post, or None."""
    for a in (first_post(t) or {}).get("attachments") or []:
        if a.get("kind") == "image" and a.get("local") and os.path.isfile(a["local"]):
            return a["local"]
    return None


def created_date(t):
    c = (t.get("created") or "")[:10]
    return c if DATE_RE.match(c) else ""


def newest_version(t, rows):
    """The files of the newest post that has map files: the rows flagged `is_newest`, else the
    post with the latest time. Sorted by file name."""
    flagged = [r for r in rows if r["is_newest"] == "yes"]
    if flagged:
        return sorted(flagged, key=lambda r: r["file_name"])
    times = {str(m.get("id")): m.get("time") or "" for m in t.get("messages") or []}
    groups = {}
    for r in rows:
        groups.setdefault(r["message_id"], []).append(r)
    newest = max(groups, key=lambda mid: (times.get(mid, ""), mid))
    return sorted(groups[newest], key=lambda r: r["file_name"])


# ---------------------------------------------------------------- previews

def find_converter():
    """(tool, exe): ImageMagick (magick, else convert), else ffmpeg. Previews are PNG (the hub reads only PNG)."""
    for name in ("magick", "convert"):
        exe = shutil.which(name)
        if exe:
            return ("im", exe)
    exe = shutil.which("ffmpeg")
    return ("ffmpeg", exe) if exe else None


def make_preview(conv, src, work):
    """Writes the picture as a PNG into `work`, scaled down to fit 480x270 (never up), with its
    colours cut to 256 where ImageMagick does it; its path, or None when the tool fails."""
    tool, exe = conv
    out = os.path.join(work, "preview.png")
    if tool == "im":
        cmd = [exe, src, "-strip", "-resize", PREVIEW_BOX + ">", "-colors", "256", out]
    else:
        scale = "scale=480:270:force_original_aspect_ratio=decrease"
        cmd = [exe, "-hide_banner", "-loglevel", "error", "-y", "-i", src, "-frames:v", "1", "-vf", scale,
               "-c:v", "png", out]
    try:
        subprocess.run(cmd, capture_output=True, timeout=120, check=True)
    except (OSError, subprocess.SubprocessError):
        return None
    return out if os.path.isfile(out) else None


# ---------------------------------------------------------------- entries

def file_ref(asset, name, lang=None, archive=False):
    ref = {"path": asset["path"], "name": name}
    if lang:
        ref["lang"] = lang
    ref.update(size=asset["size"], sha256=asset["sha256"], archive=archive)
    return ref


def make_entry(eid, kind, meta, version, languages, requires, status, note, files, base="", preview=None):
    entry = {
        "id": eid,
        "kind": kind,
        "title": {"ru": meta["title"], "en": ""},
        "author": meta["author"],
        "version": version,
        "description": {"ru": meta["description"], "en": ""},
        "languages": languages,
        "machine_translated": False,
        "requires": requires,
        "source_url": meta["source_url"],
        "added": meta["added"],
        "status": status,
        "status_note": {"ru": note.get("ru", ""), "en": note.get("en", "")},
        "base": base,
        "files": files,
    }
    if preview:
        entry["preview"] = preview
    return entry


def mod_note(status, download, program):
    if status == "full":
        return ""
    parts = []
    if program:
        parts.append(NOTE_PROGRAM)
    if download.startswith("partial"):
        parts.append(NOTE_PART)
    return " ".join(parts) or NOTE_PARTIAL


def test_result(row):
    """(ok, reason) of a test row whose result is `ok`, `FAIL <reason>` or `hang`."""
    res = row["result"].strip()
    if res == "ok":
        return True, ""
    if res.startswith("FAIL"):
        return False, res[4:].strip() or "the smoke test failed"
    if res == "hang":
        return False, "hang (no end in the test time)"
    return False, res or "not ok"


def test_mod(row):
    """The mod a test row names. `CU` is no mod (vanilla, `vanilla`); `mod <slug>` is that mod; `CU (no build
    for guessed mod)` was played on Community Update, but its mod is the row's `guessed_slug`, which has no
    build here (so the map needs that mod and gets no entry unless the catalog has it)."""
    slug = (row.get("slug") or "").strip()
    if slug.lower().startswith("cu (no build"):
        return (row.get("guessed_slug") or "").strip()
    if slug.lower().startswith("mod "):
        return slug[4:].strip()
    if slug.lower() == "cu":
        return "vanilla"
    return slug


def map_mod(rows, test_of, bymod):
    for r in rows:
        t = test_of.get(r["sha256"])
        if t and test_mod(t):
            return test_mod(t)
    return bymod


def map_status(rows, test_of):
    found = [test_of.get(r["sha256"]) for r in rows]
    bad = [why for t in found if t for ok, why in [test_result(t)] if not ok]
    if bad:
        return "broken", {"ru": f"Не проходит проверку: {bad[0]}", "en": bad[0]}
    if any(t is None for t in found):
        return "unknown", {}
    return "full", {}


def archive_status(rep):
    if rep is None:
        return "unknown", {}
    if rep["cu_vanilla_result"] == "ok" and rep["evo_mod_result"] == "ok":
        return "full", {}
    why = rep["first_failure_line"] or "the smoke test failed"
    return "broken", {"ru": f"Не проходит проверку: {why}", "en": why}


def archive_path(adir, row):
    rel = row["original_relative_path"]
    if row["source"] == "top":
        return os.path.join(adir, "raw", rel)
    return os.path.join(adir, "nested", row["source"], rel)


# ---------------------------------------------------------------- the pack

class Pack:
    """The catalog being built: its entries, asset store, skip, rename and note lists."""

    def __init__(self, src, archive, base_dir, out, work, extra=None, fixes=None):
        self.src = src
        self.archive = archive
        self.base_dir = base_dir
        self.out = out
        self.work = work
        self.extra = extra or {}  # thread id -> zip of its map files (the owner's downloads, see --extra)
        self.fixes = fixes or {}  # thread id -> chain fixes (add, drop, break), see --fixes
        self.chain_log = []       # what the campaign fixes did, for report.md
        self.failures = []        # campaign links that do not resolve and are not marked break
        self.threads = load_threads(src)
        self.users = user_names(self.threads)
        self.assets = Assets(out)
        self.conv = find_converter()
        self.entries = []
        self.mod_ids = set()
        self.map_shas = set()
        self.names = set()
        self.skipped = []
        self.notes = []
        self.pkgs = {}        # mod slug -> its package: {lower name: (name, sha256)}
        self.chains = {}      # mod slug -> the mods whose packages come before its own, in order
        self.mismatches = []  # mods whose rebuild differs from their tested game folder
        self.rebuild_log = [] # (slug, chain, problems) for report.md
        self.sound_log = {}   # mod slug -> (files packed as Ogg Vorbis, bytes saved), for report.md
        self.ogg_cache = os.path.join(src, "ogg-cache")  # the encodes, kept across runs (see ogg_encode)
        self.left_out = {}    # extension -> [files left out of packages, set of mod slugs]
        self.trees = {}
        self.cu_maps = None
        if self.conv is None:
            self.notes.append("No image converter (magick, convert or ffmpeg) was found: no entry has a preview.")

    def skip(self, what, reason):
        self.skipped.append((what, reason))

    def tree(self, path):
        if path not in self.trees:
            self.trees[path] = Tree(path)
        return self.trees[path]

    def cu(self):
        return self.tree(self.base_dir)

    def cu_map_shas(self):
        if self.cu_maps is None:
            cu = self.cu()
            self.cu_maps = {cu.sha(rel) for rel in cu.paths if rel.lower().endswith(".dtm")}
        return self.cu_maps

    def add_entry(self, entry):
        if any(e["id"] == entry["id"] for e in self.entries):
            raise Fail(f"two entries have the id {entry['id']}")
        self.entries.append(entry)
        if entry["kind"] == "mod":
            self.mod_ids.add(entry["id"])

    def claim_names(self, raws):
        """Claims the file names of an entry, kept as they are. Refused (Skip) when the hub cannot store one,
        or when another map already has it (the hub keeps all maps in one folder; names ignore case)."""
        local = set()
        for raw in raws:
            why = hub_name_problem(raw)
            if why:
                raise Skip(f"file name {raw!r} breaks the hub's name rules: {why}")
            key = raw.lower()
            if key in self.names or key in local:
                raise Skip(f"file name {raw!r} is already used by another map")
            local.add(key)
        self.names |= local

    def preview(self, image, owner):
        if image is None or self.conv is None:
            return None
        out = make_preview(self.conv, image, self.work)
        if out is None:
            self.notes.append(f"No preview for {owner}: the converter failed on {os.path.basename(image)}.")
            return None
        ext = os.path.splitext(out)[1]
        asset = self.assets.add(out, ext, owner)
        os.remove(out)
        return {"path": asset["path"], "name": "preview" + ext, "size": asset["size"],
                "sha256": asset["sha256"], "archive": False}

    def thread_fields(self, t, title, author):
        """Title, author, description, date and link of a thread (its first post); `title` and `author`
        are the fallbacks when there is no thread."""
        if t is None:
            return {"title": title, "author": author, "description": "", "added": "",
                    "source_url": "", "image": None}
        content = (first_post(t) or {}).get("content") or ""
        return {
            "title": clean_title(t["name"]),
            "author": named_author(content, self.users) or self.users.get(str(t.get("owner_id")), "") or author,
            "description": cut_text(strip_markup(content, self.users)),
            "added": created_date(t),
            "source_url": thread_url(t["id"]),
            "image": first_image(t),
        }

    # mods

    def mod_json(self, slug):
        return load_json(os.path.join(self.src, "mods", slug, "mod.json"))

    def mod_base(self, slug, mod):
        """(base title, the mod it is made on or None, that mod's folder) for a mod's `base` field."""
        b = mod.get("base") or ""
        if b == "cu" or (b == "evo" and slug == "evolution"):
            return (CU_TITLE, None, None)
        if b == "evo":
            return (self.mod_json("evolution")["title"], "evolution", "game")
        if b == "evo9":
            return (self.mod_json("evolution")["title"], "evolution", "game")
        if b.startswith("mod:"):
            dep = b[4:]
            return (self.mod_json(dep)["title"], dep, "game")
        raise Skip(f"base {b!r} is not a known install")

    def base_spec(self, slug):
        try:
            return self.mod_base(slug, self.mod_json(slug))
        except Skip as e:
            return e
        except (OSError, KeyError) as e:
            return Skip(f"no usable mod.json ({e})")

    def add_mods(self):
        rows = {r["slug"]: r for r in read_tsv(os.path.join(self.src, "mods", "status.tsv"),
                                                ["slug", "status", "download ok?"])}
        if any(r["status"] in ("full", "partial") for r in rows.values()) and not ffmpeg_available():
            raise Fail("ffmpeg is needed to pack the mods' Sounds as Ogg Vorbis: install it (it must be on PATH)")
        specs = {s: self.base_spec(s) for s in rows}
        # A mod made on another mod comes after it: that mod's folder is its base.
        order = sorted(rows, key=lambda s: (isinstance(specs[s], tuple) and specs[s][1] is not None, s))
        for slug in order:
            try:
                self.add_mod(slug, rows[slug], specs[slug])
            except Skip as e:
                self.skip(f"mod {slug}", str(e))

    def add_mod(self, slug, row, spec):
        status = row["status"]
        if status not in ("full", "partial"):
            raise Skip(f"status {status} ({(row.get('note') or '').strip()[:160]})")
        if isinstance(spec, Skip):
            raise spec
        base_title, dep, dep_dir = spec
        if dep is not None and dep not in self.mod_ids:
            raise Skip(f"its base mod {dep} has no entry")
        base = self.cu() if dep is None else self.tree(os.path.join(self.src, "mods", dep, dep_dir))
        game_dir = os.path.join(self.src, "mods", slug, "game")
        if not os.path.isdir(game_dir):
            raise Skip(f"no built folder at mods/{slug}/game")
        game = self.tree(game_dir)
        diffs = changed_files(game, base)
        prefixes = razdor_prefixes(game) | razdor_prefixes(base)
        members = [(rel, full) for rel, full in diffs if not excluded(rel, prefixes)]
        for rel, _ in diffs:
            if excluded(rel, prefixes):
                ext = os.path.splitext(rel)[1].lower() or "(no extension)"
                entry = self.left_out.setdefault(ext, [0, set()])
                entry[0] += 1
                entry[1].add(slug)
        if not members:
            raise Skip("no file differs from its base")
        decoded = [rel for rel, _ in members if rel in game.decoded]
        if decoded:
            self.notes.append(f"{slug}: {len(decoded)} file name(s) were not UTF-8 and are read as cp866, e.g. `{decoded[0]}`.")
        program = any(rel.lower().endswith(PROGRAM_EXT) and not rel.lower().rsplit("/", 1)[-1].startswith("unins")
                      for rel, _ in diffs)
        for rel, _ in members:
            why = next((hub_name_problem(p) for p in rel.split("/") if hub_name_problem(p)), None)
            if why:
                raise Skip(f"file {rel!r} breaks the hub's name rules: {why}")
        own = {rel.lower(): (rel, game.sha(rel)) for rel, _ in members}
        before = (self.chains[dep] + [dep]) if dep else []
        problems = self.rebuild_problems(game, before, own)
        self.rebuild_log.append((slug, before, problems))
        if problems:
            self.mismatches.append(slug)
            raise Skip(f"does not rebuild from its chain ({len(problems)} difference(s), see the rebuild check)")
        self.pkgs[slug] = own
        self.chains[slug] = before
        members, saved, replaced = package_sounds(slug, members, game, self.ogg_cache, self.notes)
        if replaced:
            self.sound_log[slug] = (replaced, saved)
        zip_path = os.path.join(self.work, slug + ".zip")
        build_zip(members, zip_path)
        verify_package(zip_path, base)
        asset = self.assets.add(zip_path, ".zip", slug)
        os.remove(zip_path)
        mod = self.mod_json(slug)
        thread = self.threads.get(str(mod.get("thread", "")))
        meta = self.thread_fields(thread, mod["title"], mod.get("author", ""))
        preview = self.preview(meta["image"], slug)
        note = {"ru": mod_note(status, row.get("download ok?", ""), program)} if status == "partial" else {}
        self.add_entry(make_entry(slug, "mod", meta, clean_version(mod.get("version", "")), [],
                                  [dep] if dep else [], status, note,
                                  [file_ref(asset, slug + ".zip", archive=True)],
                                  base=base_title, preview=preview))

    def rebuild_problems(self, game, before, own):
        """What differs between the game folder and Community Update plus the packages of `before` (in
        order) plus the mod's own package. Names compare ignoring case, the way an overlay does;
        executables, DLLs, the uninstaller, saves and logs are left out on both sides."""
        cu = self.cu()
        state = {rel.lower(): (rel, None) for rel in cu.paths}  # None: the Community Update file's own hash
        for slug in before:
            state.update(self.pkgs[slug])
        state.update(own)
        prefixes = razdor_prefixes(cu) | razdor_prefixes(game)
        have = {k: (rel, cu.sha(rel) if sha is None else sha) for k, (rel, sha) in state.items()
                if not excluded(rel, prefixes)}
        want = {rel.lower(): (rel, game.sha(rel)) for rel in game.paths if not excluded(rel, prefixes)}
        problems = [f"missing {want[k][0]}" for k in sorted(set(want) - set(have))]
        problems += [f"extra {have[k][0]}" for k in sorted(set(have) - set(want))]
        problems += [f"differs {want[k][0]}" for k in sorted(set(want) & set(have)) if want[k][1] != have[k][1]]
        return problems

    # forum maps

    def add_forum_maps(self):
        maps = os.path.join(self.src, "maps")
        try:
            files = read_tsv(os.path.join(maps, "files.tsv"), FILES_COLS)
            tests = read_tsv(os.path.join(maps, "test.tsv"), TEST_COLS)
            by_mod = read_tsv(os.path.join(maps, "by_mod.tsv"), BYMOD_COLS)
        except Fail as e:
            raise Fail(f"{e} (run with --no-forum-maps to leave the forum maps out)") from None
        test_of = {r["sha256"]: r for r in tests}
        mod_of = {r["thread_id"]: r["mod_slug"] for r in by_mod}
        groups = {}
        for r in files:
            groups.setdefault(r["thread_id"], []).append(r)
        for tid, path in sorted(self.extra.items()):
            if tid not in self.threads or self.threads[tid]["forum"] != "maps":
                raise Fail(f"--extra {tid}: no map thread {tid} in the crawl")
            groups[tid] = self.extra_rows(tid, path)
            for r in groups[tid]:
                # owner's download, tested ok on Community Update in an earlier run
                test_of[r["sha256"]] = {"slug": "vanilla", "result": "ok"}
        for tid, t in sorted(self.threads.items()):
            if t["forum"] != "maps" or tid in groups:
                continue
            self.skip(f"map thread {tid} {t['name']}", "no map file in the crawl")
        for tid in sorted(groups):
            t = self.threads.get(tid)
            if t is None:
                self.skip(f"map thread {tid}", "no thread JSON in the crawl")
                continue
            if t["forum"] != "maps":
                continue
            try:
                self.add_forum_map(t, groups[tid], test_of, mod_of.get(tid, ""))
            except Skip as e:
                self.skip(f"map thread {tid} {t['name']}", str(e))
        left = sum(1 for t in self.threads.values() if t["forum"] == "maps-le")
        self.notes.append(f"The maps-le forum is not read ({left} threads).")

    def add_forum_map(self, t, rows, test_of, bymod):
        tid = str(t["id"])
        newest = newest_version(t, rows)
        cu_maps = self.cu_map_shas()
        keep = [r for r in newest if r["sha256"] not in cu_maps]
        if not keep:
            raise Skip("every map of its newest version is one Community Update ships")
        fix = self.fixes.get(tid, {})
        drop = set(fix.get("drop", []))
        keep = [r for r in keep if r["file_name"] not in drop]
        for add in fix.get("add", []):
            if add["name"].lower() in {package_name(r["file_name"]).lower() for r in keep}:
                self.chain_log.append(f"{tid}: `{add['name']}` is already one of its chapters (not added again)")
                continue
            if not os.path.isfile(add["file"]):
                raise Fail(f"fixes for {tid}: {add['file']} is missing")
            sha = sha256_file(add["file"])
            if add.get("result") == "ok":
                test_of[sha] = {"slug": add.get("slug", "vanilla"), "result": "ok"}
            keep.append({"file_name": add["name"], "file_path": add["file"], "sha256": sha})
            self.chain_log.append(f"{tid}: added `{add['name']}` from `{add['file']}`" +
                                  (f" ({add['note']})" if add.get("note") else ""))
        if drop:
            self.chain_log.append(f"{tid}: dropped {', '.join(sorted(drop))}")
        if not keep:
            raise Skip("nothing left after the chain fixes")
        mod = map_mod(keep, test_of, bymod)
        requires = self.map_requires(mod)
        status, note = map_status(keep, test_of)
        tags = " ".join(t.get("applied_tags") or [])
        kind = "campaign" if ("Кампания" in tags or "Campaign" in tags or len(keep) > 1) else "map"
        eid = f"{kind}-{tid}"
        paths = [self.map_file(r["file_path"]) for r in keep]
        names = [package_name(r["file_name"]) for r in keep]
        self.claim_names(names)
        meta = self.thread_fields(t, t["name"], "")
        refs = []
        for name, full in zip(names, paths):
            asset = self.assets.add(full, ".dtm", eid)
            self.map_shas.add(asset["sha256"])
            refs.append(file_ref(asset, name, lang="ru"))
        self.add_entry(make_entry(eid, kind, meta, "", ["ru"], requires, status, note, refs,
                                  preview=self.preview(meta["image"], eid)))

    def extra_rows(self, tid, path):
        """The .dtm files of a zip given with --extra, extracted into the work folder, as files.tsv rows."""
        if not os.path.isfile(path):
            raise Fail(f"--extra {tid}: {path} is missing")
        folder = os.path.join(self.work, "extra", tid)
        os.makedirs(folder, exist_ok=True)
        rows = []
        with zipfile.ZipFile(path) as zf:
            for info in zf.infolist():
                name = os.path.basename(info.filename)
                if info.is_dir() or not name.lower().endswith(".dtm"):
                    continue
                full = os.path.join(folder, name)
                with open(full, "wb") as f:
                    f.write(zf.read(info))
                rows.append({"thread_id": tid, "message_id": tid, "file_name": name, "file_path": full,
                             "sha256": sha256_file(full), "is_newest": "yes"})
        if not rows:
            raise Fail(f"--extra {tid}: no .dtm file in {path}")
        return rows

    def map_requires(self, mod):
        if not mod:
            raise Skip("no mod known for it (no test row, no by_mod.tsv slug)")
        if mod == "vanilla":
            return []
        if mod in self.mod_ids:
            return [mod]
        raise Skip(f"its mod {mod} has no entry in the catalog")

    def map_file(self, path):
        full = path if os.path.isabs(path) else os.path.join(self.src, path)
        if not os.path.isfile(full):
            raise Skip(f"file not found: {path}")
        return full

    # archive maps

    def add_archive_maps(self):
        maps = read_tsv(os.path.join(self.archive, "maps.tsv"), MAPS_COLS)
        report = {r["flat_name"]: r for r in read_tsv(os.path.join(self.archive, "report.tsv"), REPORT_COLS)}
        seen = self.map_shas | self.cu_map_shas()
        for row in sorted(maps, key=lambda r: r["flat_name"]):
            what = f"archive map {row['flat_name']}"
            if row["duplicate_of"]:
                self.skip(what, f"duplicate of {row['duplicate_of']}")
                continue
            try:
                self.add_archive_map(row, report.get(row["flat_name"]), seen)
            except Skip as e:
                self.skip(what, str(e))

    def add_archive_map(self, row, rep, seen):
        sha = row["sha256"]
        if sha in seen:
            raise Skip("the same file as a map already in the catalog or in Community Update")
        path = archive_path(self.archive, row)
        if not os.path.isfile(path):
            raise Skip(f"file not found: {path}")
        if sha256_file(path) != sha:
            raise Fail(f"{path} changed since maps.tsv was written")
        base = package_name(os.path.basename(row["original_relative_path"].replace("\\", "/")))
        author, prefix = archive_author(package_name(row["source"]))
        eid = "arc-" + sha[:16]
        status, note = archive_status(rep)
        self.claim_names([base])
        asset = self.assets.add(path, ".dtm", eid)
        seen.add(sha)
        meta = {"title": archive_title(base, prefix), "author": author, "description": "",
                "added": file_date(path), "source_url": ""}
        self.add_entry(make_entry(eid, "map", meta, "", ["ru"], [], status, note,
                                  [file_ref(asset, base, lang="ru")]))

    # output

    def check(self):
        """The hub's own rules, run here so a bad catalog is never written."""
        ids = set()
        for e in self.entries:
            if not ID_RE.match(e["id"]) or e["id"] in ids:
                raise Fail(f"bad or repeated id {e['id']!r}")
            ids.add(e["id"])
        for e in self.entries:
            for r in e["requires"]:
                if r not in self.mod_ids:
                    raise Fail(f"{e['id']} requires {r}, which has no entry")
            for f in e["files"]:
                ok = not any(hub_name_problem(p) for p in f["name"].split("/"))
                if not ok or not ASSET_RE.match(f["path"]) or not os.path.isfile(os.path.join(self.out, f["path"])):
                    raise Fail(f"{e['id']}: bad file {f['name']!r} or asset {f['path']!r}")

    def resolve_chains(self):
        """Every campaign's next-map links must resolve to another chapter of the same campaign, by the game's own
        rule (game_ref_key and game_name_key; quotes are not stripped, as in the game). A link that does not resolve
        is allowed only where the fixes mark the campaign `break`: the campaign then ends at that chapter (status
        partial). Its chapters all stay in the entry."""
        for e in self.entries:
            if e["kind"] != "campaign":
                continue
            chapters = {game_name_key(f["name"]): f["name"] for f in e["files"]}
            links, broken = {}, []
            for f in e["files"]:
                ref = dtm_next_map(os.path.join(self.out, f["path"])) or ""
                if not ref.strip():
                    links[f["name"]] = None
                    continue
                target = chapters.get(game_ref_key(ref))
                links[f["name"]] = target
                if target is None:
                    broken.append((f["name"], ref.strip()))
            if not broken:
                continue
            tid = e["id"].split("-", 1)[1]
            if not self.fixes.get(tid, {}).get("break"):
                self.failures.append(f"{e['id']}: {broken[0][0]} links to {broken[0][1]!r}, not one of its files")
                continue
            names = [f["name"] for f in e["files"]]
            for chapter, ref in broken:
                self.chain_log.append(f"{e['id']}: `{chapter}` names `{ref}`: {REASON_TEXT[unresolved_reason(ref, names)]}")
            # the break the note names: a quoted name first, then an extension, then a missing name; within a kind,
            # the first in chain order (the note says where the campaign ends)
            rank = {"quoted": 0, "extension": 1, "missing": 2}
            chapter, ref = min(broken, key=lambda b: (rank[unresolved_reason(b[1], names)], chain_position(links, b[0])))
            n = chain_position(links, chapter)
            why = unresolved_reason(ref, names)
            if why == "quoted":
                ru = f"Кампания обрывается после главы {n}: имя следующей карты записано в кавычках, игра его не находит."
            elif why == "extension":
                ru = f"Кампания обрывается после главы {n}: имя следующей карты записано с расширением «.Dtm», игра его не находит."
            else:
                ru = f"Кампания обрывается после главы {n}: следующей карты ({ref}) нет в архиве автора."
            e["status"] = "partial"
            e["status_note"] = {"ru": ru, "en": ""}
            self.chain_log.append(f"{e['id']}: ends after chapter {n} (status partial)")

    def write(self):
        """report.md always; index.json only when every mod rebuilt from its chain and every campaign link resolves."""
        if not self.mismatches and not self.failures:
            catalog = {"format": FORMAT, "entries": sorted(self.entries, key=lambda e: e["id"])}
            with open(os.path.join(self.out, "index.json"), "w", encoding="utf-8") as f:
                f.write(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n")
        with open(os.path.join(self.out, "report.md"), "w", encoding="utf-8") as f:
            f.write(self.report_text())

    def report_text(self):
        kinds = ("map", "campaign", "mod")
        statuses = ("full", "partial", "broken", "unknown")
        sizes, owners = self.assets.sizes, self.assets.owners
        total = sum(sizes.values())
        lines = ["# Community pack catalog", "",
                 f"- scratch folder: `{self.src}`",
                 f"- archive maps: `{self.archive}`",
                 f"- Community Update base: `{self.base_dir}`", "",
                 "## Entries", "",
                 "| kind | " + " | ".join(statuses) + " | total |",
                 "|---|" + "---:|" * (len(statuses) + 1)]
        for kind in kinds:
            counts = [sum(1 for e in self.entries if e["kind"] == kind and e["status"] == s) for s in statuses]
            lines.append(f"| {kind} | " + " | ".join(str(c) for c in counts) + f" | {sum(counts)} |")
        previews = sum(1 for e in self.entries if "preview" in e)
        lines += ["", f"Entries with a preview: {previews} of {len(self.entries)}.", "",
                  f"Assets: {len(sizes)} files, {total} bytes ({total / 1e6:.1f} MB).", "",
                  "## Biggest assets", "", "| bytes | file | used by |", "|---:|---|---|"]
        for name in sorted(sizes, key=lambda n: (-sizes[n], n))[:15]:
            lines.append(f"| {sizes[name]} | `{name}` | {', '.join(sorted(owners[name]))} |")
        lines += ["", f"## Asset files ({len(sizes)})", "", "| file | bytes |", "|---|---:|"]
        lines += [f"| `{name}` | {sizes[name]} |" for name in sorted(sizes)]
        lines += ["", f"## Skipped ({len(self.skipped)})", ""]
        lines += [f"- {what}: {reason}" for what, reason in self.skipped] or ["None."]
        lines += ["", f"## Rebuild check: {len(self.rebuild_log)} mod(s), {len(self.mismatches)} failed", ""]
        for slug, before, problems in self.rebuild_log:
            via = " + ".join(["Community Update"] + before + [slug])
            if problems:
                lines.append(f"- FAIL {via}: {len(problems)} difference(s): " + "; ".join(problems[:40]))
            else:
                lines.append(f"- ok {via}")
        lines += ["", f"## Left out of packages, by extension ({len(self.left_out)} types)", "",
                  "| extension | files | mods |", "|---|---:|---:|"]
        for ext, (count, mods) in sorted(self.left_out.items(), key=lambda kv: (-kv[1][0], kv[0])):
            lines.append(f"| `{ext}` | {count} | {len(mods)} ({', '.join(sorted(mods)[:6])}{', ...' if len(mods) > 6 else ''}) |")
        cu = self.cu()
        cu_prefixes = razdor_prefixes(cu)
        own_files = sorted(rel for rel in cu.paths if razdor_made(rel, cu_prefixes))
        lines += ["", f"## Razdor-made files left out ({len(own_files)} in Community Update)", ""]
        lines += [f"- `{rel}`" for rel in own_files[:40]] or ["None."]
        saved_total = sum(s for _, s in self.sound_log.values())
        lines += ["", f"## Sounds packed as Ogg Vorbis ({len(self.sound_log)} mods, {saved_total} bytes saved in total)", "",
                  "| mod | files | bytes saved |", "|---|---:|---:|"]
        lines += [f"| {slug} | {n} | {s} |" for slug, (n, s) in sorted(self.sound_log.items())]
        lines += ["", f"## Campaign chains ({len(self.chain_log)} notes)", ""]
        lines += [f"- {line}" for line in self.chain_log] or ["None."]
        lines += ["", "## Notes", ""]
        lines += [f"- {n}" for n in self.notes] or ["None."]
        return "\n".join(lines) + "\n"


def verify_folder(folder, entries):
    """The folder holds exactly index.json, report.md and the assets the index names, and the asset list in
    report.md is that same set. Fail otherwise (nothing is published then)."""
    refs = {f["path"] for e in entries for f in e["files"] + ([e["preview"]] if "preview" in e else [])}
    listing = set(os.listdir(folder))
    want = refs | {"index.json", "report.md"}
    if listing != want:
        raise Fail(f"the folder holds files the index does not name ({sorted(listing - want)[:5]}) "
                   f"or lacks some it names ({sorted(want - listing)[:5]})")
    with open(os.path.join(folder, "report.md"), encoding="utf-8") as f:
        text = f.read()
    section = text.split("## Asset files", 1)[1].split("\n## ", 1)[0]
    printed = set(re.findall(r"^\| `([^`]+)` \| \d+ \|$", section, re.M))
    if printed != refs:
        raise Fail("the asset list in report.md differs from the folder")


def build(src, archive, base_dir, out, forum_maps=True, extra=None, fixes=None):
    """Builds the catalog in `<out>.tmp` and swaps it in for `out` only when it is complete and checked. A build
    that fails leaves `out` as it was and keeps its folder as `<out>.failed` (report.md) for reading."""
    final = os.path.abspath(out)
    tmp, failed, old = final + ".tmp", final + ".failed", final + ".old"
    for stale in (tmp, failed, old):
        shutil.rmtree(stale, ignore_errors=True)
    os.makedirs(tmp)
    work = tempfile.mkdtemp(prefix=".work-", dir=tmp)
    try:
        pack = Pack(src, archive, base_dir, tmp, work, extra, fixes)
        pack.add_mods()
        if forum_maps:
            pack.add_forum_maps()
        else:
            pack.notes.append("The forum maps were left out (--no-forum-maps).")
        pack.add_archive_maps()
        pack.resolve_chains()
        pack.check()
        pack.write()
    except BaseException:
        shutil.rmtree(work, ignore_errors=True)
        os.rename(tmp, failed)
        raise
    shutil.rmtree(work, ignore_errors=True)
    problems = []
    if pack.mismatches:
        problems.append("mod(s) do not rebuild from Community Update plus their chain: " + ", ".join(pack.mismatches))
    if pack.failures:
        problems.append("campaign links do not resolve (add the chapter with --fixes, or mark the campaign break): "
                        + "; ".join(pack.failures))
    try:
        if problems:
            raise Fail("; ".join(problems))
        verify_folder(tmp, pack.entries)
    except Fail:
        os.rename(tmp, failed)
        raise
    if os.path.isdir(final):
        os.rename(final, old)
    os.rename(tmp, final)
    shutil.rmtree(old, ignore_errors=True)
    return pack


# ---------------------------------------------------------------- self-test

def write_file(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def write_bytes(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        f.write(data)


def fake_dtm(next_name):
    """A minimal uncompressed `.DTm` payload: the header with its text offset, then the title, description,
    campaign name and next-map strings (what dtm_next_map reads)."""
    head = bytearray(DTM_HEADER_SIZE)
    head[9] = ord("4")
    struct.pack_into("<I", head, 0x18, DTM_HEADER_SIZE)
    return bytes(head) + b"T\0D\0C\0" + next_name.encode("cp1251") + b"\0"


def write_fixture(root):
    """A small scratch tree in the layout of the real inputs (for --selftest). Returns its paths."""
    src, base, adir = os.path.join(root, "src"), os.path.join(root, "cu"), os.path.join(root, "arc")
    t0 = "2024-01-01T10:00:00+00:00"
    author = {"id": "9", "username": "alice", "global_name": "Alice"}

    def thread(forum, tid, name, content, owner="9", tags=None, extra=None):
        t = {"forum": forum, "id": tid, "name": name, "owner_id": owner, "created": t0,
             "applied_tags": tags or [], "messages": [{"id": tid, "time": t0,
             "author": author, "content": content, "attachments": []}]}
        t.update(extra or {})
        write_file(os.path.join(src, "discord", "threads", forum, tid + ".json"), json.dumps(t, ensure_ascii=False))

    thread("mods", "200", "Mod A [Test]", "**Мод:** тест. Это описание мода. <@7> и <:e:12>")
    t1 = "2024-02-01T10:00:00+00:00"
    later = {"id": "301", "time": t1, "author": author, "content": "вторая версия", "attachments": []}
    thread("maps", "300", "Карта Б [modA]", "**Автор:** Иван (<@9>). Кампания из двух карт.",
           owner="5", tags=["Кампания/Campaign"],
           extra={"messages": [{"id": "300", "time": t0, "author": author,
                                "content": "**Автор:** Иван (<@9>). Кампания из двух карт.", "attachments": []},
                               later]})
    thread("maps", "302", "Пустая", "", owner="5")
    thread("maps-le", "400", "LE", "")
    write_file(os.path.join(base, "Data", "units.ini"), "base units")
    write_file(os.path.join(base, "Graphics", "a.lit"), "lit")
    write_file(os.path.join(base, "Maps_Rus", "cu.DTm"), "cu map")
    mod_a = os.path.join(src, "mods", "modA")
    write_file(os.path.join(mod_a, "mod.json"), json.dumps({"slug": "modA", "title": "Mod A", "thread": "200",
               "author": "alice", "version": "1.0", "base": "cu"}))
    write_file(os.path.join(mod_a, "game", "Data", "units.ini"), "modA units")
    write_file(os.path.join(mod_a, "game", "Graphics", "a.lit"), "lit")
    write_file(os.path.join(mod_a, "game", "Maps", "new.DTm"), "new map")
    write_file(os.path.join(mod_a, "game", "Maps_Rus", "cu.DTm"), "cu map")
    write_file(os.path.join(mod_a, "game", "DiscordTimes.exe"), "exe")
    write_file(os.path.join(mod_a, "game", "Saves", "s.sav"), "save")
    mod_c = os.path.join(src, "mods", "modC")
    write_file(os.path.join(mod_c, "mod.json"), json.dumps({"slug": "modC", "title": "Mod C", "thread": "201",
               "author": "bob", "version": "** 2.0", "base": "mod:modA"}))
    for rel, text in (("Data/units.ini", "modC units"), ("Maps/new.DTm", "new map"),
                      ("Graphics/a.lit", "lit"), ("Maps_Rus/cu.DTm", "cu map"), ("DiscordTimes.exe", "exe2"),
                      ("Saves/s.sav", "save")):
        write_file(os.path.join(mod_c, "game", rel), text)
    mod_b = os.path.join(src, "mods", "modB")
    write_file(os.path.join(mod_b, "mod.json"), json.dumps({"slug": "modB", "title": "Mod B", "base": "unknown"}))
    write_file(os.path.join(src, "mods", "status.tsv"),
               "slug\tstatus\tdownload ok?\tnote\n"
               "modA\tfull\tyes\t\nmodC\tpartial\tpartial (1 of 2)\tfixture\nmodB\tmissing\tyes\tfixture\n")
    dtm = {"old": fake_dtm("c1.DTm"), "c1": fake_dtm("c2.DTm"), "c2": fake_dtm(""), "cu": b"cu map"}
    shas = {k: sha256_bytes(v.encode()) for k, v in
            {"arc": "arc one", "crumb": "crumb map", "bad": "bad"}.items()}
    shas.update({k: sha256_bytes(v) for k, v in dtm.items()})
    for name, data in (("old.DTm", dtm["old"]), ("c1.DTm", dtm["c1"]), ("c2.DTm", dtm["c2"]),
                       ("cu.DTm", dtm["cu"])):
        write_bytes(os.path.join(src, "maps", "unpacked", "300", name), data)
    files = [("300", "300", "old.DTm", "maps/unpacked/300/old.DTm", shas["old"], "no"),
             ("300", "301", "c1.DTm", "maps/unpacked/300/c1.DTm", shas["c1"], "yes"),
             ("300", "301", "c2.DTm", "maps/unpacked/300/c2.DTm", shas["c2"], "yes"),
             ("300", "301", "cu.DTm", "maps/unpacked/300/cu.DTm", shas["cu"], "yes")]
    write_file(os.path.join(src, "maps", "files.tsv"),
               "thread_id\tmessage_id\tfile_name\tfile_path\tsha256\tis_newest\n" +
               "".join("\t".join(r) + "\n" for r in files))
    write_file(os.path.join(src, "maps", "test.tsv"),
               "thread_id\tfile_name\tsha256\tslug\tresult\n" +
               f"300\told.DTm\t{shas['old']}\tmodA\tFAIL old\n" +
               f"300\tc1.DTm\t{shas['c1']}\tmodA\tok\n" +
               f"300\tc2.DTm\t{shas['c2']}\tmodA\tFAIL crash on day 3\n")
    write_file(os.path.join(src, "maps", "by_mod.tsv"), "thread_id\tmod_slug\n300\tmodA\n302\tvanilla\n")
    arc_rows = [("top__arc1.DTm", "dtm/arc1.DTm", "top", shas["arc"], ""),
                ("top__dup.DTm", "dtm/dup.DTm", "top", shas["c1"], ""),
                ("top__copy.DTm", "dtm/copy.DTm", "top", shas["arc"], "top__arc1.DTm"),
                ("Crumb-Set__Crumb-Duchy.final.DTm", "Crumb-Duchy.final.DTm", "Crumb-Set", shas["crumb"], ""),
                ("top__bad.DTm", "dtm/bad.DTm", "top", shas["bad"], "")]
    write_file(os.path.join(adir, "maps.tsv"), "flat_name\toriginal_relative_path\tsource\tsize\tsha256\tduplicate_of\n" +
               "".join(f"{f}\t{rel}\t{s}\t1\t{h}\t{d}\n" for f, rel, s, h, d in arc_rows))
    write_file(os.path.join(adir, "report.tsv"), "flat_name\tsource\tduplicate_of\tcu_vanilla_result\t"
               "evo_mod_result\tfirst_failure_line\n" +
               "top__arc1.DTm\ttop\t\tok\tok\t\n" +
               "Crumb-Set__Crumb-Duchy.final.DTm\tCrumb-Set\t\tok\tok\t\n" +
               "top__bad.DTm\ttop\t\tfail\tfail\tcrash\n")
    write_file(os.path.join(adir, "raw", "dtm", "arc1.DTm"), "arc one")
    write_bytes(os.path.join(adir, "raw", "dtm", "dup.DTm"), dtm["c1"])
    write_file(os.path.join(adir, "raw", "dtm", "copy.DTm"), "arc one")
    write_file(os.path.join(adir, "raw", "dtm", "bad.DTm"), "bad")
    write_file(os.path.join(adir, "nested", "Crumb-Set", "Crumb-Duchy.final.DTm"), "crumb map")
    return src, base, adir


def check_selftest(name, ok):
    if not ok:
        raise AssertionError(name)
    print(f"ok  {name}")


def sine_samples(seconds):
    """A 440 Hz sine as signed 16-bit little-endian mono samples at 22050 Hz."""
    return b"".join(struct.pack("<h", int(12000 * math.sin(2 * math.pi * 440 * i / 22050)))
                    for i in range(int(22050 * seconds)))


def sine_wav(path, seconds):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(22050)
        w.writeframes(sine_samples(seconds))


def sound_package(root, game_files):
    """Packs a fake mod: `game_files` (rel -> function writing its file) over a Community Update folder with one
    file, through package_sounds, build_zip and verify_package as add_mod does. Returns (zip members, notes, saved)."""
    cu, game = os.path.join(root, "cu"), os.path.join(root, "game")
    write_bytes(os.path.join(cu, "Data", "units.ini"), b"base")
    write_bytes(os.path.join(game, "Data", "units.ini"), b"base")
    for rel, write in game_files.items():
        write(os.path.join(game, rel))
    base_tree, game_tree = Tree(cu), Tree(game)
    prefixes = razdor_prefixes(game_tree) | razdor_prefixes(base_tree)
    members = [(rel, full) for rel, full in changed_files(game_tree, base_tree) if not excluded(rel, prefixes)]
    notes = []
    members, saved, _ = package_sounds("testmod", members, game_tree, os.path.join(root, "cache"), notes)
    zip_path = os.path.join(root, "pkg.zip")
    build_zip(members, zip_path)
    verify_package(zip_path, base_tree)
    with zipfile.ZipFile(zip_path) as zf:
        names = sorted(zf.namelist())
        data = {n: zf.read(n) for n in names}
    return names, data, notes, saved


def check_sound_packages():
    """Sounds in a package: a .raw and a sine .wav become .ogg; a same-stem collision and a mod's own same-stem
    .ogg keep the originals; verify_package refuses an encoded sound that is not Ogg. Needs ffmpeg."""
    if not ffmpeg_available():
        print("skip  Sounds to Ogg Vorbis (ffmpeg is not installed)")
        return
    with tempfile.TemporaryDirectory() as d:
        raw = {"Sounds/Battle1.raw": lambda p: write_bytes(p, sine_samples(0.2))}
        names, data, _, saved = sound_package(os.path.join(d, "raw"), raw)
        check_selftest("sounds: a Sounds/*.raw is packed as Sounds/*.ogg (OggS), no .raw, and bytes are saved",
                       names == ["Sounds/Battle1.ogg"] and data[names[0]].startswith(b"OggS") and saved > 0)
        wav = {"Sounds/Hymn.wav": lambda p: sine_wav(p, 0.5)}
        names, data, _, _ = sound_package(os.path.join(d, "wav"), wav)
        check_selftest("sounds: a .wav whose ogg is under half its size is packed as .ogg",
                       names == ["Sounds/Hymn.ogg"] and data[names[0]].startswith(b"OggS"))
        clash = {"Sounds/Battle1.raw": lambda p: write_bytes(p, sine_samples(0.2)),
                 "Sounds/battle1.wav": lambda p: sine_wav(p, 0.2)}
        names, _, notes, _ = sound_package(os.path.join(d, "clash"), clash)
        check_selftest("sounds: two members of one ogg name keep their originals, with a note",
                       names == ["Sounds/Battle1.raw", "Sounds/battle1.wav"] and len(notes) == 2)
        own = {"Sounds/Hymn.raw": lambda p: write_bytes(p, sine_samples(0.2)),
               "Sounds/Hymn.ogg": lambda p: write_bytes(p, b"OggS mod's own")}
        names, _, notes, _ = sound_package(os.path.join(d, "own"), own)
        check_selftest("sounds: a mod's own same-stem .ogg keeps the .raw, and the mod's .ogg is not packed",
                       names == ["Sounds/Hymn.raw"] and len(notes) == 1)
        bad = os.path.join(d, "bad.zip")
        with zipfile.ZipFile(bad, "w") as zf:
            zf.writestr("Sounds/Bad.ogg", b"not an ogg")
        refused = False
        try:
            verify_package(bad, Tree(os.path.join(d, "raw", "cu")))
        except Fail as e:
            refused = "not an Ogg file" in str(e)
        check_selftest("sounds: verify_package refuses an encoded sound that is not Ogg", refused)


def selftest():
    users = {"1": "Zed"}
    check_selftest("markup: mentions, emoji, marks, links",
                   strip_markup("**bold** <@1> and <:e:2> https://a.b/c_d_e x", users) == "bold Zed and https://a.b/c_d_e x")
    check_selftest("markup: unknown mention goes", strip_markup("<@5> hi", users) == "hi")
    check_selftest("title: one trailing bracket goes", clean_title("X [New units]") == "X")
    check_selftest("title: only the last bracket goes", clean_title("A [b] [c]") == "A [b]")
    check_selftest("title: no bracket, unchanged", clean_title("No bracket") == "No bracket")
    check_selftest("version: decoration removed", clean_version("** 2.0") == "2.0" and clean_version("▪︎▪︎") == "")
    check_selftest("test rows: CU is vanilla, `mod X` is X, a no-build row names its guessed mod",
                   test_mod({"slug": "CU"}) == "vanilla" and test_mod({"slug": "mod new-units"}) == "new-units"
                   and test_mod({"slug": "CU (no build for guessed mod)", "guessed_slug": "krestonostsy-plus"})
                   == "krestonostsy-plus")
    check_selftest("hub names: parentheses, apostrophes and Cyrillic are kept",
                   all(hub_name_problem(n) is None for n in ["Coven Time(2).DTm", "Let's Go.DTm", "Карта 1.DTm"]))
    check_selftest("game types: the game's types stay, documents, archives, music, programs and saves go",
                   all(not excluded(p) for p in ["Graphics/a.lit", "Maps/x.DTm", "data/Units.INI", "Pic/A.TGA"])
                   and all(excluded(p) for p in ["Sounds/x.ogg", "Doc/Readme.txt", "Doc/a.DOCX", "pic.jpg",
                                                 "DiscordTimes.exe", "Saves/s.sav", "unins000.dat", "noext"]))
    check_selftest("razdor-made: translated map copies and Razdor's data files are recognised",
                   is_translated_map("Другой берег_en.DTm") and is_translated_map("РК1-Начало пути_hindi-native.DTm")
                   and not is_translated_map("Другой берег.DTm") and not is_translated_map("Map_ab.DTm")
                   and razdor_made("Maps_Rus/Другой берег_en.DTm", {"eng"})
                   and razdor_made("Eng_Units.ini", {"eng"}) and razdor_made("eng_spells.INI", {"eng"})
                   and not razdor_made("Rus_Units.ini", {"eng"}) and not razdor_made("Graphics/a.lit", {"eng"})
                   and not razdor_made("Maps/Карта.DTm", {"eng"}))
    with tempfile.TemporaryDirectory() as d:
        plain = os.path.join(d, "plain.DTm")
        write_bytes(plain, fake_dtm("Next map.DTm"))
        legacy_payload = fake_dtm("Next2.DTm")
        comp = b"AIpf" + b"\r\n" + bytes([19, 0]) + struct.pack("<I", len(legacy_payload)) + bz2.compress(legacy_payload)
        legacy = os.path.join(d, "legacy.DTm")
        write_bytes(legacy, comp)
        check_selftest("dtm: the next-map name is read from a plain map and from a container map",
                       dtm_next_map(plain) == "Next map.DTm" and dtm_next_map(legacy) == "Next2.DTm")
    check_selftest("game lookup: a reference is found as the game finds it (trim, exact .DTm/.dtm, no quote stripping)",
                   game_ref_key(" Name.DTm ") == game_name_key("Name.dtm") == game_ref_key("Name")
                   and game_ref_key("Name.dtm") == "name"
                   and game_ref_key('"Name.dtm"') != game_name_key("Name.dtm")
                   and game_ref_key("Name.Dtm") == game_name_key("Name.dtm") == "name"
                   and game_ref_key("Name.txt") != game_name_key("Name.txt"))
    chain_links = {"a": "b", "b": None, "c": "a"}
    check_selftest("game lookup: a chapter's place in its chain", chain_position(chain_links, "b") == 3
                   and chain_position(chain_links, "c") == 1)
    refused = {n: hub_name_problem(n) for n in ["a:b.DTm", "CON.txt", "x ", "a.", "..", "q?.DTm", "a\x01", "a" * 256]}
    check_selftest("hub names: each forbidden name is refused", all(refused.values()))
    check_selftest("archive author from Author-Title", archive_author("Crumb-Duchy_ravaged1") == ("Crumb", "Crumb"))
    check_selftest("archive author: none without a hyphen", archive_author("top") == (UNKNOWN_AUTHOR, ""))
    check_selftest("archive author: underscore before hyphen is no author",
                   archive_author("Shto_-_Zagadka_Eridana") == (UNKNOWN_AUTHOR, ""))
    check_selftest("archive title: .final, prefix and underscores go",
                   archive_title("Crumb-Duchy_ravaged1.final.DTm", "Crumb") == "Duchy ravaged1")
    check_selftest("cut: at the last sentence end within the limit",
                   cut_text("Первое. " * 400, 1500).endswith("Первое.") and len(cut_text("Первое. " * 400, 1500)) <= 1500)
    check_selftest("cut: short text unchanged", cut_text("коротко") == "коротко")
    check_selftest("author line: explicit name wins", named_author("**Автор:** Иван (<@1>)", users) == "Иван")
    check_selftest("id: slug kept, bad ids refused",
                   bool(ID_RE.match("iz-gryazi-v-knyazi")) and not ID_RE.match("a b"))
    raw_dos = os.fsdecode("Посыльный.dtm".encode("cp866"))
    check_selftest("names: a non-UTF-8 DOS name reads as cp866, a UTF-8 one is kept",
                   package_name(raw_dos) == "Посыльный.dtm" and package_name("plain.ini") == "plain.ini")
    with tempfile.TemporaryDirectory() as tmp:
        base = os.path.join(tmp, "base")
        game = os.path.join(tmp, "game")
        write_file(os.path.join(base, "a.ini"), "1")
        write_file(os.path.join(base, "b.ini"), "2")
        write_file(os.path.join(game, "a.ini"), "1")
        write_file(os.path.join(game, "b.ini"), "3")
        write_file(os.path.join(game, "c.ini"), "new")
        write_file(os.path.join(game, "d.exe"), "exe")
        bt = Tree(base)
        gt = Tree(game)
        changed = [rel for rel, _ in changed_files(gt, bt)]
        check_selftest("diff: new and changed files, identical ones not", changed == ["b.ini", "c.ini", "d.exe"])
        members = [(rel, full) for rel, full in changed_files(gt, bt) if not excluded(rel)]
        z1, z2 = os.path.join(tmp, "z1.zip"), os.path.join(tmp, "z2.zip")
        build_zip(members, z1)
        os.utime(os.path.join(game, "b.ini"), (1, 1))
        build_zip(list(reversed(members)), z2)
        check_selftest("zip: same files, same bytes", sha256_file(z1) == sha256_file(z2))
        verify_package(z1, bt)
        check_selftest("zip: a clean package verifies", True)
        bad = os.path.join(tmp, "bad.zip")
        build_zip([("a.ini", os.path.join(base, "a.ini")), ("c.ini", os.path.join(game, "c.ini"))], bad)
        refused = False
        try:
            verify_package(bad, bt)
        except Fail:
            refused = True
        check_selftest("zip: a base file slipped in is refused", refused)
        exe_zip = os.path.join(tmp, "exe.zip")
        build_zip([("d.exe", os.path.join(game, "d.exe"))], exe_zip)
        refused = False
        try:
            verify_package(exe_zip, bt)
        except Fail:
            refused = True
        check_selftest("zip: a program slipped in is refused", refused)
        try:
            read_tsv(os.path.join(game, "a.ini"), ["slug"])
        except Fail:
            check_selftest("tsv: a missing column is refused", True)
        raw_tsv = os.path.join(tmp, "raw.tsv")
        with open(raw_tsv, "wb") as f:
            f.write("file_name\tsha256\n".encode() + "Посыльный.dtm".encode("cp866") + b"\tab\n")
        rows = read_tsv(raw_tsv, ["file_name", "sha256"])
        check_selftest("tsv: a raw cp866 name reads and normalizes",
                       len(rows) == 1 and package_name(rows[0]["file_name"]) == "Посыльный.dtm")
        dos = os.path.join(game, os.fsdecode("Посыльный.dtm".encode("cp866")))
        with open(dos, "wb") as f:
            f.write(b"dos map")
        gt2 = Tree(game)
        in_diff = [rel for rel, _ in changed_files(gt2, bt)]
        check_selftest("names: a DOS-named file in a game folder is changed, read as cp866",
                       "Посыльный.dtm" in in_diff and "Посыльный.dtm" in gt2.decoded)
        dos_zip = os.path.join(tmp, "dos.zip")
        build_zip([("Посыльный.dtm", gt2.paths["Посыльный.dtm"])], dos_zip)
        verify_package(dos_zip, bt)
        with zipfile.ZipFile(dos_zip) as zf:
            check_selftest("names: the package member has the UTF-8 name", zf.namelist() == ["Посыльный.dtm"])
        with tempfile.TemporaryDirectory() as root:
            src, cu, adir = write_fixture(root)
            out1, out2, out3 = (os.path.join(root, n) for n in ("out1", "out2", "out3"))
            pack = build(src, adir, cu, out1)
            with open(os.path.join(out1, "index.json"), encoding="utf-8") as f:
                cat = json.load(f)
            by_id = {e["id"]: e for e in cat["entries"]}
            check_selftest("build: format 1", cat["format"] == FORMAT)
            check_selftest("build: mod A is a full mod with one archive",
                           by_id["modA"]["kind"] == "mod" and by_id["modA"]["status"] == "full"
                           and by_id["modA"]["files"][0]["archive"] is True and by_id["modA"]["requires"] == [])
            pkg = os.path.join(out1, by_id["modA"]["files"][0]["path"])
            with zipfile.ZipFile(pkg) as zf:
                members_a = sorted(zf.namelist())
            check_selftest("build: mod A's package has only its changed, non-program files",
                           members_a == ["Data/units.ini", "Maps/new.DTm"])
            check_selftest("build: mod C requires A, is partial, notes the program change and the part download",
                           by_id["modC"]["requires"] == ["modA"] and by_id["modC"]["status"] == "partial"
                           and "DiscordTimes.exe" in by_id["modC"]["status_note"]["ru"]
                           and "Загружена только часть" in by_id["modC"]["status_note"]["ru"]
                           and by_id["modC"]["version"] == "2.0" and by_id["modC"]["base"] == "Mod A")
            camp = by_id["campaign-300"]
            check_selftest("build: the campaign is broken with its reason, keeps two files, skips the CU map",
                           camp["kind"] == "campaign" and camp["status"] == "broken"
                           and len(camp["files"]) == 2 and camp["requires"] == ["modA"]
                           and camp["author"] == "Иван" and "crash on day 3" in camp["status_note"]["en"])
            check_selftest("build: a mod with no entry gets no map entry",
                           "map-302" not in by_id and "modB" not in by_id)
            arcs = [e for e in cat["entries"] if e["id"].startswith("arc-")]
            check_selftest("build: archive maps: duplicates and covered files dropped, the rest kept",
                           len(arcs) == 3 and all(e["kind"] == "map" and e["requires"] == [] for e in arcs))
            crumb = [e for e in arcs if e["author"] == "Crumb"]
            check_selftest("build: archive title and author from the name",
                           len(crumb) == 1 and crumb[0]["title"]["ru"] == "Duchy")
            check_selftest("build: the broken archive map is broken with its reason",
                           any(e["status"] == "broken" and e["files"][0]["name"] == "bad.DTm" for e in arcs))
            modc = [x for x in pack.rebuild_log if x[0] == "modC"]
            check_selftest("build: mod C rebuilds from Community Update plus mod A's package",
                           pack.mismatches == [] and len(modc) == 1 and modc[0][1] == ["modA"] and modc[0][2] == [])
            check_selftest("build: archive maps carry their file's date",
                           all(re.match(r"^\d{4}-\d{2}-\d{2}$", e["added"]) for e in arcs))
            write_file(os.path.join(out1, "stray.webp"), "left from an old build")
            stray_refused = False
            try:
                verify_folder(out1, pack.entries)
            except Fail:
                stray_refused = True
            os.remove(os.path.join(out1, "stray.webp"))
            check_selftest("folder: a file the index does not name is refused", stray_refused)
            check_selftest("build: every asset file exists and has its size",
                           all(os.path.getsize(os.path.join(out1, f["path"])) == f["size"]
                               for e in cat["entries"] for f in e["files"]))
            report = open(os.path.join(out1, "report.md"), encoding="utf-8").read()
            check_selftest("report: lists the skipped items with reasons",
                           "mod modB: status missing" in report and "no map file in the crawl" in report)
            bad_root = os.path.join(root, "bad")
            bsrc, bcu, badir = write_fixture(bad_root)
            write_file(os.path.join(bcu, "Extra", "old.ini"), "only in Community Update")
            refused = False
            try:
                build(bsrc, badir, bcu, os.path.join(bad_root, "out"))
            except Fail as e:
                refused = "rebuild" in str(e)
            with open(os.path.join(bad_root, "out.failed", "report.md"), encoding="utf-8") as f:
                bad_report = f.read()
            check_selftest("rebuild: a mod that does not rebuild fails loudly, is listed, writes no index",
                           refused and "extra Extra/old.ini" in bad_report
                           and not os.path.exists(os.path.join(bad_root, "out", "index.json")))
            chain_root = os.path.join(root, "chain")
            csrc, ccu, cadir = write_fixture(chain_root)
            write_bytes(os.path.join(csrc, "maps", "unpacked", "300", "c2.DTm"), fake_dtm("Gone.DTm"))
            refused = False
            try:
                build(csrc, cadir, ccu, os.path.join(chain_root, "out"))
            except Fail as e:
                refused = "Gone.DTm" in str(e)
            check_selftest("chain: a link to a missing chapter fails the build unless the campaign is marked break",
                           refused and not os.path.exists(os.path.join(chain_root, "out", "index.json")))
            cpack = build(csrc, cadir, ccu, os.path.join(chain_root, "out2"), fixes={"300": {"break": True}})
            camp = [e for e in cpack.entries if e["id"] == "campaign-300"][0]
            check_selftest("chain: a campaign marked break ends there, partial, with the note",
                           camp["status"] == "partial" and "Gone.DTm" in camp["status_note"]["ru"]
                           and "после главы 2" in camp["status_note"]["ru"])
            os.makedirs(out2)
            write_file(os.path.join(out2, "0000000000000000.zip"), "stale")
            build(src, adir, cu, out2)
            names = ["index.json", "report.md"] + sorted({f["path"] for e in cat["entries"] for f in e["files"]})
            same = all(open(os.path.join(out1, n), "rb").read() == open(os.path.join(out2, n), "rb").read()
                       for n in names)
            check_selftest("build: the same inputs give the same bytes", same)
            check_selftest("build: a stale asset of an earlier build is cleared",
                           not os.path.exists(os.path.join(out2, "0000000000000000.zip")))
            build(src, adir, cu, out3, forum_maps=False)
            with open(os.path.join(out3, "index.json"), encoding="utf-8") as f:
                cat3 = json.load(f)
            check_selftest("build: --no-forum-maps leaves the forum maps out",
                           not any(e["id"].startswith(("campaign-", "map-")) for e in cat3["entries"]))
    check_sound_packages()
    print("selftest: all checks passed")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description="Build the community catalog (index.json, assets, report.md) for the Razdor hub.")
    ap.add_argument("--src", help="the scratch folder (community-pack)")
    ap.add_argument("--archive-maps", help="the archive folder: maps.tsv, report.tsv, raw/, nested/")
    ap.add_argument("--base", help="the Community Update install (read only)")
    ap.add_argument("--out", help="the output folder")
    ap.add_argument("--no-forum-maps", action="store_true",
                    help="leave the forum maps out (until maps/files.tsv and maps/test.tsv exist)")
    ap.add_argument("--extra", action="append", default=[], metavar="THREAD=ZIP",
                    help="the map files of a forum thread from a zip the owner downloaded (repeatable)")
    ap.add_argument("--fixes", metavar="JSON",
                    help="campaign chain fixes by map thread id: add (a chapter under the name its next-map names), "
                         "drop (chapter files), break (the campaign ends at its last chapter)")
    ap.add_argument("--selftest", action="store_true", help="run the offline checks and exit")
    a = ap.parse_args(argv)
    fixes = {}
    if a.fixes:
        with open(a.fixes, encoding="utf-8") as f:
            fixes = {k: v for k, v in json.load(f).items() if not k.startswith("_")}
    extra = {}
    for item in a.extra:
        tid, sep, path = item.partition("=")
        if not sep or not tid or not path:
            ap.error(f"--extra wants THREAD=ZIP, got {item!r}")
        extra[tid] = path
    if a.selftest:
        return selftest()
    missing = [n for n in ("src", "archive_maps", "base", "out") if not getattr(a, n)]
    if missing:
        ap.error("missing " + ", ".join("--" + m.replace("_", "-") for m in missing))
    try:
        pack = build(a.src, a.archive_maps, a.base, a.out, forum_maps=not a.no_forum_maps, extra=extra, fixes=fixes)
        saved = sum(s for _, s in pack.sound_log.values())
        print(f"sounds: {sum(n for n, _ in pack.sound_log.values())} files of {len(pack.sound_log)} mods packed as "
              f"Ogg Vorbis, {saved} bytes saved")
    except Fail as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
