#!/usr/bin/env bash
# One command from a Discord Times map to an OpenMW world (spec docs/specs/2026-11-27-openmw-world.md,
# M6): .DTm -> JSON (razdor-dtm-json) -> OpenMW plugin, meshes and Lua (razdor-openmw) -> the
# standalone config -> an off-screen OpenMW run with a picture, PASS or FAIL.
#
#   play.sh <map.DTm | map.json> [--name <name>] [--archetype knight|archmage|ranger]
#           [--units <units.json>] [--shot <png>] [--camera x,y,z,pitch,yaw] [--probe x,y]... [--window]
#
#   --name       the run's folder under /mnt/ai_models/razdor-scratch/openmw/play/ (it must not exist).
#                Default: play-<map name>-<time>.
#   --archetype  the hero the converter plays (default: the map's first hero).
#   --units      the install's units JSON (razdor-units-json) for the real army stats and names. With a
#                .DTm and no --units, the units are exported from RAZDOR_DT_DIR (set in .env) when it is
#                set; without either, the converter keeps its placeholder stats.
#   --shot       the picture (default: <run folder>/shot.png).
#   --camera     the picture's camera (default: two tiles north-west of the hero's start, looking at
#                the first building; see view.py).
#   --probe      a world point x,y (units) the player is teleported to on the ground, up to four, one
#                after another (run.sh --probe; each reading is a RAZDOR_PROBE line). Facing is not
#                passed: run.sh has no option for it.
#   --window     do not run anything: write the standalone config and print the command that starts
#                OpenMW in a real window on the desktop, for the owner to play by hand.
#
# Everything a run writes goes into its folder (the JSON, the converter's output, the config, the
# OpenMW log and the picture). The folder is never reused. Exit 0 = PASS, 1 = FAIL (from run.sh),
# 2 = bad usage or a failed build or conversion.
#
# Build cache: CARGO_TARGET_DIR (default /mnt/ai_models/razdor-scratch/openmw/target). The machine's
# prefix (ai-resources prefix) pins heavy work; the converter's Template is OPENMW_EXAMPLE.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
ROOT=/mnt/ai_models/razdor-scratch/openmw
EXAMPLE=${OPENMW_EXAMPLE:-$ROOT/example-suite}
TEMPLATE=$EXAMPLE/game_template/data/template.omwgame
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}

INPUT=""
NAME=""
ARCH=""
UNITS=""
SHOT=""
CAMERA=""
PROBES=()
WINDOW=0

usage() { sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }

[ $# -ge 1 ] || usage
INPUT=$1
shift
while [ $# -gt 0 ]; do
    case "$1" in
        --name) NAME=$2; shift 2 ;;
        --archetype) ARCH=$2; shift 2 ;;
        --units) UNITS=$2; shift 2 ;;
        --shot) SHOT=$2; shift 2 ;;
        --camera) CAMERA=$2; shift 2 ;;
        --probe) PROBES+=("$2"); shift 2 ;;
        --window) WINDOW=1; shift ;;
        -h|--help) usage ;;
        *) echo "play.sh: unknown argument $1" >&2; usage ;;
    esac
done

[ -f "$INPUT" ] || { echo "play.sh: $INPUT is not a file" >&2; exit 2; }
[ -f "$TEMPLATE" ] || { echo "play.sh: the Game Template is missing at $EXAMPLE (OPENMW_EXAMPLE)" >&2; exit 2; }
case "$INPUT" in
    *.DTm|*.dtm) KIND=dtm ;;
    *.json) KIND=json ;;
    *) echo "play.sh: the map must be a .DTm or a .json" >&2; exit 2 ;;
esac

if [ -z "$NAME" ]; then
    BASE=$(basename "$INPUT")
    BASE=${BASE%.*}
    NAME="play-$(printf '%s' "$BASE" | tr -c 'A-Za-z0-9_-' '_')-$(date +%Y%m%d-%H%M%S)"
fi
WORK="$ROOT/play/$NAME"
[ -e "$WORK" ] && { echo "play.sh: $WORK exists, pick another --name" >&2; exit 2; }
mkdir -p "$WORK" || { echo "play.sh: cannot create $WORK" >&2; exit 2; }
echo "work folder: $WORK"

PREFIX=$(ai-resources prefix)
START_NS=$(date +%s)

# --- 1. the map as JSON (razdor-dtm-json reads only the .DTm file) ---
if [ "$KIND" = dtm ]; then
    ( cd "$REPO" && CARGO_BUILD_JOBS=12 $PREFIX cargo build --profile fasttest --bin razdor-dtm-json --bin razdor-units-json ) \
        > "$WORK/build-dtm-json.log" 2>&1 || { echo "play.sh: building the JSON exporters failed (see $WORK/build-dtm-json.log)" >&2; exit 2; }
    $PREFIX "$CARGO_TARGET_DIR/fasttest/razdor-dtm-json" "$INPUT" -o "$WORK/map.json" \
        || { echo "play.sh: razdor-dtm-json could not read $INPUT" >&2; exit 2; }
    # The units of the install (game data, stays in the work folder): exported when RAZDOR_DT_DIR is set.
    if [ -z "$UNITS" ] && [ -n "${RAZDOR_DT_DIR:-}" ]; then
        ( cd "$REPO" && $PREFIX "$CARGO_TARGET_DIR/fasttest/razdor-units-json" -o "$WORK/units.json" ) \
            || { echo "play.sh: razdor-units-json could not export the units (RAZDOR_DT_DIR)" >&2; exit 2; }
        UNITS="$WORK/units.json"
    fi
else
    cp "$INPUT" "$WORK/map.json" || { echo "play.sh: cannot copy $INPUT" >&2; exit 2; }
fi

# --- 2. the converter: JSON -> plugin, textures, meshes, Lua ---
( cd "$REPO" && CARGO_BUILD_JOBS=12 $PREFIX cargo build --profile fasttest -p razdor-openmw --bin razdor-openmw ) \
    > "$WORK/build-openmw.log" 2>&1 || { echo "play.sh: building razdor-openmw failed (see $WORK/build-openmw.log)" >&2; exit 2; }
CONVERT=("$CARGO_TARGET_DIR/fasttest/razdor-openmw" "$WORK/map.json" -o "$WORK/out" --template "$TEMPLATE")
[ -n "$ARCH" ] && CONVERT+=(--archetype "$ARCH")
if [ -n "$UNITS" ]; then
    [ -f "$UNITS" ] || { echo "play.sh: $UNITS is not a file" >&2; exit 2; }
    CONVERT+=(--units "$UNITS")
    echo "units:       $UNITS"
fi
$PREFIX "${CONVERT[@]}" > "$WORK/convert.log" 2>&1 || { echo "play.sh: the conversion failed (see $WORK/convert.log)" >&2; exit 2; }
echo "converted:   $WORK/out ($(ls "$WORK/out" | wc -l) entries)"

# --- 3a. a window: write the play config (no smoke scripts), print the command, run nothing ---
if [ "$WINDOW" = 1 ]; then
    OPENMW_SMOKE_SCRATCH="$WORK" "$HERE/run.sh" "$WORK/out" --name window --play-config || exit 2
    echo ""
    echo "Play it in a window on this desktop (this script did not start OpenMW):"
    echo "  openmw --replace=config --config $WORK/window --skip-menu --start 0,0"
    echo "The config folder is $WORK/window (its openmw.cfg and settings.cfg)."
    exit 0
fi

# --- 3b. the smoke run: off-screen, one picture, PASS or FAIL ---
if [ -z "$CAMERA" ]; then
    CAMERA=$(python3 -I "$HERE/view.py" "$WORK/map.json") || { echo "play.sh: no default camera" >&2; exit 2; }
fi
[ -n "$SHOT" ] || SHOT="$WORK/shot.png"
# m5_auto answers the map's windows (the start message) so the picture is not covered by them.
PROBE_ARGS=()
for p in "${PROBES[@]+"${PROBES[@]}"}"; do PROBE_ARGS+=(--probe "$p"); done
OPENMW_SMOKE_SCRATCH="$WORK" "$HERE/run.sh" "$WORK/out" --name smoke --camera "$CAMERA" --shot "$SHOT" \
    --extra-data "$HERE/m5" --content m5_auto.omwscripts ${PROBE_ARGS[@]+"${PROBE_ARGS[@]}"}
RC=$?
echo "camera:      $CAMERA"
echo "total:       $(( $(date +%s) - START_NS )) s"
exit "$RC"
