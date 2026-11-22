#!/usr/bin/env bash
# Builds razdor-server as a static musl binary and packs it into the API image (server/deploy/Containerfile).
#
#   server/deploy/build.sh [image-tag]      (default: localhost/razdor-api:<version>)
#
# Three ways to get the musl binary; the first that is available is used:
#   1. the host has the musl target and musl-gcc: a plain cargo build;
#   2. cargo-zigbuild with zig on the PATH (zig is the C compiler that rusqlite and zstd-sys need);
#   3. a podman builder container (docker.io/library/rust:1.91-alpine: Alpine's gcc targets musl
#      natively). This is the path on a plain Fedora or Debian workstation without musl tools.
#
# Env: CARGO_TARGET_DIR (default: <repo>/target), RAZDOR_BUILDER_IMAGE (default rust:1.91-alpine),
# RAZDOR_CARGO_HOME (default ~/.cargo; its registry cache is shared with the builder container).
# The build is CPU-heavy. On a shared machine run it pinned and at low priority, for example:
#   nice -n 19 taskset -c 8-19 server/deploy/build.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO/target}"
TRIPLE=x86_64-unknown-linux-musl
BUILDER_IMAGE="${RAZDOR_BUILDER_IMAGE:-docker.io/library/rust:1.91-alpine}"
CARGO_HOME_DIR="${RAZDOR_CARGO_HOME:-$HOME/.cargo}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/server/Cargo.toml" | head -n 1)"
TAG="${1:-localhost/razdor-api:$VERSION}"
BIN_OUT="$TARGET_DIR/$TRIPLE/release/razdor-server"

build_native() {
  rustup target add "$TRIPLE" >/dev/null
  cargo build --release --locked -p razdor-server --target "$TRIPLE" --manifest-path "$REPO/Cargo.toml"
}

build_zig() {
  rustup target add "$TRIPLE" >/dev/null
  cargo zigbuild --release --locked -p razdor-server --target "$TRIPLE" --manifest-path "$REPO/Cargo.toml"
}

build_container() {
  mkdir -p "$TARGET_DIR" "$CARGO_HOME_DIR/registry"
  podman run --rm \
    -v "$REPO:/src:ro,Z" \
    -v "$TARGET_DIR:/target:Z" \
    -v "$CARGO_HOME_DIR/registry:/usr/local/cargo/registry:Z" \
    -w /src \
    -e CARGO_TARGET_DIR=/target \
    -e CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-12}" \
    "$BUILDER_IMAGE" \
    sh -c "cargo build --release --locked -p razdor-server --target $TRIPLE && strip /target/$TRIPLE/release/razdor-server"
}

if command -v musl-gcc >/dev/null && rustup target list --installed 2>/dev/null | grep -qx "$TRIPLE"; then
  echo "build: native musl (musl-gcc)"
  build_native
elif command -v cargo-zigbuild >/dev/null && command -v zig >/dev/null; then
  echo "build: cargo zigbuild"
  build_zig
else
  echo "build: podman builder $BUILDER_IMAGE"
  build_container
fi

[ -f "$BIN_OUT" ] || { echo "build: no binary at $BIN_OUT" >&2; exit 1; }
strip "$BIN_OUT" 2>/dev/null || true
file "$BIN_OUT" | grep -q "static" && ! file "$BIN_OUT" | grep -q "dynamically linked" || { echo "build: $BIN_OUT is not static" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp "$BIN_OUT" "$STAGE/razdor-server"
cp "$HERE/Containerfile" "$STAGE/Containerfile"
podman build --format docker -t "$TAG" -f "$STAGE/Containerfile" "$STAGE"
podman image inspect "$TAG" --format 'image {{.Id}} size {{.Size}} bytes'
