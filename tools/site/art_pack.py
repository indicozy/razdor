#!/usr/bin/env python3
"""Packs the website's pictures and sounds from the player's own install and screenshots: the art of the Pages site.

Input: a Discord Times install (its Graphics and Sounds folders), a folder of Razdor screenshots, and the
repository's own files (ads/thorny-times/capsule.png). Output: <out>/art/<name> for every entry of
tools/site/art.json, and <out>/site-art.zip holding the same files under art/. The site's pictures are never
committed: the zip is published as the asset site-art.zip of the pre-release site-art, and pages.yml
downloads it at build time.

Entry kinds: a sprite (UGS frames, decoded here), a shot (a Razdor screenshot), a file (a repository file), and
the kinds in art.json with "kind": lit (any non-UGS picture, decoded by the engine's own reader through the
dump_rgba example), dt-title (the burning title, animated), pcm (a DT sound to Ogg), button88 (the animated
88x31 button, an APNG).

The UGS decoding follows the engine's reader (src/dt/gfx.rs): a pixel is rotr16(v, 3) ^ 0x5555 with four
4-bit channels A, R, G, B (shifts 12, 8, 4, 0), premultiplied by alpha; additive effects (S-Fire) keep their
light in R, G, B with alpha 0 and get alpha = the brightest channel.

Before writing, every /art/ reference in site/src is checked against art.json: a reference that names no
entry fails the run, so a picture can never go missing silently.

Usage: python3 -I tools/site/art_pack.py --dt INSTALL --shots SHOTS_DIR --out OUT_DIR [--dump PATH_TO_DUMP_RGBA]
Needs Python 3 and Pillow; ffmpeg for the sound; cargo when --dump is omitted and a lit/title entry needs it.
"""

import argparse
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
ART_JSON = HERE / "art.json"
SITE_SRC = REPO / "site" / "src"
WEBP_QUALITY = 85
LOSSLESS_MAX_FRAME = 64  # sprites with frames this small or smaller are stored lossless
TITLE_MAX_BYTES = 250_000
FONT_CANDIDATES = [
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
]
ALLOWED_SUFFIXES = (".webp", ".png", ".ogg")


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


# --- the sprite entries -------------------------------------------------------------------------------

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


def image_writer(im: Image.Image, lossless: bool):
    def write(target: Path) -> str:
        if lossless:
            im.save(target, "WEBP", lossless=True, method=6)
        else:
            im.save(target, "WEBP", quality=WEBP_QUALITY, method=6)
        return f"{im.size[0]}x{im.size[1]}" + (" lossless" if lossless else "")
    return write


# --- DT pictures decoded by the engine's own reader (dump_rgba) ----------------------------------------

def decode_with_dump(dump: Path, path: Path, cache: dict) -> list:
    """All frames of a DT picture, decoded by the Rust example (dump_rgba, the engine's own decoder)."""
    key = ("dump", str(path))
    if key in cache:
        return cache[key]
    if not dump.is_file():
        raise PackError(f"no dump_rgba binary at {dump}")
    tmp = Path(tempfile.mkdtemp(prefix="dump-"))
    try:
        proc = subprocess.run([str(dump), str(tmp), str(path)], capture_output=True, text=True)
        if proc.returncode != 0:
            raise PackError(f"dump_rgba failed on {path.name}: {proc.stderr.strip()}")
        files = sorted(tmp.glob(f"{path.stem}-*.rgba"), key=lambda p: int(p.stem.rsplit("-", 1)[1]))
        if not files:
            raise PackError(f"dump_rgba wrote no frames for {path.name}")
        frames = []
        for f in files:
            d = f.read_bytes()
            w, h = struct.unpack_from("<II", d, 0)
            if len(d) != 8 + w * h * 4:
                raise PackError(f"dump_rgba: bad size in {f.name}")
            frames.append(Image.frombytes("RGBA", (w, h), d[8:]))
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    cache[key] = frames
    return frames


def dump_binary(dump) -> Path:
    """The dump_rgba binary: the one given, else built from this repository."""
    if dump is not None:
        return Path(dump)
    target = Path(os.environ.get("CARGO_TARGET_DIR", REPO / "target"))
    try:
        subprocess.run(
            ["cargo", "build", "--release", "--no-default-features", "--example", "dump_rgba"],
            cwd=REPO, check=True,
        )
    except (OSError, subprocess.CalledProcessError) as e:
        raise PackError(f"cannot build dump_rgba: {e}")
    return target / "release" / "examples" / "dump_rgba"


# --- the burning title ---------------------------------------------------------------------------------

TITLE_FW, TITLE_FH = 256, 56  # one flame frame of the flame sheet (two halves per row)
TITLE_NFLAME = 9


def key_dark(im: Image.Image, threshold: int) -> Image.Image:
    """A picture on a black ground: pixels whose brightest channel is below threshold become transparent."""
    rgb = im.convert("RGB")
    bright = ImageChops.lighter(ImageChops.lighter(rgb.getchannel("R"), rgb.getchannel("G")), rgb.getchannel("B"))
    out = rgb.convert("RGBA")
    out.putalpha(bright.point(lambda v: 255 if v >= threshold else 0))
    return out


def _to_rgba(rgb: Image.Image) -> Image.Image:
    """Additive light on transparency: alpha is the brightest channel, the colour un-premultiplied.

    A black pixel has alpha 0 and stays fully transparent. One pass over the pixels with putdata (the
    colour division has no Pillow point or chops form).
    """
    im = rgb.convert("RGB")
    out = []
    pixels = im.get_flattened_data() if hasattr(im, "get_flattened_data") else im.getdata()
    for r, g, b in pixels:
        a = max(r, g, b)
        if a == 0:
            out.append((0, 0, 0, 0))
        else:
            out.append((min(255, r * 255 // a), min(255, g * 255 // a), min(255, b * 255 // a), a))
    res = Image.new("RGBA", im.size)
    res.putdata(out)
    return res


def title_frames(title: Image.Image, flame: Image.Image) -> list:
    """The burning title as 18 RGBA frames: 9 flame frames, each cross-faded half-way to the next.

    The layout follows src/ui/main_menu.rs logo(): the flame sits behind the title's left part, tinted orange.
    """
    k = 896 / 840
    fw, fh = TITLE_FW, TITLE_FH
    base_w = 448
    base_h = base_w * fh / fw
    flame_w = round(base_w * 0.95)
    flame_h = round(base_h * 1.52)
    W, H = 900, 190
    x, y = 2, 40
    base_y = y + (128 - base_h) / 2
    flame_y = round(base_y + base_h - flame_h + 12 * k)
    tint = (0.85, 0.55 * 0.85, 0.2 * 0.85)
    flame_rgb = flame.convert("RGB")

    def layer(i: int) -> Image.Image:
        acc = Image.new("RGB", (W, H), (0, 0, 0))
        for half in range(2):
            src = flame_rgb.crop((half * fw, i * fh, half * fw + fw, i * fh + fh))
            src = src.resize((flame_w, flame_h), Image.BILINEAR)
            r, g, b = src.split()
            src = Image.merge("RGB", [c.point(lambda v, m=m: int(v * m)) for c, m in zip((r, g, b), tint)])
            dx = round(x + half * base_w - (flame_w - base_w) / 2 - 16 * k)
            canvas = Image.new("RGB", (W, H))
            canvas.paste(src, (dx, flame_y))
            acc = ImageChops.add(acc, canvas)
        return acc

    layers = [layer(i) for i in range(TITLE_NFLAME)]
    frames = []
    for i in range(TITLE_NFLAME):
        for t in (0.0, 0.5):
            f = _to_rgba(Image.blend(layers[i], layers[(i + 1) % TITLE_NFLAME], t))
            f.alpha_composite(title, (x, y))
            frames.append(f)
    return frames


def _scaled(frames: list, width: int) -> list:
    w, h = frames[0].size
    size = (width, round(h * width / w))
    return [f.resize(size, Image.LANCZOS) for f in frames]


def build_title(entry: dict, dt: Path, dump: Path, cache: dict):
    """The burning title: an animated WebP (50 ms a frame), or its first frame as a still."""
    def get_frames() -> list:
        if "title" not in cache:
            title = decode_with_dump(dump, find_case_insensitive(dt, entry["title"]), cache)[0]
            flame = decode_with_dump(dump, find_case_insensitive(dt, entry["flame"]), cache)[0]
            cache["title"] = title_frames(title, flame)
        return cache["title"]

    if entry.get("still"):
        def write_still(target: Path) -> str:
            frame = get_frames()[0]
            frame.save(target, "WEBP", quality=WEBP_QUALITY, method=6)
            return f"{frame.size[0]}x{frame.size[1]} still"
        return write_still

    def write_anim(target: Path) -> str:
        frames = get_frames()

        def save(fs: list, quality: int):
            fs[0].save(target, "WEBP", save_all=True, append_images=fs[1:], duration=50, loop=0,
                       quality=quality, alpha_quality=quality, method=6)

        save(frames, 70)
        if target.stat().st_size > TITLE_MAX_BYTES:
            # Over the size target at full width: a 720 px canvas at a lower quality (the flames are soft).
            frames = _scaled(frames, 720)
            save(frames, 40)
        return f"{frames[0].size[0]}x{frames[0].size[1]} x{len(frames)} frames"
    return write_anim


# --- the menu sound ------------------------------------------------------------------------------------

def write_pcm_ogg(entry: dict, dt: Path, target: Path) -> str:
    raw = find_case_insensitive(dt, entry["sound"])
    encoders = subprocess.run(["ffmpeg", "-hide_banner", "-encoders"], capture_output=True, text=True).stdout
    opus = "libopus" in encoders
    codec = ["-c:a", "libopus", "-b:a", "48k"] if opus else ["-c:a", "libvorbis", "-q:a", "3"]
    proc = subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", "-f", "s16le", "-ar", "22050", "-ac", "1",
         "-i", str(raw), *codec, str(target)],
        capture_output=True, text=True,
    )
    if proc.returncode != 0:
        raise PackError(f"ffmpeg failed: {proc.stderr.strip()}")
    return "mono 22050 Hz " + ("opus" if opus else "vorbis")


# --- the 88x31 button (APNG) ---------------------------------------------------------------------------

BUTTON_W, BUTTON_H = 88, 31
BUTTON_LIGHT = (223, 223, 223)  # the 1 px bevel: light top and left, dark bottom and right
BUTTON_DARK = (96, 96, 96)
BUTTON_GOLD = (255, 204, 0)
BUTTON_TEXT = "НОВАЯ ИГРА!"
BUTTON_PAN_CANVAS = 130  # the capsule is scaled to this width, so the 86 px window has room to pan
BUTTON_DIM = 0.45  # the capsule under the text is darkened to this


def _button_text_mask() -> Image.Image:
    """The button text as a hard 1-bit mask (no antialiasing), as large as fits inside the bevel."""
    for size in range(12, 5, -1):
        font = _button_font(size)
        probe = ImageDraw.Draw(Image.new("L", (BUTTON_W, BUTTON_H)))
        l, t, r, b = probe.textbbox((0, 0), BUTTON_TEXT, font=font)
        if r - l <= BUTTON_W - 4:
            break
    else:
        raise PackError("the button text does not fit 88x31 at any size")
    mask = Image.new("L", (BUTTON_W, BUTTON_H), 0)
    x = (BUTTON_W - (r - l)) // 2 - l
    y = (BUTTON_H - (b - t)) // 2 - t
    ImageDraw.Draw(mask).text((x, y), BUTTON_TEXT, font=font, fill=255)
    return mask.point(lambda v: 255 if v >= 128 else 0)


def _button_font(size: int):
    for path in FONT_CANDIDATES:
        if Path(path).is_file():
            return ImageFont.truetype(path, size)
    raise PackError("no TTF font with Cyrillic found for the button text")


def _bevel(canvas: Image.Image) -> Image.Image:
    d = ImageDraw.Draw(canvas)
    w, h = canvas.size
    d.line([(0, 0), (w - 1, 0)], fill=BUTTON_LIGHT)
    d.line([(0, 0), (0, h - 1)], fill=BUTTON_LIGHT)
    d.line([(0, h - 1), (w - 1, h - 1)], fill=BUTTON_DARK)
    d.line([(w - 1, 0), (w - 1, h - 1)], fill=BUTTON_DARK)
    return canvas


def button_frames(capsule: Image.Image) -> list:
    """Six 88x31 frames: four slow pans of the capsule left to right, then the gold text blinking over it."""
    inner_w, inner_h = BUTTON_W - 2, BUTTON_H - 2
    cw = BUTTON_PAN_CANVAS
    ch = round(capsule.size[1] * cw / capsule.size[0])
    big = capsule.convert("RGB").resize((cw, ch), Image.NEAREST)
    top = (ch - inner_h) // 2
    big = big.crop((0, top, cw, top + inner_h))
    travel = cw - inner_w
    offsets = [0, travel // 3, 2 * travel // 3, travel]

    def framed(body: Image.Image) -> Image.Image:
        canvas = Image.new("RGB", (BUTTON_W, BUTTON_H), (0, 0, 0))
        canvas.paste(body, (1, 1))
        return _bevel(canvas)

    frames = [framed(big.crop((o, 0, o + inner_w, inner_h))) for o in offsets]
    dim = big.crop((travel, 0, travel + inner_w, inner_h)).point(lambda v: int(v * BUTTON_DIM))
    text_on = framed(dim)
    mask = _button_text_mask()
    shadow = Image.new("L", (BUTTON_W, BUTTON_H), 0)
    shadow.paste(mask, (1, 1))  # a 1 px black drop shadow, down and right
    text_on.paste((0, 0, 0), (0, 0, BUTTON_W, BUTTON_H), shadow)
    text_on.paste(BUTTON_GOLD, (0, 0, BUTTON_W, BUTTON_H), mask)
    frames.append(_bevel(text_on))
    frames.append(framed(dim))  # the blink's off phase
    return frames


def build_button(entry: dict, repo: Path):
    """thorny-88x31.png: the capsule's animated 88x31 button, an APNG (the owner's 2003 banner style)."""
    src = repo / entry["file"]
    if not src.is_file():
        raise PackError(f"{entry['name']}: no file {entry['file']} in the repository")
    capsule = Image.open(src).convert("RGB")

    def write(target: Path) -> str:
        frames = button_frames(capsule)
        durations = [200, 200, 200, 200, 250, 200]  # ms: four pans, the text on, the text off
        frames[0].save(target, "PNG", save_all=True, append_images=frames[1:], duration=durations, loop=0)
        return f"{BUTTON_W}x{BUTTON_H} APNG x{len(frames)} frames"
    return write


# --- building one entry -------------------------------------------------------------------------------

def build_entry(entry: dict, dt: Path, shots: Path, repo: Path, cache: dict, dump=None):
    """The entry's writer: a function (target path) -> a description of what it wrote."""
    name = entry["name"]
    kind = entry.get("kind")
    if kind == "lit":
        path = find_case_insensitive(dt, entry["lit"])
        im = decode_with_dump(dump, path, cache)[0]
        if entry.get("key_below") is not None:
            im = key_dark(im, int(entry["key_below"]))
        im = resize_to_width(im, entry.get("width"))
        if entry.get("height"):
            im = im.resize((im.size[0], int(entry["height"])), Image.LANCZOS)
        return image_writer(im, bool(entry.get("lossless")))
    if kind == "dt-title":
        return build_title(entry, dt, dump, cache)
    if kind == "pcm":
        return lambda target: write_pcm_ogg(entry, dt, target)
    if kind == "button88":
        return build_button(entry, repo)
    if kind is not None:
        raise PackError(f"{name}: unknown kind {kind!r}")
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
        return image_writer(sheet, lossless)
    if "shot" in entry:
        path = shots / entry["shot"]
        if not path.is_file():
            raise PackError(f"{name}: no screenshot {entry['shot']} in {shots}")
        im = Image.open(path).convert("RGBA")
        if entry.get("crop"):
            x, y, w, h = entry["crop"]
            im = im.crop((x, y, x + w, y + h))
        return image_writer(resize_to_width(im, entry.get("width")), False)
    if "file" in entry:
        path = repo / entry["file"]
        if not path.is_file():
            raise PackError(f"{name}: no file {entry['file']} in the repository")
        im = Image.open(path).convert("RGBA")
        return image_writer(resize_to_width(im, entry.get("width")), False)
    raise PackError(f"{name}: an entry needs sprite, shot, file or a kind")


def load_entries(art_json: Path) -> list:
    import json

    data = json.loads(art_json.read_text(encoding="utf-8"))
    entries = data["entries"] if isinstance(data, dict) else data
    names = [e["name"] for e in entries]
    if len(set(names)) != len(names):
        raise PackError("art.json: a name is listed twice")
    for n in names:
        if not n.endswith(ALLOWED_SUFFIXES) or "/" in n:
            raise PackError(f"art.json: bad name {n!r}")
    return entries


# --- the reference check -----------------------------------------------------------------------------

# /art/<name> in a URL, and art("<name>") or art(`<name>`) calls. ${...} is a wildcard for the name's
# dynamic part (portrait-${n}.webp matches portrait-00.webp and the rest).
NAME = r"[A-Za-z0-9._${}\[\]-]+\.(?:webp|png|ogg)"
URL_REF = re.compile(r"art/(" + NAME + r")")
CALL_REF = re.compile(r"""\bart\(\s*["'`](""" + NAME + r""")["'`]""")


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

def pack(dt: Path, shots: Path, out: Path, art_json: Path = ART_JSON, repo: Path = REPO,
         site_src: Path = SITE_SRC, dump=None):
    entries = load_entries(art_json)
    names = [e["name"] for e in entries]
    missing = check_references(site_src, names)
    if missing:
        raise PackError("site references pictures art.json does not pack: " + ", ".join(missing))
    art_dir = out / "art"
    art_dir.mkdir(parents=True, exist_ok=True)
    cache: dict = {}
    if any(e.get("kind") in ("lit", "dt-title") for e in entries):
        dump = dump_binary(dump)
    total = 0
    written = []
    for entry in entries:
        writer = build_entry(entry, dt, shots, repo, cache, dump)
        target = art_dir / entry["name"]
        info = writer(target)
        size = target.stat().st_size
        total += size
        written.append(target)
        print(f"info: {entry['name']} {info} {size} bytes")
    zip_path = out / "site-art.zip"
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as z:
        for target in written:
            z.write(target, f"art/{target.name}")
    print(f"info: {len(written)} pictures, {total} bytes; {zip_path} {zip_path.stat().st_size} bytes")
    return written


def main(argv: list) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--dt", required=True, type=Path, help="the Discord Times install folder")
    ap.add_argument("--shots", required=True, type=Path, help="folder of Razdor screenshots (PNG)")
    ap.add_argument("--out", required=True, type=Path, help="output folder (art/ and site-art.zip)")
    ap.add_argument("--dump", type=Path, default=None,
                    help="the dump_rgba example binary (default: built from this repository)")
    args = ap.parse_args(argv[1:])
    try:
        pack(args.dt, args.shots, args.out, dump=args.dump)
    except (PackError, OSError, KeyError, ValueError, subprocess.SubprocessError) as e:
        print(f"crit: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
