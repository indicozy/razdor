#!/usr/bin/env python3
"""Makes the site's retro pixel pieces: an «В разработке» sign and four 88x31 buttons, as GIFs.

Original pixel art, drawn here (no game art): the sign is drawn at 1x and scaled 2x with nearest-neighbour
sampling; the buttons are 88x31 at 1x with a 1 px bevel. The text is a Cyrillic TTF rendered without
antialiasing (a hard 1-bit mask), so it stays crisp.

Output (default: site/public/retro/): under-construction.gif, razdor-88x31.gif, discord-88x31.gif,
notepad-88x31.gif, browsers-88x31.gif.

Usage: python3 -I tools/site/retro_gifs.py [--out DIR]
Needs Python 3 and Pillow only.
"""

import argparse
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
DEFAULT_OUT = REPO / "site" / "public" / "retro"
FONT_CANDIDATES = [
    "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation-sans-fonts/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
]
YELLOW = (255, 204, 0)
BLACK = (0, 0, 0)
GOLD = (255, 204, 0)
PALE_GOLD = (255, 240, 170)
GREEN = (34, 74, 46)
BLURPLE = (88, 101, 242)
WHITE = (255, 255, 255)
GREY_LIGHT = (223, 223, 223)
GREY_DARK = (96, 96, 96)
PAUSE_SIGN = 600
PAUSE_BLINK = 200


class RetroError(Exception):
    pass


def _font(size: int):
    for path in FONT_CANDIDATES:
        if Path(path).is_file():
            return ImageFont.truetype(path, size)
    raise RetroError("no TTF font with Cyrillic found")


def text_mask(size: tuple, lines: list, max_w: int, max_h: int) -> Image.Image:
    """The lines as a hard 1-bit mask on a canvas of `size`, at the largest font size that fits the box."""
    chosen = None
    for px in range(12, 4, -1):
        font = _font(px)
        probe = ImageDraw.Draw(Image.new("L", size))
        boxes = [probe.textbbox((0, 0), line, font=font) for line in lines]
        w = max(b[2] - b[0] for b in boxes)
        h = sum(b[3] - b[1] for b in boxes) + (len(lines) - 1)
        if w <= max_w and h <= max_h:
            chosen = (font, boxes, w, h)
            break
    if chosen is None:
        raise RetroError(f"the text {lines!r} does not fit {max_w}x{max_h}")
    font, boxes, w, h = chosen
    mask = Image.new("L", size, 0)
    d = ImageDraw.Draw(mask)
    y = (size[1] - h) // 2
    for line, b in zip(lines, boxes):
        lw = b[2] - b[0]
        d.text(((size[0] - lw) // 2 - b[0], y - b[1]), line, font=font, fill=255)
        y += (b[3] - b[1]) + 1
    return mask.point(lambda v: 255 if v >= 128 else 0)


def bevel(canvas: Image.Image) -> Image.Image:
    d = ImageDraw.Draw(canvas)
    w, h = canvas.size
    d.line([(0, 0), (w - 1, 0)], fill=GREY_LIGHT)
    d.line([(0, 0), (0, h - 1)], fill=GREY_LIGHT)
    d.line([(0, h - 1), (w - 1, h - 1)], fill=GREY_DARK)
    d.line([(w - 1, 0), (w - 1, h - 1)], fill=GREY_DARK)
    return canvas


def button_frame(lines: list, bg: tuple, fg: tuple) -> Image.Image:
    """One 88x31 button frame: a flat background, the text in fg, the 1 px bevel."""
    size = (88, 31)
    canvas = Image.new("RGB", size, bg)
    mask = text_mask(size, lines, max_w=84, max_h=25)
    canvas.paste(fg, (0, 0, size[0], size[1]), mask)
    return bevel(canvas)


def sign_frame(phase: int) -> Image.Image:
    """The sign at 1x (96x22): yellow and black stripes on the top and bottom bands, which move with the phase."""
    w, h = 96, 22
    canvas = Image.new("RGB", (w, h), BLACK)
    d = ImageDraw.Draw(canvas)
    d.rectangle([0, 4, w - 1, 17], fill=YELLOW)
    for band in ((0, 3), (18, 21)):
        for y in range(band[0], band[1] + 1):
            for x in range(w):
                if ((x + y + phase) // 2) % 2 == 0:
                    canvas.putpixel((x, y), YELLOW)
    mask = text_mask((w, h), ["В разработке"], max_w=w - 8, max_h=11)
    canvas.paste(BLACK, (0, 0, w, h), mask)
    return canvas.resize((w * 2, h * 2), Image.NEAREST)


def save_gif(frames: list, durations: list, path: Path) -> None:
    pal = [f.convert("P", palette=Image.ADAPTIVE, colors=16) for f in frames]
    pal[0].save(path, save_all=True, append_images=pal[1:], duration=durations, loop=0, optimize=False)


def build(out: Path) -> list:
    out.mkdir(parents=True, exist_ok=True)
    written = []

    sign = [sign_frame(0), sign_frame(2)]
    p = out / "under-construction.gif"
    save_gif(sign, [PAUSE_SIGN // 2, PAUSE_SIGN // 2], p)
    written.append(p)

    razdor = [button_frame(["RAZDOR"], GREEN, GOLD), button_frame(["RAZDOR"], GREEN, PALE_GOLD)]
    p = out / "razdor-88x31.gif"
    save_gif(razdor, [600, PAUSE_BLINK], p)
    written.append(p)

    p = out / "discord-88x31.gif"
    save_gif([button_frame(["DISCORD"], BLURPLE, WHITE)], [1000], p)
    written.append(p)

    p = out / "notepad-88x31.gif"
    save_gif([button_frame(["Сделано в", "Блокноте"], (235, 235, 210), (30, 30, 30))], [1000], p)
    written.append(p)

    p = out / "browsers-88x31.gif"
    save_gif([button_frame(["IE 6 /", "Firefox"], (20, 20, 70), (250, 250, 250))], [1000], p)
    written.append(p)
    return written


def main(argv: list) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--out", type=Path, default=DEFAULT_OUT, help="output folder")
    args = ap.parse_args(argv[1:])
    try:
        for p in build(args.out):
            print(f"info: {p} {p.stat().st_size} bytes")
    except (RetroError, OSError) as e:
        print(f"crit: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
