# Razdor API: deploy (Podman, quadlet, host nginx)

The API (`razdor-server`) serves analytics and cloud saves from one binary. On the VPS it runs as a
rootful Podman container managed by systemd through a quadlet. nginx runs on the host, shared with the
owner's other sites, and proxies one domain:

    https://razdor.indicozy.dev/          the website (static, later)
    https://razdor.indicozy.dev/hub/      the download mirror (static, later)
    https://razdor.indicozy.dev/api/...   the API; nginx strips /api, so the service sees /v1/..., /admin
    https://razdor.indicozy.dev/api/admin the admin dashboard

The container publishes port 8080 on 127.0.0.1 only. Nothing else reaches it.

Files in this folder:

| File | What it is |
|---|---|
| `build.sh` | Builds the static musl binary and the image (works without musl tools on the workstation) |
| `Containerfile` | The image: `FROM scratch`, the binary, UID/GID 10001, about 7 MB |
| `razdor-api.container` | The quadlet: the container, its data folder, port, env file, limits, restart |
| `nginx.conf` | The host's nginx site for the domain (TLS, `/api`, rate limits, the dashboard rewrite) |
| `backup.sh` | Daily copy of the SQLite files and the battle, crash and save folders (14 days) |

Nothing here holds a real address or secret. The admin hashes live in `/etc/razdor-api/env` on the
VPS, never in git.

## 1. Build the image (on the workstation)

    server/deploy/build.sh                  # tags localhost/razdor-api:<version>
    nice -n 19 taskset -c 8-19 server/deploy/build.sh   # on a shared machine

The script builds `x86_64-unknown-linux-musl` with the first of: `musl-gcc`, `cargo zigbuild`, or a
podman builder container (`docker.io/library/rust:1.91-alpine`). The builder path needs no extra tools.
The result is a static binary (about 7 MB stripped) in an image of about 7 MB.

## 2. Requirements on the VPS

- Podman 4.4 or newer, with the quadlet generator (`/usr/libexec/podman/quadlet`). Rootful: the units
  live in `/etc/containers/systemd/` and run as root; the container itself runs as UID 10001.
- nginx (the host's), certbot (Let's Encrypt, webroot mode), `sqlite3` and `rsync` (for the backups).
- cgroup v2, for the memory and CPU caps. Quadlet does not work without it. It is the default on current
  distributions but not on EL8: see "AlmaLinux 8" below.

## 3. Ship the image

    podman save localhost/razdor-api:0.1.0 | ssh root@<vps> podman load

Or build on the VPS with the same script (it needs podman and about 2 GB of disk for the builder image).
The quadlet's `Image=` line must name the tag you loaded.

## 4. Folders, secrets, the service

1. Data and config folders. The container runs as UID 10001, so the data folder must belong to it:

       install -d -m 0750 -o 10001 -g 10001 /var/lib/razdor-api
       install -d -m 0700 -o root -g root /etc/razdor-api

2. The admin password and token, as hashes (the secrets are never stored). Run the image's own helpers:

       read -r -s PW;    printf '%s\n' "$PW"    | podman run --rm -i localhost/razdor-api:0.1.0 hash-password
       read -r -s TOKEN; printf '%s\n' "$TOKEN" | podman run --rm -i localhost/razdor-api:0.1.0 token-hash

   Write `/etc/razdor-api/env` (mode 0600, owner root) with the two lines:

       RAZDOR_ADMIN_PASSWORD_HASH=<the hash-password line>
       RAZDOR_ADMIN_TOKEN_SHA256=<the token-hash line>

   Optional: `RAZDOR_ALLOWED_VERSIONS=1.5.4,1.5.5` (empty means any X.Y.Z release).
   The verifier and the weekly digest send the token itself in the `x-razdor-admin` header.

3. The quadlet and the service:

       install -m 0644 server/deploy/razdor-api.container /etc/containers/systemd/
       systemctl daemon-reload
       systemctl enable --now razdor-api
       systemctl status razdor-api --no-pager
       curl -s 127.0.0.1:8080/healthz          # prints: ok

   If port 8080 is taken on the VPS, change `PublishPort=` in the quadlet and the upstream in
   `nginx.conf` (the service inside the container always listens on 8080).

## 5. nginx and the certificate

The site file needs its certificate before nginx can load it. Order:

1. Create the web roots and a minimal port-80 server that answers the challenge:

       install -d /var/www/letsencrypt /var/www/razdor-site /var/www/razdor-hub
       # a server block on port 80 for razdor.indicozy.dev with
       #   location ^~ /.well-known/acme-challenge/ { root /var/www/letsencrypt; }
       # nginx -t && systemctl reload nginx

2. Get the certificate:

       certbot certonly --webroot -w /var/www/letsencrypt -d razdor.indicozy.dev

3. Install the site (it is at http{} level, so `conf.d` is the place on Fedora; on Debian/Ubuntu copy it
   to `/etc/nginx/conf.d/razdor.conf` too, since it contains `limit_req_zone`):

       install -m 0644 server/deploy/nginx.conf /etc/nginx/conf.d/razdor.conf
       nginx -t && systemctl reload nginx

   Renewal needs nginx to reload: add `--deploy-hook "systemctl reload nginx"` to the renewal (in
   `/etc/letsencrypt/renewal/razdor.indicozy.dev.conf` as `renew_hook`, or `certbot renew --dry-run` to
   test). The site's access log is off (the service keeps no addresses either); errors go to nginx's
   error log.

Rate limits: `/api` allows 40 requests a second per address with a burst of 120 (`limit_req`). That is
gentle on purpose: many players share one address behind carrier NAT. The service has its own per-address
limits (1200 a minute, `ip_per_minute`), counted on the address nginx sets.

Storage and retention limits (the TOML `[limits]` table; the defaults are these):

- `max_data_bytes = 21474836480` (20 GiB) and `min_free_bytes = 2147483648` (2 GiB). The stored files (battles,
  crash reports, saves) count against the first; the disk's free space must stay above the second. Past either,
  no new uploads: saves and accounts get 507 (the client shows «Облако заполнено»), the analytics writes 429.
  Pings and reads go on. The count is kept in memory, scanned from the data folder at start; the dashboard's
  Storage block shows it, and the log says once a minute while the budget refuses uploads.
- `crash_per_day = 10` and `perf_per_day = 48` per install and UTC day (429, no ban).
- `saves_per_device = 100` (a save past it is refused with the full-cloud 413; nothing is deleted; the
  client's `cloud_saves_per_device` must not exceed it) and `saves_per_account = 400` (a safety net, 413 too).
- `account_expiry_days = 90`: an account not used for that long (a keyed request marks it used, at most once an
  hour) is deleted by the daily retention job, with its saves. `0` keeps every account.

The count drops when the service deletes a file (a save, a player's delete, the expiry); files removed by hand
on the host are not seen until the next restart.

## 6. Backups

`backup.sh` needs the host's `sqlite3` and `rsync`. The data folder is the host folder
`/var/lib/razdor-api` (bind mount), so the script reads the live files: `.backup` is safe while the
service runs. Run it daily from cron (`/etc/cron.d/razdor-backup`):

    10 4 * * * root RAZDOR_DATA_DIR=/var/lib/razdor-api RAZDOR_BACKUP_DIR=/var/backups/razdor-api /usr/local/bin/backup.sh

Install it with `install -m 0750 server/deploy/backup.sh /usr/local/bin/backup.sh`. It keeps 14 days.
Backups hold the account key hashes and the saves: the backup folder is mode 0700.

A run writes into `.tmp-<date>-<pid>` and renames it to `<date>` only when every step succeeded, so a
failed run leaves no dated folder and the next run tries again. The `links` table (link-code keys) is
emptied in the copy.

### Off-host copy (on the owner's PC)

A backup on the same disk as the data does not survive a dead disk or a lost VPS. Run
`server/deploy/pull-backup.sh` on the owner's PC once a day (cron or a systemd user timer). It pulls the
newest dated folder over ssh into `/mnt/ai_models/razdor-backups/<date>` and keeps 30 days:

    RAZDOR_VPS=backup@<vps-host> /path/to/razdor/server/deploy/pull-backup.sh

The ssh key must be able to read `/var/backups/razdor-api` on the VPS (the owner sets it up).

### Restore

Restore into the data folder of the service, with the service stopped. A dated copy holds `stats.sqlite`,
`cloud.sqlite`, `ip_salt` and the `battles`, `crashes`, `saves` folders. Changes made after the copy was
taken are lost (up to a day).

1. `systemctl stop razdor-api` (or the quadlet's unit name on the host).
2. Move the current data folder aside, for example `mv /var/lib/razdor-api /var/lib/razdor-api.broken`.
3. Make a fresh folder and copy the dated backup into it:
   `install -d -m 0700 /var/lib/razdor-api && cp -a /var/backups/razdor-api/<date>/. /var/lib/razdor-api/`
   (or from the PC copy, with `rsync -a` over ssh).
4. `ip_salt` is in the copy. A copy made before it was added has none: then copy it from the broken
   folder (`cp -a /var/lib/razdor-api.broken/ip_salt /var/lib/razdor-api/`). Without the old salt the IP
   bans are forgotten (the hashed IPs no longer match); nothing else breaks.
5. `systemctl start razdor-api`, then check `https://razdor.indicozy.dev/api/v1/...` (a cloud save of a
   test account, or the dashboard's counts) against the numbers from before.

Tested (2026-11-22, a scratch folder, server at 4fcf9873, the python3 fallback because the host had no
sqlite3 CLI): one account with a 1 KB and a 40 KB save, settings, marks and one link code. The copy
restored into a fresh folder. Both saves came back with the same SHA-256, the settings and marks matched,
the row counts of every table matched, `PRAGMA integrity_check` said `ok` for both files, and the link
code was gone from the copy. The drill did not cover the battle and crash folders (empty there), the
container's bind mount and file owner on the VPS, or the PC copy.

Known gaps:

- Link codes are not in the copy, on purpose. A player who had a code issues a new one.
- Anything written after the copy was taken is lost (up to a day).
- A copy older than the `ip_salt` change needs step 4 from the broken folder.
- The battle and crash folders, the container's file owner and the PC copy (`pull-backup.sh`) are not
  drilled yet. Run the drill again after a change to the data layout or to the script.

## 7. Operate

- Logs: `journalctl -u razdor-api -f` (the container's stderr goes to the journal through the unit).
  `podman logs razdor-api` shows the same.
- Status: `systemctl status razdor-api`; the container: `podman ps`, `podman stats --no-stream razdor-api`.
- Update: build the new version (step 1), ship it (step 3), change `Image=` in
  `/etc/containers/systemd/razdor-api.container` to the new tag, `systemctl daemon-reload`,
  `systemctl restart razdor-api`. Roll back by putting the old tag back. Keep the previous image until the
  new one has run a day.
- The dashboard: `https://razdor.indicozy.dev/api/admin`. The API's JSON summary:
  `https://razdor.indicozy.dev/api/admin/api/summary.json` with the `x-razdor-admin` header.

### The dashboard's rewrite

The service serves its dashboard at `/admin` (its cookie is `Path=/admin` and its forms post to
`/admin/...`). nginx serves it at `/api/admin` and rewrites the cookie path, the login redirect and the
form links with `proxy_cookie_path`, `proxy_redirect` and `sub_filter`. After a change to the dashboard's
HTML, check the login and the Unban button once.

### Rotate

- The admin password: run `hash-password` again, replace the line in `/etc/razdor-api/env`, restart. Open
  sessions stay valid for 12 hours; restart the service to end them at once.
- The admin token: run `token-hash` with the new token, replace `RAZDOR_ADMIN_TOKEN_SHA256`, restart,
  and update the verifier's and the digest's copy.
- Account keys are never rotated by the server. A player who loses one deletes the account in the game,
  or links the device again from a working one.

### Delete an install's data or an account

Players do this themselves («Удалить мои данные» in the game). An owner's request uses the API's
`POST /v1/install/delete` with the install id, or `DELETE /v1/account` with the key. Both remove the
rows and the files. A ban record of the install id stays, so deleting the data does not reset its
strikes.

### Lifting a ban

On the dashboard, the bans table has an Unban button per active ban. The strikes stay: the next offence
is the next step (24 h, 7 days, for good).

## 8. Resources and measurements

The quadlet caps the container at 160 MB of memory (no swap), half a CPU and 256 processes, and runs it
with a read-only root filesystem, no capabilities, and `no-new-privileges`.

Measured on the workstation, rootless Podman 5.8, Fedora 44, through the host's nginx (TLS), with
`server/tools/load.rs` (4000 installs, 8 event batches each, 64 connections; 40 000 requests, all 200).
The load tool sends every install from one loopback address, so the test raised `ip_per_minute` for that
phase only (a test-only TOML; every other limit keeps its default):

| Setup | Requests/s | p50 | p99 | Container CPU | Memory |
|---|---|---|---|---|---|
| Container, `--cpus 0.5` (the quadlet's cap), bridge network | 246 | 218 ms | 774 ms | about 0.3 of a core | 10 MB (peak RSS of the server 11.5 MB) |
| Container, `--cpus 0.5`, host network | 232 | 289 ms | 593 ms | 0.49 (at the cap) | 10 MB |
| Container, `--cpus 1.0`, host network | 371 | 142 ms | 681 ms | 0.77 | 11 MB |
| Static binary on the host, no container, no nginx | 614 | 78 ms | 288 ms | not measured | 12.7 MB peak |
| Same, rebuilt after C2/C3 (2026-11-22, workstation, pinned to CPUs 8-19) | 1087 | 54 ms | 161 ms | not measured | 16.1 MB peak |

The last row: 40 000 requests, all 200; the stored rows match what was sent (4000 pings, 4000 perf
reports, 640 000 events = 4000 installs x 8 batches x 20 events). The numbers are from another machine
state than the first row, so they show no regression, not a speed-up.

Idle, the container uses about 1 MB. The quadlet's half-CPU cap is the limit of the first two rows: the
service is CPU-bound at the cap. The host and bridge networks make no difference. A VPS that can spare a
full core roughly doubles the throughput. Each request costs about 2 ms of CPU in this run. The cap's
200-odd requests a second is far above what the players send (a few requests a day each); raise `--cpus`
only if the logs or `podman stats` show the service held back.

Data growth: the stats database grew to about 145 MB for 640 000 events (about 220 bytes an event), with
the events and pings kept at their retention (see the analytics spec).

## AlmaLinux 8 (the current VPS)

The VPS runs AlmaLinux 8. Its hoster's image was 8.3 (kernel 4.18.0-240). The steps above work there
with the changes below. They were tested on 2026-11-23, and the host passed a reboot check after each
step.

Order: make one risky change per reboot, and check the network after each one (`ping -c 50` from home and
a ping from several countries on check-host.net). If a reboot breaks the network, you then know which
change did it.

1. Upgrade the image first. The AlmaLinux 8 signing key changed in 2024, so import it before `dnf`:

       rpm --import https://repo.almalinux.org/almalinux/RPM-GPG-KEY-AlmaLinux
       dnf -y upgrade        # 8.3 to 8.10, kernel 4.18.0-553; then reboot and check

2. Turn on cgroup v2. EL8 boots with cgroup v1, and quadlet needs v2 (`man podman-systemd.unit` says so):

       grubby --update-kernel=ALL --args="systemd.unified_cgroup_hierarchy=1"
       # reboot, then:
       podman info --format '{{.Host.CgroupsVersion}}'   # prints v2

3. Packages. Podman 4.9 comes from AppStream and has every quadlet key that `razdor-api.container` uses.
   nginx comes from a module stream (the default stream is 1.14). certbot comes from EPEL.

       dnf -y install epel-release
       dnf -y module enable nginx:1.24
       dnf -y install podman nginx certbot sqlite rsync cronie tar
       firewall-cmd --permanent --add-service=http --add-service=https && firewall-cmd --reload

   sqlite 3.26 is new enough for `backup.sh` (it uses `.backup` and `VACUUM`). The image has no `tar`.

4. sshd: AlmaLinux 8 has no `/etc/ssh/sshd_config.d/`. Edit `/etc/ssh/sshd_config` itself
   (`PermitRootLogin prohibit-password`, `PasswordAuthentication no`, `ChallengeResponseAuthentication no`),
   then run `sshd -t` and reload sshd.

5. The certificate: EPEL's certbot 1.22 enables `certbot-renew.timer` itself. Give it the nginx reload
   with `--deploy-hook "systemctl reload nginx"`, which certbot saves as `renew_hook` in the renewal file.

Known quirks of this host:

- SELinux is disabled in the hoster's image, so the `:Z` on the quadlet's volume does nothing. If SELinux
  is turned on later, check that nginx can still reach 127.0.0.1:8080.
- After the 8.3 to 8.10 upgrade, `useradd` prints `sss_cache` warnings ("DB version too old"). SSSD does
  not run on the host, and the warnings are harmless.
- The hoster's network drops port 22 from one address after about four SSH connections in a minute. The
  block lasts about 15 minutes. Ping and the web ports still work, so the host looks up while ssh times
  out. Use one shared connection for a deploy (`ControlMaster auto`, `ControlPersist 10m` in
  `~/.ssh/config`), and never retry ssh in a tight loop.
- A reboot can take two minutes to shut down. sshd stops first, so port 22 refuses connections while ping
  still answers. Wait about three minutes before you decide that a reboot went wrong.

Monitoring on the host (outside this folder):

- vnstat (EPEL) records the traffic per interface.
- `txwatch.timer` runs `/usr/local/sbin/txwatch` every hour. If eth0 sent more than 1 GB in the last
  hour, it logs at crit and writes `/root/TX-ALERT` with the open connections and the top processes.
- `netsnap.timer` runs `/usr/local/sbin/netsnap` every minute. It appends the eth0 counters and the open
  connections to `/var/log/netsnap.log` (logrotate keeps 14 days).
- The journal is persistent (`Storage=persistent`), so the logs from before a reboot are kept.
- `dnf-automatic.timer` installs security updates only (`upgrade_type = security`). It never reboots.

## Logs and privacy

The service logs its own events (startup, bans with their reason, retention counts) and no client
addresses. The IP address is used in memory for the rate limits and hashed (with a salt kept in `ip_salt`
in the data folder) for the IP bans. nginx's access log is off for the host, so nothing keeps addresses.
