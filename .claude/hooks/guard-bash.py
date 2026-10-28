#!/usr/bin/env python3
"""PreToolUse guard for Bash commands in the Razdor repo.

Reads the hook's JSON on stdin and exits 2 (with the reason on stderr) to block a command:

- cargo build/test/run/clippy/check/bench not pinned with `taskset -c 16-19`
  (the other cores belong to the owner and other sessions);
- the game started in a window (`target/*/razdor`, `cargo run`, `Razdor.exe`, `wine …Razdor`)
  unless under `xvfb-run`, with `DISPLAY` set to another (Xvfb) display in the same command,
  or with `RAZDOR_ALLOW_WINDOW=1` (the owner asked for a visible run);
- `git add` / `git commit` / `git update-index` that would put the game's content in the repo
  (.lit .ugs .dtm .dtc .wav .ogg .mp3 .bik .rzsave, Rus_*.ini, _Global.ini);
- pushes to the frozen dt-feat, force pushes, `--no-verify`;
- recursive `rm`, `find -delete` and `git clean -f` outside the scratch folders
  (/tmp/claude-<id>/…, /mnt/ai_models/razdor-scratch/…).

Commands are parsed, not grepped: `git push` inside a commit message or an `echo` is not a push.
`bash -c` / `sh -c` / `eval` / `env -S` strings, `$( )` and backtick substitutions (also in
double quotes and unquoted heredocs) and git aliases are checked too.

What this guards against: accidents in the owner's own repo, not a hostile author of commands.
So the dangerous categories (push to dt-feat, force push, --no-verify, game content, deletes)
fail CLOSED: a command the parser cannot read, or whose target it cannot know (a variable or a
substitution in a refspec, a cwd, an rm path; a shell fed a script on stdin; xargs into git or
rm), is blocked when it mentions such an action. The advisory rules (cargo pinning, the window)
fail open. Accepted gaps, as they need deliberate effort rather than a slip: code that runs git
or rm from another language (python -c, perl -e), a script file run with `sh file` / `source`,
the owner's interactive shell aliases, and plumbing such as `git commit-tree`.

The hook runs only read-only git commands, never anything taken from the command text as code,
and runs them hardened: no system config, fsmonitor and hooks forced off on the command line
(over the repo's and the owner's global config, which is read: push.default and aliases live
there and decide what a push does), no optional locks,
no external diff or textconv, and only listings that never run filters (ls-files, diff --cached).

Cases: guard-bash.cases.tsv, run by test-guard.sh.
"""

import json
import os
import re
import shlex
import subprocess
import sys

ALLOWED_CPUS = set(range(16, 20))
PIN_HINT = "CARGO_BUILD_JOBS=4 taskset -c 16-19 cargo …"
FROZEN = {"dt-feat", "dt-original"}
GAME_EXT = re.compile(r"\.(lit|ugs|dtm|dtc|wav|ogg|mp3|bik|rzsave)$", re.I)
GAME_INI = re.compile(r"(^|/)(rus_[^/]*|_global)\.ini$", re.I)
SCRATCH = (re.compile(r"^/tmp/claude-[^/]+/.+"), re.compile(r"^/mnt/ai_models/razdor-scratch/.+"))
CARGO_PINNED = {"build", "b", "test", "t", "run", "r", "clippy", "check", "c", "bench"}
SHELLS = {"bash", "sh", "zsh", "dash", "ksh", "fish"}
# Words that make an unreadable command dangerous enough to refuse.
DANGER = re.compile(r"\b(git|rm|find|xargs|unlink|shred)\b|--no-verify|dt-feat|--force|\+[\w/]+:")
KEYWORDS = {"{", "}", "!", "if", "then", "else", "elif", "fi", "do", "done", "while", "until", "time", "coproc"}
BRACES = re.compile(r"\{[^}]*(,|\.\.)[^}]*\}")
ASSIGN = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
OPCHARS = set(";&|()\n")
GIT_TIMEOUT = 3


class Block(Exception):
    pass


class Unknown(Exception):
    """The parser cannot tell what the shell would run."""


# ---------------------------------------------------------------- shell reading


def substitutions(text):
    """The commands inside `$( )` and backticks that the shell would run: outside single quotes
    and ANSI-C `$'…'` strings, inside double quotes too."""
    out, i, n, state = [], 0, len(text), "normal"
    while i < n:
        c = text[i]
        if state == "single":
            if c == "'":
                state = "normal"
        elif state == "ansi":
            if c == "\\":
                i += 1
            elif c == "'":
                state = "normal"
        else:
            if c == "\\":
                i += 2
                continue
            if state == "normal" and c == "'":
                state = "single"
            elif state == "normal" and text.startswith("$'", i):
                state = "ansi"
                i += 1
            elif c == '"':
                state = "double" if state == "normal" else "normal"
            elif text.startswith("$((", i):
                i += 3  # arithmetic; may still hold a substitution, scanned as text
                continue
            elif text.startswith("$(", i):
                depth, j = 1, i + 2
                while j < n and depth:
                    depth += {"(": 1, ")": -1}.get(text[j], 0)
                    j += 1
                if depth:
                    raise Unknown("an unclosed $(")
                out.append(text[i + 2:j - 1])
                i = j
                continue
            elif c == "`":
                j = text.find("`", i + 1)
                if j < 0:
                    raise Unknown("an unclosed backtick")
                out.append(text[i + 1:j])
                i = j + 1
                continue
        i += 1
    if state in ("single", "double", "ansi"):
        raise Unknown("an unclosed quote")
    return out


def strip_comments(text):
    """Drops `#` comments as bash reads them: only a `#` that starts a word, outside quotes
    (shlex would also cut `a#b`, hiding what follows on the line)."""
    out, i, n, state = [], 0, len(text), "normal"
    while i < n:
        c = text[i]
        if state == "single":
            if c == "'":
                state = "normal"
        elif state == "double":
            if c == "\\":
                out.append(text[i:i + 2])
                i += 2
                continue
            if c == '"':
                state = "normal"
        else:
            if c == "\\":
                out.append(text[i:i + 2])
                i += 2
                continue
            if c == "'":
                state = "single"
            elif c == '"':
                state = "double"
            elif c == "#" and (i == 0 or text[i - 1] in " \t\n;&|()"):
                j = text.find("\n", i)
                i = n if j < 0 else j
                continue
        out.append(c)
        i += 1
    return "".join(out)


HEREDOC = re.compile(r"(?<!<)<<(?!<)-?\s*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\1")


def split_heredocs(text):
    """Drops heredoc bodies (their lines are data) and returns (text, bodies that expand)."""
    out, expanding, lines, i = [], [], text.split("\n"), 0
    while i < len(lines):
        line = lines[i]
        out.append(line)
        i += 1
        for quote, tag in HEREDOC.findall(line):
            body = []
            while i < len(lines) and lines[i].strip() != tag:
                body.append(lines[i])
                i += 1
            i += 1
            if not quote:
                expanding.append("\n".join(body))
    return "\n".join(out), expanding


def segments(text):
    """Splits a command line into simple commands (lists of words)."""
    lex = shlex.shlex(text.replace("\\\n", " "), posix=True, punctuation_chars=";&|()\n")
    lex.whitespace = " \t\r"
    lex.whitespace_split = True
    lex.commenters = ""
    seg, segs = [], []
    try:
        for tok in lex:
            if tok and set(tok) <= OPCHARS:
                if seg:
                    segs.append(seg)
                seg = []
            else:
                seg.append(tok)
    except ValueError as e:
        raise Unknown(str(e))
    if seg:
        segs.append(seg)
    return segs


def cpus(spec):
    out = set()
    for part in spec.split(","):
        if "-" in part:
            a, b = part.split("-", 1)
            out |= set(range(int(a), int(b) + 1))
        else:
            out.add(int(part))
    return out


def unwrap(words, nested):
    """Strips assignments and wrappers; returns (argv, env, pinned, xvfb). Strings a wrapper
    runs through a shell (`env -S`, `watch`) are appended to `nested`."""
    env, pinned, xvfb = {}, False, False
    w = list(words)
    while w:
        head = os.path.basename(w[0])
        if ASSIGN.match(w[0]):
            k, v = w[0].split("=", 1)
            env[k] = v
            w = w[1:]
        elif head == "env":
            w = w[1:]
            while w and (w[0].startswith("-") or ASSIGN.match(w[0])):
                flag = w[0]
                w = w[1:]
                if ASSIGN.match(flag):
                    k, v = flag.split("=", 1)
                    env[k] = v
                elif flag in ("-S", "--split-string") and w:
                    nested.append(" ".join(w))
                    return [], env, pinned, xvfb
                elif flag.startswith("--split-string=") or (flag.startswith("-S") and len(flag) > 2):
                    nested.append(flag.split("=", 1)[1] if "=" in flag else flag[2:])
                    nested.append(" ".join(w))
                    return [], env, pinned, xvfb
                elif flag in ("-u", "-C", "--unset", "--chdir") and w:
                    w = w[1:]
        elif w[0] in KEYWORDS:
            w = w[1:]
        elif head == "taskset":
            w = w[1:]
            spec = None
            while w and w[0].startswith("-"):
                flag = w[0]
                w = w[1:]
                if flag in ("-c", "--cpu-list") and w:
                    spec = w[0]
                    w = w[1:]
                elif flag.startswith("--cpu-list="):
                    spec = flag.split("=", 1)[1]
            if spec is None and w:
                w = w[1:]  # a hex mask: not the E-cores by list
            try:
                pinned = spec is not None and cpus(spec) <= ALLOWED_CPUS
            except ValueError:
                pinned = False
        elif head == "xvfb-run":
            xvfb = True
            w = w[1:]
            while w and w[0].startswith("-"):
                flag = w[0]
                w = w[1:]
                if flag in ("-s", "-n", "-e", "-f", "-p", "--server-args", "--server-num", "--error-file", "--auth-file", "--xauth-protocol") and w:
                    w = w[1:]
        elif head in ("sudo", "doas", "nohup", "setsid", "command", "exec", "time", "builtin", "nice", "ionice", "stdbuf", "timeout", "chrt", "unbuffer"):
            w = w[1:]
            takes = {"-u", "-g", "-n", "-c", "-k", "-s", "-o", "-e", "-i", "-a", "--signal", "--kill-after", "--user", "--group"}
            while w and w[0].startswith("-"):
                flag = w[0]
                w = w[1:]
                if flag in takes and w:
                    w = w[1:]
            if head in ("timeout", "chrt") and w:
                w = w[1:]  # the duration / priority
        elif head == "watch":
            w = w[1:]
            while w and w[0].startswith("-"):
                flag = w[0]
                w = w[1:]
                if flag in ("-n", "-d", "--interval") and w:
                    w = w[1:]
            nested.append(" ".join(w))
            return [], env, pinned, xvfb
        elif head in ("perf", "strace", "ltrace", "valgrind"):
            if "--" in w:
                w = w[w.index("--") + 1:]
                continue
            w = w[1:]
            if head == "perf":
                if not w or w[0] not in ("record", "stat", "trace"):
                    return [], env, pinned, xvfb
                w = w[1:]
            takes = {"-F", "-o", "-e", "-c", "-p", "-t", "-C", "-m", "-s", "-O", "-u", "-E", "--call-graph", "--freq", "--output", "--event"}
            while w and w[0].startswith("-"):
                flag = w[0]
                w = w[1:]
                if flag in takes and w:
                    w = w[1:]
        else:
            break
    return w, env, pinned, xvfb


# ---------------------------------------------------------------- git, read-only and hardened


def git_out(args, cwd):
    """A read-only git listing, with no config that can run code."""
    if cwd is None:
        raise Unknown("the working folder")
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_OPTIONAL_LOCKS="0", GIT_TERMINAL_PROMPT="0", GIT_PAGER="cat", PAGER="cat")
    hard = ["-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", "-c", "core.untrackedCache=false", "-c", "diff.external=", "--no-pager"]
    try:
        r = subprocess.run(["git", *hard, *args], cwd=cwd, env=env, capture_output=True, text=True, timeout=GIT_TIMEOUT, stdin=subprocess.DEVNULL)
    except Exception as e:
        raise Unknown(f"git {args[0]}: {e}")
    return r.stdout if r.returncode == 0 else ""


def is_game_content(path):
    return bool(GAME_EXT.search(path) or GAME_INI.search(path))


def check_paths(paths, what):
    bad = sorted({p for p in paths if p and is_game_content(p)})
    if bad:
        raise Block(f"{what} would put the game's content in the repo ({', '.join(bad[:5])}). Razdor is public: never commit the original's files, pictures, sounds, maps, ini files or saves.")


def listed(cwd, spec):
    """Every file `git add` could take under the pathspecs: untracked (not ignored) and tracked.
    Listings only: nothing here compares contents, so no filter or textconv runs."""
    files = git_out(["ls-files", "--others", "--exclude-standard", "--", *spec], cwd).splitlines()
    files += git_out(["ls-files", "--", *spec], cwd).splitlines()
    return files


def commits_all(arg):
    """`-a` / `--all`, alone or in a cluster like `-am` (letters after a value flag are its value)."""
    if arg in ("-a", "--all"):
        return True
    if not arg.startswith("-") or arg.startswith("--"):
        return False
    for ch in arg[1:]:
        if ch == "a":
            return True
        if ch in "mFCctS":
            return False
    return False


def check_git(argv, cwd, env, depth):
    w = argv[1:]
    configs = []
    while w and w[0].startswith("-"):
        flag = w[0]
        w = w[1:]
        if flag in ("-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path") and w:
            if flag == "-C":
                cwd = None if cwd is None or "$" in w[0] else os.path.normpath(os.path.join(cwd, w[0]))
            elif flag == "-c":
                configs.append(w[0])
            elif flag in ("--git-dir", "--work-tree"):
                cwd = None
            w = w[1:]
        elif flag.startswith(("--git-dir=", "--work-tree=")):
            cwd = None
        elif flag.startswith("--config-env"):
            configs.append(flag)
    if not w:
        return
    sub, args = w[0], w[1:]
    if "$" in sub or "`" in sub:
        raise Unknown("a git subcommand from a variable")
    if any(k in env for k in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE")) or any(k.startswith("GIT_CONFIG") for k in env):
        cwd = None
    risky = [c for c in configs if re.match(r"(alias|push|remote|core\.hookspath|branch)\.", c, re.I) or c.startswith("--config-env")]
    known = {"push", "add", "commit", "clean", "update-index", "status", "log", "diff", "show", "rev-parse", "branch", "tag", "fetch", "pull", "checkout", "switch", "restore", "reset", "stash", "worktree", "cherry-pick", "rebase", "merge", "init", "clone", "config", "remote", "ls-files", "grep", "blame", "describe", "symbolic-ref", "rev-list", "shortlog", "mv", "rm", "apply", "am", "format-patch", "bisect", "notes", "gc", "fsck", "reflog", "submodule", "archive", "help", "version", "--version", "cat-file", "ls-tree", "for-each-ref", "name-rev", "merge-base", "whatchanged", "range-diff", "sparse-checkout", "maintenance", "count-objects", "verify-commit", "send-email", "request-pull", "difftool", "mergetool", "citool", "gui", "instaweb", "replace", "filter-branch", "hash-object", "read-tree", "write-tree", "commit-tree", "update-ref", "check-ignore", "check-attr", "var"}
    if any(c.lower().startswith("alias.") for c in configs):
        raise Unknown("an alias set on the command line")
    if sub not in known:
        if depth > 3:
            raise Unknown("alias loop")
        expansion = git_out(["config", "--get", f"alias.{sub}"], cwd).strip() if cwd is not None else ""
        if not expansion:
            if cwd is None:
                raise Unknown("a git command in an unknown repository")
            return
        if expansion.startswith("!"):
            check_text(expansion[1:], cwd, depth + 1)
            return
        try:
            exp = shlex.split(expansion)
        except ValueError:
            raise Unknown("an unreadable git alias")
        check_git(["git", *exp, *args], cwd, env, depth + 1)
        return
    dangerous = sub in ("push", "add", "commit", "clean", "update-index")
    if dangerous and risky:
        raise Unknown(f"git {sub} with config set on the command line ({', '.join(risky)})")
    if "--no-verify" in args or (sub in ("commit", "merge", "am", "rebase") and "-n" in args and sub == "commit"):
        raise Block("--no-verify skips the repo's git hooks; fix what they report instead.")
    if sub == "push":
        check_push(args, cwd)
    elif sub in ("add", "update-index"):
        flags = [a for a in args if a.startswith("-")]
        paths = [a for a in args if not a.startswith("-") and a != "--"]
        check_paths(paths, f"git {sub}")
        if sub == "update-index":
            return
        if any("$" in p or "`" in p for p in paths):
            raise Unknown("git add of a path from a variable")
        if cwd is None:
            raise Unknown("git add in an unknown repository")
        spec = [] if any(f in ("-A", "--all") for f in flags) or not paths else paths
        check_paths(listed(cwd, spec), f"git {sub}")
    elif sub == "commit":
        if cwd is None:
            raise Unknown("git commit in an unknown repository")
        staged = git_out(["diff", "--cached", "--name-only", "--no-ext-diff", "--no-textconv"], cwd).splitlines()
        paths = [a for a in args if not a.startswith("-")]
        if any(commits_all(a) for a in args):
            staged += git_out(["ls-files"], cwd).splitlines()
        check_paths(staged, "git commit")
        # `git commit <paths>` commits those paths as they are in the folder.
        value_flags = {"-m", "-F", "-C", "-c", "-t", "--author", "--date", "--message", "--file", "--template", "--fixup", "--squash", "--trailer", "--cleanup"}
        pathspecs, skip = [], False
        for a in args:
            if skip:
                skip = False
            elif a in value_flags or (a.startswith("-") and not a.startswith("--") and a[-1] in "mFCct" and len(a) > 1):
                skip = True
            elif not a.startswith("-"):
                pathspecs.append(a)
        if pathspecs:
            check_paths(pathspecs, "git commit")
            check_paths(git_out(["ls-files", "--", *pathspecs], cwd).splitlines(), "git commit")
    elif sub == "clean":
        flags = "".join(a[1:] for a in args if a.startswith("-") and not a.startswith("--"))
        if ("f" in flags or "--force" in args) and "n" not in flags and "--dry-run" not in args:
            if cwd is None or not in_scratch(cwd):
                raise Block("git clean -f deletes untracked and maybe ignored files (.env, docs/notes, target). Run it with -n first and ask the owner.")


def current_branch(cwd):
    return git_out(["symbolic-ref", "--short", "-q", "HEAD"], cwd).strip()


def resolve_dest(ref, cwd):
    if ref in ("HEAD", "@", ""):
        return current_branch(cwd)
    full = git_out(["rev-parse", "--symbolic-full-name", ref], cwd).strip()
    if full.startswith("refs/heads/"):
        return full[len("refs/heads/"):]
    return ref[len("refs/heads/"):] if ref.startswith("refs/heads/") else ref


def check_push(args, cwd):
    for a in args:
        if a in ("-f", "--force", "--mirror", "--all", "--prune") or a.startswith(("--force-with-lease", "--force-if-includes")) or (a.startswith("-") and not a.startswith("--") and "f" in a[1:]):
            raise Block(f"git push {a}: force, mirror, prune and all-branch pushes are not allowed; push the branch normally.")
    takes = {"-o", "--push-option", "--repo", "--receive-pack", "--exec"}
    pos, skip = [], False
    for a in args:
        if skip:
            skip = False
        elif a in takes:
            skip = True
        elif not a.startswith("-"):
            pos.append(a)
    if cwd is None:
        raise Unknown("a push from an unknown repository")
    refspecs = pos[1:]
    cur = current_branch(cwd)
    default = git_out(["config", "--get", "push.default"], cwd).strip() or "simple"
    remote = pos[0] if pos else (git_out(["config", "--get", f"branch.{cur}.pushRemote"], cwd).strip() or git_out(["config", "--get", "remote.pushDefault"], cwd).strip() or git_out(["config", "--get", f"branch.{cur}.remote"], cwd).strip() or "origin")
    mapped = "$" not in remote and git_out(["config", "--get-all", f"remote.{remote}.push"], cwd).strip()
    merge = git_out(["config", "--get", f"branch.{cur}.merge"], cwd).strip() if cur else ""
    upstream = merge[len("refs/heads/"):] if merge.startswith("refs/heads/") else merge
    if not refspecs:
        if not cur or default in ("matching", "nothing") or mapped or "$" in remote:
            raise Unknown("a push whose branches come from the config")
        dests = [cur] + ([upstream] if upstream and default in ("upstream", "tracking", "simple") else [])
    else:
        if mapped:
            raise Unknown(f"remote.{remote}.push maps the branches")
        dests = []
        for spec in refspecs:
            if spec.startswith("+"):
                raise Block(f"git push {spec}: a '+' refspec is a force push.")
            if "$" in spec or "`" in spec or "*" in spec:
                raise Unknown(f"the refspec {spec}")
            if any(ch in spec for ch in "?[{"):
                raise Unknown(f"the refspec {spec}")
            src, _, dst = spec.partition(":")
            dests.append(resolve_dest(dst if ":" in spec else src, cwd))
            if ":" not in spec and upstream and resolve_dest(src, cwd) == cur and default in ("upstream", "tracking"):
                dests.append(upstream)
    for d in dests:
        if d in FROZEN:
            raise Block(f"{d} is frozen: nothing is pushed to it. Work goes to main.")


# ---------------------------------------------------------------- deletes


def in_scratch(path):
    real = os.path.realpath(path)
    return any(r.match(real) for r in SCRATCH)


def check_delete_targets(targets, cwd, what):
    for t in targets:
        if "$" in t or "`" in t or t.startswith("~"):
            raise Block(f"{what} {t}: the path comes from a variable; spell it out (deletes only inside /tmp/claude-<id>/… or /mnt/ai_models/razdor-scratch/…).")
        if cwd is None and not t.startswith("/"):
            raise Block(f"{what} {t}: a relative path after an unknown cd.")
        p = os.path.normpath(os.path.join(cwd or "/", t))
        if not in_scratch(p.rstrip("*?") or "/"):
            raise Block(f"{what} {t}: recursive deletes only inside /tmp/claude-<id>/… or /mnt/ai_models/razdor-scratch/…. Ask the owner (or queue it for them) before deleting anything else.")


def check_rm(argv, cwd):
    args = argv[1:]
    targets, flags, end = [], "", False
    for a in args:
        if end or not a.startswith("-") or a == "-":
            targets.append(a)
        elif a == "--":
            end = True
        elif a.startswith("--"):
            flags += "r" if a == "--recursive" else ""
        else:
            flags += a[1:]
    if "r" in flags or "R" in flags:
        check_delete_targets(targets, cwd, "rm -r")


def check_find(argv, cwd):
    args = argv[1:]
    deletes = "-delete" in args or any(a in ("-exec", "-execdir", "-ok", "-okdir") and i + 1 < len(args) and os.path.basename(args[i + 1]) in ("rm", "shred", "unlink") for i, a in enumerate(args))
    if deletes:
        starts = []
        for a in args:
            if a.startswith(("-", "(", "!")):
                break
            starts.append(a)
        check_delete_targets(starts or ["."], cwd, "find -delete")


# ---------------------------------------------------------------- the checks


def cargo_outside_repo(rest, cwd):
    """True only when the cargo command's manifest (`--manifest-path`, else the nearest
    Cargo.toml above `cwd`) lies outside this repository and its worktrees: a helper crate.
    Names (`-p`, `[package] name`) are not trusted; anything unclear counts as the game."""
    manifest = None
    for i, a in enumerate(rest):
        if a == "--manifest-path" and i + 1 < len(rest):
            manifest = rest[i + 1]
        elif a.startswith("--manifest-path="):
            manifest = a.split("=", 1)[1]
    base = cwd or os.getcwd()
    if manifest is None:
        d = base
        while True:
            if os.path.isfile(os.path.join(d, "Cargo.toml")):
                manifest = os.path.join(d, "Cargo.toml")
                break
            up = os.path.dirname(d)
            if up == d:
                return False
            d = up
    elif not os.path.isabs(manifest):
        manifest = os.path.join(base, manifest)
    if any(ch in manifest for ch in "$`*?[{"):
        return False
    real = os.path.realpath(manifest)
    if not os.path.isfile(real):
        return False
    repo = os.path.realpath(os.environ.get("CLAUDE_PROJECT_DIR") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
    if real == repo or real.startswith(repo + os.sep):
        return False
    # Every worktree and clone-free checkout of Razdor shares this repository's git store
    # (dt-original's worktree, agents' worktrees anywhere): those are the game too. A clone
    # elsewhere is caught by its own top-level Cargo.toml naming the razdor package.
    def common(d):
        out = git_out(["rev-parse", "--path-format=absolute", "--git-common-dir"], d).strip()
        return os.path.realpath(out) if out else ""
    here = common(os.path.dirname(real))
    if here and here == common(repo):
        return False
    try:
        with open(real, encoding="utf-8", errors="replace") as f:
            if re.search(r'^\s*name\s*=\s*"razdor"', f.read(), re.M):
                return False
    except OSError:
        return False
    return True


def launches_game(argv, cwd=None):
    head = argv[0]
    base = os.path.basename(head)
    if base == "cargo":
        rest = [a for a in argv[1:] if not a.startswith("+")]
        sub = next((a for a in rest if not a.startswith("-")), "")
        if sub in ("run", "r"):
            if "--example" in rest or any(a.startswith("--example=") for a in rest):
                return False
            # A helper crate outside the repository (a scratch decoder, a tool) is not the game.
            if cargo_outside_repo(rest, cwd):
                return False
            if "--bin" in rest:
                i = rest.index("--bin")
                return i + 1 < len(rest) and rest[i + 1] == "razdor"
            return True
        return False
    if base in ("wine", "wine64", "wine-stable"):
        return any(os.path.basename(a).lower() in ("razdor.exe", "razdor") for a in argv[1:])
    # cargo's hashed copies in target/*/deps: the game's binary and the test binaries share the
    # name `razdor-<hash>`, so either may be the game (2026-10-20: an agent ran the game from
    # there as if it were a test binary, on the owner's desktop and save folder). Run tests
    # through `cargo test`, or such a binary under xvfb-run.
    if re.fullmatch(r"razdor-[0-9a-f]{16}", base) and "/" in head:
        return True
    return base in ("razdor", "razdor-macos", "Razdor.exe") and ("/" in head or base == "Razdor.exe")


def check_segment(words, cwd, exported, depth):
    nested = []
    argv, env, pinned, xvfb = unwrap(words, nested)
    for text in nested:
        check_text(text, cwd, depth + 1)
    if not argv:
        if env and len(env) == len(words):
            exported.update(env)  # a bare assignment holds for the rest of the line
        return
    head = os.path.basename(argv[0])
    if "$" in argv[0] or "`" in argv[0]:
        raise Unknown("a command name from a variable")
    if head == "export":
        for a in argv[1:]:
            if ASSIGN.match(a):
                k, v = a.split("=", 1)
                exported[k] = v
        return
    if head in SHELLS:
        cflag = next((i for i, a in enumerate(argv[1:], 1) if a.startswith("-") and not a.startswith("--") and "c" in a[1:]), None)
        if cflag is not None:
            if cflag + 1 < len(argv):
                check_text(argv[cflag + 1], cwd, depth + 1)
            return
        words, skip = [], False
        for a in argv[1:]:
            if skip:
                skip = False
            elif re.match(r"^\d*[<>]", a):
                skip = re.fullmatch(r"\d*(<<<|<<-?|<|>>?|&>>?|>&|<&)", a) is not None
            elif not a.startswith("-"):
                words.append(a)
        if not words:
            raise Unknown("a shell reading its script from stdin")
        return  # `sh script.sh`: an accepted gap (see the top)
    if head == "eval":
        check_text(" ".join(argv[1:]), cwd, depth + 1)
        return
    if head == "xargs":
        rest = argv[1:]
        while rest and rest[0].startswith("-"):
            flag = rest[0]
            rest = rest[1:]
            if flag in ("-I", "-n", "-P", "-L", "-d", "-E", "-s", "-a") and rest:
                rest = rest[1:]
        if rest and os.path.basename(rest[0]) in {"git", "rm", "unlink", "shred", "find"} | SHELLS:
            raise Unknown(f"xargs into {rest[0]} (its arguments come from stdin)")
        return
    allenv = {**exported, **env}
    if head == "cargo":
        rest = [a for a in argv[1:] if not a.startswith("+")]
        sub = next((a for a in rest if not a.startswith("-")), "")
        if sub in CARGO_PINNED and not pinned:
            raise Block(f"cargo {sub} must run on the E-cores: {PIN_HINT} (cores 0–15 are the owner's and other sessions').")
    if launches_game(argv, cwd):
        display = allenv.get("DISPLAY")
        if not (xvfb or allenv.get("RAZDOR_ALLOW_WINDOW") == "1" or (display and display not in (":0", ":0.0", os.environ.get("DISPLAY", ":0")))):
            raise Block("This starts Razdor in a window on the owner's desktop. Run it off-screen under xvfb-run (see the offscreen-snapshot skill), or add RAZDOR_ALLOW_WINDOW=1 only when the owner asked for a visible run.")
    if head in ("git", "rm", "find") and any(BRACES.search(a) for a in argv[1:]):
        raise Unknown("brace expansion in the arguments")
    if head == "git":
        check_git(argv, cwd, allenv, depth)
    elif head == "rm":
        check_rm(argv, cwd)
    elif head == "find":
        check_find(argv, cwd)
    elif head in ("unlink", "shred") and len(argv) > 1:
        pass  # single files: not a recursive delete


def check_text(text, cwd, depth=0):
    if depth > 8:
        raise Unknown("nesting too deep")
    body, expanding = split_heredocs(text)
    body = strip_comments(body)
    for inner in substitutions(body) + [s for b in expanding for s in substitutions(b)]:
        check_text(inner, cwd, depth + 1)
    exported = {}
    for seg in segments(body):
        head = os.path.basename(seg[0]) if seg else ""
        if head in ("cd", "pushd"):
            target = seg[1] if len(seg) > 1 else os.path.expanduser("~")
            if cwd is None or "$" in target or "`" in target:
                cwd = None
            elif target != "-":
                cwd = os.path.normpath(os.path.join(cwd, os.path.expanduser(target)))
            else:
                cwd = None
            continue
        check_segment(seg, cwd, exported, depth)


def main():
    raw = sys.stdin.read()
    try:
        data = json.loads(raw)
        cmd = data.get("tool_input", {}).get("command", "")
        cwd = data.get("cwd") or os.getcwd()
    except Exception:
        return 0
    try:
        check_text(cmd, cwd)
    except Block as b:
        print(f"Blocked by .claude/hooks/guard-bash.py: {b}", file=sys.stderr)
        return 2
    except Exception as e:  # Unknown, or the hook's own error
        if DANGER.search(cmd):
            print(f"Blocked by .claude/hooks/guard-bash.py: the guard cannot tell what this command does ({e}), and it may push, stage game files or delete. Spell it out plainly (no variables or substitutions in the dangerous part), or ask the owner to run it.", file=sys.stderr)
            return 2
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
