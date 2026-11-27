#!/usr/bin/env python3
"""Sets the canonical address of the site's HTML pages to the owner's domain, https://razdor.indicozy.dev.

The site and the wiki are one site on https://razdor.indicozy.dev (the site's own pages already name it).
The GitHub Pages copy and the wiki's generated links name GitHub's address instead (https://indicozy.github.io/
razdor/...). Search engines must index the domain's address, so:

- a page whose canonical link already names the domain keeps it;
- a page with a canonical link to any other address gets the domain's address for its path;
- a page without a canonical link gets one.

The address is the page's path under the site root (the /razdor/ prefix of the GitHub copy is not part of it):
index.html -> /, wiki/en/index.html -> /wiki/en/.

Usage: python3 -I tools/site/canonical.py DIR   (rewrites the HTML files under DIR in place)
"""

import re
import sys
from pathlib import Path

DOMAIN = "https://razdor.indicozy.dev"
CANONICAL = re.compile(r"""<link\s+rel=["']canonical["'][^>]*>\s*""", re.IGNORECASE)
CANONICAL_HREF = re.compile(r"""<link\s+rel=["']canonical["'][^>]*href=["']([^"']*)["']""", re.IGNORECASE)


def canonical_url(rel: str) -> str:
    """The domain's address of a page, from its path under the site root (rel uses '/')."""
    if rel == "index.html":
        return DOMAIN + "/"
    if rel.endswith("/index.html"):
        return DOMAIN + "/" + rel[: -len("index.html")]
    return DOMAIN + "/" + rel


def fix_canonical(html: str, rel: str) -> str:
    """The page with its canonical link on the domain (see the module doc)."""
    m = CANONICAL_HREF.search(html)
    if m and m.group(1).startswith(DOMAIN):
        return html
    tag = f'<link rel="canonical" href="{canonical_url(rel)}">'
    if m:
        return CANONICAL.sub(lambda _: tag, html, count=1)
    if "</head>" in html:
        return html.replace("</head>", tag + "</head>", 1)
    return tag + html


def fix_tree(root: Path) -> int:
    """Fixes every HTML page under root in place; returns how many files changed."""
    changed = 0
    for path in sorted(root.rglob("*.html")):
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        new = fix_canonical(text, rel)
        if new != text:
            path.write_text(new, encoding="utf-8")
            changed += 1
    return changed


def main(argv: list) -> int:
    if len(argv) != 2:
        print("usage: canonical.py DIR", file=sys.stderr)
        return 2
    root = Path(argv[1])
    if not root.is_dir():
        print(f"crit: {root} is not a folder", file=sys.stderr)
        return 1
    print(f"info: canonical links set in {fix_tree(root)} pages of {root}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
