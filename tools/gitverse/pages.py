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


HOSTS = {
    # gitverse: the repo's main, the site in docs/ (the update feed, the config and the hub's pages)
    "gitverse": {"plain": PLAIN_URL, "branch": "main", "site_dir": "docs",
                 "base": "https://indicozy.gitverse.site/" + gv.HUB_REPO + "/"},
    # github: the repo's gh-pages branch, the site at its root (the Pages source of the github.io site)
    "github": {"plain": "https://github.com/" + gv.OWNER + "/{repo}.git", "branch": "gh-pages", "site_dir": "",
               "base": "https://" + gv.OWNER + ".github.io/" + gv.HUB_REPO + "/"},
}


def github_token():
    """The personal GitHub account's token (`gh auth token -u indicozy`): never the work account's."""
    r = subprocess.run(["gh", "auth", "token", "-u", gv.OWNER], capture_output=True, text=True)
    token = r.stdout.strip()
    if r.returncode != 0 or not token:
        raise PagesError("no GitHub token for the indicozy account (gh auth token -u indicozy)")
    return token


def _load_token_for(host):
    return github_token() if host == "github" else gv.load_token()


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
    """A fresh clone of a repo whose site is served by Pages. Use as a context manager or call
    `commit_push` yourself; nothing is pushed until commit_push.

    host "gitverse" (the default): the site is `docs/` of the GitVerse repo's `main`, served at
    `indicozy.gitverse.site/<repo>/`. host "github": the site is the root of the GitHub repo's
    `gh-pages` branch (the Pages source of `indicozy.github.io/<repo>/`); the branch must exist
    before the first push (see the publish notes in tools/gitverse/README.md)."""

    def __init__(self, repo=gv.HUB_REPO, token=None, remote=None, scratch=None, host="gitverse"):
        """`remote` (tests only) replaces the host's address with a local repository, and the token
        is then not needed."""
        if host not in HOSTS:
            raise PagesError(f"unknown Pages host {host!r}")
        self.repo = repo
        self.host = host
        self.remote = remote
        self.branch = HOSTS[host]["branch"]
        self.site_dir = HOSTS[host]["site_dir"]
        self.base = HOSTS[host]["base"]
        self.plain_url = HOSTS[host]["plain"].format(repo=repo)
        self.token = None if remote else (token or _load_token_for(host))
        self.scratch = scratch or SCRATCH
        self.dir = os.path.join(self.scratch, repo if host == "gitverse" else f"{repo}-{host}")
        self._clone()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def _url(self):
        if self.remote:
            return self.remote
        return f"https://{AUTHOR[0]}:{self.token}@" + self.plain_url[len("https://"):]

    def _clone(self):
        if os.path.exists(self.dir):
            if not os.path.abspath(self.dir).startswith(os.path.abspath(self.scratch) + os.sep):
                raise PagesError(f"refusing to delete {self.dir}")
            shutil.rmtree(self.dir)
        os.makedirs(self.scratch, exist_ok=True)
        # a real clone is shallow: the site's history is not needed to add a commit, and the site is hundreds of MB
        depth = [] if self.remote else ["--depth", "1"]
        _git(["-c", "credential.helper=", "clone", "--quiet", *depth, "--branch", self.branch, self._url(), self.dir],
             cwd=self.scratch, token=self.token)
        _git(["remote", "set-url", "origin", self.remote or self.plain_url], cwd=self.dir)
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

    def _site_root(self):
        """The folder of the clone that is the site (docs/ on GitVerse, the clone's root on GitHub)."""
        return os.path.join(self.dir, self.site_dir) if self.site_dir else self.dir

    def _site_files(self):
        """The site's files as site-relative paths (the clone's .git folder is never part of the site)."""
        root = self._site_root()
        out = []
        if not os.path.isdir(root):
            return out
        for dirpath, dirnames, names in os.walk(root):
            dirnames[:] = [d for d in dirnames if not (dirpath == self.dir and d == ".git")]
            for n in names:
                out.append(os.path.relpath(os.path.join(dirpath, n), root).replace(os.sep, "/"))
        return out

    def _in_repo(self, site_rel):
        """A site-relative path as the clone's repo path."""
        return f"{self.site_dir}/{site_rel}" if self.site_dir else site_rel

    def sync_site(self, files):
        """Makes the site hold exactly `files` ({site-relative path: bytes}): files of an older catalog that
        are not in it are removed. The update feed (releases.json), the remote config and .nojekyll are kept."""
        for rel in self._site_files():
            if rel not in files and rel not in KEEP_IN_DOCS:
                os.remove(os.path.join(self._site_root(), *rel.split("/")))
        for rel, data in files.items():
            self.write(self._in_repo(rel), data)

    def sync_site_dir(self, src, remove=True, skip=()):
        """`sync_site` with the new site as a folder `src` (copied file by file, not read into memory: the
        pages layout's site is hundreds of MB). The same keeps. `remove=False` only adds and overwrites (the
        first push of a two-step publish: the live index still names the old files); `skip` leaves out
        site-relative paths (the index, in that first push)."""
        wanted = set()
        for dirpath, _, names in os.walk(src):
            for n in names:
                wanted.add(os.path.relpath(os.path.join(dirpath, n), src).replace(os.sep, "/"))
        wanted -= set(skip)
        root = self._site_root()
        if remove:
            for rel in self._site_files():
                if rel not in wanted and rel not in KEEP_IN_DOCS and rel not in skip:
                    os.remove(os.path.join(root, *rel.split("/")))
        for rel in sorted(wanted):
            dst = os.path.join(root, *rel.split("/"))
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copyfile(os.path.join(src, *rel.split("/")), dst)

    def ensure_nojekyll(self):
        path = os.path.join(self._site_root(), ".nojekyll")
        if not os.path.exists(path):
            self.write(self._in_repo(".nojekyll"), b"")

    def commit_push(self, message):
        """Commits what changed (nothing: no commit, no push) and pushes to the host's branch (main on
        GitVerse). Returns the new short id, or None when nothing changed. Every push writes config.json
        from data/remote/config.json first (`config_bytes`)."""
        self.write(self._in_repo("config.json"), config_bytes())
        self.ensure_nojekyll()
        _git(["add", "-A"], cwd=self.dir)
        if not _git(["status", "--porcelain"], cwd=self.dir).strip():
            return None
        _git(["commit", "--quiet", "-m", message], cwd=self.dir)
        head = _git(["rev-parse", "--short=8", "HEAD"], cwd=self.dir).strip()
        _git(["push", "--quiet", self._url(), f"HEAD:refs/heads/{self.branch}"], cwd=self.dir, token=self.token)
        return head

    def wait_served(self, expected, wait=WAIT_DEFAULT, interval=20, sleep=time.sleep):
        """Polls the anonymous URLs of `expected` ({repo path or site path: bytes}) until each serves exactly
        those bytes. A key starting with the site folder's name ("docs/") is taken as a site-relative path.
        Returns the seconds waited; raises PagesError at the deadline."""
        started = time.time()
        pending = dict(expected)
        base = getattr(self, "base", None) or PAGES_BASE
        site_dir = getattr(self, "site_dir", "docs")
        while True:
            for rel in list(pending):
                prefix = site_dir + "/" if site_dir else None
                site = rel[len(prefix):] if prefix and rel.startswith(prefix) else rel
                try:
                    got = gv.fetch_bytes(base + site, tries=2)
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
