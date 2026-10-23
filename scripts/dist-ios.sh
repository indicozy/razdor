#!/bin/sh
# Builds dist/razdor.ipa: Razdor for iPhone and iPad (arm64, iOS 14 and later). Experimental:
# touch only (a tap is a left click). It plays the Discord Times install the player copies into
# the app's folder in the Files app («На iPhone → Раздор», README, «iOS»); the app needs no
# permission for it.
#
# The IPA is NOT signed by a developer: it is ad-hoc signed here only so that the arm64 binary
# loads, and AltStore or Sideloadly re-sign it with the player's own Apple ID when they install
# it. Nothing from an Apple developer account is needed to build it.
#
# Prerequisites (the release workflow's `ios` job does the same, .github/workflows/release.yml):
#   - a Mac with Xcode or its Command Line Tools (the iOS SDK, and codesign, plutil, zip);
#   - the Rust target of the pinned toolchain (rust-toolchain.toml lists it too):
#       rustup target add aarch64-apple-ios
#
# Not yet: an app icon. The bundle has no AppIcon, although the master picture is there
# (data/icon/razdor.png, 1024 px): an iOS icon set (AppIcon, no alpha) would come from it. Until
# then iOS shows a generic icon.
#
# The bundle's Info.plist is written here (the keys are Razdor's: the Files app sharing, the
# landscape orientations, the empty launch screen that makes iOS run it at the full screen size,
# see the comments below). The version comes from Cargo.toml.
set -eu
cd "$(dirname "$0")/.."
ROOT=$PWD
mkdir -p dist

# Source paths in panic messages, the same way as scripts/dist-macos.sh.
REMAP="--remap-path-prefix=$HOME=~ --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=cargo"
RUSTC_COMMIT=$(rustc -vV | sed -n 's/^commit-hash: //p')
for src in "${RUSTUP_HOME:-$HOME/.rustup}"/toolchains/*/lib/rustlib/src/rust; do
    [ -d "$src" ] && REMAP="$REMAP --remap-path-prefix=$src=/rustc/$RUSTC_COMMIT"
done
REMAP="$REMAP --remap-path-prefix=$ROOT=razdor"

# iOS 14 is the oldest the build targets (MinimumOSVersion in the Info.plist below).
export IPHONEOS_DEPLOYMENT_TARGET=14.0
RUSTFLAGS="$REMAP" cargo build --release --target aarch64-apple-ios --no-default-features --features audio --bin razdor

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
# The staging folder is under the target folder, not in the repository.
STAGE=target/dist-ios
rm -rf "$STAGE"
APP="$STAGE/Payload/Razdor.app"
mkdir -p "$APP"
cp target/aarch64-apple-ios/release/razdor "$APP/razdor"

cat > "$APP/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>razdor</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.indicozy.razdor</string>
    <key>CFBundleName</key>
    <string>Razdor</string>
    <key>CFBundleDisplayName</key>
    <string>Раздор</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleSupportedPlatforms</key>
    <array>
        <string>iPhoneOS</string>
    </array>
    <key>CFBundleDevelopmentRegion</key>
    <string>ru</string>
    <key>MinimumOSVersion</key>
    <string>14.0</string>
    <key>LSRequiresIPhoneOS</key>
    <true/>
    <key>UIDeviceFamily</key>
    <array>
        <integer>1</integer>
        <integer>2</integer>
    </array>
    <key>UIRequiredDeviceCapabilities</key>
    <array>
        <string>arm64</string>
    </array>
    <!-- The app's Documents folder shows in the Files app as «На iPhone → Раздор»: the game is
         copied there, and the saves are written there, with no permission to ask for. -->
    <key>UIFileSharingEnabled</key>
    <true/>
    <key>LSSupportsOpeningDocumentsInPlace</key>
    <true/>
    <key>UISupportsDocumentBrowser</key>
    <false/>
    <key>UIRequiresFullScreen</key>
    <true/>
    <key>UIStatusBarHidden</key>
    <true/>
    <!-- An empty launch screen: without one, iOS runs the app letterboxed at an old screen size. -->
    <key>UILaunchScreen</key>
    <dict/>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
    <key>UISupportedInterfaceOrientations~ipad</key>
    <array>
        <string>UIInterfaceOrientationLandscapeLeft</string>
        <string>UIInterfaceOrientationLandscapeRight</string>
    </array>
</dict>
</plist>
EOF
plutil -convert binary1 "$APP/Info.plist"
plutil -lint "$APP/Info.plist"

# Ad-hoc: the arm64 binary must carry a signature to load; the installer re-signs the app.
codesign --force --sign - --timestamp=none "$APP"

rm -f dist/razdor.ipa
(cd "$STAGE" && zip -qr "$ROOT/dist/razdor.ipa" Payload)
unzip -l dist/razdor.ipa

touch dist/SHA256SUMS
grep -v ' razdor\.ipa$' dist/SHA256SUMS > dist/SHA256SUMS.new || true
(cd dist && shasum -a 256 razdor.ipa >> SHA256SUMS.new && mv SHA256SUMS.new SHA256SUMS)
ls -l dist
cat dist/SHA256SUMS
