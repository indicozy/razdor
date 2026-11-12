#!/bin/sh
# Builds the macOS program into dist/: `razdor-macos`, one universal file for Apple Silicon
# (aarch64) and Intel (x86_64) Macs, from macOS 11 on. Like the others, put it into a
# Discord Times folder (next to DiscordTimes.exe, e.g. inside a Wine or CrossOver bottle) to
# play that install; elsewhere it uses RAZDOR_DT_DIR (also read from a .env file).
# Its SHA-256 is added to dist/SHA256SUMS (check a download with `shasum -a 256 -c SHA256SUMS`).
#
# Runs on a Mac only (it links against Apple's SDK, from Xcode or its Command Line Tools);
# the targets come from rust-toolchain.toml. The program is not signed by a developer: the
# linker signs each half ad hoc, which Apple Silicon requires, and Gatekeeper blocks it once
# downloaded until the quarantine is lifted (see the README).
set -eu
cd "$(dirname "$0")/.."
mkdir -p dist

# Source paths in panic messages, the same way as scripts/dist.sh.
REMAP="--remap-path-prefix=$HOME=~ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo"
RUSTC_COMMIT=$(rustc -vV | sed -n 's/^commit-hash: //p')
for src in "${RUSTUP_HOME:-$HOME/.rustup}"/toolchains/*/lib/rustlib/src/rust; do
    [ -d "$src" ] && REMAP="$REMAP --remap-path-prefix=$src=/rustc/$RUSTC_COMMIT"
done
REMAP="$REMAP --remap-path-prefix=$PWD=razdor"

export MACOSX_DEPLOYMENT_TARGET=11.0
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    RUSTFLAGS="$REMAP" cargo build --release --target "$target"
done
lipo -create -output dist/razdor-macos \
    target/aarch64-apple-darwin/release/razdor \
    target/x86_64-apple-darwin/release/razdor
lipo -info dist/razdor-macos
# The map translator, the same way.
lipo -create -output dist/razdor-translate-macos \
    target/aarch64-apple-darwin/release/razdor-translate \
    target/x86_64-apple-darwin/release/razdor-translate
lipo -info dist/razdor-translate-macos
# The promo ads (`ads/`, read at run time next to the program, see src/ui/ads.rs).
rm -rf dist/ads
cp -R ads dist/ads
# The first screen's art (src/ui/menu_art.rs), beside the program: menu-art.zip, pinned in
# scripts/menu-art.env.
sh scripts/fetch-menu-art.sh dist/menu-art

touch dist/SHA256SUMS
grep -v ' razdor-macos$\| razdor-translate-macos$' dist/SHA256SUMS > dist/SHA256SUMS.new || true
(cd dist && shasum -a 256 razdor-macos razdor-translate-macos >> SHA256SUMS.new && mv SHA256SUMS.new SHA256SUMS)
ls -l dist
cat dist/SHA256SUMS
