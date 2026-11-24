#!/bin/sh
# Monthly update of the country table (DB-IP IP to Country Lite, CC BY 4.0, "IP Geolocation by DB-IP",
# https://db-ip.com). Run as root by razdor-geo-update.timer. It fetches the month's CSV, checks it,
# writes the API's copy (read at the API's start) and nginx's `geo` include, reloads nginx and restarts the API.
# Nothing is committed to git and nothing is sent anywhere: the table is only downloaded.
set -eu

API_DATA=/var/lib/razdor-api/geo
NGINX_GEO=/etc/nginx/razdor-geo-country.conf
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

install -d -o 10001 -g 10001 -m 0750 "$API_DATA"
install -o 10001 -g 10001 -m 0640 "$T/table.csv" "$API_DATA/dbip-country-lite.csv"

# Keep the old include so a failed test puts it back.
[ -f "$NGINX_GEO" ] && cp -p "$NGINX_GEO" "$T/old.conf"
install -m 0644 "$T/nginx.conf" "$NGINX_GEO"
if ! nginx -t; then
    [ -f "$T/old.conf" ] && install -m 0644 "$T/old.conf" "$NGINX_GEO"
    echo "geo: nginx refused the new table, the old one is back" >&2
    exit 1
fi
systemctl reload nginx
# The API reads its table when it starts.
systemctl restart razdor-api
echo "geo: updated for $MONTH"
