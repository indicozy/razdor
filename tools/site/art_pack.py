#!/usr/bin/env python3
"""Packs the website's pictures from the player's own install and screenshots: the art of the Pages site.

Input: a Discord Times install (its Graphics folder: UGS sprites), a folder of Razdor screenshots, and the
repository's own files (ads/thorny-times/capsule.png). Output: <out>/art/<name>.webp for every entry of
tools/site/art.json, and <out>/site-art.zip holding the same files under art/. The site's pictures are never
committed: the zip is published as the asset site-art.zip of the pre-release site-art, and pages.yml
downloads it at build time.

The UGS decoding follows the engine's reader (src/dt/gfx.rs): a pixel is rotr16(v, 3) ^ 0x5555 with four
4-bit channels A, R, G, B (shifts 12, 8, 4, 0), premultiplied by alpha; additive effects (S-Fire) keep their
light in R, G, B with alpha 0 and get alpha = the brightest channel.

Before writing, every /art/ reference in site/src is checked against art.json: a reference that names no
entry fails the run, so a picture can never go missing silently.

Usage: python3 -I tools/site/art_pack.py --dt INSTALL --shots SHOTS_DIR --out OUT_DIR
Needs Python 3 and Pillow only.
"""

import argparse
import re
import struct
import sys
import zipfile
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ART_JSON = HERE / "art.json"
SITE_SRC = REPO / "site" / "src"
WEBP_QUALITY = 85
LOSSLESS_MAX_FRAME = 64  # sprites with frames this small or smaller are stored lossless


class PackError(Exception):
    pass


# --- UGS decoding (the same rules as src/dt/gfx.rs and the verified extract script) ---------------

def _u16(d, o):
    return d[o] | (d[o + 1] << 8)


def _rotr16(v, n):
    return ((v >> n) | (v << (16 - n))) & 0xFFFF


def pixel_rgba(v):
    """One UGS pixel to (r, g, b, a) with the premultiplied colour undone."""
    p = _rotr16(v, 3) ^ 0x5555
    a, r, g, b = ((p >> 12) & 15) * 17, ((p >> 8) & 15) * 17, ((p >> 4) & 15) * 17, (p & 15) * 17
    if a == 0:
        return (0, 0, 0, 0)
    return (min(255, round(r * 255 / a)), min(255, round(g * 255 / a)), min(255, round(b * 255 / a)), a)


def pixel_additive(v):
    """One additive pixel (S-Fire): the stored light is the colour, alpha is the brightest channel."""
    p = _rotr16(v, 3) ^ 0x5555
    r, g, b = ((p >> 8) & 15) * 17, ((p >> 4) & 15) * 17, (p & 15) * 17
    return (r, g, b, max(r, g, b))


def decode_ugs(d, additive=False):
    """All frames of a UGS file as PIL RGBA images."""
    conv = pixel_additive if additive else pixel_rgba
    w, h = _u16(d, 0), _u16(d, 2)
    n = w * h * 2
    repeated = len(d) >= 8 and _u16(d, 4) == w and _u16(d, 6) == h and 8 + n <= len(d)
    plain_next = 4 + n < len(d) and _u16(d, 4 + n) == w and _u16(d, 6 + n) == h
    hdr = 8 if (repeated and not plain_next) else 4
    frames, o = [], 0
    while o + hdr <= len(d):
        w, h = _u16(d, o), _u16(d, o + 2)
        if w == 0 or h == 0 or o + hdr + w * h * 2 > len(d):
            raise PackError(f"bad UGS frame at byte {o}")
        words = struct.unpack_from("<%dH" % (w * h), d, o + hdr)
        im = Image.new("RGBA", (w, h))
        im.putdata([conv(v) for v in words])
        frames.append(im)
        o += hdr + w * h * 2
    if not frames:
        raise PackError("no frames")
    return frames


def find_case_insensitive(root: Path, rel: str) -> Path:
    """The file at a relative path under root, matching each part without regard to case."""
    cur = root
    for part in rel.split("/"):
        if not cur.is_dir():
            raise PackError(f"not found: {rel} under {root}")
        hits = [c for c in cur.iterdir() if c.name.lower() == part.lower()]
        if not hits:
            raise PackError(f"not found: {rel} under {root}")
        cur = hits[0]
    if not cur.is_file():
        raise PackError(f"not a file: {rel}")
    return cur


# --- the entries -------------------------------------------------------------------------------------

def select_frames(spec, count: int, sprite: str) -> list:
    """Frame indices from the entry's frames: "all", a list of ints, or "a-b" (inclusive)."""
    if spec in (None, "all"):
        idx = list(range(count))
    elif isinstance(spec, str):
        m = re.fullmatch(r"(\d+)-(\d+)", spec)
        if not m:
            raise PackError(f"{sprite}: bad frames {spec!r}")
        idx = list(range(int(m.group(1)), int(m.group(2)) + 1))
    else:
        idx = [int(i) for i in spec]
    for i in idx:
        if not 0 <= i < count:
            raise PackError(f"{sprite}: frame {i} is out of range (the file has {count})")
    return idx


def compose(frames: list, layout: str, cols: int) -> Image.Image:
    fw, fh = frames[0].size
    if layout == "row":
        cols = len(frames)
    elif layout != "grid":
        raise PackError(f"bad layout {layout!r}")
    rows = (len(frames) + cols - 1) // cols
    sheet = Image.new("RGBA", (fw * cols, fh * rows), (0, 0, 0, 0))
    for i, f in enumerate(frames):
        sheet.paste(f, ((i % cols) * fw, (i // cols) * fh))
    return sheet


def resize_to_width(im: Image.Image, width) -> Image.Image:
    if not width or width == im.size[0]:
        return im
    return im.resize((width, max(1, round(im.size[1] * width / im.size[0]))), Image.LANCZOS)


def build_entry(entry: dict, dt: Path, shots: Path, repo: Path, cache: dict):
    """The entry's picture as (image, lossless flag)."""
    name = entry["name"]
    if "sprite" in entry:
        key = entry["sprite"]
        if key not in cache:
            path = find_case_insensitive(dt, key)
            cache[key] = decode_ugs(path.read_bytes(), additive=bool(entry.get("additive")))
        frames = cache[key]
        idx = select_frames(entry.get("frames", "all"), len(frames), key)
        picked = [frames[i] for i in idx]
        sheet = compose(picked, entry.get("layout", "row"), int(entry.get("cols", 8)))
        if entry.get("scale"):
            s = float(entry["scale"])
            sheet = sheet.resize((round(sheet.size[0] * s), round(sheet.size[1] * s)), Image.LANCZOS)
        lossless = picked[0].size[0] <= LOSSLESS_MAX_FRAME
        return sheet, lossless
    if "shot" in entry:
        path = shots / entry["shot"]
        if not path.is_file():
            raise PackError(f"{name}: no screenshot {entry['shot']} in {shots}")
        im = Image.open(path).convert("RGBA")
        if entry.get("crop"):
            x, y, w, h = entry["crop"]
            im = im.crop((x, y, x + w, y + h))
        return resize_to_width(im, entry.get("width")), False
    if "file" in entry:
        path = repo / entry["file"]
        if not path.is_file():
            raise PackError(f"{name}: no file {entry['file']} in the repository")
        im = Image.open(path).convert("RGBA")
        return resize_to_width(im, entry.get("width")), False
    raise PackError(f"{name}: an entry needs sprite, shot or file")


def load_entries(art_json: Path) -> list:
    import json

    data = json.loads(art_json.read_text(encoding="utf-8"))
    entries = data["entries"] if isinstance(data, dict) else data
    names = [e["name"] for e in entries]
    if len(set(names)) != len(names):
        raise PackError("art.json: a name is listed twice")
    for n in names:
        if not n.endswith(".webp") or "/" in n:
            raise PackError(f"art.json: bad name {n!r}")
    return entries


# --- the reference check -----------------------------------------------------------------------------

# /art/<name> in a URL, and art("<name>") or art(`<name>`) calls. ${...} is a wildcard for the name's
# dynamic part (portrait-${n}.webp matches portrait-00.webp and the rest).
URL_REF = re.compile(r"art/([A-Za-z0-9._${}\[\]-]+\.webp)")
CALL_REF = re.compile(r"""\bart\(\s*["'`]([A-Za-z0-9._${}\[\]-]+\.webp)["'`]""")


def references(site_src: Path) -> set:
    refs = set()
    for path in site_src.rglob("*"):
        if path.suffix not in {".astro", ".ts", ".tsx", ".js", ".mjs", ".css", ".md", ".html"} or not path.is_file():
            continue
        text = path.read_text(encoding="utf-8")
        refs.update(URL_REF.findall(text))
        refs.update(CALL_REF.findall(text))
    # A reference that is only a variable (the helper's own ${name}) names no file.
    return {r for r in refs if not r.startswith("${")}


def reference_pattern(ref: str):
    parts = re.split(r"\$\{[^}]*\}", ref)
    return re.compile(".+?".join(re.escape(p) for p in parts))


def check_references(site_src: Path, names: list) -> list:
    """The references that match no entry of art.json (empty when every picture is packed)."""
    missing = []
    for ref in sorted(references(site_src)):
        pat = reference_pattern(ref)
        if not any(pat.fullmatch(n) for n in names):
            missing.append(ref)
    return missing


# --- the run ----------------------------------------------------------------------------------------

def pack(dt: Path, shots: Path, out: Path, art_json: Path = ART_JSON, repo: Path = REPO, site_src: Path = SITE_SRC):
    entries = load_entries(art_json)
    names = [e["name"] for e in entries]
    missing = check_references(site_src, names)
    if missing:
        raise PackError("site references pictures art.json does not pack: " + ", ".join(missing))
    art_dir = out / "art"
    art_dir.mkdir(parents=True, exist_ok=True)
    cache: dict = {}
    total = 0
    written = []
    for entry in entries:
        im, lossless = build_entry(entry, dt, shots, repo, cache)
        target = art_dir / entry["name"]
        if lossless:
            im.save(target, "WEBP", lossless=True, method=6)
        else:
            im.save(target, "WEBP", quality=WEBP_QUALITY, method=6)
        size = target.stat().st_size
        total += size
        written.append(target)
        print(f"info: {entry['name']} {im.size[0]}x{im.size[1]} {size} bytes" + (" lossless" if lossless else ""))
    zip_path = out / "site-art.zip"
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for target in written:
            z.write(target, f"art/{target.name}")
    print(f"info: {len(written)} pictures, {total} bytes as WebP; {zip_path} {zip_path.stat().st_size} bytes")
    return written


def main(argv: list) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dt", required=True, type=Path, help="the Discord Times install folder")
    ap.add_argument("--shots", required=True, type=Path, help="folder of Razdor screenshots (PNG)")
    ap.add_argument("--out", required=True, type=Path, help="output folder (art/ and site-art.zip)")
    args = ap.parse_args(argv[1:])
    try:
        pack(args.dt, args.shots, args.out)
    except (PackError, OSError, KeyError, ValueError) as e:
        print(f"crit: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
