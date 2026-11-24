#!/bin/sh
# One-time root setup of the Razdor VPS (AlmaLinux 8) for the owner's own download mirror, the site copy,
# the hub log and the country counters: every root step of server/deploy/README.md sections 9 and 10.
# Decisions: docs/specs/own-mirror-2026-11-24.md.
#
# Run as root on the VPS, after the deploy user uploaded the files with server/deploy/stage-upload.sh:
#   ssh root@<vps> bash -s < server/deploy/root-setup-2026-11-24.sh
#
# It installs FROM the staging folder (RAZDOR_STAGE, default /home/razdor/stage), because it is read from
# stdin and cannot see the repository. Idempotent: safe to run twice. It prints each step and stops at the
# first error. It does not touch other sites' nginx files. Its sudo rule is /etc/sudoers.d/razdor, written
# from the staged sudoers-razdor only after visudo accepts it.
set -eu

STAGE="${RAZDOR_STAGE:-/home/razdor/stage}"
DATE_TAG="$(date +%Y%m%d-%H%M%S)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

step() { printf '\n== %s\n' "$*"; }
die() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }

step "preflight"
[ "$(id -u)" = 0 ] || die "run as root"
for c in install systemctl nginx visudo podman useradd stat sha256sum sed mktemp curl od tr; do
  command -v "$c" >/dev/null 2>&1 || die "missing command: $c"
done
getent group nginx >/dev/null || die "no nginx group: install nginx first (README section 5)"
# The stage is razdor-owned: a staged file must be a plain file, never a link (install would follow it).
[ ! -L "$STAGE" ] || die "$STAGE is a symlink"
for f in razdor-mirror razdor-server razdor-api.tar nginx.conf geo-update.sh geo-apply.sh razdor-install-bin.sh \
  razdor-mirror.toml razdor-mirror-sync.service razdor-mirror-sync.timer razdor-mirror-alert.service \
  razdor-geo-update.service razdor-geo-update.timer razdor-mirror-sync.polkit.rules \
  razdor-hub.logrotate razdor-api.container sudoers-razdor; do
  [ -f "$STAGE/$f" ] && [ ! -L "$STAGE/$f" ] || die "not a plain file in $STAGE: $f (upload first: server/deploy/stage-upload.sh)"
done
echo "all staged files present in $STAGE"

step "mirror user and folders"
if id razdor-mirror >/dev/null 2>&1; then
  echo "user razdor-mirror exists"
else
  useradd --system --home-dir /var/lib/razdor-mirror --no-create-home --shell /sbin/nologin razdor-mirror
  echo "created user razdor-mirror"
fi
# The first site folder (the placeholder) is moved aside once: it is not owned by the mirror user yet.
if [ -d /var/www/razdor-site ] && [ ! -e /var/www/razdor-site/versions ] \
  && [ "$(stat -c %U /var/www/razdor-site)" != razdor-mirror ]; then
  mv /var/www/razdor-site "/var/www/razdor-site.placeholder-$DATE_TAG"
  echo "placeholder moved aside: /var/www/razdor-site.placeholder-$DATE_TAG (delete it by hand later)"
fi
install -d -o razdor-mirror -g nginx -m 0755 /var/www/razdor-hub
install -d -o razdor-mirror -g nginx -m 0755 /var/www/razdor-site /var/www/razdor-site/versions
install -d -o razdor-mirror -g razdor-mirror -m 0750 /var/lib/razdor-mirror
install -d -o razdor-mirror -g razdor-mirror -m 0750 /var/lib/razdor-mirror/stage
install -d -o nginx -g nginx -m 0755 /var/log/razdor-hub
# The country table's unprivileged user (it runs the download and writes its own folder only), and the
# root-owned work folder of the two root helpers (razdor-install-bin, razdor-geo-apply).
if id razdor-geo >/dev/null 2>&1; then
  echo "user razdor-geo exists"
else
  useradd --system --home-dir /var/lib/razdor-geo --no-create-home --shell /sbin/nologin razdor-geo
  echo "created user razdor-geo"
fi
install -d -o razdor-geo -g razdor-geo -m 0750 /var/lib/razdor-geo
install -d -o 10001 -g 10001 -m 0750 /var/lib/razdor-api/geo
install -d -o root -g root -m 0700 /var/lib/razdor-install
[ "$(stat -c %d /var/www/razdor-hub)" = "$(stat -c %d /var/lib/razdor-mirror/stage)" ] \
  || die "the stage folder must be on the same filesystem as /var/www/razdor-hub"
echo "folders ready, stage on the web root's filesystem"

step "host binaries"
install -m 0755 -o root -g root "$STAGE/razdor-mirror" /usr/local/bin/razdor-mirror
install -m 0755 -o root -g root "$STAGE/razdor-server" /usr/local/bin/razdor-server
# The deploy user's only route to these binaries: the root wrapper (server/deploy/razdor-install-bin.sh), which
# copies the staged file without following links and checks it. Root never runs a file from the stage or /usr/local/bin/razdor-*.
install -m 0755 -o root -g root "$STAGE/razdor-install-bin.sh" /usr/local/sbin/razdor-install-bin
echo "installed /usr/local/bin/razdor-mirror, /usr/local/bin/razdor-server and the wrapper /usr/local/sbin/razdor-install-bin"

step "mirror config, units, log rotation, polkit, country table file"
install -m 0644 -o root -g root "$STAGE/razdor-mirror.toml" /etc/razdor-mirror.toml
install -m 0644 -o root -g root "$STAGE/razdor-mirror-sync.service" "$STAGE/razdor-mirror-sync.timer" \
  "$STAGE/razdor-mirror-alert.service" "$STAGE/razdor-geo-update.service" "$STAGE/razdor-geo-update.timer" \
  /etc/systemd/system/
install -m 0755 -o root -g root "$STAGE/geo-update.sh" /usr/local/sbin/razdor-geo-update
install -m 0755 -o root -g root "$STAGE/geo-apply.sh" /usr/local/sbin/razdor-geo-apply
install -m 0644 -o root -g root "$STAGE/razdor-hub.logrotate" /etc/logrotate.d/razdor-hub
# The polkit rule lets the deploy user (razdor) start the sync on demand; the file in git says "deploy".
install -d -m 0755 /etc/polkit-1/rules.d
sed 's/subject\.user == "deploy"/subject.user == "razdor"/' "$STAGE/razdor-mirror-sync.polkit.rules" > "$WORK/polkit"
grep -q 'subject.user == "razdor"' "$WORK/polkit" || die "polkit rule: the deploy user was not replaced"
install -m 0644 -o root -g root "$WORK/polkit" /etc/polkit-1/rules.d/50-razdor-mirror.rules
# nginx's geo include must exist before the new nginx.conf is loaded (an empty file is valid).
[ -e /etc/nginx/razdor-geo-country.conf ] || : > /etc/nginx/razdor-geo-country.conf
systemctl daemon-reload
echo "units, timers and the polkit rule installed"

step "first sync (the mirror and the site copy), started in the background"
systemctl enable --now razdor-mirror-sync.timer
systemctl start --no-block razdor-mirror-sync.service
echo "started razdor-mirror-sync.service (follow it: journalctl -u razdor-mirror-sync.service -f)"
# nginx's root for / is .../razdor-site/current, a link the first successful site step creates. Wait for it
# (bounded), so the site does not answer 404 for longer than it must. A later miss is a warning, not a stop.
i=0
while [ ! -L /var/www/razdor-site/current ] && [ "$i" -lt 60 ]; do
  sleep 10
  i=$((i + 1))
done
if [ -L /var/www/razdor-site/current ]; then
  echo "site copy is live: /var/www/razdor-site/current"
else
  echo "WARNING: no site copy after 10 minutes. / answers 404 until a sync publishes it: journalctl -u razdor-mirror-sync.service"
fi

step "nginx: /etc/nginx/conf.d/razdor.conf (tested before reload)"
NEW_CONF=/etc/nginx/conf.d/razdor.conf
BAK=""
if [ -f "$NEW_CONF" ] && [ "$(sha256sum < "$NEW_CONF")" = "$(sha256sum < "$STAGE/nginx.conf")" ]; then
  echo "nginx config unchanged"
else
  if [ -f "$NEW_CONF" ]; then
    BAK="/etc/nginx/razdor.conf.bak-$DATE_TAG"
    cp -p "$NEW_CONF" "$BAK"
    echo "old config kept: $BAK (outside conf.d, so nginx does not load it)"
  fi
  install -m 0644 -o root -g root "$STAGE/nginx.conf" "$NEW_CONF"
  if ! nginx -t >"$WORK/nginx-t" 2>&1; then
    cat "$WORK/nginx-t" >&2
    if [ -n "$BAK" ]; then
      cp -p "$BAK" "$NEW_CONF"
    else
      rm -f "$NEW_CONF"
    fi
    if nginx -t >/dev/null 2>&1; then
      echo "previous nginx config restored, nothing reloaded" >&2
    else
      echo "WARNING: nginx -t fails even after the restore: check /etc/nginx by hand" >&2
    fi
    die "nginx -t refused the new config"
  fi
  systemctl reload nginx
  echo "nginx config loaded"
fi

step "API image and quadlet (README section 10)"
IMG="$(sed -n 's/^Image=//p' "$STAGE/razdor-api.container")"
[ -n "$IMG" ] || die "no Image= line in razdor-api.container"
BEFORE_ID="$(podman image inspect --format '{{.Id}}' "$IMG" 2>/dev/null || echo none)"
podman load -i "$STAGE/razdor-api.tar"
AFTER_ID="$(podman image inspect --format '{{.Id}}' "$IMG" 2>/dev/null || echo none)"
[ "$AFTER_ID" != none ] || die "the image $IMG is not in podman after the load"
CHANGED=0
[ "$BEFORE_ID" = "$AFTER_ID" ] || CHANGED=1
if [ ! -f /etc/containers/systemd/razdor-api.container ] \
  || [ "$(sha256sum < /etc/containers/systemd/razdor-api.container)" != "$(sha256sum < "$STAGE/razdor-api.container")" ]; then
  install -m 0644 -o root -g root "$STAGE/razdor-api.container" /etc/containers/systemd/razdor-api.container
  CHANGED=1
fi
if [ "$CHANGED" = 1 ]; then
  systemctl daemon-reload
  systemctl restart razdor-api
  echo "razdor-api restarted with $IMG"
else
  echo "razdor-api unchanged ($IMG)"
fi
i=0
until curl -fsS --max-time 5 http://127.0.0.1:8080/healthz >/dev/null 2>&1; do
  i=$((i + 1))
  [ "$i" -lt 15 ] || die "razdor-api does not answer on 127.0.0.1:8080/healthz"
  sleep 2
done
echo "razdor-api answers /healthz"

step "sudo rule for razdor (visudo checks the file before it is put in place)"
install -m 0440 -o root -g root "$STAGE/sudoers-razdor" /etc/sudoers.d/.razdor.new
if ! visudo -cf /etc/sudoers.d/.razdor.new >/dev/null; then
  rm -f /etc/sudoers.d/.razdor.new
  die "visudo refused sudoers-razdor: /etc/sudoers.d/razdor is unchanged"
fi
mv -f /etc/sudoers.d/.razdor.new /etc/sudoers.d/razdor
visudo -c >/dev/null || die "visudo -c fails after the install: check /etc/sudoers.d by hand"
echo "/etc/sudoers.d/razdor installed"

step "country table and its timer (README section 10)"
systemctl enable --now razdor-geo-update.timer
systemctl start --no-block razdor-geo-update.service
echo "started razdor-geo-update.service (journalctl -u razdor-geo-update -f)"

step "status"
systemctl is-active razdor-mirror-sync.timer razdor-geo-update.timer razdor-api nginx || true
if [ -f /var/www/razdor-hub/status.json ]; then
  head -c 400 /var/www/razdor-hub/status.json
  echo
else
  echo "no /var/www/razdor-hub/status.json yet (the first sync is still running)"
fi
cat <<'NOTE'

Not done by this script (outside the repository, check by hand):
- txwatch: the owner's alert on the host (/usr/local/sbin/txwatch, not in git) should alert above 5 GB an hour.
- Rollback of the placeholder site folder and the old nginx config: the paths are printed above.
NOTE
echo "done"
