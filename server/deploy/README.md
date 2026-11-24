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
| `razdor-mirror.toml` | The download mirror's sync tool config (section 9) |
| `razdor-mirror-sync.service`, `.timer` | The mirror's sync every 15 minutes (section 9) |
| `razdor-mirror-alert.service` | Logs a crit line when a sync fails |
| `razdor-mirror-sync.polkit.rules` | Lets the deploy user start the sync on demand, without sudo |

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
   test). The website's access log is off. The API has its own log that keeps no addresses (see
   «Logs and privacy»). Errors go to nginx's error log.

   nginx's own 429 (the per-address limit below) carries `Retry-After: 1`. The service's 429s carry their
   own `Retry-After`: the seconds until the window frees, or 60 when it is not known.

Rate limits: `/api` allows 20 requests a second per address with a burst of 60 (`limit_req`), and the
admin dashboard a burst of 20. That is gentle on purpose: many players share one address behind carrier NAT. The service has its own per-address
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
- Retries are safe. A save upload that the server already holds answers with the same row and writes
  nothing. An account creation with a key the server already holds answers with the same account. A
  device sends its key on a retry, so a lost answer never makes a second account.

### The error alert

`api-alert.sh` reads the last 10 minutes of the API's nginx log (`/var/log/nginx/razdor-api.log`). It
counts the 5xx responses and the other 4xx responses, except 404 and 429. It raises an alert when there
are at least 5 server errors, or at least 20 other client errors. The alert goes to the journal at
priority crit, and the same line goes to `/root/API-ALERT` with a UTC time. The timer runs it every 10
minutes, five minutes after boot. The script always exits 0.

Install the alert and the log rotation (on the VPS, as root):

    install -m 0755 server/deploy/api-alert.sh /usr/local/sbin/razdor-api-alert.sh
    install -m 0644 server/deploy/razdor-api-alert.service server/deploy/razdor-api-alert.timer /etc/systemd/system/
    install -m 0644 server/deploy/razdor-api.logrotate /etc/logrotate.d/razdor-api
    systemctl daemon-reload
    systemctl enable --now razdor-api-alert.timer
    logrotate -d /etc/logrotate.d/razdor-api        # a dry run: prints what it would do

Check it: `systemctl list-timers razdor-api-alert.timer`, `journalctl -t razdor-api-alert`, and
`cat /root/API-ALERT`. To test the script by hand, run `systemctl start razdor-api-alert.service`.

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

## 9. The download mirror (`razdor.indicozy.dev/hub/`)

`razdor-mirror` copies the public release files from GitHub into `/var/www/razdor-hub`, which nginx serves at
`/hub/`. The owner's server is the first download mirror (GitHub the second, GitVerse the fallback for Russia).
A systemd timer runs it every 15 minutes, and the release skill starts it on demand. It reads public URLs only,
so it needs no secret on GitHub and no upload by hand. Every file is checked by its SHA-256 (GitHub's digest),
written to a staging folder, and a release folder appears in one rename. The feed and `latest` move last, so a
client never reads a file that is not there. Design: `docs/specs/own-mirror-2026-11-24.md`.

What it keeps (`/etc/razdor-mirror.toml`): the newest 3 published app releases, their versioned files (the old
fixed names stay on GitHub; the translation tools and the iOS package are not copied; the ads zip is); the
newest 2 catalogs and the game, ad and depth-model releases they name. `status.json` lists what is on disk:

    curl -s https://razdor.indicozy.dev/hub/status.json     # synced_at, app, catalog, errors, ok

GitHub's API allows 60 requests an hour per address. A run makes 3 (the app's list, the site copy's release lookup,
and the catalog's list when a new catalog needs it), plus one lookup for a release that the list lacks (rare). A
rate limit ends the run quietly; the next one tries again.

### The site copy (`razdor.indicozy.dev/`, and the wiki at `/wiki/`)

The Pages site (the hand-made site and the wiki) is also served at the root of this domain. The Pages workflow's
`root-copy` job (`.github/workflows/pages.yml`) builds the root copy with `tools/site/root_copy.py` (the wiki with base
`/wiki`, the `/razdor/` links rewritten, canonical links to GitHub's pages) and packs it as `site.tar.gz` into the
pre-release `site-latest` of this repository (`GITHUB_TOKEN` cannot write to the community repository, and no
secret is set). The mirror looks that release up (one call a run) and, when its digest is new, fetches the archive
(digest checked), unpacks it into `/var/www/razdor-site/versions/<sha256>` (a link, or a path that leaves the folder,
fails the unpack) and moves `/var/www/razdor-site/current` to it with one rename. The copy before the live one is
kept, so a rollback is `ln -sfn versions/<sha256> /var/www/razdor-site/current` by hand.

`status.json` lists the live copy as `site` and the one before it as `site_previous`:

    curl -s https://razdor.indicozy.dev/hub/status.json     # site, site_previous, errors, ok

Install (once, as root; the old placeholder folder is moved aside, the config needs no change, its `site_dir` is the
default). Order: the folder and the unit first, then the first sync, then nginx, so the site never answers 404 for
longer than the sync takes:

    mv /var/www/razdor-site /var/www/razdor-site.old                       # the placeholder; delete it later
    install -d -o razdor-mirror -g nginx -m 0755 /var/www/razdor-site /var/www/razdor-site/versions
    install -m 0644 server/deploy/razdor-mirror-sync.service /etc/systemd/system/
    systemctl daemon-reload
    systemctl start --no-block razdor-mirror-sync.service                   # wait for status.json to show site
    journalctl -u razdor-mirror-sync.service -n 30
    install -m 0644 server/deploy/nginx.conf /etc/nginx/conf.d/razdor.conf  # root is .../current now
    nginx -t && systemctl reload nginx

The unit may write `/var/www/razdor-site` (`ReadWritePaths`); the mirror user owns that folder and nothing else
under `/var/www`. Check after the install:

    curl -sI https://razdor.indicozy.dev/ | head -5          # 200, Cache-Control: no-cache
    curl -sI https://razdor.indicozy.dev/wiki/ | head -5     # 200

A bad archive (a symlink in it, a wrong digest, no front page) is logged as `site:` in the run's errors and the
live copy stays.

### Install (once, as root on the VPS)

1. Copy the binary (built by `server/deploy/build.sh`, `target/x86_64-unknown-linux-musl/release/razdor-mirror`):

       install -m 0755 razdor-mirror /usr/local/bin/razdor-mirror

2. The user and the folders. The staging folder must be on the same filesystem as the web root (the tool checks):

       useradd --system --home /var/lib/razdor-mirror --shell /sbin/nologin razdor-mirror
       install -d -o razdor-mirror -g nginx -m 0755 /var/www/razdor-hub
       install -d -o razdor-mirror -g razdor-mirror -m 0750 /var/lib/razdor-mirror/stage

3. The config, the units and the alert:

       install -m 0644 server/deploy/razdor-mirror.toml /etc/razdor-mirror.toml
       install -m 0644 server/deploy/razdor-mirror-sync.service server/deploy/razdor-mirror-sync.timer server/deploy/razdor-mirror-alert.service /etc/systemd/system/
       systemctl daemon-reload

4. The first fill, by hand, and watch it (about 1.3 GiB; the first run is the big one):

       systemctl start --no-block razdor-mirror-sync.service
       journalctl -u razdor-mirror-sync.service -f

   A run that cannot publish something logs `crit` and exits 1; nothing half-written is served.

5. Check it:

       curl -sI https://razdor.indicozy.dev/hub/status.json                       # 200
       curl -r 0-99 -o /dev/null -w '%{http_code}\n' https://razdor.indicozy.dev/hub/indicozy/razdor/releases/download/<tag>/<versioned file>   # 206
       sha256sum <the same file from GitHub>  # against the digest in the release's API entry
       grep -c '/hub/' /var/log/nginx/razdor-api.log                              # 0: the mirror has no access log

6. nginx (the `/hub/` location and its cache map are in `nginx.conf`):

       install -m 0644 server/deploy/nginx.conf /etc/nginx/conf.d/razdor.conf
       nginx -t && systemctl reload nginx

7. The timer:

       systemctl enable --now razdor-mirror-sync.timer

8. The on-demand start for the release skill (no sudo). Install the polkit rule, after changing `"deploy"` in
   it to the deploy user's name on the host. It allows that one user to start that one unit, and nothing else:

       install -m 0644 server/deploy/razdor-mirror-sync.polkit.rules /etc/polkit-1/rules.d/50-razdor-mirror.rules

   The deploy user then runs `systemctl start --no-block razdor-mirror-sync.service` over ssh and polls
   `status.json` until it lists the new tag. A sync that is running makes the start a no-op (the lock).

### Operate

- Logs: `journalctl -u razdor-mirror-sync.service` (each run), `journalctl -t razdor-mirror-alert` (a failed run).
- Stop it: `systemctl disable --now razdor-mirror-sync.timer`. The files stay served; `status.json` stops changing.
- Space: the tool holds at most `max_bytes` (8 GiB) and keeps `min_free_bytes` (20 GiB) free. Past either, it
  downloads nothing and logs `crit` (what is served stays).
- Traffic: the owner's `txwatch` alert (below) now trips above 5 GB sent an hour; the mirror's downloads are
  not what the players send, but a release day with many players may still trip it.

### What it does not do yet

- A release file with no SHA-256 (no digest in the API and no line in SHA256SUMS) is not served; its release is
  not published, and the run logs it.
- Files shared by two releases are not linked across releases (only within one release).
- The catalog's ad image and the depth model come from their own releases (`ad-thorny-times-1`,
  `depth-model-v1`); when such a tag is not in the newest 100 releases, one extra API lookup is made for it.

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
- `txwatch.timer` runs `/usr/local/sbin/txwatch` every hour. If eth0 sent more than 5 GB in the last
  hour, it logs at crit and writes `/root/TX-ALERT` with the open connections and the top processes.
- `netsnap.timer` runs `/usr/local/sbin/netsnap` every minute. It appends the eth0 counters and the open
  connections to `/var/log/netsnap.log` (logrotate keeps 14 days).
- The journal is persistent (`Storage=persistent`), so the logs from before a reboot are kept.
- `dnf-automatic.timer` installs security updates only (`upgrade_type = security`). It never reboots.

## Logs and privacy

The service logs its own events (startup, bans with their reason, retention counts) and no client
addresses. The IP address is used in memory for the rate limits and hashed (with a salt kept in `ip_salt`
in the data folder) for the IP bans. The website has no access log. The download mirror (`/hub/`) has a log of its own, `/var/log/razdor-hub/hub.log`:
one line a request with the time, the file, the status, the bytes sent, the file's size and the country (section 10); no address and no user agent. It is kept 14 days raw, then only daily totals remain.

The API has its own nginx log, `/var/log/nginx/razdor-api.log`. It has one line for each request. Each
line has these fields, in this order: the time (ISO 8601), the method, the path, the status, the request
time, the request size, the response size, the app version and the app commit. The path has no query
string. The log keeps no client address and no user agent. It is kept for 14 days, then rotated and
compressed.

## 10. Country and download counters

Three things use a country: the daily ping (the server stores the country of the address, never the address),
and each `/hub/` download line in the mirror's log. Both use one table, DB-IP's IP to Country Lite (CC BY 4.0,
"IP Geolocation by DB-IP", https://db-ip.com). It is downloaded each month, never committed. The privacy page
(`wiki/src/content/docs/faq/what-we-send.md`) says what is kept.

Choices, for the owner to check:

- **CSV, not MMDB.** The CSV needs no binary parser and no new crate, and its tests write a small table as text.
- **nginx `geo` for `/hub/`, not the geoip2 module.** The module is not in the AlmaLinux 8 nginx 1.24 stream as far
  as we know (not verified on the host). The core `geo` module with `ranges` reads the same table, held in nginx's
  memory; nginx keeps no address. IPv6 clients get "ZZ" (no country): the `ranges` form is documented for IPv4.
  Check the nginx worker memory after the first reload (`ps -o rss,cmd -C nginx`); the table is a few tens of MB.
- **The GitHub poll and the log fold run inside the API** (a daily job, `downloads.rs`), not as timers: the API
  reads `/var/log/razdor-hub` read-only and writes its own database.

### One-time steps (as root on the VPS)

1. Build (on the workstation): `server/deploy/build.sh`. It makes `razdor-server` (musl) in the image and the
   stage folder. Copy the static `razdor-server` to the host (the table script calls it to print the nginx ranges):
   `install -m 0755 <build output>/razdor-server /usr/local/bin/razdor-server`.
2. Load the new API image (the version stays 0.1.2 in the quadlet, so `podman load` replaces the tag), then:
   `install -d -o nginx -g nginx -m 0755 /var/log/razdor-hub`.
3. `install -m 0644 razdor-hub.logrotate /etc/logrotate.d/razdor-hub`.
4. Make the nginx include exist before the new `nginx.conf` is loaded (an empty file is valid):
   `touch /etc/nginx/razdor-geo-country.conf`.
5. `install -m 0644 nginx.conf /etc/nginx/conf.d/razdor.conf; nginx -t; systemctl reload nginx`.
6. Quadlet: `install -m 0644 razdor-api.container /etc/containers/systemd/`, `systemctl daemon-reload`,
   `systemctl restart razdor-api`.
7. The table: `install -m 0755 geo-update.sh /usr/local/sbin/razdor-geo-update`, then
   `install -m 0644 razdor-geo-update.service razdor-geo-update.timer /etc/systemd/system/`,
   `systemctl daemon-reload`, `systemctl start razdor-geo-update` and watch `journalctl -u razdor-geo-update`.
   The URL pattern `https://download.db-ip.com/free/dbip-country-lite-YYYY-MM.csv.gz` was NOT checked against the
   live site from here: if the first run says the download failed, check the month's file name on db-ip.com and
   edit `URL` in the script.
8. `systemctl enable --now razdor-geo-update.timer`.

The daily job starts ten minutes after the API starts. Its first GitHub poll is the baseline (no day's downloads
yet); the second day gives the first daily figures. The API stores nothing for the country until the table is
loaded: until then every ping says "unknown", which is the designed fallback.

