#!/usr/bin/env bash
# Checks that the community's game archive is alive before a release: every address the app tries
# (src/mirror.rs, GAME: GitHub first, then GitVerse) must serve the file Razdor expects, whose size
# is the built-in game's (src/remote_config.rs, fn builtin). Exit 0 when every address does, 1 when
# a link is dead or refused, or serves another file (it says which address), 2 on a network error
# on this machine. If both kinds occur, 1 wins. The addresses and size are read from the code, so
# the check cannot drift from it: a line it cannot parse exits 1. Network only: a range request
# for the first 1 KiB of each address (redirects followed, as the game download does); nothing is
# downloaded in full and nothing is built or installed.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
fail() { echo "check-game-link: $1" >&2; exit 1; }

mirror_rs="$root/src/mirror.rs"
config_rs="$root/src/remote_config.rs"

# The GAME addresses, in order: the lines between `pub const GAME: [&str; N] = [` and `];`.
declared="$(sed -n 's/^pub const GAME: \[&str; \([0-9]*\)\] = \[$/\1/p' "$mirror_rs" | head -n 1)"
[ -n "$declared" ] || fail "GAME not found in src/mirror.rs"
urls="$(awk '/^pub const GAME: /{f=1; next} f && /^\];/{f=0} f' "$mirror_rs" \
  | sed -n 's/^    "\(https:\/\/[^"]*\)",$/\1/p')"
count="$(printf '%s\n' "$urls" | grep -c . || true)"
[ "$count" = "$declared" ] || fail "src/mirror.rs GAME: $count address lines parsed, $declared declared"

# The built-in game's size: the first `size:` line in fn builtin (remote_config.rs).
size="$(awk '/fn builtin\(\)/{f=1} f && /^ *size: [0-9_]+,$/{print; exit}' "$config_rs" \
  | sed -n 's/^ *size: \([0-9_]*\),$/\1/p' | tr -d _)"
[[ "$size" =~ ^[0-9]+$ ]] || fail "the archive size not found in src/remote_config.rs (builtin game)"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# check_one <address> <index>: 0 alive, 1 dead or another file, 2 a network error. Says why on stderr.
check_one() {
  local url="$1" n="$2" headers body code total
  headers="$tmp/headers-$n.txt"
  body="$tmp/body-$n.bin"
  # The address the game download asks for, the first 1 KiB as a range.
  if ! code="$(curl -sSL -m 60 -r 0-1023 -D "$headers" -o "$body" -w '%{http_code}' "$url")"; then
    echo "check-game-link: network error fetching $url" >&2
    return 2
  fi
  case "$code" in
    200|206) ;;
    403|429)
      echo "check-game-link: $url refuses the archive for now (HTTP $code): too many downloads, or not public" >&2
      return 1 ;;
    *)
      echo "check-game-link: $url returned HTTP $code: the link is dead or not public" >&2
      return 1 ;;
  esac

  # The total size is in Content-Range (a range answer) or Content-Length (a whole file); with
  # redirects the last answer's header counts.
  total="$(tr -d '\r' < "$headers" | sed -n 's|^[Cc]ontent-[Rr]ange: bytes [0-9]*-[0-9]*/\([0-9]*\)$|\1|p' | tail -n 1)"
  if [ -z "$total" ]; then
    total="$(tr -d '\r' < "$headers" | sed -n 's|^[Cc]ontent-[Ll]ength: \([0-9]*\)$|\1|p' | tail -n 1)"
  fi
  if [ -z "$total" ]; then
    echo "check-game-link: the answer has no size ($url)" >&2
    return 1
  fi
  if [ "$total" != "$size" ]; then
    echo "check-game-link: the archive at $url is $total bytes, not the $size Razdor expects: the archive changed" >&2
    return 1
  fi
  if head -c 256 "$body" | grep -qi '<html\|<!doctype'; then
    echo "check-game-link: the archive at $url came as a web page, not the file" >&2
    return 1
  fi
  if [ "$(head -c 2 "$body")" != "PK" ]; then
    echo "check-game-link: the archive at $url does not start like a .zip" >&2
    return 1
  fi
  echo "check-game-link: ok ($url, $total bytes)"
  return 0
}

worst=0
n=0
while IFS= read -r url; do
  n=$((n + 1))
  rc=0
  check_one "$url" "$n" || rc=$?
  # A dead link (1) outranks a network error on this machine (2).
  if [ "$rc" -eq 1 ] || { [ "$rc" -eq 2 ] && [ "$worst" -eq 0 ]; }; then
    worst=$rc
  fi
done <<< "$urls"

if [ "$worst" -eq 0 ]; then
  echo "check-game-link: all $n game addresses are alive"
fi
exit "$worst"
