#!/usr/bin/env bash
# Off-screen screenshot of Razdor for the player wiki.
#
#   tools/wiki/shot.sh SCENE OUT.webp [WIDTHxHEIGHT]   (default 1280x800)
#   tools/wiki/shot.sh --list                          (the scene names)
#
# Follows .claude/skills/offscreen-snapshot: the release binary under Xvfb, its own
# XDG_RUNTIME_DIR, the owner's log and saves left alone, RAZDOR_DT_DIR from the main repo's
# .env (quotes stripped), pinned to the E-cores. Nothing appears on the desktop.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRATCH=/mnt/ai_models/razdor-scratch/wiki/tmp
BIN="${RAZDOR_BIN:-/mnt/ai_models/razdor-target/release/razdor}"

# The scene names are the first word of each backticked scene in the header of
# src/ui/snapshot.rs (lowercase; the RAZDOR_* helpers are upper case and skipped).
list_scenes() {
	sed -n '6,17p' "$ROOT/src/ui/snapshot.rs" \
		| grep -oP '`\K[a-z][a-z0-9-]*(?=[:`\[])' \
		| grep -vx 'keys' | sort -u
}

if [[ "${1:-}" == "--list" ]]; then
	list_scenes
	exit 0
fi

if [[ $# -lt 2 || $# -gt 3 ]]; then
	echo "usage: $0 SCENE OUT.webp [WIDTHxHEIGHT] | $0 --list" >&2
	exit 2
fi
SCENE="$1"
OUT="$2"
SIZE="${3:-1280x800}"
if [[ ! "$SIZE" =~ ^[0-9]+x[0-9]+$ ]]; then
	echo "shot.sh: size must look like 1280x800, got '$SIZE'" >&2
	exit 2
fi
if ! list_scenes | grep -qx "${SCENE%%[:]*}"; then
	echo "shot.sh: '${SCENE%%:*}' is not a scene in src/ui/snapshot.rs (see --list)" >&2
	exit 2
fi

# RAZDOR_DT_DIR: the environment first, else the main repo's .env (the worktree has none).
MAIN_REPO="$(dirname "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)")"
if [[ -z "${RAZDOR_DT_DIR:-}" ]]; then
	for env_file in "$MAIN_REPO/.env" "$ROOT/.env"; do
		if [[ -f "$env_file" ]]; then
			RAZDOR_DT_DIR="$(grep -m1 '^RAZDOR_DT_DIR=' "$env_file" | cut -d= -f2- | tr -d '"')"
			break
		fi
	done
fi
if [[ -z "${RAZDOR_DT_DIR:-}" ]]; then
	echo "shot.sh: RAZDOR_DT_DIR is not set (environment or .env)" >&2
	exit 1
fi
if [[ ! -x "$BIN" ]]; then
	echo "shot.sh: $BIN is missing; building the release binary" >&2
	(cd "$ROOT" && CARGO_TARGET_DIR="$(dirname "$(dirname "$BIN")")" CARGO_BUILD_JOBS=4 \
		taskset -c 16-19 cargo build --release)
fi

# Tools for the WebP conversion: cwebp, else ImageMagick, else python PIL.
if command -v cwebp >/dev/null 2>&1; then
	CONVERTER=cwebp
elif command -v magick >/dev/null 2>&1; then
	CONVERTER=magick
elif command -v convert >/dev/null 2>&1; then
	CONVERTER=convert
elif python3 -I -c 'import PIL' >/dev/null 2>&1; then
	CONVERTER=pil
else
	echo "shot.sh: no WebP converter (cwebp, magick, convert or python PIL)" >&2
	exit 1
fi

mkdir -p "$SCRATCH"
TMP="$(mktemp -d "$SCRATCH/shot.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
chmod 700 "$TMP"
mkdir -p "$TMP/runtime" "$TMP/saves"
chmod 700 "$TMP/runtime"

# Xvfb is started and stopped by xvfb-run; the timeout stops a run that hangs.
env XDG_RUNTIME_DIR="$TMP/runtime" \
	RAZDOR_LOG="$TMP/razdor.log" RAZDOR_SAVE_DIR="$TMP/saves" \
	RAZDOR_DT_DIR="$RAZDOR_DT_DIR" RAZDOR_SIZE="$SIZE" \
	RAZDOR_SCENE="$SCENE" RAZDOR_SNAPSHOT="$TMP/shot.png" \
	taskset -c 16-19 timeout 180 \
	xvfb-run -a -s "-screen 0 ${SIZE}x24" "$BIN" >"$TMP/run.out" 2>&1 || {
		echo "shot.sh: the run failed; its output:" >&2
		tail -20 "$TMP/run.out" >&2
		exit 1
	}
if [[ ! -s "$TMP/shot.png" ]]; then
	echo "shot.sh: no picture was written for scene '$SCENE'" >&2
	tail -20 "$TMP/razdor.log" >&2 || true
	exit 1
fi

mkdir -p "$(dirname "$OUT")"
case "$CONVERTER" in
	cwebp) taskset -c 16-19 cwebp -quiet -noalpha -q 82 "$TMP/shot.png" -o "$OUT" ;;
	magick) taskset -c 16-19 magick "$TMP/shot.png" +repage -alpha off -quality 82 "$OUT" ;;
	convert) taskset -c 16-19 convert "$TMP/shot.png" +repage -alpha off -quality 82 "$OUT" ;;
	pil) taskset -c 16-19 python3 -I -c "
import sys
from PIL import Image
Image.open(sys.argv[1]).convert('RGB').save(sys.argv[2], 'WEBP', quality=82)
" "$TMP/shot.png" "$OUT" ;;
esac
echo "wrote $OUT ($SCENE, $SIZE)"
