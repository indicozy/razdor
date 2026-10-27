#!/usr/bin/env bash
# Delegates a batch of wiki screenshots to the local Qwen (qwen3-coder:30b through Ollama).
#
#   tools/wiki/qwen-shots.sh LIST.txt
#
# LIST.txt has one shot per line:  scene output-name [WxH] # what the picture must show
# (blank lines and lines starting with # are skipped). Qwen runs tools/wiki/shot.sh for each
# line into wiki/src/assets/shots/<output-name>.webp, checks the picture, and prints a table.
# The brief lives in a scratch folder under /mnt/ai_models/razdor-scratch/wiki/qwen/.
# QWEN_SHOTS_DRY_RUN=1 writes the brief and stops before Qwen starts.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
QWEN_ROOT=/mnt/ai_models/razdor-scratch/wiki/qwen
SHOTS_DIR="$ROOT/wiki/src/assets/shots"

if [[ $# -ne 1 || ! -f "$1" ]]; then
	echo "usage: $0 LIST.txt" >&2
	exit 2
fi
LIST="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"

if ! curl -s -m 5 -o /dev/null http://localhost:11434/api/tags; then
	echo "qwen-shots.sh: Ollama is not answering on localhost:11434; start it first" >&2
	exit 1
fi

KNOWN_SCENES="$("$ROOT/tools/wiki/shot.sh" --list)"
ROWS=""
COUNT=0
while IFS= read -r raw || [[ -n "$raw" ]]; do
	[[ -z "${raw//[[:space:]]/}" || "${raw#"${raw%%[![:space:]]*}"}" == \#* ]] && continue
	code="${raw%%#*}"
	comment=""
	[[ "$raw" == *"#"* ]] && comment="${raw#*#}"
	read -r scene name size extra <<< "$code" || true
	if [[ -z "${scene:-}" || -z "${name:-}" || -n "${extra:-}" ]]; then
		echo "qwen-shots.sh: bad line (want: scene output-name [WxH] # comment): $raw" >&2
		exit 2
	fi
	if [[ ! "$name" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
		echo "qwen-shots.sh: output name must be lowercase words with dashes: $name" >&2
		exit 2
	fi
	if ! grep -qx "${scene%%:*}" <<< "$KNOWN_SCENES"; then
		echo "qwen-shots.sh: '${scene%%:*}' is not a scene (see tools/wiki/shot.sh --list)" >&2
		exit 2
	fi
	size="${size:-1280x800}"
	comment="$(sed 's/^[[:space:]]*//; s/[[:space:]]*$//' <<< "$comment")"
	ROWS+="| $scene | $name | $size | $comment |"$'\n'
	COUNT=$((COUNT + 1))
done < "$LIST"

if [[ "$COUNT" -eq 0 ]]; then
	echo "qwen-shots.sh: no shots in $LIST" >&2
	exit 2
fi

mkdir -p "$QWEN_ROOT" "$SHOTS_DIR"
RUN="$(mktemp -d "$QWEN_ROOT/run.XXXXXX")"
BRIEF="$RUN/brief.md"

cat > "$BRIEF" <<BRIEF
# Brief: wiki screenshots

You take screenshots of Razdor for its player wiki and check that each one shows what it must.
Work only with the commands below. Do not edit any file in the repository.

Repository (worktree): $ROOT
Screenshot script: $ROOT/tools/wiki/shot.sh
Output folder: $SHOTS_DIR

## Shots

For each row, in order:

1. Run the script from the repository root:
   \`$ROOT/tools/wiki/shot.sh <scene> $SHOTS_DIR/<output-name>.webp <size>\`
   (use the scene, output-name and size as written in the row).
2. Open the picture and check it against the comment (what it must show).
3. If the picture does not show what the comment says, look for a better scene with
   \`$ROOT/tools/wiki/shot.sh --list\` (the scene names; parameters such as \`load:manual\`
   are written after a colon, e.g. \`map:<map>\`; a map name needs the install's map, so
   prefer scenes without one when you can), run the script again with it into the same output
   name, and check again. Do not try more than three scenes for one row.
4. If you cannot open images in this session, say so and mark the row as unchecked.

Write nothing anywhere except the files that the script writes in the output folder above.
Do not run any other command than the screenshot script and \`--list\`.

| scene | output-name | size | must show |
|---|---|---|---|
$ROWS
## Report

Finish with a Markdown table, one line per shot, and nothing after it:

| name | scene used | ok / problem |
|---|---|---|

"ok" when the picture shows what the comment says; "problem" with a few words when not;
"unchecked" when you could not look at it.
BRIEF

echo "brief: $BRIEF" >&2
if [[ -n "${QWEN_SHOTS_DRY_RUN:-}" ]]; then
	echo "dry run: Qwen not started" >&2
	exit 0
fi
cd "$RUN"
qwen --auth-type openai --openai-base-url http://localhost:11434/v1 --openai-api-key ollama \
	-m qwen3-coder:30b --add-dir "$ROOT" -p "$(cat "$BRIEF")" --yolo < /dev/null
