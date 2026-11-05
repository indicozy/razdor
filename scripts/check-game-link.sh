#!/usr/bin/env bash
# Checks that the community's game archive on Google Drive (the «Скачать игру» download of
# src/gdrive.rs) is alive before a release: exit 0 when it is, 1 when the link is dead or the file
# is not the one Razdor expects, 2 on a network error. The file id and size are read from the code,
# so the check cannot drift from it. Network only: a range request for the first bytes, nothing is
# downloaded in full and nothing is built or installed.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
fail() { echo "check-game-link: $1" >&2; exit "${2:-1}"; }

code_rs="$root/src/gdrive.rs"
# The GAME constant's fields: the first `    id:` and `    size:` at the constant's indent (the tests use deeper indents).
id="$(sed -n 's/^    id: "\([^"]*\)",$/\1/p' "$code_rs" | head -n 1)"
size="$(sed -n 's/^    size: \([0-9_]*\),$/\1/p' "$code_rs" | head -n 1 | tr -d _)"
[ -n "$id" ] || fail "the archive id not found in src/gdrive.rs"
[ -n "$size" ] || fail "the archive size not found in src/gdrive.rs"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
headers="$tmp/headers.txt"
head_bytes="$tmp/head.bin"

# The same URL scheme the game download uses (gdrive::file_url), the first 1 KiB as a range.
url="https://drive.usercontent.google.com/download?id=$id&export=download&confirm=t"
code="$(curl -sS -m 120 -r 0-1023 -D "$headers" -o "$head_bytes" -w '%{http_code}' "$url")" \
  || fail "network error fetching the archive (file $id)" 2
case "$code" in
  200|206) ;;
  403|429) fail "Google Drive refuses the file for now (HTTP $code, file $id): too many downloads, or not public" ;;
  *) fail "downloading the archive returned HTTP $code (file $id): the link is dead or not public" ;;
esac

# The total size is in Content-Range (a range answer) or Content-Length (a whole file).
total="$(tr -d '\r' < "$headers" | sed -n 's|^[Cc]ontent-[Rr]ange: bytes [0-9]*-[0-9]*/\([0-9]*\)$|\1|p' | tail -n 1)"
if [ -z "$total" ]; then
  total="$(tr -d '\r' < "$headers" | sed -n 's|^[Cc]ontent-[Ll]ength: \([0-9]*\)$|\1|p' | tail -n 1)"
fi
[ -n "$total" ] || fail "the answer has no size (file $id)"
[ "$total" = "$size" ] || fail "the archive on Drive is $total bytes, not the $size Razdor expects (file $id): the archive changed"

if head -c 256 "$head_bytes" | grep -qi '<html\|<!doctype'; then
  fail "the archive came as a web page, not the file (file $id): Google's warning or quota page"
fi
[ "$(head -c 2 "$head_bytes")" = "PK" ] || fail "the archive does not start like a .zip (file $id)"

echo "check-game-link: ok (file $id, $total bytes)"
