#!/bin/sh
# Monthly update of the country table (DB-IP IP to Country Lite, CC BY 4.0, "IP Geolocation by DB-IP",
# https://db-ip.com). Run by razdor-geo-update.timer as the unprivileged user razdor-geo (User= in the
# service), never as root. It fetches the month's CSV, checks it, and writes two files into razdor-geo's
# own folder: the CSV and nginx's `geo` ranges. Then razdor-geo-apply (root, ExecStartPost=+ in the service)
# checks the copies and installs them; that root step also runs nginx -t, reloads nginx and the API restarts.
# Nothing is committed to git and nothing is sent anywhere: the table is only downloaded.
set -eu
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

OUT=/var/lib/razdor-geo
# The root-owned copy of the server binary (installed by the root setup). Run here as razdor-geo, never by root.
BIN=/usr/local/bin/razdor-server
MONTH=$(date -u +%Y-%m)
URL="https://download.db-ip.com/free/dbip-country-lite-${MONTH}.csv.gz"

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

curl -fsSL --max-time 900 -o "$T/table.csv.gz" "$URL"
gzip -dc "$T/table.csv.gz" > "$T/table.csv"
# razdor-server refuses a file with no country ranges; the line count guards against a truncated table.
"$BIN" geo-nginx "$T/table.csv" > "$T/nginx.conf"
if [ "$(wc -l < "$T/nginx.conf")" -lt 1000 ]; then
    echo "geo: the table has too few ranges, nothing changed" >&2
    exit 1
fi

install -m 0644 "$T/table.csv" "$OUT/dbip-country-lite.csv"
install -m 0644 "$T/nginx.conf" "$OUT/razdor-geo-country.conf"
echo "geo: downloaded for $MONTH, waiting for razdor-geo-apply to install it"
