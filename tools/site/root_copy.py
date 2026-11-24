#!/usr/bin/env python3
"""Makes the root copy of the Pages site: the files served at https://razdor.indicozy.dev/ (and /wiki/).

Input: the assembled Pages site (the gh-pages files, the wiki built into wiki/, config.json). Output: a copy
of it for the owner's server, where the site is at the root of the domain instead of under /razdor/:

- root-relative links that start with /razdor/ lose that prefix (/razdor/wiki/x/ -> /wiki/x/). The wiki's
  Markdown writes such links by hand, and Astro does not rewrite them when the base changes;
- the wiki's absolute addresses (https://indicozy.github.io/wiki/...) get the GitHub path back, so the
  Open Graph tags name GitHub's pages;
- every HTML page gets a <link rel="canonical"> to its GitHub address, so search engines index GitHub's copy.

The source is never changed. The output must hold index.html and wiki/index.html, or the run fails.

Usage: python3 -I tools/site/root_copy.py SRC OUT   (OUT must not exist)
"""

import re
import shutil
import sys
from pathlib import Path

GITHUB_SITE = "https://indicozy.github.io/razdor"
TEXT_SUFFIXES = {".html", ".htm", ".css", ".js", ".mjs", ".json", ".xml", ".txt", ".webmanifest", ".svg"}

# A root-relative /razdor/ (not the one after a host name): preceded by a quote, a bracket, a space, = or a comma.
ROOT_RAZDOR = re.compile(r"""(?P<pre>["'(=\s,])/razdor/""")
# Astro's absolute addresses for the wiki when the base is /wiki (Open Graph and similar tags).
ABS_WIKI = re.compile(r"https://indicozy\.github\.io/wiki(?=[/\"'])")
CANONICAL = re.compile(r"""<link\s+rel=["']canonical["'][^>]*>\s*""", re.IGNORECASE)


def rewrite_text(text: str) -> str:
    """The root-copy rewrite of one text file (no canonical link; see canonical_for)."""
    text = ROOT_RAZDOR.sub(lambda m: m.group("pre") + "/", text)
    return ABS_WIKI.sub(GITHUB_SITE + "/wiki", text)


def github_path(rel: str) -> str:
    """The GitHub address of a page, from its path under the site root (index.html -> the folder's URL)."""
    if rel == "index.html":
        return GITHUB_SITE + "/"
    if rel.endswith("/index.html"):
        return GITHUB_SITE + "/" + rel[: -len("index.html")]
    return GITHUB_SITE + "/" + rel


def canonical_for(html: str, rel: str) -> str:
    """The page with its canonical link set to its GitHub address (any old canonical link is replaced)."""
    html = CANONICAL.sub("", html)
    tag = f'<link rel="canonical" href="{github_path(rel)}">'
    if "</head>" in html:
        return html.replace("</head>", tag + "</head>", 1)
    return tag + html


def make_root_copy(src: Path, out: Path) -> dict:
    if out.exists():
        raise FileExistsError(f"{out} exists")
    shutil.copytree(src, out, symlinks=False)
    counts = {"text": 0, "pages": 0}
    for path in sorted(out.rglob("*")):
        if not path.is_file() or path.suffix.lower() not in TEXT_SUFFIXES:
            continue
        rel = path.relative_to(out).as_posix()
        text = path.read_text(encoding="utf-8")
        new = rewrite_text(text)
        if path.suffix.lower() in {".html", ".htm"}:
            new = canonical_for(new, rel)
            counts["pages"] += 1
        if new != text:
            path.write_text(new, encoding="utf-8")
            counts["text"] += 1
    for must in ("index.html", "wiki/index.html"):
        if not (out / must).is_file():
            raise FileNotFoundError(f"the root copy has no {must}")
    return counts


def main(argv: list) -> int:
    if len(argv) != 3:
        print("usage: root_copy.py SRC OUT", file=sys.stderr)
        return 2
    try:
        counts = make_root_copy(Path(argv[1]), Path(argv[2]))
    except (OSError, UnicodeDecodeError) as e:
        print(f"crit: {e}", file=sys.stderr)
        return 1
    print(f"info: root copy in {argv[2]}: {counts['pages']} pages, {counts['text']} text files rewritten")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
