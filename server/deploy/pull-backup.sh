#!/usr/bin/env bash
# Off-host copy of the API backups, run on the owner's PC (not on the VPS). It pulls the newest dated
# folders from the VPS's backup folder (backup.sh writes them) over ssh with rsync into a dated folder here.
# A pull goes into a temporary folder and is renamed only when rsync succeeded, so an interrupted pull
# is retried by the next run. Unchanged files are hard links to the previous pull. Keeps 30 days.
#
# Set up once: an ssh key that can read the backup folder on the VPS (read-only use is enough).
#
#   RAZDOR_VPS            ssh target, e.g. backup@vps.example (required)
#   RAZDOR_REMOTE_BACKUP  the VPS folder (default /var/backups/razdor-api)
#   RAZDOR_PULL_DEST      where the copies go (default /mnt/ai_models/razdor-backups)
#   RAZDOR_PULL_KEEP_DAYS how many days to keep (default 30)
#
# Run it from cron or a systemd user timer, e.g. daily at 06:30.
# The copies hold the account key hashes and the saves: the folder is private (mode 0700).
set -euo pipefail
umask 077

VPS="${RAZDOR_VPS:?set RAZDOR_VPS to the ssh target of the VPS}"
REMOTE="${RAZDOR_REMOTE_BACKUP:-/var/backups/razdor-api}"
DEST="${RAZDOR_PULL_DEST:-/mnt/ai_models/razdor-backups}"
KEEP_DAYS="${RAZDOR_PULL_KEEP_DAYS:-30}"

command -v rsync >/dev/null || { echo "pull-backup: rsync is needed" >&2; exit 1; }
mkdir -p "$DEST"
chmod 0700 "$DEST"

# The newest dated folder on the VPS.
newest="$(ssh -o BatchMode=yes "$VPS" "find '$REMOTE' -mindepth 1 -maxdepth 1 -type d -name '????-??-??' | sort | tail -n 1")"
[ -n "$newest" ] || { echo "pull-backup: no dated backup on $VPS" >&2; exit 1; }
stamp="$(basename "$newest")"
target="$DEST/$stamp"
if [ -e "$target" ]; then
  echo "pull-backup: $target exists, nothing to do" >&2
  exit 0
fi

work="$DEST/.tmp-$stamp-$$"
rm -rf "$work"
trap 'rm -rf "$work"' EXIT
mkdir "$work"

# The day before, for the hard links (empty on the first pull).
previous="$(find "$DEST" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' ! -name "$stamp" | sort | tail -n 1 || true)"
link=()
if [ -n "$previous" ]; then
  link=(--link-dest="$previous")
fi

rsync -a --exclude='*.part' --exclude='.tmp-*' "${link[@]}" "$VPS:$newest/" "$work/"

mv "$work" "$target"
trap - EXIT

find "$DEST" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' | sort | head -n -"$KEEP_DAYS" | while read -r old; do
  rm -rf "$old"
done

echo "pull-backup: $target written"
