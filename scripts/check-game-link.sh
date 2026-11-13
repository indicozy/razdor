#!/usr/bin/env bash
# Checks that the community's game archive on GitHub (the «Скачать игру» download of
# src/gdrive.rs, GAME) is alive before a release: exit 0 when it is, 1 when the link is dead or the
# file is not the one Razdor expects, 2 on a network error. The address and size are read from the
# code, so the check cannot drift from it. Network only: a range request for the first bytes
# (redirects followed, as the game download does), nothing is downloaded in full and nothing is
# built or installed.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
fail() { echo "check-game-link: $1" >&2; exit "${2:-1}"; }

code_rs="$root/src/gdrive.rs"
# The GAME constant's fields: the first `    link:` and `    size:` at the constant's indent (the tests use deeper indents).
url="$(sed -n 's/^    link: Link::Url(Cow::Borrowed("\([^"]*\)")),$/\1/p' "$code_rs" | head -n 1)"
size="$(sed -n 's/^    size: \([0-9_]*\),$/\1/p' "$code_rs" | head -n 1 | tr -d _)"
[ -n "$url" ] || fail "the archive address not found in src/gdrive.rs"
[ -n "$size" ] || fail "the archive size not found in src/gdrive.rs"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
headers="$tmp/headers.txt"
head_bytes="$tmp/head.bin"

# The address the game download asks for (gdrive::file_page of GAME), the first 1 KiB as a range.
code="$(curl -sSL -m 120 -r 0-1023 -D "$headers" -o "$head_bytes" -w '%{http_code}' "$url")" \
  || fail "network error fetching the archive ($url)" 2
case "$code" in
  200|206) ;;
  403|429) fail "the server refuses the archive for now (HTTP $code, $url): too many downloads, or not public" ;;
  *) fail "downloading the archive returned HTTP $code ($url): the link is dead or not public" ;;
esac

# The total size is in Content-Range (a range answer) or Content-Length (a whole file); with
# redirects the last answer's header counts.
total="$(tr -d '\r' < "$headers" | sed -n 's|^[Cc]ontent-[Rr]ange: bytes [0-9]*-[0-9]*/\([0-9]*\)$|\1|p' | tail -n 1)"
if [ -z "$total" ]; then
  total="$(tr -d '\r' < "$headers" | sed -n 's|^[Cc]ontent-[Ll]ength: \([0-9]*\)$|\1|p' | tail -n 1)"
fi
[ -n "$total" ] || fail "the answer has no size ($url)"
[ "$total" = "$size" ] || fail "the archive on the server is $total bytes, not the $size Razdor expects ($url): the archive changed"

if head -c 256 "$head_bytes" | grep -qi '<html\|<!doctype'; then
  fail "the archive came as a web page, not the file ($url)"
fi
[ "$(head -c 2 "$head_bytes")" = "PK" ] || fail "the archive does not start like a .zip ($url)"

echo "check-game-link: ok ($url, $total bytes)"
