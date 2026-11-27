"""Tests of the canonical links (tools/site/canonical.py). Hermetic."""

import unittest

import canonical


class CanonicalTest(unittest.TestCase):
    def test_paths_are_taken_from_the_site_root(self):
        self.assertEqual(canonical.canonical_url("index.html"), "https://razdor.indicozy.dev/")
        self.assertEqual(canonical.canonical_url("wiki/en/mods/index.html"), "https://razdor.indicozy.dev/wiki/en/mods/")
        self.assertEqual(canonical.canonical_url("en/download/index.html"), "https://razdor.indicozy.dev/en/download/")

    def test_a_page_without_canonical_gets_one(self):
        out = canonical.fix_canonical("<html><head><title>t</title></head></html>", "wiki/x/index.html")
        self.assertIn('<link rel="canonical" href="https://razdor.indicozy.dev/wiki/x/"></head>', out)

    def test_a_github_canonical_is_replaced_not_duplicated(self):
        html = (
            '<html><head><link rel="canonical" href="https://indicozy.github.io/razdor/wiki/x/">'
            "<title>t</title></head></html>"
        )
        out = canonical.fix_canonical(html, "wiki/x/index.html")
        self.assertEqual(out.count('rel="canonical"'), 1)
        self.assertIn('href="https://razdor.indicozy.dev/wiki/x/"', out)
        self.assertNotIn("indicozy.github.io", out)

    def test_a_domain_canonical_is_kept(self):
        html = '<html><head><link rel="canonical" href="https://razdor.indicozy.dev/en/"></head></html>'
        self.assertEqual(canonical.fix_canonical(html, "en/index.html"), html)


if __name__ == "__main__":
    unittest.main()
