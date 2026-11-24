#!/bin/sh
# /usr/local/sbin/razdor-geo-apply (root:root 0755): the root step after the country table update
# (razdor-geo-update.service, ExecStartPost=+). razdor-geo-update runs as the unprivileged user razdor-geo
# and writes two files into its own folder, /var/lib/razdor-geo: the nginx ranges and the DB-IP CSV.
# This script runs as root, so it never uses those files in place. It copies each one without following
# links into a root-owned folder, checks the copy, and only then installs it:
#   - the CSV into the API's data folder (read by the API at its start, owned by the API's user 10001);
#   - the ranges into nginx's include /etc/nginx/razdor-geo-country.conf, after nginx -t passes with it.
# A failed test puts the old include back. Only then does nginx reload. The ranges must match the exact
# line form that razdor-server geo-nginx prints (a.b.c.d-a.b.c.d CC;): nothing else can reach nginx's config.
# Exit 1 on any refusal, with the reason on stderr.
umask 022
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH
LC_ALL=C
export LC_ALL

GEO_DIR="${RZ_GEO_DIR:-/var/lib/razdor-geo}"
API_DIR="${RZ_GEO_API_DIR:-/var/lib/razdor-api/geo}"
NGINX_CONF="${RZ_GEO_NGINX_CONF:-/etc/nginx/razdor-geo-country.conf}"
WORK_ROOT="${RZ_GEO_WORK:-/var/lib/razdor-install}"
MAX_CSV=268435456   # 256 MiB
MAX_CONF=67108864   # 64 MiB
MIN_RANGES=1000
RANGE_RE='^([0-9]{1,3}\.){3}[0-9]{1,3}-([0-9]{1,3}\.){3}[0-9]{1,3} [A-Z]{2};$'

die() {
    echo "razdor-geo-apply: refused: $*" >&2
    exit 1
}

[ ! -L "$GEO_DIR" ] && [ -d "$GEO_DIR" ] || die "$GEO_DIR is missing or a symlink"
[ -d "$WORK_ROOT" ] && [ ! -L "$WORK_ROOT" ] || die "$WORK_ROOT is missing"

WORK="$(mktemp -d "$WORK_ROOT/geo.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

# copy_checked NAME LIMIT DEST: copy GEO_DIR/NAME as a plain file, check the copy's size.
copy_checked() {
    [ -f "$GEO_DIR/$1" ] && [ ! -L "$GEO_DIR/$1" ] || die "$1 is not a regular file in $GEO_DIR"
    ( ulimit -f 524288 2>/dev/null; cp -P -- "$GEO_DIR/$1" "$3" ) || die "the copy of $1 failed"
    [ -f "$3" ] && [ ! -L "$3" ] || die "the copy of $1 is not a regular file"
    size="$(stat -c %s "$3")"
    [ "$size" -gt 0 ] && [ "$size" -le "$2" ] || die "$1 has size $size, outside 1..$2 bytes"
}

copy_checked dbip-country-lite.csv "$MAX_CSV" "$WORK/table.csv"
copy_checked razdor-geo-country.conf "$MAX_CONF" "$WORK/geo.conf"

ranges="$(wc -l < "$WORK/geo.conf")"
[ "$ranges" -ge "$MIN_RANGES" ] || die "the ranges file has $ranges lines, fewer than $MIN_RANGES"
bad="$(grep -Evc "$RANGE_RE" "$WORK/geo.conf" || true)"
[ "$bad" = 0 ] || die "$bad line(s) of the ranges file do not have the range form"

# The API's copy of the table: its data folder, owned by the API's user, read when the API starts.
[ -d "$API_DIR" ] && [ ! -L "$API_DIR" ] || die "$API_DIR is missing or a symlink"
install -m 0640 -o 10001 -g 10001 "$WORK/table.csv" "$API_DIR/dbip-country-lite.csv"

# nginx's include: keep the old one for a restore.
if [ -f "$NGINX_CONF" ]; then
    cp -p "$NGINX_CONF" "$WORK/old.conf"
fi
install -m 0644 -o root -g root "$WORK/geo.conf" "$NGINX_CONF"
if ! nginx -t >"$WORK/nginx-t" 2>&1; then
    if [ -f "$WORK/old.conf" ]; then
        install -m 0644 -o root -g root "$WORK/old.conf" "$NGINX_CONF"
    else
        rm -f "$NGINX_CONF"
    fi
    cat "$WORK/nginx-t" >&2
    die "nginx -t refused the new ranges: the old include is back, nothing reloaded"
fi
systemctl reload nginx
echo "razdor-geo-apply: $ranges ranges installed in $NGINX_CONF, nginx reloaded"
