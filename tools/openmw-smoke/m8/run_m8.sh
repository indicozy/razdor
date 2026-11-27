#!/usr/bin/env bash
# The M8 synthetic fight end to end (tools/openmw-smoke/m8): make_m8_map.py (the map and a units v2
# file), razdor-openmw (--tile-units 512 --units), then run.sh for 120 real seconds with the test driver
# (the player is no target). PASS = the log holds a melee, a shot and a strike hit, and a
# RAZDOR_FIGHT_END line within 120 s of the first player health line. Exit 0 = PASS, 1 = FAIL,
# 2 = setup. Synthetic input only (no game content).
#
#   run_m8.sh [--name <run name>]     the run folder is <scratch>/openmw/m8-lua/<name> (must not exist)
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SMOKE=$(cd "$HERE/.." && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
ROOT=/mnt/ai_models/razdor-scratch/openmw
EXAMPLE=${OPENMW_EXAMPLE:-$ROOT/example-suite}
TEMPLATE=$EXAMPLE/game_template/data/template.omwgame
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target-spells}
NAME=m8-$(date +%Y%m%d-%H%M%S)
while [ $# -gt 0 ]; do
    case "$1" in
        --name) NAME=$2; shift 2 ;;
        *) echo "run_m8.sh: unknown argument $1" >&2; exit 2 ;;
    esac
done
WORK="$ROOT/m8-lua/$NAME"
[ -e "$WORK" ] && { echo "run_m8.sh: $WORK exists" >&2; exit 2; }
mkdir -p "$WORK" || exit 2
[ -f "$TEMPLATE" ] || { echo "run_m8.sh: the Game Template is missing at $EXAMPLE" >&2; exit 2; }
PREFIX=$(ai-resources prefix)

python3 -I "$HERE/make_m8_map.py" "$WORK/m8.json" "$WORK/units.json" || exit 2
( cd "$REPO" && CARGO_BUILD_JOBS=12 $PREFIX cargo build --profile fasttest -p razdor-openmw --bin razdor-openmw ) \
    > "$WORK/build.log" 2>&1 || { echo "run_m8.sh: build failed (see $WORK/build.log)" >&2; exit 2; }
$PREFIX "$CARGO_TARGET_DIR/fasttest/razdor-openmw" "$WORK/m8.json" -o "$WORK/out" --template "$TEMPLATE" \
    --tile-units 512 --units "$WORK/units.json" > "$WORK/convert.log" 2>&1 \
    || { echo "run_m8.sh: conversion failed (see $WORK/convert.log)" >&2; exit 2; }

OPENMW_SMOKE_SCRATCH="$WORK" "$SMOKE/run.sh" "$WORK/out" --name run --seconds 120 \
    --extra-data "$HERE" --content m8_test.omwscripts --shot "$WORK/fight.png"
RUN_RC=$?
[ "$RUN_RC" = 2 ] && { echo "RESULT: FAIL (run.sh setup)"; exit 2; }

# The picture mid-fight: a second run, stopped at 20 s (the first hits come within seconds, the wipe
# at about 35 s). Its own log and picture; the check is the main run's.
OPENMW_SMOKE_SCRATCH="$WORK" "$SMOKE/run.sh" "$WORK/out" --name mid --seconds 20 \
    --extra-data "$HERE" --content m8_test.omwscripts --shot "$WORK/mid.png" > "$WORK/mid.log" 2>&1

LOG="$WORK/run/openmw.log"
FAILS=()
for kind in melee shot strike; do
    grep -q "RAZDOR_FIGHT hit from=.* kind=$kind amount=" "$LOG" 2>/dev/null || FAILS+=("no $kind hit in the log")
done
END=$(grep -o 'RAZDOR_FIGHT_END winner=[0-9]* reason=wipe t=[0-9]*' "$LOG" 2>/dev/null | tail -1)
START_T=$(grep -o 'RAZDOR_FIGHT player hp=[0-9-]* t=[0-9]*' "$LOG" 2>/dev/null | head -1 | sed 's/.* t=//')
if [ -z "$END" ]; then
    FAILS+=("no RAZDOR_FIGHT_END")
else
    END_T=${END##*t=}
    ELAPSED=$(( END_T - ${START_T:-$END_T} ))
    [ "$ELAPSED" -le 120 ] || FAILS+=("the fight ended after $ELAPSED s")
fi

echo "run dir:    $WORK/run"
echo "fight:      ${END:-no RAZDOR_FIGHT_END}"
echo "hits:       melee=$(grep -c 'kind=melee amount=' "$LOG" 2>/dev/null) shot=$(grep -c 'kind=shot amount=' "$LOG" 2>/dev/null) strike=$(grep -c 'kind=strike amount=' "$LOG" 2>/dev/null) heal=$(grep -c 'kind=heal amount=' "$LOG" 2>/dev/null) bless=$(grep -c 'kind=bless amount=' "$LOG" 2>/dev/null)"
echo "deaths:     $(grep -c 'RAZDOR_FIGHT_DEAD' "$LOG" 2>/dev/null)"
echo "picture:    $WORK/fight.png (the end) and $WORK/mid.png (20 s, mid-fight)"
if [ "${#FAILS[@]}" -eq 0 ]; then
    echo "RESULT: PASS"
    exit 0
fi
for f in "${FAILS[@]}"; do echo "  reason: $f"; done
echo "RESULT: FAIL"
exit 1
