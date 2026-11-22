#!/usr/bin/env bash
# Daily backup of the API's data: a consistent copy of each SQLite file (`sqlite3 .backup`, safe
# while the service runs) and the battle, crash and save folders (rsync; unchanged files are hard
# links to the previous day). Keeps 14 days. Run it from a systemd timer or cron as the service's
# user or root. Backups hold account key hashes and saves: the folder is private (mode 0700).
#
#   RAZDOR_DATA_DIR      the service's data folder (default /var/lib/razdor-api)
#   RAZDOR_BACKUP_DIR    where the copies go (default /var/backups/razdor-api)
#   RAZDOR_BACKUP_KEEP_DAYS  how many days to keep (default 14)
set -euo pipefail
umask 077

DATA="${RAZDOR_DATA_DIR:-/var/lib/razdor-api}"
DEST="${RAZDOR_BACKUP_DIR:-/var/backups/razdor-api}"
KEEP_DAYS="${RAZDOR_BACKUP_KEEP_DAYS:-14}"

command -v sqlite3 >/dev/null || { echo "backup: sqlite3 is needed" >&2; exit 1; }
command -v rsync >/dev/null || { echo "backup: rsync is needed" >&2; exit 1; }

mkdir -p "$DEST"
chmod 0700 "$DEST"
stamp="$(date +%F)"
target="$DEST/$stamp"
if [ -e "$target" ]; then
  echo "backup: $target exists, nothing to do" >&2
  exit 0
fi
mkdir -p "$target"

# The day before, for the hard links (empty on the first run).
previous="$(find "$DEST" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' ! -name "$stamp" | sort | tail -n 1 || true)"

for db in stats cloud; do
  if [ -f "$DATA/$db.sqlite" ]; then
    sqlite3 "$DATA/$db.sqlite" ".backup '$target/$db.sqlite'"
  fi
done

for folder in battles crashes saves; do
  [ -d "$DATA/$folder" ] || continue
  link=()
  if [ -n "$previous" ] && [ -d "$previous/$folder" ]; then
    link=(--link-dest="$previous/$folder")
  fi
  rsync -a --exclude='*.part' "${link[@]}" "$DATA/$folder/" "$target/$folder/"
done

# Keep the newest KEEP_DAYS copies; older ones go.
find "$DEST" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' -mtime +"$KEEP_DAYS" -exec rm -rf {} +

echo "backup: $target written"
