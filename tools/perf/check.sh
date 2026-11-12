#!/bin/bash
# The Razdor performance check on the RTX 4060, headless: any time, not only at a release.
#
#   tools/perf/check.sh                      build this checkout's release (pinned), bench it on the
#                                            RTX 4060 (about 8 minutes), compare with the newest
#                                            tools/perf/baselines/v*-gpu-4060.json, print the table
#   tools/perf/check.sh --bin <razdor>       skip the build, measure this binary
#   tools/perf/check.sh --against <file|v>   compare with this baseline file, or with the version's
#                                            tools/perf/baselines/<v>-gpu-4060.json
#   tools/perf/check.sh --quick              one run per cell (a smoke check, about 2.5 minutes)
#
# Results go to $RAZDOR_PERF_SCRATCH/<commit>-<time>/ (default /mnt/ai_models/razdor-scratch/perf-check):
# the build target, the result JSON and the Markdown tables. Nothing is written into the repo.
#
# Exit: the compare.py code when the bench ran (0 no regression, 1 a regression or a cell not measured,
# 2 not comparable or no baseline); 3 the GPU is not the RTX 4060 or not NVIDIA (no run made);
# 4 the build failed; 5 usage.
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SCRATCH=${RAZDOR_PERF_SCRATCH:-/mnt/ai_models/razdor-scratch/perf-check}
BIN=""
AGAINST=""
RUNS_ARG=""

usage() { sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
    case "$1" in
        --bin) BIN=${2:-}; shift 2 ;;
        --against) AGAINST=${2:-}; shift 2 ;;
        --quick) RUNS_ARG="--runs 1"; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "check.sh: unknown argument: $1" >&2; usage >&2; exit 5 ;;
    esac
done

COMMIT=$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)
if ! git -C "$ROOT" diff --quiet HEAD -- 2>/dev/null; then
    COMMIT="$COMMIT-dirty"
fi
OUT="$SCRATCH/$COMMIT-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$OUT" || { echo "check.sh: cannot create $OUT" >&2; exit 5; }

# 1. The binary: built here (pinned, the desktop keeps CPUs 0-7) unless --bin.
if [ -z "$BIN" ]; then
    echo "check.sh: building the release of $COMMIT into $SCRATCH/target" >&2
    (cd "$ROOT" && CARGO_TARGET_DIR="$SCRATCH/target" CARGO_BUILD_JOBS=12 \
        nice -n 19 taskset -c 8-19 cargo build --release) > "$OUT/build.log" 2>&1 || {
        echo "check.sh: the build failed (see $OUT/build.log)" >&2
        exit 4
    }
    BIN="$SCRATCH/target/release/razdor"
fi
if [ ! -x "$BIN" ]; then
    echo "check.sh: not an executable: $BIN" >&2
    exit 5
fi

# 2. The baseline to compare with.
BASE=""
if [ -n "$AGAINST" ]; then
    if [ -f "$AGAINST" ]; then
        BASE=$AGAINST
    elif [ -f "$ROOT/tools/perf/baselines/$AGAINST-gpu-4060.json" ]; then
        BASE="$ROOT/tools/perf/baselines/$AGAINST-gpu-4060.json"
    else
        echo "check.sh: no baseline $AGAINST (a file, or tools/perf/baselines/$AGAINST-gpu-4060.json)" >&2
        exit 2
    fi
else
    NEWEST=$(cd "$ROOT/tools/perf/baselines" && ls v*-gpu-4060.json 2>/dev/null | sort -V | tail -1)
    if [ -z "$NEWEST" ]; then
        echo "check.sh: no tools/perf/baselines/v*-gpu-4060.json to compare with (pass --against)" >&2
        exit 2
    fi
    BASE="$ROOT/tools/perf/baselines/$NEWEST"
fi
echo "check.sh: comparing with $(basename "$BASE")" >&2

# 3. The bench on the RTX 4060 (it checks the card and the renderer first).
LABEL="check-$COMMIT"
nice -n 19 taskset -c 8-19 python3 -I -B "$ROOT/tools/perf/bench.py" --bin "$BIN" --gpu-4060 \
    --commit "$COMMIT" --label "$LABEL" --machine "RTX 4060 headless (VirtualGL, private Xvfb)" \
    --work "$OUT/work" --out "$OUT/result.json" --md "$OUT/result.md" $RUNS_ARG
RC=$?
if [ "$RC" -ge 2 ]; then
    echo "check.sh: the bench stopped with code $RC (no comparison made)" >&2
    exit "$RC"
fi
if [ "$RC" -eq 1 ]; then
    echo "check.sh: some runs failed (see the FAILED lines below); the comparison counts them as not measured" >&2
fi

# 4. The comparison: the table on stdout, the Markdown next to the result.
python3 -I -B "$ROOT/tools/perf/compare.py" "$OUT/result.json" "$BASE" --md "$OUT/compare.md"
CRC=$?
echo "check.sh: results in $OUT" >&2
exit "$CRC"
