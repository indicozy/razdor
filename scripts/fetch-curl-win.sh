#!/bin/sh
# Puts the Windows curl (curl.exe and its license, COPYING.txt as curl-COPYING.txt) into <dest>/: the
# official static build named in scripts/curl-win.env. The zip is cached under .cache/curl-win/ and
# checked against the pin, size and SHA-256, before anything is taken from it. Used by
# scripts/dist-win7.sh (Razdor's Windows 7 package: Windows 7 has no curl of its own).
set -eu
cd "$(dirname "$0")/.."
. scripts/curl-win.env
dest=${1:?usage: sh scripts/fetch-curl-win.sh <dest-dir>}

sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

zip=.cache/curl-win/curl-win.zip
if [ ! -f "$zip" ] || [ "$(sha_of "$zip")" != "$CURL_WIN_SHA256" ]; then
    mkdir -p .cache/curl-win
    if ! curl -fsSL -o "$zip.part" "$CURL_WIN_URL"; then
        rm -f "$zip.part"
        echo "curl for Windows is not at $CURL_WIN_URL" >&2
        exit 1
    fi
    mv "$zip.part" "$zip"
fi
size=$(wc -c < "$zip" | tr -d ' ')
sha=$(sha_of "$zip")
if [ "$size" != "$CURL_WIN_SIZE" ] || [ "$sha" != "$CURL_WIN_SHA256" ]; then
    echo "$zip does not match scripts/curl-win.env: size $size (pinned $CURL_WIN_SIZE), sha256 $sha (pinned $CURL_WIN_SHA256)" >&2
    exit 1
fi
mkdir -p "$dest"
unzip -p "$zip" "$CURL_WIN_DIR/bin/curl.exe" > "$dest/curl.exe"
unzip -p "$zip" "$CURL_WIN_DIR/COPYING.txt" > "$dest/curl-COPYING.txt"
echo "curl for Windows: $dest/curl.exe ($size-byte zip, sha256 $sha)"
