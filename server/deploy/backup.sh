#!/usr/bin/env bash
# Daily backup of the API's data: a consistent copy of each SQLite file (`sqlite3 .backup`, safe
# while the service runs) and the battle, crash and save folders (rsync; unchanged files are hard
# links to the previous day). Keeps 14 days. Run it from a systemd timer or cron as the service's
# user or root. Backups hold account key hashes and saves: the folder is private (mode 0700).
#
# A run writes into a temporary folder (`.tmp-<date>-<pid>`) and renames it to the dated folder only
# when every step succeeded. A failed run leaves no dated folder, so the next run tries again. The
# link-code table (`links`) is emptied in the copy: a link code's key must not sit in a backup.
# `ip_salt` (the salt of the hashed IPs in the bans) is copied too, so the bans still match after a
# restore.
#
# The SQLite steps use the sqlite3 CLI when it is installed. Without it they fall back to python3
# (`python3 -I`, the sqlite3 module: `backup()` for the copy, then DELETE + VACUUM for the links).
# Both give the same files; python3 is needed only when sqlite3 is missing.
#
# The API runs in a container (razdor-api.container) whose data folder is the host folder
# /var/lib/razdor-api (a bind mount), so this script reads the live files on the host. It needs the
# host's rsync package and either sqlite3 or python3. Copy the folder off the host with pull-backup.sh
# (on the PC). The restore steps are in server/deploy/README.md (section Restore).
#
#   RAZDOR_DATA_DIR      the service's data folder (default /var/lib/razdor-api)
#   RAZDOR_BACKUP_DIR    where the copies go (default /var/backups/razdor-api)
#   RAZDOR_BACKUP_KEEP_DAYS  how many days to keep (default 14)
set -euo pipefail
umask 077

DATA="${RAZDOR_DATA_DIR:-/var/lib/razdor-api}"
DEST="${RAZDOR_BACKUP_DIR:-/var/backups/razdor-api}"
KEEP_DAYS="${RAZDOR_BACKUP_KEEP_DAYS:-14}"

command -v rsync >/dev/null || { echo "backup: rsync is needed" >&2; exit 1; }
if command -v sqlite3 >/dev/null; then
  USE_SQLITE3=1
elif command -v python3 >/dev/null; then
  USE_SQLITE3=0
else
  echo "backup: sqlite3 or python3 is needed" >&2
  exit 1
fi

# sqlite_copy SRC DST: a consistent copy of the database SRC (safe while the service writes).
sqlite_copy() {
  if [ "$USE_SQLITE3" = 1 ]; then
    sqlite3 "$1" ".backup '$2'"
  else
    python3 -I -c '
import sqlite3, sys
src = sqlite3.connect(sys.argv[1])
dst = sqlite3.connect(sys.argv[2])
src.backup(dst)
dst.close()
src.close()
' "$1" "$2"
  fi
}

# sqlite_clear_links FILE: deletes the link codes in the copy FILE, then rewrites the file (VACUUM).
sqlite_clear_links() {
  if [ "$USE_SQLITE3" = 1 ]; then
    sqlite3 "$1" "DELETE FROM links; VACUUM;"
  else
    python3 -I -c '
import sqlite3, sys
con = sqlite3.connect(sys.argv[1], isolation_level=None)
con.execute("DELETE FROM links")
con.execute("VACUUM")
con.close()
' "$1"
  fi
}

mkdir -p "$DEST"
chmod 0700 "$DEST"
stamp="$(date +%F)"
target="$DEST/$stamp"
if [ -e "$target" ]; then
  echo "backup: $target exists, nothing to do" >&2
  exit 0
fi
work="$DEST/.tmp-$stamp-$$"
rm -rf "$work"
mkdir "$work"
# A failed run removes its half copy; the dated folder never appears for it.
trap 'rm -rf "$work"' EXIT

# The day before, for the hard links (empty on the first run).
previous="$(find "$DEST" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' ! -name "$stamp" | sort | tail -n 1 || true)"

for db in stats cloud; do
  if [ -f "$DATA/$db.sqlite" ]; then
    sqlite_copy "$DATA/$db.sqlite" "$work/$db.sqlite"
  fi
done

# Link codes are short-lived secrets: drop them from the copy, then rewrite the file so the old pages go.
if [ -f "$work/cloud.sqlite" ]; then
  sqlite_clear_links "$work/cloud.sqlite"
fi

# The salt of the hashed IPs (the bans): restore needs it to keep the bans matching.
if [ -f "$DATA/ip_salt" ]; then
  cp "$DATA/ip_salt" "$work/ip_salt"
fi

for folder in battles crashes saves; do
  [ -d "$DATA/$folder" ] || continue
  link=()
  if [ -n "$previous" ] && [ -d "$previous/$folder" ]; then
    link=(--link-dest="$previous/$folder")
  fi
  rsync -a --exclude='*.part' "${link[@]}" "$DATA/$folder/" "$work/$folder/"
done

# The copy is complete: publish it under its date in one rename.
mv "$work" "$target"
trap - EXIT

# Keep the newest KEEP_DAYS dated copies; older ones go (by name: the names sort by date).
find "$DEST" -mindepth 1 -maxdepth 1 -type d -name '????-??-??' | sort | head -n -"$KEEP_DAYS" | while read -r old; do
  rm -rf "$old"
done

echo "backup: $target written"
