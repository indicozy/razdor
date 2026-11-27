#!/usr/bin/env bash
# Off-screen OpenMW smoke test for the converter's output (spec docs/specs/2026-11-27-openmw-world.md, 5).
# Test only: not part of cargo test. Everything it writes goes under the scratch folder.
#
#   run.sh <converter out dir> [--name <run name>] [--cell cx,cy] [--seconds N] [--shot <png>]
#          [--probe x,y]... [--camera x,y,z,pitch,yaw] [--extra-data <dir>]...
#
#   --cell     exterior cell the game starts in (openmw --start). Default 0,0.
#   --seconds  when the RAZDOR_SMOKE line is printed (after the start). Default 10.
#   --probe    world point (units) the player is teleported to, one after another, from second 1
#              (2 s each); the z read after each is printed as RAZDOR_PROBE. Up to four.
#   --camera   static camera position and angles (radians) for the picture; default: the player's view.
#   --shot     where the PNG goes. Default: <run dir>/shot.png.
#   --content     an .omwscripts name, found in an --extra-data folder, loaded as content (test drivers).
#   --cyrillic-test  show the Russian test message (off by default).
#   --play-config    write the standalone config for a window (no smoke scripts, no run), print it, exit 0.
#   --extra-data  a folder added to the data path (e.g. the Template's example_animated_creature/data
#              for meshes/land_racer.dae). Repeatable.
#
# Exit 0 = PASS, 1 = FAIL, 2 = bad usage or setup. Override the Template folder with OPENMW_EXAMPLE.
set -u

HERE=$(cd "$(dirname "$0")" && pwd)
SCRATCH=${OPENMW_SMOKE_SCRATCH:-/mnt/ai_models/razdor-scratch/openmw/smoke-run}
EXAMPLE=${OPENMW_EXAMPLE:-/mnt/ai_models/razdor-scratch/openmw/example-suite}
RESOURCES=/usr/share/openmw/resources

OUT=""
NAME=""
CELL="0,0"
SECS=10
SHOT=""
CAMERA=""
PROBES=()
EXTRA_DATA=()
CONTENT=()
CYRILLIC=0
PLAY_CONFIG=0

usage() { sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'; exit 2; }

[ $# -ge 1 ] || usage
OUT=$1
shift
while [ $# -gt 0 ]; do
    case "$1" in
        --name) NAME=$2; shift 2 ;;
        --cell) CELL=$2; shift 2 ;;
        --seconds) SECS=$2; shift 2 ;;
        --shot) SHOT=$2; shift 2 ;;
        --camera) CAMERA=$2; shift 2 ;;
        --probe) PROBES+=("$2"); shift 2 ;;
        --extra-data) EXTRA_DATA+=("$2"); shift 2 ;;
        --content) CONTENT+=("$2"); shift 2 ;;
        --cyrillic-test) CYRILLIC=1; shift ;;
        --play-config) PLAY_CONFIG=1; shift ;;
        -h|--help) usage ;;
        *) echo "run.sh: unknown argument $1" >&2; usage ;;
    esac
done

OUT=$(cd "$OUT" 2>/dev/null && pwd) || { echo "run.sh: no converter out dir" >&2; exit 2; }
[ -f "$OUT/razdor_map.omwaddon" ] || { echo "run.sh: $OUT/razdor_map.omwaddon missing" >&2; exit 2; }
[ -f "$EXAMPLE/game_template/data/template.omwgame" ] || { echo "run.sh: Game Template missing at $EXAMPLE" >&2; exit 2; }
[ "${#PROBES[@]}" -le 4 ] || { echo "run.sh: at most four --probe points" >&2; exit 2; }

RUN="$SCRATCH/${NAME:-$(date +%Y%m%d-%H%M%S)-$$}"
[ -e "$RUN" ] && { echo "run.sh: $RUN exists, pick another --name" >&2; exit 2; }
mkdir -p "$RUN/userdata" "$RUN/xdg" "$RUN/smokedata/scripts/razdor_smoke"
chmod 700 "$RUN/xdg"
[ -n "$SHOT" ] || SHOT="$RUN/shot.png"
PREFIX=$(ai-resources prefix)
START_NS=$(date +%s)

# --- standalone config (spec 2.4): its own folder, the owner's OpenMW setup is not touched ---
cat > "$RUN/openmw.cfg" <<EOF
replace=config
replace=data
replace=content
resources=$RESOURCES
user-data=$RUN/userdata
data=$RESOURCES/vfs
data=$EXAMPLE/game_template/data
data=$OUT
EOF
# The smoke scripts are test-only: a play config (--play-config) has none of them.
if [ "$PLAY_CONFIG" = 0 ]; then
    printf 'data=%s\ndata=%s\n' "$HERE/smoke" "$RUN/smokedata" >> "$RUN/openmw.cfg"
fi
for d in "${EXTRA_DATA[@]+"${EXTRA_DATA[@]}"}"; do
    echo "data=$(cd "$d" && pwd)" >> "$RUN/openmw.cfg"
done
# OpenMW 0.51 loads builtin.omwscripts itself; listing it again aborts ("specified more than once").
{
    echo "content=template.omwgame"
    echo "content=razdor_map.omwaddon"
    # The converter's Lua runtime (M4+): only when the converter wrote it.
    [ -f "$OUT/razdor_map.omwscripts" ] && echo "content=razdor_map.omwscripts"
    [ "$PLAY_CONFIG" = 0 ] && echo "content=smoke.omwscripts"
    for c in "${CONTENT[@]+"${CONTENT[@]}"}"; do echo "content=$c"; done
    echo "encoding=win1251"
} >> "$RUN/openmw.cfg"

# The Template's own settings (its [Models] and [Game] lines), plus a small windowed screen.
cp "$EXAMPLE/settings.cfg" "$RUN/settings.cfg"
cat >> "$RUN/settings.cfg" <<'EOF'

[Video]
resolution x = 1280
resolution y = 720
fullscreen = false
EOF
# A wider view distance for the smoke pictures only (the fog hides the scenery at the default);
# the play config keeps the game's own setting.
if [ "$PLAY_CONFIG" = 0 ]; then
    printf '\n[Camera]\nviewing distance = 12000\n' >> "$RUN/settings.cfg"
fi

# --- test-only Lua config (read by smoke/scripts/razdor_smoke/player.lua) ---
LUA_PROBES=""
for p in "${PROBES[@]+"${PROBES[@]}"}"; do
    LUA_PROBES="$LUA_PROBES{${p%%,*},${p#*,}},"
done
LUA_CAMERA="nil"
LUA_CYRILLIC=false
[ "$CYRILLIC" = 1 ] && LUA_CYRILLIC=true
if [ -n "$CAMERA" ]; then
    IFS=, read -r cx cy cz cp cyw <<< "$CAMERA"
    LUA_CAMERA="{x=$cx,y=$cy,z=$cz,pitch=$cp,yaw=$cyw}"
fi
cat > "$RUN/smokedata/scripts/razdor_smoke/config.lua" <<EOF
-- Written by run.sh for this run only.
return {
    seconds = $SECS,
    quitAfter = 5,
    probes = { $LUA_PROBES },
    camera = $LUA_CAMERA,
    cyrillic = $LUA_CYRILLIC,
}
EOF

# --- play config: written, printed, not run (play.sh --window starts it by hand) ---
if [ "$PLAY_CONFIG" = 1 ]; then
    echo "config:     $RUN"
    exit 0
fi

# --- a free display (never :0, the owner's desktop) ---
D=""
for n in $(seq 99 -1 90); do
    if [ ! -e "/tmp/.X${n}-lock" ] && [ ! -e "/tmp/.X11-unix/X${n}" ]; then D=$n; break; fi
done
[ -n "$D" ] || { echo "run.sh: no free display 90-99" >&2; exit 2; }

XPID=""; OPID=""
cleanup() {
    [ -n "$OPID" ] && kill -0 "$OPID" 2>/dev/null && kill "$OPID" 2>/dev/null
    sleep 1
    [ -n "$OPID" ] && kill -0 "$OPID" 2>/dev/null && kill -9 "$OPID" 2>/dev/null
    pkill -9 -f "openmw --replace=config --config $RUN" 2>/dev/null
    [ -n "$XPID" ] && kill "$XPID" 2>/dev/null
    sleep 1
    [ -n "$XPID" ] && kill -9 "$XPID" 2>/dev/null
    true
}
trap cleanup EXIT

XDG_RUNTIME_DIR="$RUN/xdg" $PREFIX Xvfb ":$D" -screen 0 1280x720x24 -nolisten tcp > "$RUN/xvfb.log" 2>&1 &
XPID=$!
for i in $(seq 1 40); do [ -S "/tmp/.X11-unix/X$D" ] && break; sleep 0.25; done
[ -S "/tmp/.X11-unix/X$D" ] || { echo "run.sh: Xvfb :$D did not start (see $RUN/xvfb.log)" >&2; exit 2; }

DISPLAY=":$D" XDG_RUNTIME_DIR="$RUN/xdg" LIBGL_ALWAYS_SOFTWARE=1 $PREFIX openmw \
    --replace=config --config "$RUN" --skip-menu --start "$CELL" --no-grab --no-sound \
    > "$RUN/openmw.stdout" 2>&1 &
OPID=$!

# --- wait (bounded) for the smoke line in the log ---
LOG=""
for i in $(seq 1 240); do
    LOG="$RUN/openmw.log"
    if [ -n "$LOG" ] && grep -q 'RAZDOR_SMOKE' "$LOG" 2>/dev/null; then break; fi
    if [ -z "$LOG" ] && grep -q 'RAZDOR_SMOKE' "$RUN/openmw.stdout" 2>/dev/null; then break; fi
    kill -0 "$OPID" 2>/dev/null || break
    sleep 1
done
sleep 1.5   # let the message box and the last frame draw
LINE_SEEN=0
grep -q 'RAZDOR_SMOKE' "$RUN/openmw.stdout" "$LOG" 2>/dev/null && LINE_SEEN=1

# --- picture of the whole screen, then the exit ---
SHOT_OK=0
if [ "$LINE_SEEN" = 1 ]; then
    if DISPLAY=":$D" import -window root "$SHOT" 2>"$RUN/import.log"; then SHOT_OK=1; fi
fi
for i in $(seq 1 60); do kill -0 "$OPID" 2>/dev/null || break; sleep 1; done
EXIT_CODE="running"
if kill -0 "$OPID" 2>/dev/null; then
    kill "$OPID" 2>/dev/null; sleep 2; kill -9 "$OPID" 2>/dev/null
    EXIT_CODE="killed (timeout)"
else
    wait "$OPID" 2>/dev/null; EXIT_CODE="exit $?"
fi
OPID=""

# --- checks ---
LOG="$RUN/openmw.log"
FAILS=()
[ "$LINE_SEEN" = 1 ] || FAILS+=("no RAZDOR_SMOKE line in the log")
if [ "$SHOT_OK" = 1 ]; then
    # the top 560 rows only: the message box and the HUD are drawn over the bottom
    SD=$(magick "$SHOT" -crop 1280x560+0+0 +repage -format '%[fx:standard_deviation]' info: 2>/dev/null || echo 0)
    if awk "BEGIN{exit !($SD < 0.005)}"; then FAILS+=("picture is one flat colour (sd $SD)"); fi
else
    FAILS+=("no picture")
fi
if [ -n "$LOG" ] && grep -i 'error' "$LOG" | grep -qi 'razdor'; then
    FAILS+=("log has an Error line naming razdor files")
fi
for f in "$RUN/openmw.stdout" "$LOG"; do
    [ -n "$f" ] && [ -f "$f" ] && grep -i 'error' "$f" | grep -i 'razdor' | head -3 | sed 's/^/  log: /'
done

echo "run dir:    $RUN"
echo "display:    :$D  (Xvfb pid $XPID, cleaned up on exit)"
echo "openmw:     $EXIT_CODE"
[ -n "$LOG" ] && echo "log:        $LOG"
# The converter's lines (RAZDOR_START, ARMY_*, CHECK*, GUARD, ...) come in log order; the
# harness's own lines are listed as before.
{ grep -h 'RAZDOR_\(START\|CLOCK\|ARMY_\|SHIP\|CHECK\|GUARD\|TILE\)' ${LOG:+"$LOG"} 2>/dev/null
  grep -h 'RAZDOR_SMOKE\|RAZDOR_PROBE' "$RUN/openmw.stdout" ${LOG:+"$LOG"} 2>/dev/null | sort -u
} | sed 's/^/  /'
[ "$SHOT_OK" = 1 ] && echo "picture:    $SHOT"
echo "time:       $(( $(date +%s) - START_NS )) s"
if [ "${#FAILS[@]}" -eq 0 ]; then
    echo "RESULT: PASS"
    exit 0
fi
for f in "${FAILS[@]}"; do echo "  reason: $f"; done
echo "RESULT: FAIL"
exit 1
