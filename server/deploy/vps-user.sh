#!/bin/sh
# Creates the `razdor` deploy user on the VPS (run as root there): key login only, no password,
# and the sudo list in server/deploy/sudoers-razdor (the API, its image, nginx checks, certbot, the Razdor
# binaries and the mirror units). The list below must match that file (test_deploy.py checks it).
# Usage: ssh root@<vps> sh -s < server/deploy/vps-user.sh "<ssh public key line>"
set -eu
PUB="$1"
id razdor >/dev/null 2>&1 || useradd -m -s /bin/bash -c 'Razdor deploy' razdor
passwd -l razdor >/dev/null
# Logs and unit status without sudo (journalctl/systemctl under sudo open a pager, a root shell escape).
usermod -aG systemd-journal razdor
install -d -m 700 -o razdor -g razdor /home/razdor/.ssh
printf '%s\n' "$PUB" > /home/razdor/.ssh/authorized_keys
chown razdor:razdor /home/razdor/.ssh/authorized_keys
chmod 600 /home/razdor/.ssh/authorized_keys
cat > /etc/sudoers.d/razdor <<'EOF'
# /etc/sudoers.d/razdor: the Razdor deploy user (razdor). Installed by server/deploy/root-setup-2026-11-24.sh
# (after visudo -cf) and written by server/deploy/vps-user.sh: the two copies must match (test_deploy.py checks).
# Exact command lines only, no wildcards. Nothing that could break the server is allowed here: no nginx
# config, no units, no sudoers, no users, no polkit, no firewall, no package installs.
# No status/journalctl here: they open a pager; the systemd-journal group reads them instead.

# The API service, its image, nginx checks and certbot (as before, 2026-11-23).
Cmnd_Alias RZ_SVC = /usr/bin/systemctl start razdor-api, /usr/bin/systemctl stop razdor-api, \
  /usr/bin/systemctl restart razdor-api, /usr/bin/systemctl daemon-reload, \
  /usr/bin/systemctl reload nginx
Cmnd_Alias RZ_IMG = /usr/bin/podman load, /usr/bin/podman images, /usr/bin/podman ps, \
  /usr/bin/podman image prune -f
Cmnd_Alias RZ_WEB = /usr/sbin/nginx -t, \
  /usr/bin/certbot certonly --webroot -w /var/www/letsencrypt -d razdor.indicozy.dev, \
  /usr/bin/certbot renew, /usr/bin/certbot renew --dry-run

# Routine updates of the Razdor host binaries (2026-11-24). The root-owned wrapper /usr/local/sbin/razdor-install-bin
# copies the named file out of the razdor-owned staging folder WITHOUT following links, checks the copy (a regular
# ELF file of a sane size) and installs it as /usr/local/bin/razdor-<name>. The owner's decision: razdor may replace
# these two binaries this way. Only these two exact lines: no install of a path razdor can write.
Cmnd_Alias RZ_BIN = /usr/local/sbin/razdor-install-bin mirror, /usr/local/sbin/razdor-install-bin server

# The mirror's sync and the GeoIP table: start, stop and restart of these four units only (2026-11-24).
Cmnd_Alias RZ_MIRROR = /usr/bin/systemctl start razdor-mirror-sync.service, \
  /usr/bin/systemctl start --no-block razdor-mirror-sync.service, \
  /usr/bin/systemctl stop razdor-mirror-sync.service, \
  /usr/bin/systemctl restart razdor-mirror-sync.service, \
  /usr/bin/systemctl start razdor-mirror-sync.timer, \
  /usr/bin/systemctl stop razdor-mirror-sync.timer, \
  /usr/bin/systemctl restart razdor-mirror-sync.timer, \
  /usr/bin/systemctl start razdor-geo-update.service, \
  /usr/bin/systemctl start --no-block razdor-geo-update.service, \
  /usr/bin/systemctl stop razdor-geo-update.service, \
  /usr/bin/systemctl restart razdor-geo-update.service, \
  /usr/bin/systemctl start razdor-geo-update.timer, \
  /usr/bin/systemctl stop razdor-geo-update.timer, \
  /usr/bin/systemctl restart razdor-geo-update.timer

razdor ALL=(root) NOPASSWD: RZ_SVC, RZ_IMG, RZ_WEB, RZ_BIN, RZ_MIRROR
EOF
chmod 440 /etc/sudoers.d/razdor
visudo -cf /etc/sudoers.d/razdor
echo "razdor user ready"
