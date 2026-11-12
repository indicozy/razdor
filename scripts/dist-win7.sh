#!/bin/sh
# Builds Razdor for Windows 7 (and 8): dist/Razdor-win7.exe, and the zip dist/razdor-<version>-windows7.zip
# with that program, curl.exe beside it (scripts/fetch-curl-win.sh), the promo ads and the first
# screen's art. Windows 10 and later use the normal Razdor.exe (scripts/dist.sh).
#
# The target is x86_64-win7-windows-gnu (Rust's tier-3 Windows 7 target: the standard library built
# from source for Windows 7, no Windows 8+ API linked directly). It needs, besides the pinned Rust
# (rust-toolchain.toml): the rust-src component, the x86_64-pc-windows-gnu target (only its crt objects
# rsbegin.o and rsend.o are copied into the Windows 7 sysroot), clang and lld (ld.lld), and mingw-w64
# with its msvcrt sysroot, gcc's libgcc (for the gcc that names it) and the windres of binutils.
# Debian and Ubuntu: `apt install clang lld mingw-w64` provides all of it (CI does this).
#
# Razdor's own changes for Windows 7 are three patches in scripts/win7/, applied to copies of the
# crates they name (miniquad, dirs-sys, rfd) and used through cargo's --config patches. They are
# what the Windows 7 build needs beyond the crates as published: each one is a run-time lookup or
# a different DLL for a Windows 8 function (see the patches). Cargo.lock is restored after the build.
#
# Environment (defaults for Debian and Ubuntu):
#   WIN7_CLANG      clang with the mingw target (default: clang)
#   WIN7_SYSROOT    the mingw-w64 sysroot with lib/ and include/ (default: /usr/x86_64-w64-mingw32)
#   WIN7_GCC        a mingw-w64 gcc, asked for the folder of libgcc.a (default: x86_64-w64-mingw32-gcc)
#   WIN7_EXTRA_LIBDIR  one more folder of libraries, e.g. a static winpthreads (optional)
#   WIN7_AR         the archiver (default: ar)
#   WINDRES         the resource compiler of the icon (default: x86_64-w64-mingw32-windres)
#   CARGO_TARGET_DIR  the build folder (default: target)
set -eu
cd "$(dirname "$0")/.."
ROOT=$(pwd)
mkdir -p dist
die() { echo "dist-win7.sh: $*" >&2; exit 1; }

CLANG=${WIN7_CLANG:-clang}
SYSROOT=${WIN7_SYSROOT:-/usr/x86_64-w64-mingw32}
GCC=${WIN7_GCC:-x86_64-w64-mingw32-gcc}
AR=${WIN7_AR:-ar}
WINDRES=${WINDRES:-x86_64-w64-mingw32-windres}
TARGET_DIR=${CARGO_TARGET_DIR:-target}
case "$TARGET_DIR" in
    /*) ;;
    *) TARGET_DIR=$ROOT/$TARGET_DIR ;;
esac
WORK=${WIN7_WORK:-$TARGET_DIR/win7}
command -v "$CLANG" >/dev/null 2>&1 || die "$CLANG not found (WIN7_CLANG)"
command -v ld.lld >/dev/null 2>&1 || die "ld.lld not found: install lld"
command -v "$GCC" >/dev/null 2>&1 || die "$GCC not found (WIN7_GCC)"
command -v zip >/dev/null 2>&1 || die "zip not found"
command -v python3 >/dev/null 2>&1 || die "python3 not found"
[ -f "$SYSROOT/lib/crt2.o" ] || die "no msvcrt startup object in $SYSROOT/lib (WIN7_SYSROOT)"

# The Rust side: the standard library's source, and the crt objects of the Windows 7 target.
RUSTSYS=$(rustc --print sysroot)
[ -d "$RUSTSYS/lib/rustlib/src/rust" ] || die "rust-src is missing: rustup component add rust-src"
WIN7LIB=$RUSTSYS/lib/rustlib/x86_64-win7-windows-gnu/lib
if [ ! -f "$WIN7LIB/rsbegin.o" ]; then
    GNULIB=$RUSTSYS/lib/rustlib/x86_64-pc-windows-gnu/lib
    [ -f "$GNULIB/rsbegin.o" ] || die "rustup target add x86_64-pc-windows-gnu (its rsbegin.o is used)"
    mkdir -p "$WIN7LIB"
    cp "$GNULIB/rsbegin.o" "$GNULIB/rsend.o" "$WIN7LIB/"
fi

# libgcc.a: the folder gcc names (for -L and -B).
LIBGCC=$(dirname "$("$GCC" -print-libgcc-file-name)")
[ -f "$LIBGCC/libgcc.a" ] || die "libgcc.a not in $LIBGCC (WIN7_GCC)"

# The linker and the C compiler of the target (tract's kernels are compiled with cc): clang on the
# mingw sysroot. ld.lld links it, as llvm-mingw does.
mkdir -p "$WORK/bin"
EXTRA=
if [ -n "${WIN7_EXTRA_LIBDIR:-}" ]; then
    EXTRA="-L$WIN7_EXTRA_LIBDIR"
fi
cat > "$WORK/bin/linker" <<EOF
#!/bin/sh
exec "$CLANG" --target=x86_64-w64-windows-gnu --sysroot="$SYSROOT" -fuse-ld=lld -L"$LIBGCC" -B"$LIBGCC" $EXTRA "\$@"
EOF
cat > "$WORK/bin/cc" <<EOF
#!/bin/sh
exec "$CLANG" --target=x86_64-w64-windows-gnu --sysroot="$SYSROOT" "\$@"
EOF
chmod +x "$WORK/bin/linker" "$WORK/bin/cc"

# The three crates, patched: copies of the sources cargo already has (fetched here), with the patch
# of scripts/win7/ applied to each. Cargo.lock names them by their published source: it is restored.
cp Cargo.lock "$WORK/Cargo.lock.orig"
trap 'cp "$WORK/Cargo.lock.orig" Cargo.lock' EXIT
cargo fetch --locked >/dev/null
cargo metadata --locked --format-version 1 > "$WORK/metadata.json"
python3 -I - "$WORK/metadata.json" > "$WORK/crates.txt" <<'PY'
import json
import sys

meta = json.load(open(sys.argv[1]))
want = {"miniquad": "git+", "dirs-sys": "0.5.0", "rfd": "0.17.2"}
for pkg in meta["packages"]:
    name = pkg["name"]
    if name == "miniquad" and pkg.get("source", "").startswith("git+"):
        print(name, pkg["manifest_path"].rsplit("/", 1)[0])
    elif name in ("dirs-sys", "rfd") and pkg["version"] == want[name]:
        print(name, pkg["manifest_path"].rsplit("/", 1)[0])
PY
[ "$(wc -l < "$WORK/crates.txt" | tr -d ' ')" = 3 ] || die "cannot find the three crates in cargo metadata"
SRC=$WORK/src
rm -rf "$SRC"
mkdir -p "$SRC"
while read -r name dir; do
    cp -R "$dir" "$SRC/$name"
    chmod -R u+w "$SRC/$name"
    patch -p1 -s -d "$SRC/$name" < "scripts/win7/$name.patch" || die "the patch of $name does not apply"
done < "$WORK/crates.txt"

# The build: the same remapped paths as scripts/dist.sh (the SHA does not depend on the folder).
REMAP="--remap-path-prefix=$HOME=~ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo"
REMAP="$REMAP --remap-path-prefix=$RUSTSYS/lib/rustlib/src/rust=/rustc/$(rustc -vV | sed -n 's/^commit-hash: //p')"
REMAP="$REMAP --remap-path-prefix=$ROOT=razdor"
CARGO_TARGET_X86_64_WIN7_WINDOWS_GNU_LINKER="$WORK/bin/linker"
CARGO_TARGET_X86_64_WIN7_WINDOWS_GNU_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-Wl,--no-insert-timestamp $REMAP"
export CARGO_TARGET_X86_64_WIN7_WINDOWS_GNU_LINKER CARGO_TARGET_X86_64_WIN7_WINDOWS_GNU_RUSTFLAGS
CC_x86_64_win7_windows_gnu="$WORK/bin/cc"
AR_x86_64_win7_windows_gnu=$AR
WINDRES=$WINDRES
export CC_x86_64_win7_windows_gnu AR_x86_64_win7_windows_gnu WINDRES
RUSTC_BOOTSTRAP=1
export RUSTC_BOOTSTRAP
# -Z build-std builds the standard library for this target; RUSTC_BOOTSTRAP lets the pinned stable
# compiler take that one flag (the same compiler and sources as the normal build).
cargo build --release --target x86_64-win7-windows-gnu -Z build-std=std,panic_abort --bin razdor \
    --no-default-features --features audio,depth-model \
    --config "patch.crates-io.miniquad.path=\"$SRC/miniquad\"" \
    --config "patch.crates-io.dirs-sys.path=\"$SRC/dirs-sys\"" \
    --config "patch.crates-io.rfd.path=\"$SRC/rfd\""
cp "$TARGET_DIR/x86_64-win7-windows-gnu/release/razdor.exe" dist/Razdor-win7.exe

# The package: the program, curl beside it, the ads and the first screen's art (as the zip of the
# main Windows package: a folder the player unpacks beside DiscordTimes.exe).
sh scripts/fetch-curl-win.sh "$WORK/pack"
cp dist/Razdor-win7.exe "$WORK/pack/Razdor-win7.exe"
rm -rf "$WORK/pack/ads" "$WORK/pack/menu-art"
cp -R ads "$WORK/pack/ads"
sh scripts/fetch-menu-art.sh "$WORK/pack/menu-art"
if [ "${GITHUB_REF_TYPE:-}" = tag ]; then
    V="${GITHUB_REF_NAME#v}"
else
    V=$(awk '/^\[/ {p = ($0 == "[package]")} p && /^version = / {gsub(/"/, "", $3); print $3; exit}' Cargo.toml)
fi
rm -f "dist/razdor-$V-windows7.zip"
(cd "$WORK/pack" && zip -qr "$ROOT/dist/razdor-$V-windows7.zip" .)
ls -l dist/Razdor-win7.exe "dist/razdor-$V-windows7.zip"
(cd dist && sha256sum Razdor-win7.exe "razdor-$V-windows7.zip")
