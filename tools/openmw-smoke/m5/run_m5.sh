#!/usr/bin/env bash
# The M5 synthetic scenario end to end (tools/openmw-smoke/m5): make_m5_map.py, then razdor-openmw
# (one DT hour is 6 s: --time-scale 600), then run.sh with the test driver and the event order checked
# against EXPECTED.txt (run.sh --expect). Exit 0 = PASS. Synthetic input only.
#
#   run_m5.sh [--name <run name>]     the run folder is <scratch>/openmw/m5/<name> (must not exist)
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SMOKE=$(cd "$HERE/.." && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
ROOT=/mnt/ai_models/razdor-scratch/openmw
EXAMPLE=${OPENMW_EXAMPLE:-$ROOT/example-suite}
TEMPLATE=$EXAMPLE/game_template/data/template.omwgame
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target-pause}
NAME=m5-$(date +%Y%m%d-%H%M%S)
while [ $# -gt 0 ]; do
    case "$1" in
        --name) NAME=$2; shift 2 ;;
        *) echo "run_m5.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
WORK="$ROOT/m5/$NAME"
[ -e "$WORK" ] && { echo "run_m5.sh: $WORK exists" >&2; exit 2; }
mkdir -p "$WORK" || exit 2
[ -f "$TEMPLATE" ] || { echo "run_m5.sh: the Game Template is missing at $EXAMPLE" >&2; exit 2; }
PREFIX=$(ai-resources prefix)

python3 -I "$HERE/make_m5_map.py" "$WORK/m5.json" || exit 2
( cd "$REPO" && CARGO_BUILD_JOBS=12 $PREFIX cargo build --profile fasttest -p razdor-openmw --bin razdor-openmw ) \
    > "$WORK/build.log" 2>&1 || { echo "run_m5.sh: build failed (see $WORK/build.log)" >&2; exit 2; }
$PREFIX "$CARGO_TARGET_DIR/fasttest/razdor-openmw" "$WORK/m5.json" -o "$WORK/out" --template "$TEMPLATE" \
    --time-scale 600 > "$WORK/convert.log" 2>&1 || { echo "run_m5.sh: conversion failed (see $WORK/convert.log)" >&2; exit 2; }

OPENMW_SMOKE_SCRATCH="$WORK" "$SMOKE/run.sh" "$WORK/out" --name run --seconds 40 \
    --extra-data "$HERE" --content m5_test.omwscripts --expect "$HERE/EXPECTED.txt"
