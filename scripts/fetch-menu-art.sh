#!/bin/sh
# Puts the first screen's art into <dest>/ (its files at their own paths, Graphics/Windows/...): the
# menu-art.zip published with the community's releases (address and pin in scripts/menu-art.env). The
# zip is cached under .cache/menu-art/ (gitignored) and checked against the pin, size and SHA-256,
# before it is unpacked. RAZDOR_MENU_ART_ZIP=<file> takes a local copy instead (it is checked the same
# way). Used by the dist scripts; nothing here needs a toolchain.
set -eu
cd "$(dirname "$0")/.."
. scripts/menu-art.env
dest=${1:?usage: sh scripts/fetch-menu-art.sh <dest-dir>}

sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

zip=${RAZDOR_MENU_ART_ZIP:-.cache/menu-art/menu-art.zip}
if [ -z "${RAZDOR_MENU_ART_ZIP:-}" ] && { [ ! -f "$zip" ] || [ "$(sha_of "$zip")" != "$MENU_ART_SHA256" ]; }; then
    mkdir -p .cache/menu-art
    if ! curl -fsSL -o "$zip.part" "$MENU_ART_URL"; then
        rm -f "$zip.part"
        echo "menu-art.zip is not published at $MENU_ART_URL: publish it, or set RAZDOR_MENU_ART_ZIP to a local copy" >&2
        exit 1
    fi
    mv "$zip.part" "$zip"
fi
if [ ! -f "$zip" ]; then
    echo "menu-art.zip not found: $zip" >&2
    exit 1
fi
size=$(wc -c < "$zip" | tr -d ' ')
sha=$(sha_of "$zip")
if [ "$size" != "$MENU_ART_SIZE" ] || [ "$sha" != "$MENU_ART_SHA256" ]; then
    echo "$zip does not match scripts/menu-art.env: size $size (pinned $MENU_ART_SIZE), sha256 $sha (pinned $MENU_ART_SHA256)" >&2
    exit 1
fi
rm -rf "$dest"
mkdir -p "$dest"
unzip -q "$zip" -d "$dest"
echo "menu art: $dest ($size bytes, sha256 $sha)"
