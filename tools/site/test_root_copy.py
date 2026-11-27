"""Tests of the root copy of the Pages site (tools/site/root_copy.py). Hermetic: temporary folders only."""

import tempfile
import unittest
from pathlib import Path

import root_copy


class RewriteTest(unittest.TestCase):
    def test_root_relative_razdor_links_lose_the_prefix(self):
        html = '<a href="/razdor/wiki/tutorials/interface/">x</a> <img src="/razdor/fire/a.webp">'
        self.assertEqual(
            root_copy.rewrite_text(html),
            '<a href="/wiki/tutorials/interface/">x</a> <img src="/fire/a.webp">',
        )

    def test_css_and_script_urls_are_rewritten_too(self):
        self.assertEqual(root_copy.rewrite_text("background:url(/razdor/fonts/a.woff2)"), "background:url(/fonts/a.woff2)")
        self.assertEqual(root_copy.rewrite_text('const b="/razdor/wiki";'), 'const b="/wiki";')

    def test_github_addresses_outside_links_are_kept(self):
        html = 'see https://github.com/indicozy/razdor and https://indicozy.github.io/razdor-tools/'
        self.assertEqual(root_copy.rewrite_text(html), html)

    def test_the_site_own_links_become_root_relative(self):
        self.assertEqual(
            root_copy.rewrite_text('<a href="https://indicozy.github.io/razdor/wiki/">w</a>'),
            '<a href="/wiki/">w</a>',
        )
        self.assertEqual(
            root_copy.rewrite_text('<a href="https://indicozy.github.io/razdor">h</a>'),
            '<a href="/">h</a>',
        )

    def test_the_wiki_absolute_addresses_go_back_to_github(self):
        html = '<meta property="og:url" content="https://indicozy.github.io/wiki/start/"/>'
        self.assertEqual(
            root_copy.rewrite_text(html),
            '<meta property="og:url" content="https://indicozy.github.io/razdor/wiki/start/"/>',
        )

    def test_a_word_that_ends_in_razdor_is_not_a_link(self):
        self.assertEqual(root_copy.rewrite_text("see /my/razdor/x"), "see /my/razdor/x")


class CopyTest(unittest.TestCase):
    def make_site(self, base: Path) -> Path:
        src = base / "site"
        (src / "wiki" / "en").mkdir(parents=True)
        (src / "index.html").write_text('<html><head></head><a href="/razdor/wiki/">w</a></html>', encoding="utf-8")
        (src / "wiki" / "index.html").write_text("<html><head></head><body>wiki</body></html>", encoding="utf-8")
        (src / "wiki" / "en" / "index.html").write_text("<html><head></head></html>", encoding="utf-8")
        (src / "style.css").write_text("a{background:url(/razdor/fire/x.webp)}", encoding="utf-8")
        (src / "fire").mkdir()
        (src / "fire" / "x.webp").write_bytes(b"\x00\x01\x02")
        return src

    def test_the_copy_is_rewritten_and_the_source_is_untouched(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            src = self.make_site(base)
            out = base / "root"
            counts = root_copy.make_root_copy(src, out)
            self.assertEqual(counts["pages"], 3)
            self.assertIn('href="/wiki/"', (out / "index.html").read_text(encoding="utf-8"))
            self.assertIn("url(/fire/x.webp)", (out / "style.css").read_text(encoding="utf-8"))
            self.assertEqual((out / "fire" / "x.webp").read_bytes(), b"\x00\x01\x02")
            self.assertIn("/razdor/wiki/", (src / "index.html").read_text(encoding="utf-8"))
            self.assertIn(
                '<link rel="canonical" href="https://razdor.indicozy.dev/wiki/en/">',
                (out / "wiki" / "en" / "index.html").read_text(encoding="utf-8"),
            )

    def test_a_copy_without_the_wiki_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            src = base / "site"
            src.mkdir()
            (src / "index.html").write_text("<html><head></head></html>", encoding="utf-8")
            with self.assertRaises(FileNotFoundError):
                root_copy.make_root_copy(src, base / "root")

    def test_an_existing_output_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            src = self.make_site(base)
            (base / "root").mkdir()
            with self.assertRaises(FileExistsError):
                root_copy.make_root_copy(src, base / "root")


if __name__ == "__main__":
    unittest.main()
