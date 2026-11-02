#!/usr/bin/env python3
"""The gate of a content package (`.rzpack`) pull request to razdor-community. Stdlib only.

Subcommands:

  check   --razdor-pack PATH [--hashes FILE] [--index FILE] [--sim auto|yes|no]
          [--summary FILE] PACK...
          Checks each package the PR adds and prints a Markdown summary (for the PR's comment;
          also written to --summary). Exit 0 when every package passes, 1 when one fails, 2 on a
          usage problem.
  hashes  INSTALL_DIR
          Prints `<sha256>  <path>` for every file of a Discord Times install, sorted by path: the
          hash list `check` reads (`dt-hashes.txt`, kept in the community repo). Only hashes leave the
          install; no game file is copied.

What `check` enforces per package (each shown as a row of the summary):
- size: at most 50 MB;
- zip: every member is a path inside the package (no `..`, no leading `/`, no backslash, no colon);
- manifest: package.toml parses, license `MIT`, an author;
- game files: no member with a game extension (ini, dtm, ugs, lit, spi, exe, dll);
- install hashes: no packed file (empty ones aside) has the SHA-256 of a file in the install's hash list;
- razdor-pack check: `razdor-pack check --json` has no error;
- razdor-pack sim: `razdor-pack sim --json` passes (auto: when RAZDOR_DT_DIR is set; yes: it must run;
  no: skipped, and the summary says so);
- author: a package whose id the catalog (--index, the current index.json) already has comes from the
  same author; a new id is accepted from anyone.

The check does not verify the author's identity, the licence text or the content: those are the
owner's review. Razdor's own tools check the content (`razdor-pack check`, `sim`).
"""

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tomllib
import zipfile

MAX_BYTES = 50 * 1024 * 1024
GAME_EXT = frozenset({"ini", "dtm", "ugs", "lit", "spi", "exe", "dll"})
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
CHUNK = 1 << 20


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def read_hashes(path):
    """The install's file hashes (`hashes` output): a set of lower-case SHA-256 digests."""
    out = set()
    with open(path, encoding="utf-8") as f:
        for line in f:
            token = line.strip().split(" ", 1)[0].lower() if line.strip() else ""
            if SHA_RE.match(token):
                out.add(token)
    return out


def read_index(path):
    """The catalog's entries by id ({} when there is no index yet)."""
    if not path or not os.path.isfile(path):
        return {}
    with open(path, encoding="utf-8") as f:
        catalog = json.load(f)
    return {e["id"]: e for e in catalog.get("entries", []) if isinstance(e, dict) and "id" in e}


def last_json(text):
    """The JSON value of razdor-pack's output: the whole text when it is JSON (`check --json` is pretty-printed),
    else the last line that parses (`sim --json` prints its install log before its JSON), or None."""
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line.startswith(("[", "{")):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return None


def run_razdor_pack(binary, args):
    """`razdor-pack ARGS`: (exit code, stdout, stderr)."""
    done = subprocess.run([binary, *args], capture_output=True, text=True, timeout=1800)
    return done.returncode, done.stdout, done.stderr


def check_zip(path):
    """The zip's members and its manifest: (members, manifest, problems). Problems are for the summary."""
    problems = []
    with zipfile.ZipFile(path) as z:
        names = z.namelist()
        for name in names:
            if name.startswith("/") or ".." in name.split("/") or "\\" in name or ":" in name:
                problems.append(f"the member `{name}` is not a path inside the package")
        game = sorted(n for n in names if n.rsplit(".", 1)[-1].lower() in GAME_EXT)
        if game:
            problems.append("game files: " + ", ".join(f"`{n}`" for n in game[:5]) + (" ..." if len(game) > 5 else ""))
        members = {}
        for name in names:
            if not name.endswith("/"):
                members[name] = z.read(name)
    manifest = {}
    if "package.toml" not in members:
        problems.append("no package.toml")
    else:
        try:
            manifest = tomllib.loads(members["package.toml"].decode("utf-8"))
        except (UnicodeDecodeError, tomllib.TOMLDecodeError) as e:
            problems.append(f"package.toml does not parse: {e}")
    return members, manifest, problems


def summarise_one(path, args, hashes, index):
    """One package's checks: (ok, the summary section, the rows as (name, passed, text))."""
    rows = []
    name = os.path.basename(path)
    size = os.path.getsize(path)
    size_ok = size <= MAX_BYTES
    rows.append(("size", size_ok, f"{size} bytes, at most {MAX_BYTES // (1024 * 1024)} MB" if size_ok else f"{size} bytes: more than 50 MB"))
    if not size_ok:
        return finish(name, rows)
    try:
        members, manifest, problems = check_zip(path)
    except (zipfile.BadZipFile, OSError) as e:
        rows.append(("zip", False, f"not a readable .rzpack: {e}"))
        return finish(name, rows)
    rows.append(("zip", not any(p.startswith("the member") for p in problems),
                 "every member is inside the package" if not any(p.startswith("the member") for p in problems) else
                 "; ".join(p for p in problems if p.startswith("the member"))))
    game_problem = [p for p in problems if p.startswith("game files")]
    rows.append(("game files", not game_problem, "none" if not game_problem else game_problem[0]))
    license_ok = manifest.get("license") == "MIT"
    author = str(manifest.get("author", "")).strip()
    manifest_ok = bool(manifest) and license_ok and bool(author)
    rows.append(("manifest", manifest_ok,
                 f"license MIT, author {author}" if manifest_ok else
                 f"license is {manifest.get('license', 'missing')!r}, author {author or 'missing'}; both must be MIT and a name"))
    if hashes is None:
        rows.append(("install hashes", True, "no hash list given: not compared"))
    else:
        matched = sorted(n for n, data in members.items() if data and sha256_bytes(data) in hashes)
        rows.append(("install hashes", not matched,
                     "no packed file is a file of the install" if not matched else "a packed file is a file of the install: " + ", ".join(f"`{n}`" for n in matched[:5])))
    code, out, err = run_razdor_pack(args.razdor_pack, ["check", path, "--json"])
    report = last_json(out)
    if code == 2 or not isinstance(report, dict):
        rows.append(("razdor-pack check", False, f"the tool could not check it (exit {code}): {err.strip()[:200] or out.strip()[:200]}"))
    else:
        errors = [d for d in report.get("diagnostics", []) if d.get("severity") == "error"]
        warnings = [d for d in report.get("diagnostics", []) if d.get("severity") == "warning"]
        ok = bool(report.get("ok")) and not errors
        text = f"{len(errors)} errors, {len(warnings)} warnings"
        if errors:
            first = errors[0]
            text += f"; first: {first.get('file', '')}:{first.get('line', '')}: {first.get('message', '')}"
        rows.append(("razdor-pack check", ok, text))
    sim = args.sim
    if sim == "auto":
        sim = "yes" if os.environ.get("RAZDOR_DT_DIR") else "no"
    if sim == "no":
        rows.append(("razdor-pack sim", True, "skipped (no install on this runner: RAZDOR_DT_DIR is not set)"))
    else:
        code, out, err = run_razdor_pack(args.razdor_pack, ["sim", path, "--json"])
        result = last_json(out)
        if code == 0 and isinstance(result, list):
            played = ", ".join(f"{r.get('map')} {r.get('days')} days" for r in result if isinstance(r, dict))
            rows.append(("razdor-pack sim", True, f"played: {played}"))
        else:
            rows.append(("razdor-pack sim", False, f"exit {code}: {err.strip()[:200] or out.strip()[-200:]}"))
    existing = index.get(manifest.get("id", ""))
    if existing is None:
        rows.append(("author", True, "a new package id"))
    else:
        same = str(existing.get("author", "")).strip() == author
        rows.append(("author", same, f"the same author as the catalog's ({author})" if same else
                     f"the catalog's {manifest.get('id')} is by {existing.get('author', '')}: a new version comes from the same author"))
    return finish(name, rows, manifest)


def finish(name, rows, manifest=None):
    ok = all(passed for _, passed, _ in rows)
    title = f"{manifest.get('id', name)} {manifest.get('version', '')}".strip() if manifest else name
    lines = [f"## {'pass' if ok else 'FAIL'}: {title}", "", f"File: `{name}`", "", "| check | result |", "|---|---|"]
    lines += [f"| {label} | {'ok' if passed else 'FAIL'}: {text} |" for label, passed, text in rows]
    return ok, "\n".join(lines) + "\n"


def cmd_check(args):
    hashes = read_hashes(args.hashes) if args.hashes else None
    index = read_index(args.index)
    sections, failed = [], 0
    for path in args.packs:
        if not os.path.isfile(path):
            sections.append(f"## FAIL: {path}\n\nNot a file.\n")
            failed += 1
            continue
        ok, text = summarise_one(path, args, hashes, index)
        failed += 0 if ok else 1
        sections.append(text)
    total = len(args.packs)
    head = f"# Package check: {total - failed} of {total} pass\n"
    if total == 0:
        head = "# Package check: no package in this pull request\n"
    report = head + "\n" + "\n".join(sections)
    print(report)
    if args.summary:
        with open(args.summary, "w", encoding="utf-8") as f:
            f.write(report)
    return 1 if failed else 0


def cmd_hashes(args):
    root = args.install_dir
    if not os.path.isdir(root):
        print(f"{root} is not a folder", file=sys.stderr)
        return 2
    rows = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames.sort()
        for name in sorted(filenames):
            full = os.path.join(dirpath, name)
            if os.path.islink(full) or not os.path.isfile(full):
                continue
            digest = hashlib.sha256()
            with open(full, "rb") as f:
                for block in iter(lambda: f.read(CHUNK), b""):
                    digest.update(block)
            rows.append((os.path.relpath(full, root).replace(os.sep, "/"), digest.hexdigest()))
    for rel, digest in sorted(rows):
        print(f"{digest}  {rel}")
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description="The gate of a content package pull request (see the module's docstring).")
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check", help="check the packages a pull request adds")
    c.add_argument("--razdor-pack", required=True, metavar="PATH", help="the razdor-pack binary")
    c.add_argument("--hashes", metavar="FILE", help="the install's hash list (`hashes` output)")
    c.add_argument("--index", metavar="FILE", help="the catalog's current index.json (the authors of the ids it has)")
    c.add_argument("--sim", choices=("auto", "yes", "no"), default="auto", help="run razdor-pack sim: auto (when RAZDOR_DT_DIR is set), yes, or no")
    c.add_argument("--summary", metavar="FILE", help="also write the Markdown summary to this file")
    c.add_argument("packs", nargs="*", metavar="PACK", help="the .rzpack files the pull request adds")
    h = sub.add_parser("hashes", help="print the SHA-256 of every file of an install")
    h.add_argument("install_dir", help="the Discord Times install folder")
    args = ap.parse_args(argv)
    if args.cmd == "check":
        return cmd_check(args)
    return cmd_hashes(args)


if __name__ == "__main__":
    sys.exit(main())
