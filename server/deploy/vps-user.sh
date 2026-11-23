#!/bin/sh
# Creates the `razdor` deploy user on the VPS (run as root there): key login only, no password,
# and a sudo list limited to the Razdor API service, its image, nginx checks and certbot.
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
# Razdor deploy user. Only the Razdor API service, its image, nginx checks and certbot.
# No status/journalctl here: they open a pager; the systemd-journal group reads them instead.
# Exact command lines only, no wildcards.
Cmnd_Alias RZ_SVC = /usr/bin/systemctl start razdor-api, /usr/bin/systemctl stop razdor-api, \
  /usr/bin/systemctl restart razdor-api, /usr/bin/systemctl daemon-reload, \
  /usr/bin/systemctl reload nginx
Cmnd_Alias RZ_IMG = /usr/bin/podman load, /usr/bin/podman images, /usr/bin/podman ps, \
  /usr/bin/podman image prune -f
Cmnd_Alias RZ_WEB = /usr/sbin/nginx -t, \
  /usr/bin/certbot certonly --webroot -w /var/www/letsencrypt -d razdor.indicozy.dev, \
  /usr/bin/certbot renew, /usr/bin/certbot renew --dry-run
razdor ALL=(root) NOPASSWD: RZ_SVC, RZ_IMG, RZ_WEB
EOF
chmod 440 /etc/sudoers.d/razdor
visudo -cf /etc/sudoers.d/razdor
echo "razdor user ready"
