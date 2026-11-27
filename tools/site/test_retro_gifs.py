"""Tests of the retro GIFs (tools/site/retro_gifs.py). Hermetic: a temporary folder only.

Run from the repository root: python3 -I -m unittest discover -s tools/site -p 'test_*.py'
"""

import tempfile
import unittest
from pathlib import Path

from PIL import Image

import retro_gifs


class RetroGifsTest(unittest.TestCase):
    def test_every_gif_is_written_at_its_size(self):
        with tempfile.TemporaryDirectory() as tmp:
            written = retro_gifs.build(Path(tmp))
            names = sorted(p.name for p in written)
            self.assertEqual(names, ["browsers-88x31.gif", "discord-88x31.gif", "notepad-88x31.gif",
                                     "razdor-88x31.gif", "under-construction.gif"])
            with Image.open(Path(tmp) / "razdor-88x31.gif") as im:
                self.assertEqual(im.size, (88, 31))
                self.assertEqual(im.n_frames, 2)
            with Image.open(Path(tmp) / "under-construction.gif") as im:
                self.assertEqual(im.size, (192, 44))

    def test_button_has_a_grey_bevel(self):
        frame = retro_gifs.button_frame(["RAZDOR"], retro_gifs.GREEN, retro_gifs.GOLD)
        self.assertEqual(frame.size, (88, 31))
        self.assertEqual(frame.getpixel((0, 0)), retro_gifs.GREY_LIGHT)
        self.assertEqual(frame.getpixel((87, 30)), retro_gifs.GREY_DARK)


if __name__ == "__main__":
    unittest.main()
