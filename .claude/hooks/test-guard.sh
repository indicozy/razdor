#!/bin/sh
# Runs guard-bash.py on every case of guard-bash.cases.tsv against fixture repos in a temp dir.
# Usage: sh .claude/hooks/test-guard.sh   (exit 1 if any case fails)
here=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fixture() { # name branch staged-file
    git init -q -b main "$tmp/$1"
    git -C "$tmp/$1" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
    git -C "$tmp/$1" branch dt-feat
    [ "$2" = main ] || git -C "$tmp/$1" checkout -q "$2"
    mkdir -p "$tmp/$1/$(dirname "$3")"
    echo x > "$tmp/$1/$3"
    git -C "$tmp/$1" add "$3"
}
fixture clean main src/ok.rs
fixture dirty main data/x.lit
fixture feat dt-feat src/ok.rs
git -C "$tmp/clean" config alias.pp push
# Pushes whose target the config decides.
fixture upstream main src/ok.rs
git -C "$tmp/upstream" config push.default upstream
git -C "$tmp/upstream" config branch.main.remote origin
git -C "$tmp/upstream" config branch.main.merge refs/heads/dt-feat
fixture remotepush main src/ok.rs
git -C "$tmp/remotepush" config remote.origin.push refs/heads/main:refs/heads/dt-feat
fixture matching main src/ok.rs
git -C "$tmp/matching" config push.default matching
# A repo whose config and attributes would run code if the hook's git ran them.
fixture evil main src/ok.rs
git -C "$tmp/evil" config core.fsmonitor "touch $tmp/PWNED-fsmonitor; false"
git -C "$tmp/evil" config filter.x.clean "touch $tmp/PWNED-filter; cat"
git -C "$tmp/evil" config alias.st "!touch $tmp/PWNED-alias"
git -C "$tmp/evil" config core.hooksPath "$tmp/evil-hooks"
mkdir -p "$tmp/evil-hooks"; printf '#!/bin/sh\ntouch %s/PWNED-hook\n' "$tmp" > "$tmp/evil-hooks/post-index-change"; chmod +x "$tmp/evil-hooks/post-index-change"
echo '* filter=x' > "$tmp/evil/.gitattributes"
echo y > "$tmp/evil/src/ok.rs"
rm -f "$tmp"/PWNED-*
mkdir -p "$tmp/clean/data" "$tmp/clean/saves" "$tmp/clean/docs"
touch "$tmp/clean/data/x.lit" "$tmp/clean/saves/backrok.rzsave" "$tmp/clean/Rus_Units.ini" "$tmp/clean/docs/_Global.ini"
# A helper crate outside every repository (a cargo run of it is not the game); cases name it
# as @TMP@/helper.
mkdir -p "$tmp/helper/src"
printf '[package]\nname = "helper"\nversion = "0.1.0"\nedition = "2021"\n' > "$tmp/helper/Cargo.toml"
echo 'fn main() {}' > "$tmp/helper/src/main.rs"
pass=0 fail=0
while IFS='	' read -r expect repo cmd; do
    case "$expect" in ''|'#'*) continue ;; esac
    json=$(CMD="$cmd" CWD="$tmp/$repo" TMPD="$tmp" python3 -I -c 'import json,os;print(json.dumps({"tool_name":"Bash","cwd":os.environ["CWD"],"tool_input":{"command":os.environ["CMD"].replace("\\n","\n").replace("@TMP@",os.environ["TMPD"])}}))')
    printf '%s' "$json" | DISPLAY=:0 python3 -I "$here/guard-bash.py" 2>/dev/null
    code=$?
    got=allow; [ $code -eq 2 ] && got=block
    if [ "$got" = "$expect" ]; then pass=$((pass + 1)); else fail=$((fail + 1)); echo "FAIL (expected $expect, got $got): [$repo] $cmd"; fi
done < "$here/guard-bash.cases.tsv"
for f in "$tmp"/PWNED-*; do [ -e "$f" ] && { fail=$((fail + 1)); echo "FAIL: the hook's git ran repo code ($(basename "$f"))"; }; done
echo "$pass passed, $fail failed"
[ $fail -eq 0 ]
