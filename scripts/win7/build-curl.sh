#!/bin/sh
# Builds curl.exe for Windows 7 from source (no Universal C Runtime: msvcrt only), static, with
# mbedTLS for TLS, and the Mozilla CA bundle as curl-ca-bundle.crt. The pins (versions, URLs and
# SHA-256s of the curl and mbedTLS sources and of cacert.pem) are scripts/curl-win.env. Used by
# scripts/dist-win7.sh (Razdor's Windows 7 package) and by the release workflow's windows7 job.
#
# Usage: sh scripts/win7/build-curl.sh <out-dir>
# The out-dir gets curl.exe, curl-ca-bundle.crt and the licenses (curl-COPYING.txt,
# mbedtls-LICENSE.txt, curl-ca-bundle-LICENSE.txt). The sources and the build tree go to
# $WIN7_WORK/curl (default: .cache/curl-win/build, gitignored).
#
# Toolchain (the same as the Razdor Windows 7 build, see scripts/dist-win7.sh): clang with the
# mingw-w64 msvcrt sysroot, ld.lld, gcc's libgcc, windres, and cmake and ninja.
# Environment:
#   WIN7_CLANG, WIN7_SYSROOT, WIN7_GCC, WIN7_EXTRA_LIBDIR, WIN7_AR, WINDRES: as in dist-win7.sh
#   WIN7_EXTRA_INCDIR: one more folder of C headers (optional, e.g. mingw-w64 headers kept apart)
set -eu
cd "$(dirname "$0")/../.."
ROOT=$(pwd)
die() { echo "build-curl.sh: $*" >&2; exit 1; }
[ $# -eq 1 ] || die "usage: sh scripts/win7/build-curl.sh <out-dir>"
case "$1" in /*) OUT=$1 ;; *) OUT=$ROOT/$1 ;; esac
. scripts/curl-win.env

CLANG=${WIN7_CLANG:-clang}
SYSROOT=${WIN7_SYSROOT:-/usr/x86_64-w64-mingw32}
GCC=${WIN7_GCC:-x86_64-w64-mingw32-gcc}
AR=${WIN7_AR:-ar}
WINDRES=${WINDRES:-x86_64-w64-mingw32-windres}
WORK=${WIN7_WORK:-$ROOT/.cache/curl-win/build}
for t in cmake ninja ld.lld "$CLANG" "$GCC" "$AR" "$WINDRES"; do
    command -v "$t" >/dev/null 2>&1 || die "$t not found"
done
[ -f "$SYSROOT/lib/crt2.o" ] || die "no msvcrt startup object in $SYSROOT/lib (WIN7_SYSROOT)"
LIBGCC=$(dirname "$("$GCC" -print-libgcc-file-name)")
EXTRA=
if [ -n "${WIN7_EXTRA_LIBDIR:-}" ]; then
    EXTRA="-L$WIN7_EXTRA_LIBDIR"
fi
INC=
if [ -n "${WIN7_EXTRA_INCDIR:-}" ]; then
    INC="-isystem $WIN7_EXTRA_INCDIR"
fi

sha_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}
# fetch <url> <sha256> <file>: downloads once into the cache, checks the pin every time.
fetch() {
    if [ ! -f "$3" ] || [ "$(sha_of "$3")" != "$2" ]; then
        rm -f "$3"
        curl -fsSL -o "$3.part" "$1" || die "cannot download $1"
        mv "$3.part" "$3"
    fi
    [ "$(sha_of "$3")" = "$2" ] || die "$3 does not match its pin in scripts/curl-win.env"
}

mkdir -p "$WORK/src"
cd "$WORK/src"
fetch "$CURL_SRC_URL" "$CURL_SRC_SHA256" "curl-$CURL_VERSION.tar.xz"
fetch "$MBEDTLS_SRC_URL" "$MBEDTLS_SRC_SHA256" "mbedtls-$MBEDTLS_VERSION.tar.bz2"
fetch "$CACERT_URL" "$CACERT_SHA256" cacert.pem
rm -rf "curl-$CURL_VERSION" "mbedtls-$MBEDTLS_VERSION"
tar -xf "curl-$CURL_VERSION.tar.xz"
# Razdor's patch: the Windows secure CRT functions curl uses are replaced by classic msvcrt ones.
patch -p1 -s -d "curl-$CURL_VERSION" < "$ROOT/scripts/win7/curl-msvcrt.patch" || die "the curl patch does not apply"
tar -xf "mbedtls-$MBEDTLS_VERSION.tar.bz2"

# The CMake toolchain: clang for the mingw target on the msvcrt sysroot, lld as the linker.
# _WIN32_WINNT 0x0601 is Windows 7: nothing newer is named in the headers either.
cd "$WORK"
cat > "$WORK/mingw7.cmake" <<EOF
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR x86_64)
set(CMAKE_C_COMPILER "$CLANG")
set(CMAKE_C_COMPILER_TARGET x86_64-w64-windows-gnu)
set(CMAKE_C_FLAGS_INIT "--sysroot=$SYSROOT $INC -D_WIN32_WINNT=0x0601 -DWINVER=0x0601 -Wno-error")
set(CMAKE_EXE_LINKER_FLAGS_INIT "-fuse-ld=lld -static -L$LIBGCC -B$LIBGCC $EXTRA")
set(CMAKE_SHARED_LINKER_FLAGS_INIT "-fuse-ld=lld -L$LIBGCC -B$LIBGCC $EXTRA")
set(CMAKE_AR "$AR")
set(CMAKE_RC_COMPILER "$WINDRES")
set(CMAKE_FIND_ROOT_PATH "$SYSROOT")
set(CMAKE_TRY_COMPILE_TARGET_TYPE STATIC_LIBRARY)
EOF

# mbedTLS (static, library only): installed into $WORK/prefix for curl's find module.
cmake -G Ninja -S "$WORK/src/mbedtls-$MBEDTLS_VERSION" -B "$WORK/mbedtls" \
    -DCMAKE_TOOLCHAIN_FILE="$WORK/mingw7.cmake" -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$WORK/prefix" \
    -DENABLE_PROGRAMS=OFF -DENABLE_TESTING=OFF -DMBEDTLS_FATAL_WARNINGS=OFF \
    -DUSE_SHARED_MBEDTLS_LIBRARY=OFF -DUSE_STATIC_MBEDTLS_LIBRARY=ON
cmake --build "$WORK/mbedtls"
cmake --install "$WORK/mbedtls"

# curl: static library and tool, TLS through mbedTLS, the other optional libraries off.
cmake -G Ninja -S "$WORK/src/curl-$CURL_VERSION" -B "$WORK/curl" \
    -DCMAKE_TOOLCHAIN_FILE="$WORK/mingw7.cmake" -DCMAKE_BUILD_TYPE=Release \
    -DBUILD_SHARED_LIBS=OFF -DCURL_STATICLIB=ON -DBUILD_CURL_EXE=ON \
    -DBUILD_TESTING=OFF -DENABLE_CURL_MANUAL=OFF -DBUILD_LIBCURL_DOCS=OFF -DBUILD_MISC_DOCS=OFF \
    -DCURL_USE_MBEDTLS=ON -DMBEDTLS_INCLUDE_DIR="$WORK/prefix/include" \
    -DMBEDTLS_LIBRARY="$WORK/prefix/lib/libmbedtls.a" \
    -DMBEDX509_LIBRARY="$WORK/prefix/lib/libmbedx509.a" \
    -DMBEDCRYPTO_LIBRARY="$WORK/prefix/lib/libmbedcrypto.a" \
    -DCURL_USE_SCHANNEL=OFF -DCURL_USE_OPENSSL=OFF -DCURL_USE_GNUTLS=OFF -DCURL_USE_WOLFSSL=OFF \
    -DCURL_ZLIB=OFF -DCURL_BROTLI=OFF -DCURL_ZSTD=OFF -DUSE_NGHTTP2=OFF -DUSE_NGTCP2=OFF \
    -DCURL_USE_LIBPSL=OFF -DCURL_USE_LIBSSH2=OFF -DCURL_USE_LIBSSH=OFF -DUSE_LIBIDN2=OFF \
    -DCURL_DISABLE_LDAP=ON -DCURL_DISABLE_LDAPS=ON -DCURL_DISABLE_DICT=ON \
    -DCURL_DISABLE_GOPHER=ON -DCURL_DISABLE_TELNET=ON -DCURL_DISABLE_TFTP=ON \
    -DCURL_DISABLE_IMAP=ON -DCURL_DISABLE_POP3=ON -DCURL_DISABLE_SMTP=ON \
    -DCURL_DISABLE_RTSP=ON -DCURL_DISABLE_MQTT=ON -DCURL_DISABLE_SMB=ON \
    -DCURL_DISABLE_FTP=ON -DCURL_DISABLE_FILE=ON -DCURL_DISABLE_LIBCURL_OPTION=ON \
    -DENABLE_UNIX_SOCKETS=OFF -DCURL_CA_SEARCH_SAFE=ON
cmake --build "$WORK/curl"

# The package's files. The CA bundle sits next to curl.exe, where curl finds it by itself on Windows
# (CURL_CA_FALLBACK, no --cacert needed).
mkdir -p "$OUT"
cp "$WORK/curl/src/curl.exe" "$OUT/curl.exe"
cp "$WORK/src/curl-$CURL_VERSION/COPYING" "$OUT/curl-COPYING.txt"
cp "$WORK/src/mbedtls-$MBEDTLS_VERSION/LICENSE" "$OUT/mbedtls-LICENSE.txt" 2>/dev/null \
    || cp "$WORK/src/mbedtls-$MBEDTLS_VERSION/LICENSE.txt" "$OUT/mbedtls-LICENSE.txt"
cp "$WORK/src/cacert.pem" "$OUT/curl-ca-bundle.crt"
grep '^##' "$WORK/src/cacert.pem" > "$OUT/curl-ca-bundle-LICENSE.txt"
echo "curl.exe: $(sha_of "$OUT/curl.exe") ($(wc -c < "$OUT/curl.exe" | tr -d ' ') bytes)"
