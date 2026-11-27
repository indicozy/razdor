"""Tests of the site's picture pack (tools/site/art_pack.py). Hermetic: temporary folders only.

Run from the repository root: python3 -I -m unittest discover -s tools/site -p 'test_*.py'
"""

import json
import shutil
import struct
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

from PIL import Image, ImageDraw

import art_pack


def size_of(path):
    with Image.open(path) as im:
        return im.size


def rotl16(v, n):
    return ((v << n) | (v >> (16 - n))) & 0xFFFF


def word_for(p):
    """The stored UGS word whose decoded nibbles are p (inverse of the reader's rotation and XOR)."""
    return rotl16(p ^ 0x5555, 3)


class PixelTest(unittest.TestCase):
    def test_nibbles_become_channels(self):
        # A=F, R=0, G=F, B=0: a solid opaque green-yellow pixel with no premultiplication.
        self.assertEqual(art_pack.pixel_rgba(word_for(0xF0F0)), (0, 255, 0, 255))

    def test_transparent_pixel_stays_transparent(self):
        self.assertEqual(art_pack.pixel_rgba(word_for(0x0F0F)), (0, 0, 0, 0))

    def test_premultiplied_colour_is_undone(self):
        # A=8 (136), R=4 (68): stored premultiplied, so the colour is 68 * 255 / 136 = 127.5, rounded to 128.
        self.assertEqual(art_pack.pixel_rgba(word_for(0x8400)), (128, 0, 0, 136))

    def test_additive_light_is_the_colour_and_alpha_is_the_brightest_channel(self):
        self.assertEqual(art_pack.pixel_additive(word_for(0x0840)), (136, 68, 0, 136))


class DecodeTest(unittest.TestCase):
    def test_a_plain_header_gives_one_frame_per_block(self):
        words = [word_for(0xF0F0), word_for(0x0F0F)]
        data = struct.pack("<HH", 2, 1) + struct.pack("<2H", *words)
        frames = art_pack.decode_ugs(data)
        self.assertEqual(len(frames), 1)
        self.assertEqual(frames[0].size, (2, 1))
        self.assertEqual(frames[0].getpixel((0, 0)), (0, 255, 0, 255))
        self.assertEqual(frames[0].getpixel((1, 0)), (0, 0, 0, 0))

    def test_a_repeated_header_is_read_as_one_frame(self):
        words = [word_for(0xF0F0)]
        data = struct.pack("<4H", 1, 1, 1, 1) + struct.pack("<H", words[0])
        frames = art_pack.decode_ugs(data)
        self.assertEqual(len(frames), 1)
        self.assertEqual(frames[0].getpixel((0, 0)), (0, 255, 0, 255))

    def test_a_truncated_frame_is_refused(self):
        with self.assertRaises(art_pack.PackError):
            art_pack.decode_ugs(struct.pack("<HH", 4, 4) + b"\x00\x00")


class FramesTest(unittest.TestCase):
    def test_frame_specs(self):
        self.assertEqual(art_pack.select_frames("40-42", 64, "x"), [40, 41, 42])
        self.assertEqual(art_pack.select_frames([3], 24, "x"), [3])
        self.assertEqual(art_pack.select_frames("all", 3, "x"), [0, 1, 2])

    def test_a_frame_out_of_range_is_refused(self):
        with self.assertRaises(art_pack.PackError):
            art_pack.select_frames("0-9", 4, "x")


class ReferenceTest(unittest.TestCase):
    def make_site(self, base: Path, text: str) -> Path:
        src = base / "src"
        (src / "components").mkdir(parents=True)
        (src / "components" / "A.astro").write_text(text, encoding="utf-8")
        return src

    def test_every_reference_must_be_packed(self):
        text = (
            "const art = (name) => `background-image:url('${B}art/${name}')`;\n"
            'art("a-big.webp"); art(`portrait-${n}.webp`); url(\'${B}art/shot-${shots[i]}.webp\');\n'
            "url('/art/missing.webp')\n"
        )
        names = ["a-big.webp", "portrait-00.webp", "shot-game.webp"]
        with tempfile.TemporaryDirectory() as tmp:
            src = self.make_site(Path(tmp), text)
            self.assertEqual(art_pack.check_references(src, names), ["missing.webp"])
            self.assertEqual(art_pack.check_references(src, names + ["missing.webp"]), [])

    def test_the_helper_variable_is_not_a_reference(self):
        with tempfile.TemporaryDirectory() as tmp:
            src = self.make_site(Path(tmp), "art/${name}")
            self.assertEqual(art_pack.check_references(src, []), [])


class PackTest(unittest.TestCase):
    def test_a_small_tree_packs_into_webp_and_a_zip(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            dt = base / "install"
            (dt / "Graphics" / "Units").mkdir(parents=True)
            data = struct.pack("<HH", 2, 1) + struct.pack("<2H", word_for(0xF0F0), word_for(0x0F0F))
            (dt / "Graphics" / "Units" / "Test.ugs").write_bytes(data)
            shots = base / "shots"
            shots.mkdir()
            Image.new("RGBA", (40, 20), (200, 10, 10, 255)).save(shots / "map.png")
            repo = base / "repo"
            (repo / "ads").mkdir(parents=True)
            Image.new("RGBA", (10, 10), (0, 0, 255, 255)).save(repo / "ads" / "cap.png")
            art_json = base / "art.json"
            art_json.write_text(
                json.dumps(
                    {
                        "entries": [
                            # the lookup is case-insensitive, as in the install
                            {"name": "test-row.webp", "sprite": "graphics/units/test.ugs", "frames": "all"},
                            {"name": "map.webp", "shot": "map.png", "crop": [0, 0, 20, 20]},
                            {"name": "cap.webp", "file": "ads/cap.png", "width": 5},
                        ]
                    }
                ),
                encoding="utf-8",
            )
            site = base / "src"
            site.mkdir()
            (site / "A.astro").write_text("art/test-row.webp art/map.webp art('cap.webp')", encoding="utf-8")
            out = base / "out"
            art_pack.pack(dt, shots, out, art_json=art_json, repo=repo, site_src=site)
            self.assertEqual(size_of(out / "art" / "test-row.webp"), (2, 1))
            self.assertEqual(size_of(out / "art" / "map.webp"), (20, 20))
            self.assertEqual(size_of(out / "art" / "cap.webp"), (5, 5))
            with zipfile.ZipFile(out / "site-art.zip") as z:
                self.assertEqual(
                    sorted(z.namelist()), ["art/cap.webp", "art/map.webp", "art/test-row.webp"]
                )

    def test_a_missing_reference_stops_the_pack(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            art_json = base / "art.json"
            art_json.write_text(json.dumps({"entries": []}), encoding="utf-8")
            site = base / "src"
            site.mkdir()
            (site / "A.astro").write_text("art/nothing.webp", encoding="utf-8")
            with self.assertRaises(art_pack.PackError):
                art_pack.pack(base, base, base / "out", art_json=art_json, repo=base, site_src=site)


class ShippedListTest(unittest.TestCase):
    def test_the_committed_art_json_covers_the_site(self):
        names = [e["name"] for e in art_pack.load_entries(art_pack.ART_JSON)]
        self.assertEqual(art_pack.check_references(art_pack.SITE_SRC, names), [])



class NewKindsTest(unittest.TestCase):
    """The kinds added for the 2003 look: the title, the sound, the 88x31 button and the lit pictures."""

    def test_rgba_of_additive_light(self):
        im = Image.new("RGB", (3, 1))
        im.putdata([(0, 0, 0), (136, 68, 0), (10, 200, 30)])
        px = list(art_pack._to_rgba(im).getdata())
        self.assertEqual(px[0], (0, 0, 0, 0))
        self.assertEqual(px[1], (255, 127, 0, 136))
        self.assertEqual(px[2][3], 200)

    def test_title_has_18_frames_of_the_canvas_size(self):
        title = Image.new("RGBA", (448, 100), (200, 200, 200, 255))
        flame = Image.new("RGB", (512, 504), (60, 30, 10))
        frames = art_pack.title_frames(title, flame)
        self.assertEqual(len(frames), 18)
        self.assertEqual(frames[0].size, (900, 190))

    def test_button_is_six_frames_with_a_one_pixel_bevel(self):
        capsule = Image.new("RGB", (462, 174), (30, 90, 160))
        frames = art_pack.button_frames(capsule)
        self.assertEqual(len(frames), 6)
        for f in frames:
            self.assertEqual(f.size, (88, 31))
            self.assertEqual(f.getpixel((0, 0))[:3], art_pack.BUTTON_LIGHT)
            self.assertEqual(f.getpixel((87, 30))[:3], art_pack.BUTTON_DARK)

    def test_button_file_is_an_apng_with_the_durations(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            (repo / "ads").mkdir()
            cap = Image.new("RGB", (462, 174))
            d = ImageDraw.Draw(cap)
            for x in range(462):  # a gradient, so the pan frames differ (APNG merges identical frames)
                d.line([(x, 0), (x, 173)], fill=(x % 256, 90, 160 - x % 100))
            cap.save(repo / "ads" / "capsule.png")
            writer = art_pack.build_entry(
                {"name": "thorny-88x31.png", "kind": "button88", "file": "ads/capsule.png"},
                repo, repo, repo, {})
            target = repo / "out.png"
            writer(target)
            with Image.open(target) as im:
                self.assertEqual(im.n_frames, 6)
                self.assertEqual(im.size, (88, 31))
                im.seek(4)
                self.assertEqual(im.info.get("duration"), 250)

    def test_load_entries_accepts_ogg_and_png(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "art.json"
            p.write_text(json.dumps({"entries": [{"name": "menu-theme.ogg"}, {"name": "x.png"}, {"name": "y.webp"}]}))
            self.assertEqual([e["name"] for e in art_pack.load_entries(p)], ["menu-theme.ogg", "x.png", "y.webp"])
            p.write_text(json.dumps({"entries": [{"name": "bad.gif"}]}))
            with self.assertRaises(art_pack.PackError):
                art_pack.load_entries(p)

    def test_dump_frames_are_read_from_the_example_output(self):
        with tempfile.TemporaryDirectory() as tmp:
            dump = Path(tmp) / "dump_fake"
            dump.write_text(
                f"#!{sys.executable} -I\n"
                "import struct, sys\n"
                "out, src = sys.argv[1], sys.argv[2]\n"
                "stem = src.rsplit('/', 1)[-1].rsplit('.', 1)[0]\n"
                "open(out + '/' + stem + '-0.rgba', 'wb').write(struct.pack('<II', 1, 1) + bytes([1, 2, 3, 255]))\n"
            )
            dump.chmod(0o755)
            src = Path(tmp) / "Pic.lit"
            src.write_bytes(b"")
            frames = art_pack.decode_with_dump(dump, src, {})
            self.assertEqual(len(frames), 1)
            self.assertEqual(frames[0].getpixel((0, 0)), (1, 2, 3, 255))

    def test_pcm_becomes_an_ogg_when_ffmpeg_is_present(self):
        if shutil.which("ffmpeg") is None:
            self.skipTest("ffmpeg is not installed")
        with tempfile.TemporaryDirectory() as tmp:
            dt = Path(tmp) / "dt"
            (dt / "Sounds").mkdir(parents=True)
            (dt / "Sounds" / "menu.raw").write_bytes(b"\x00\x10" * 22050)
            out = Path(tmp) / "menu-theme.ogg"
            art_pack.write_pcm_ogg({"sound": "Sounds/menu.raw"}, dt, out)
            self.assertGreater(out.stat().st_size, 100)


if __name__ == "__main__":
    unittest.main()
