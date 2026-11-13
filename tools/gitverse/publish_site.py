#!/usr/bin/env python3
"""Publishes the Razdor website to GitVerse Pages: https://indicozy.gitverse.site/razdor/ (stdlib only).

It is what https://indicozy.github.io/razdor/ serves, assembled the way .github/workflows/pages.yml does:
  - the hand-made site: the gh-pages branch of indicozy/razdor (origin/gh-pages), without its workflow;
  - the player wiki (wiki/, Starlight) built with `npm ci && npm run build`, copied into its wiki/ folder;
  - the remote config (data/remote/config.json) at the root, as config.json.
The build reads the committed HEAD (git archive), not the working tree, and runs in a scratch copy, so
nothing is written into the repo. Heavy jobs are pinned to CPUs 8-19 at nice 19.

GitVerse Pages runs a workflow (the owner's setting: Pages source "workflow"). The site goes to the `site`
branch of indicozy/razdor as the folder `site/` plus `.github/workflows/gitverse-pages.yaml`
(tools/gitverse/site-pages.yaml), as a fresh orphan commit, force-pushed to that branch only. `main` (the
code mirror that the release tags point into) is never touched. The script then waits for the workflow
run of that commit (it fails fast on a failed run and names its jobs), and for the anonymous URLs to serve
exactly the new bytes: an unknown path answers 200 with an HTML "not found" page on GitVerse, so the bytes
are compared, not the status.

Usage:
  python3 -I tools/gitverse/publish_site.py                   # build, assemble, push, wait, check
  python3 -I tools/gitverse/publish_site.py --assemble-only   # build and assemble in scratch; no network push
  python3 -I tools/gitverse/publish_site.py --check-only      # check the live site against the scratch site
Scratch: RAZDOR_GV_SITE_SCRATCH, default /mnt/ai_models/razdor-scratch/site-gv (never /tmp).
"""

import argparse
import glob
import html.parser
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import time
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import gitverse as gv  # noqa: E402
import pages  # noqa: E402

REPO_ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
WORKFLOW_SRC = os.path.join(HERE, "site-pages.yaml")
WORKFLOW_DEST = ".github/workflows/gitverse-pages.yaml"
SITE_BRANCH = "site"
GH_PAGES_REF = "origin/gh-pages"
SRC_PATHS = ("wiki", "data/remote")  # what the build reads, from the committed HEAD
PUBLIC = "https://indicozy.gitverse.site/razdor/"
SCRATCH = os.environ.get("RAZDOR_GV_SITE_SCRATCH") or "/mnt/ai_models/razdor-scratch/site-gv"
HEAVY = ["nice", "-n", "19", "taskset", "-c", "8-19"]  # the owner's pinning: CPUs 0-7 are the desktop
WAIT_DEFAULT = 900
INTERVAL = 20
PAGE_SAMPLE = ("index.html", "wiki/index.html", "wiki/en/index.html")  # plus one ru and one en page
SUCCESS = frozenset({"success"})
FAILED = frozenset({"failure", "failed", "cancelled", "canceled", "error"})


class SiteError(gv.GitVerseError):
    pass


def pinned(cmd):
    """The command under the owner's CPU pinning (when the tools exist)."""
    if shutil.which("taskset") and shutil.which("nice"):
        return HEAVY + cmd
    return cmd


def note(msg):
    print(msg, file=sys.stderr, flush=True)


def reset_dir(path, scratch):
    """Empties `path` (it must be inside the scratch folder) and creates it."""
    full = os.path.abspath(path)
    if not full.startswith(os.path.abspath(scratch) + os.sep):
        raise SiteError(f"refusing to delete {full}: outside the scratch folder")
    if os.path.lexists(full):
        if os.path.isdir(full) and not os.path.islink(full):
            shutil.rmtree(full)
        else:
            os.remove(full)
    os.makedirs(full)
    return full


def _run(cmd, cwd):
    r = subprocess.run(cmd, cwd=cwd)
    if r.returncode != 0:
        raise SiteError(f"{' '.join(cmd[:3])} failed in {cwd} (exit {r.returncode})")


def _git(args, cwd, token=None, check=True):
    r = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0 and check:
        msg = gv.redact((r.stderr or r.stdout).strip(), token)
        raise SiteError(f"git {args[0]} failed: {msg}")
    return r.stdout


def git_archive_bytes(ref, paths=None):
    """The tar of `ref` (optionally only `paths`), as bytes, from the repo."""
    cmd = ["git", "archive", "--format=tar", ref]
    if paths:
        cmd += ["--", *paths]
    r = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True)
    if r.returncode != 0:
        raise SiteError(f"git archive {ref} failed: {r.stderr.decode('utf-8', 'replace').strip()}")
    return r.stdout


def extract_tar(data, dest):
    with tarfile.open(fileobj=io.BytesIO(data)) as tf:
        tf.extractall(dest, filter="data")


# ---------------------------------------------------------------- build and assemble

def build_wiki(scratch):
    """Extracts the committed wiki/ and data/remote/ into scratch/src and builds the wiki there the way
    pages.yml does (`npm ci && npm run build`). Returns the path of wiki/dist."""
    src = reset_dir(os.path.join(scratch, "src"), scratch)
    extract_tar(git_archive_bytes("HEAD", SRC_PATHS), src)
    wiki = os.path.join(src, "wiki")
    _run(pinned(["npm", "ci"]), wiki)
    _run(pinned(["npm", "run", "build"]), wiki)
    dist = os.path.join(wiki, "dist")
    if not os.path.isfile(os.path.join(dist, "index.html")):
        raise SiteError("the wiki build wrote no dist/index.html")
    return dist, os.path.join(src, "data", "remote", "config.json")


def assemble(site, wiki_dist, config_path, hand_made=None):
    """Makes `site` hold the hand-made site (origin/gh-pages, without .git and .github), the wiki as its
    wiki/ folder (replacing an older one) and the remote config as config.json. `hand_made` is the tar
    of that site (tests); by default it is the gh-pages branch."""
    extract_tar(hand_made if hand_made is not None else git_archive_bytes(GH_PAGES_REF), site)
    for junk in (".git", ".github"):
        p = os.path.join(site, junk)
        if os.path.isdir(p):
            shutil.rmtree(p)
    wiki_out = os.path.join(site, "wiki")
    if os.path.isdir(wiki_out):
        shutil.rmtree(wiki_out)
    shutil.copytree(wiki_dist, wiki_out)
    shutil.copyfile(config_path, os.path.join(site, "config.json"))
    if not os.path.isfile(os.path.join(site, "index.html")):
        raise SiteError("the assembled site has no index.html (is origin/gh-pages fetched?)")
    return site


def markers(site):
    """Files whose bytes show that the new build is served: the config, the hand-made index, the wiki's
    two home pages and one hashed stylesheet (its name changes with every build). {rel: local bytes}."""
    rels = ["config.json", "index.html", "wiki/index.html", "wiki/en/index.html"]
    css = sorted(glob.glob(os.path.join(site, "wiki", "_astro", "*.css")))
    if css:
        rels.append(os.path.relpath(css[0], site).replace(os.sep, "/"))
    return {rel: read(os.path.join(site, rel)) for rel in rels}


def read(path):
    with open(path, "rb") as f:
        return f.read()


# ---------------------------------------------------------------- the push

def push_site(site, workflow_src, remote, scratch, token, message):
    """Pushes `site` as the `site` branch: a fresh orphan commit holding site/ and the workflow file,
    force-pushed to that branch only. Returns the full commit id."""
    work = reset_dir(os.path.join(scratch, "push"), scratch)
    _git(["init", "-q", "-b", SITE_BRANCH], cwd=work)
    shutil.copytree(site, os.path.join(work, "site"))
    wf = os.path.join(work, WORKFLOW_DEST)
    os.makedirs(os.path.dirname(wf))
    shutil.copyfile(workflow_src, wf)
    _git(["add", "-A"], cwd=work)
    _git(["-c", f"user.name={pages.AUTHOR[0]}", "-c", f"user.email={pages.AUTHOR[1]}",
          "commit", "-q", "-m", message], cwd=work)
    plain = f"{gv.HOST}/{gv.OWNER}/{gv.APP_REPO}.git"
    url = remote or f"https://{pages.AUTHOR[0]}:{token}@" + plain[len("https://"):]
    _git(["push", "-q", "--force", url, f"HEAD:refs/heads/{SITE_BRANCH}"], cwd=work, token=token)
    return _git(["rev-parse", "HEAD"], cwd=work).strip()


# ---------------------------------------------------------------- waiting

def workflow_run(client, sha):
    """The Actions run of the `site` branch's commit `sha`, or None while GitVerse has not listed it."""
    _, _, body = client.request("GET", f"/repos/{gv.OWNER}/{gv.APP_REPO}/actions/runs")
    for run in json.loads(body).get("workflow_runs", []):
        if run.get("commit_sha") == sha and run.get("ref") == f"refs/heads/{SITE_BRANCH}":
            return run
    return None


def run_problem(client, run):
    """Describes a failed run from its jobs (names, statuses, steps), for the error message."""
    lines = [f"workflow run {run['id']} ended {run['status']}"]
    try:
        _, _, body = client.request("GET", f"/repos/{gv.OWNER}/{gv.APP_REPO}/actions/runs/{run['id']}/jobs")
        for job in json.loads(body).get("jobs", []):
            lines.append(f"  job {job.get('name')}: {job.get('status')}")
            for step in job.get("steps", []):
                if step.get("status") not in ("success", "skipped", None):
                    lines.append(f"    step {step.get('name')}: {step.get('status')}")
    except (gv.GitVerseError, ValueError) as e:
        lines.append(f"  (jobs unreadable: {e})")
    return "\n".join(lines)


def wait_workflow(client, sha, wait=WAIT_DEFAULT, interval=INTERVAL, sleep=time.sleep):
    """Waits for the run of `sha` to succeed. A failed run raises at once, with its jobs."""
    started = time.time()
    while True:
        run = workflow_run(client, sha)
        status = run.get("status") if run else None
        if status in SUCCESS:
            return run["id"]
        if status in FAILED:
            raise SiteError(run_problem(client, run))
        if time.time() - started > wait:
            raise SiteError(f"no successful workflow run for {sha[:8]} within {wait} s "
                            f"(last status: {status or 'not listed'})")
        note(f"  workflow run: {status or 'not listed yet'}")
        sleep(interval)


def wait_bytes(expected, base, wait=WAIT_DEFAULT, interval=INTERVAL, fetch=gv.fetch_bytes,
               sleep=time.sleep, on_tick=None):
    """Polls base+rel for each rel of `expected` ({rel: bytes}) until each serves exactly those bytes.
    `on_tick` runs before each round (it may raise to stop the wait). Returns the seconds waited."""
    started = time.time()
    pending = dict(expected)
    while True:
        if on_tick:
            on_tick()
        for rel in list(pending):
            try:
                got = fetch(base + rel)
            except gv.GitVerseError:
                got = None
            if got == pending[rel]:
                del pending[rel]
        if not pending:
            return time.time() - started
        if time.time() - started > wait:
            raise SiteError(f"{base} does not serve {sorted(pending)} yet (waited {wait} s)")
        note(f"  waiting for Pages: {', '.join(sorted(pending))}")
        sleep(interval)


# ---------------------------------------------------------------- checking the live site

class _Refs(html.parser.HTMLParser):
    """The asset references of a page: link, script, img, source, video, audio (not <a>: pages are not
    assets)."""
    TAGS = {"link", "script", "img", "source", "video", "audio"}
    ATTRS = ("href", "src", "poster")

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.refs = []

    def handle_starttag(self, tag, attrs):
        if tag not in self.TAGS:
            return
        for name, value in attrs:
            if name in self.ATTRS and value:
                self.refs.append(value)


CSS_URL = re.compile(r"url\(\s*['\"]?([^'\")\s]+)['\"]?\s*\)")


def page_refs(page_url, text):
    p = _Refs()
    p.feed(text)
    return {urllib.parse.urljoin(page_url, r) for r in p.refs}


def css_refs(css_url, text):
    return {urllib.parse.urljoin(css_url, r) for r in CSS_URL.findall(text)}


def to_rel(url, public=PUBLIC):
    """The site-relative file of a URL under the site (a directory is its index.html); None elsewhere."""
    if not url.startswith(public):
        return None
    rel = url[len(public):].split("#")[0].split("?")[0]
    if rel == "" or rel.endswith("/"):
        rel += "index.html"
    return rel


def sample_pages(site):
    """The pages checked: the home pages, one ru wiki page and one en wiki page (the first in sort order)."""
    wiki = os.path.join(site, "wiki")
    found = []
    for root, _, names in os.walk(wiki):
        for n in names:
            if n == "index.html":
                found.append(os.path.relpath(os.path.join(root, n), site).replace(os.sep, "/"))
    ru = sorted(r for r in found if not r.startswith("wiki/en/") and r.count("/") >= 2)
    en = sorted(r for r in found if r.startswith("wiki/en/") and r.count("/") >= 3)
    rels = list(PAGE_SAMPLE)
    for picks in (ru, en):
        if picks:
            rels.append(picks[0])
    return rels


def check_live(site, fetch=gv.fetch_bytes, public=PUBLIC):
    """Fetches the sample pages and the config, then every asset they reference that is under the site,
    and compares the bytes with the scratch site. Returns the list of problems (empty when all match)."""
    problems = []
    checked = set()

    def compare(url, rel):
        checked.add(rel)
        local = os.path.join(site, rel)
        if not os.path.isfile(local):
            problems.append(f"{rel}: not in the build")
            return None
        try:
            live = fetch(url)
        except gv.GitVerseError as e:
            problems.append(f"{rel}: {e}")
            return None
        if live != read(local):
            problems.append(f"{rel}: the live bytes differ from the build")
            return None
        return live

    for rel in sample_pages(site) + ["config.json"]:
        url = public + (rel[:-len("index.html")] if rel.endswith("index.html") else rel)
        live = compare(url, rel)
        if live is None or not rel.endswith(".html"):
            continue
        assets = page_refs(url, live.decode("utf-8", "replace"))
        for ref in sorted(assets):
            rel2 = to_rel(ref, public)
            if rel2 is None or rel2 in checked:
                continue
            body = compare(ref, rel2)
            if body is not None and rel2.endswith(".css"):
                for sub in sorted(css_refs(ref, body.decode("utf-8", "replace"))):
                    rel3 = to_rel(sub, public)
                    if rel3 and rel3 not in checked:
                        compare(sub, rel3)
    return problems


# ---------------------------------------------------------------- main

def main(argv):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--assemble-only", action="store_true", help="build and assemble in scratch; no push")
    ap.add_argument("--check-only", action="store_true", help="check the live site against the scratch site")
    ap.add_argument("--wait", type=int, default=WAIT_DEFAULT, help="seconds to wait for the run and Pages")
    args = ap.parse_args(argv)
    scratch = os.path.abspath(SCRATCH)
    os.makedirs(scratch, exist_ok=True)
    site = os.path.join(scratch, "site")
    try:
        if args.check_only:
            problems = check_live(site)
        else:
            head = _git(["rev-parse", "--short=8", "HEAD"], cwd=REPO_ROOT).strip()
            print(f"site: from the committed HEAD {head} (wiki/ and data/remote/)")
            wiki_dist, config = build_wiki(scratch)
            reset_dir(site, scratch)
            assemble(site, wiki_dist, config)
            files = sum(len(f) for _, _, f in os.walk(site))
            print(f"site: assembled in {site} ({files} files)")
            if args.assemble_only:
                return 0
            token = gv.load_token()
            sha = push_site(site, WORKFLOW_SRC, None, scratch, token,
                            f"Site: the wiki and config.json of {head}")
            print(f"site: pushed {sha[:8]} to the {SITE_BRANCH} branch of {gv.APP_REPO}")
            client = gv.Client(token)
            started = time.time()
            wait_workflow(client, sha, wait=args.wait)
            print(f"site: workflow run done (waited {time.time() - started:.0f} s)")
            waited = wait_bytes(markers(site), PUBLIC, wait=args.wait)
            print(f"site: served (waited {waited:.0f} s)")
            problems = check_live(site)
        if problems:
            print("check: problems:\n  " + "\n  ".join(problems), file=sys.stderr)
            return 1
        print("check: the live site matches the build")
        return 0
    except gv.GitVerseError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
