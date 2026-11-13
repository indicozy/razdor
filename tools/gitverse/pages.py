#!/usr/bin/env python3
"""The GitHub-free Pages push for the razdor-community repo on GitVerse (stdlib only).

The repo's `docs/` folder is the site (`https://indicozy.gitverse.site/razdor-community/`): the
update feed `releases.json`, the hub catalog `index.json` with its `previews/`, and the hub's web
page (`index.html`, `hub.js`, `hub.css` from tools/community-pack/web/). Each push clones the repo
fresh into the scratch folder, changes only the files the caller names, commits as the owner, pushes
to `main` and waits until the anonymous URLs serve exactly the bytes that were pushed (a reply counts
only when it is the new file: an unknown path answers 200 with an HTML page on GitVerse).

Auth: the token goes into the clone and push URLs only, never printed; the clone's remote is set back
to the tokenless address at once.
"""

import os
import re
import shutil
import subprocess
import sys
import time
import urllib.parse

import gitverse as gv

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
WEB_DIR = os.path.join(REPO_ROOT, "tools", "community-pack", "web")
SCRATCH = os.environ.get("RAZDOR_GITVERSE_SCRATCH") or "/mnt/ai_models/razdor-scratch/gitverse-pages"
PLAIN_URL = gv.HOST + "/" + gv.OWNER + "/{repo}.git"
PAGES_BASE = "https://indicozy.gitverse.site/" + gv.HUB_REPO + "/"
AUTHOR = ("indicozy", "indicozy@gmail.com")
WAIT_DEFAULT = 900
WEB_FILES = ("index.html", "hub.js", "hub.css")
KEEP_IN_DOCS = (".nojekyll", "releases.json", "config.json")  # never removed by a site sync (the feed is the release script's; the config is written on every push)
CONFIG_SRC = os.path.join(REPO_ROOT, "data", "remote", "config.json")  # the remote config the app reads (the same bytes as GitHub's)
META_FILES_BASE = re.compile(r"<meta\s+name=[\"']razdor-files-base[\"'][^>]*>", re.IGNORECASE)
COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)


class PagesError(gv.GitVerseError):
    pass


def web_files():
    """The hub page's files as {name: bytes}, for the site root. The files base must stay unset on
    GitVerse: the catalog's previews are relative and live next to index.json (hub.js: the page's own
    folder). An active `razdor-files-base` meta is refused rather than rewritten."""
    out = {}
    for name in WEB_FILES:
        with open(os.path.join(WEB_DIR, name), "rb") as f:
            data = f.read()
        if name == "index.html":
            text = COMMENT.sub("", data.decode("utf-8"))
            if META_FILES_BASE.search(text):
                raise PagesError("tools/community-pack/web/index.html sets razdor-files-base: unset it for GitVerse")
        out[name] = data
    return out


def config_bytes():
    """The remote config (`data/remote/config.json`) as the bytes the site serves."""
    with open(CONFIG_SRC, "rb") as f:
        return f.read()


def _git(args, cwd, token=None):
    cmd = ["git", *args]
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        msg = gv.redact((r.stderr or r.stdout).strip(), token)
        raise PagesError(f"git {args[0]} failed: {msg}")
    return r.stdout


class Pages:
    """A fresh clone of a GitVerse repo whose `docs/` is the site. Use as a context manager or call
    `commit_push` yourself; nothing is pushed until commit_push."""

    def __init__(self, repo=gv.HUB_REPO, token=None, remote=None, scratch=None):
        """`remote` (tests only) replaces the GitVerse address with a local repository, and the token
        is then not needed."""
        self.repo = repo
        self.remote = remote
        self.token = None if remote else (token or gv.load_token())
        self.scratch = scratch or SCRATCH
        self.dir = os.path.join(self.scratch, repo)
        self._clone()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def _url(self):
        if self.remote:
            return self.remote
        return f"https://{AUTHOR[0]}:{self.token}@" + PLAIN_URL.format(repo=self.repo)[len("https://"):]

    def _clone(self):
        if os.path.exists(self.dir):
            if not os.path.abspath(self.dir).startswith(os.path.abspath(self.scratch) + os.sep):
                raise PagesError(f"refusing to delete {self.dir}")
            shutil.rmtree(self.dir)
        os.makedirs(self.scratch, exist_ok=True)
        _git(["-c", "credential.helper=", "clone", "--quiet", "--branch", "main", self._url(), self.dir],
             cwd=self.scratch, token=self.token)
        _git(["remote", "set-url", "origin", self.remote or PLAIN_URL.format(repo=self.repo)], cwd=self.dir)
        _git(["config", "user.name", AUTHOR[0]], cwd=self.dir)
        _git(["config", "user.email", AUTHOR[1]], cwd=self.dir)

    def read(self, rel):
        path = os.path.join(self.dir, rel)
        if not os.path.exists(path):
            return None
        with open(path, "rb") as f:
            return f.read()

    def write(self, rel, data):
        path = os.path.join(self.dir, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)

    def clear(self, rel_dir):
        path = os.path.join(self.dir, rel_dir)
        if os.path.isdir(path):
            shutil.rmtree(path)

    def sync_site(self, files):
        """Makes docs/ hold exactly `files` ({docs-relative path: bytes}): files of an older catalog that
        are not in it are removed. The update feed (releases.json) and .nojekyll are kept."""
        docs = os.path.join(self.dir, "docs")
        if os.path.isdir(docs):
            for root, _, names in os.walk(docs):
                for n in names:
                    rel = os.path.relpath(os.path.join(root, n), docs).replace(os.sep, "/")
                    if rel not in files and rel not in KEEP_IN_DOCS:
                        os.remove(os.path.join(root, n))
        for rel, data in files.items():
            self.write("docs/" + rel, data)

    def ensure_nojekyll(self):
        path = os.path.join(self.dir, "docs", ".nojekyll")
        if not os.path.exists(path):
            self.write("docs/.nojekyll", b"")

    def commit_push(self, message):
        """Commits what changed (nothing: no commit, no push) and pushes to main. Returns the new
        short id, or None when nothing changed. Every push writes docs/config.json from
        data/remote/config.json first (`config_bytes`)."""
        self.write("docs/config.json", config_bytes())
        self.ensure_nojekyll()
        _git(["add", "-A"], cwd=self.dir)
        if not _git(["status", "--porcelain"], cwd=self.dir).strip():
            return None
        _git(["commit", "--quiet", "-m", message], cwd=self.dir)
        head = _git(["rev-parse", "--short=8", "HEAD"], cwd=self.dir).strip()
        _git(["push", "--quiet", self._url(), "HEAD:refs/heads/main"], cwd=self.dir, token=self.token)
        return head

    def wait_served(self, expected, wait=WAIT_DEFAULT, interval=20, sleep=time.sleep):
        """Polls the anonymous URLs of `expected` ({site-relative path: bytes}) until each serves exactly
        those bytes. Returns the seconds waited; raises PagesError at the deadline."""
        started = time.time()
        pending = dict(expected)  # repo path -> bytes; docs/ is the site root
        while True:
            for rel in list(pending):
                site = rel[len("docs/"):] if rel.startswith("docs/") else rel
                try:
                    got = gv.fetch_bytes(PAGES_BASE + site, tries=2)
                except gv.GitVerseError:
                    got = None  # not served yet (or an error): poll again
                if got == pending[rel]:
                    del pending[rel]
            if not pending:
                return time.time() - started
            if time.time() - started > wait:
                raise PagesError(f"Pages do not serve {sorted(pending)} yet (waited {wait} s); the push is in "
                                 f"{self.repo}, check {PAGES_BASE} later")
            print(f"  waiting for Pages: {', '.join(sorted(pending))}", file=sys.stderr, flush=True)
            sleep(interval)


def delete_git_tag(repo, tag, token=None):
    """Deletes tag `tag` on the GitVerse repo (the git tag, not the release). Runs in this checkout."""
    token = token or gv.load_token()
    url = f"https://{AUTHOR[0]}:{token}@" + PLAIN_URL.format(repo=repo)[len("https://"):]
    _git(["push", "--quiet", url, f":refs/tags/{urllib.parse.unquote(tag)}"], cwd=REPO_ROOT, token=token)
