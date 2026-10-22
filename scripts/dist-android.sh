#!/bin/sh
# Builds dist/razdor.apk: Razdor for Android (arm64-v8a, armeabi-v7a and x86_64, the last for
# emulators: see the build_targets in Cargo.toml). Experimental: touch only (a tap is a left
# click). It plays the Discord Times install the player keeps on the shared storage (README,
# «Android»); the app asks for the all-files access itself.
#
# Prerequisites (the release workflow installs the same, .github/workflows/release.yml):
#   - the Android SDK in $ANDROID_HOME: platform-tools, platform android-34, build-tools 35.0.0
#     (or another 34.x / 35.x: the highest one is used);
#   - the NDK 29 (29.0.14206865) in $NDK_HOME, or in $ANDROID_HOME/ndk/29.0.14206865;
#   - a JDK (javac, keytool) on the PATH or in $JAVA_HOME: cargo-quad-apk compiles the glue;
#   - cargo-quad-apk from its master, pinned to d411c8fe1c08339e46d7dc6b40ac04ea5e3a01b6:
#       cargo install --git https://github.com/not-fl3/cargo-quad-apk --rev d411c8fe1c08339e46d7dc6b40ac04ea5e3a01b6 --locked
#     (the crates.io release 0.1.4 needs javac 1.7 and dx, which current JDKs and build-tools
#     no longer have; master uses d8 and aapt2 and builds with NDK 29);
#   - the Rust targets of the pinned toolchain:
#       rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
#
# Signing, three modes:
#   - RAZDOR_ANDROID_UNSIGNED=1 (the release workflow's build step): no signature at all;
#     the aligned APK goes to dist/razdor-unsigned.apk. The release workflow signs it in its
#     own step, with the secrets in that step's env only (.github/workflows/release.yml).
#   - RAZDOR_ANDROID_KEYSTORE set (a keystore file), with RAZDOR_ANDROID_KEY_ALIAS and
#     RAZDOR_ANDROID_KEYSTORE_PASSWORD (the keystore's and the key's password, the same): the
#     APK is signed with it. The password is read from the environment only: never put it in
#     the repo or on the command line.
#   - otherwise the APK is signed with the debug key that cargo-quad-apk makes in ~/.android:
#     installable, but not the release signature (the script says so).
#
# The APK is built in $CARGO_TARGET_DIR (or target/) under android-artifacts/release/apk/.
set -eu
cd "$(dirname "$0")/.."
mkdir -p dist

if [ -z "${ANDROID_HOME:-}" ]; then
    echo "ANDROID_HOME is not set: the Android SDK folder" >&2
    exit 1
fi
if [ -z "${NDK_HOME:-}" ]; then
    NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
    export NDK_HOME
fi
if [ ! -d "$NDK_HOME" ]; then
    echo "the NDK is not at $NDK_HOME (set NDK_HOME)" >&2
    exit 1
fi
if ! command -v cargo-quad-apk >/dev/null 2>&1; then
    echo "cargo-quad-apk not found: see the prerequisites in this script's header" >&2
    exit 1
fi
# The newest build-tools (apksigner, zipalign: cargo-quad-apk picks the same one).
APKSIGNER=$(ls -d "$ANDROID_HOME"/build-tools/*/apksigner 2>/dev/null | sort -V | tail -n 1)

# The same paths remapping as scripts/dist.sh: panic messages name no home folder.
REMAP="--remap-path-prefix=$HOME=~ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo"
RUSTC_COMMIT=$(rustc -vV | sed -n 's/^commit-hash: //p')
for src in "${RUSTUP_HOME:-$HOME/.rustup}"/toolchains/*/lib/rustlib/src/rust; do
    [ -d "$src" ] && REMAP="$REMAP --remap-path-prefix=$src=/rustc/$RUSTC_COMMIT"
done
REMAP="$REMAP --remap-path-prefix=$PWD=razdor"

# The program only (`--bin razdor`, not the map translator), without gamepads (gilrs).
BUILD="--release --no-default-features --features audio --bin razdor"
APK="${CARGO_TARGET_DIR:-target}/android-artifacts/release/apk/razdor.apk"

if [ "${RAZDOR_ANDROID_UNSIGNED:-}" = 1 ]; then
    # Aligned by cargo-quad-apk, not signed: the caller signs it (no secret is read here).
    RUSTFLAGS="$REMAP" cargo quad-apk build $BUILD --nosign
    cp "$APK" dist/razdor-unsigned.apk
    ls -l dist/razdor-unsigned.apk
    sha256sum dist/razdor-unsigned.apk
    exit 0
elif [ -n "${RAZDOR_ANDROID_KEYSTORE:-}" ]; then
    if [ -z "${RAZDOR_ANDROID_KEY_ALIAS:-}" ] || [ -z "${RAZDOR_ANDROID_KEYSTORE_PASSWORD:-}" ]; then
        echo "RAZDOR_ANDROID_KEYSTORE needs RAZDOR_ANDROID_KEY_ALIAS and RAZDOR_ANDROID_KEYSTORE_PASSWORD too" >&2
        exit 1
    fi
    # Unsigned and aligned by cargo-quad-apk; signed here with the release keystore.
    RUSTFLAGS="$REMAP" cargo quad-apk build $BUILD --nosign
    "$APKSIGNER" sign --ks "$RAZDOR_ANDROID_KEYSTORE" --ks-key-alias "$RAZDOR_ANDROID_KEY_ALIAS" \
        --ks-pass env:RAZDOR_ANDROID_KEYSTORE_PASSWORD --key-pass env:RAZDOR_ANDROID_KEYSTORE_PASSWORD \
        --out dist/razdor.apk "$APK"
else
    echo "warning: RAZDOR_ANDROID_KEYSTORE is not set: signing with the debug key (not a release signature)" >&2
    RUSTFLAGS="$REMAP" cargo quad-apk build $BUILD
    cp "$APK" dist/razdor.apk
fi

"$APKSIGNER" verify --print-certs dist/razdor.apk | sed -n '1,3p'
ls -l dist/razdor.apk
sha256sum dist/razdor.apk
