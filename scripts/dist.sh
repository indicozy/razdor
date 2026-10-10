#!/bin/sh
# Builds the release programs into dist/: `razdor` for Linux (x86_64) and `Razdor.exe` for
# Windows (x86_64). Either one, put into a Discord Times folder (next to DiscordTimes.exe),
# plays that install; elsewhere it uses RAZDOR_DT_DIR (also read from a .env file).
# dist/SHA256SUMS lists their SHA-256: publish it with the files, and check a download with
# `sha256sum -c SHA256SUMS` (Linux) or `Get-FileHash Razdor.exe` (Windows PowerShell).
#
# The Linux build needs the headers of ALSA and udev (libasound2-dev and libudev-dev on
# Debian / Ubuntu, alsa-lib-devel and systemd-devel on Fedora): sound and gamepads.
#
# The Windows build needs the target (`rustup target add x86_64-pc-windows-gnullvm`) and an
# llvm-mingw toolchain (https://github.com/mstorsjo/llvm-mingw, the ucrt build): set
# LLVM_MINGW to its folder, or have x86_64-w64-mingw32-clang on the PATH.
set -eu
cd "$(dirname "$0")/.."
mkdir -p dist

# Panic messages name source files: without the builder's home and folders. When several
# prefixes match a path, rustc applies the last one, so the most specific comes last: the
# project folder is always razdor/ and Cargo's cargo/, wherever they are (with the home
# last, a project inside the home became ~/…/razdor, and the SHAs depended on the folder).
REMAP="--remap-path-prefix=$HOME=~ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo"
# The standard library's own sources: with the rust-src component installed, rustc writes
# their real paths (under the toolchain) where it would write /rustc/<commit>, and a cache
# built by another toolchain of the same release can bring them in too. Every toolchain's
# sources map back to /rustc/<commit>, so the programs do not depend on what is installed.
RUSTC_COMMIT=$(rustc -vV | sed -n 's/^commit-hash: //p')
for src in "${RUSTUP_HOME:-$HOME/.rustup}"/toolchains/*/lib/rustlib/src/rust; do
    [ -d "$src" ] && REMAP="$REMAP --remap-path-prefix=$src=/rustc/$RUSTC_COMMIT"
done
REMAP="$REMAP --remap-path-prefix=$PWD=razdor"
RUSTFLAGS="$REMAP" cargo build --release
cp target/release/razdor dist/razdor

if [ -n "${LLVM_MINGW:-}" ]; then
    PATH="$LLVM_MINGW/bin:$PATH"
fi
if ! command -v x86_64-w64-mingw32-clang >/dev/null; then
    echo "x86_64-w64-mingw32-clang not found: set LLVM_MINGW to the llvm-mingw folder" >&2
    exit 1
fi
# crt-static links libunwind in, so the exe needs no DLL of its own. The linker would stamp
# the build time into the exe's header; without it the same commit always gives the same SHA.
CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang \
CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-Wl,--no-insert-timestamp $REMAP" \
CC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang \
AR_x86_64_pc_windows_gnullvm=llvm-ar \
    cargo build --release --target x86_64-pc-windows-gnullvm
cp target/x86_64-pc-windows-gnullvm/release/razdor.exe dist/Razdor.exe

(cd dist && sha256sum razdor Razdor.exe > SHA256SUMS)
ls -l dist
cat dist/SHA256SUMS
