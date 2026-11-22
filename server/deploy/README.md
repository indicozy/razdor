# Razdor API: deploy

The API (`razdor-server`) serves analytics (`docs/superpowers/specs/2026-11-22-analytics-design.md`)
and cloud saves (`…-cloud-saves-design.md`) from one binary, behind nginx on `api.<domain>`. This
folder holds the service unit, the nginx snippet and the backup script. Nothing here holds a real
domain, address or secret: put those in the server's own files, never in git.

## Build

On a machine with Rust 1.91 or newer (the server needs no game libraries):

    cargo build --release -p razdor-server

The binary is `target/release/razdor-server`. The build compiles SQLite (`rusqlite`, bundled), so a C
compiler is needed. On the VPS, build on a faster machine and copy the binary, or build there once.

## Install

1. Users and folders:

       sudo useradd --system --home /var/lib/razdor-api --shell /usr/sbin/nologin razdor-api
       sudo install -m 0755 target/release/razdor-server /usr/local/bin/razdor-server
       sudo install -d -m 0750 -o razdor-api -g razdor-api /var/lib/razdor-api
       sudo install -d -m 0750 -o root -g razdor-api /etc/razdor-api

2. The config (`/etc/razdor-api/config.toml`, mode 0640, group `razdor-api`). Start from this; the
   values in angle brackets are the owner's:

       data_dir = "/var/lib/razdor-api"
       bind = "127.0.0.1:8080"
       trust_proxy = true          # nginx sets X-Real-IP; the service listens on localhost only
       secure_cookies = true       # the dashboard is HTTPS only
       allowed_versions = []       # empty: any X.Y.Z release; or list the exact versions
       admin_password_hash = "<from hash-password>"
       admin_token_sha256 = "<from token-hash>"

   Only the service's own limits are in the file (`[limits]`); every number has a default that
   matches the spec. Override one with `[limits]` and its name, for example `pings_per_day = 5`.

3. The admin password and token (both are hashes; the secrets themselves are never stored):

       read -r -s PW && printf '%s\n' "$PW" | /usr/local/bin/razdor-server hash-password
       read -r -s TOKEN && printf '%s\n' "$TOKEN" | /usr/local/bin/razdor-server token-hash

   Put the first line in `admin_password_hash` and the second in `admin_token_sha256`. The
   verifier and the weekly digest send the token itself in the `x-razdor-admin` header.

4. The service:

       sudo install -m 0644 server/deploy/razdor-api.service /etc/systemd/system/
       sudo systemctl daemon-reload
       sudo systemctl enable --now razdor-api
       journalctl -u razdor-api -f

   The unit caps memory at 256 MB, runs at low priority and restarts on failure. `curl -s
   127.0.0.1:8080/healthz` prints `ok`.

5. nginx: copy the server blocks of `server/deploy/nginx.conf` into the owner's nginx (replace
   `<domain>`; the certificate is Let's Encrypt, as for the other sites), then `nginx -t` and reload.

6. Backups: `server/deploy/backup.sh` (needs `sqlite3` and `rsync`). Run it daily from a timer, for
   example `OnCalendar=*-*-* 04:10` in a unit that runs `/usr/local/bin/backup.sh` as root with
   `RAZDOR_DATA_DIR` and `RAZDOR_BACKUP_DIR` set. It keeps 14 days.

## Rotate

- The admin password: run `hash-password` again, replace `admin_password_hash`, restart. Open
  sessions stay valid until they expire (12 hours); restart the service to end them at once.
- The admin token: run `token-hash` with the new token, replace `admin_token_sha256`, restart, and
  update the verifier's and the digest's copy.
- Account keys are never rotated by the server; a player who loses one deletes the account from the
  game, or links the device again from a working one.

## Delete an install's data or an account

Players do this themselves («Удалить мои данные» in the game). An owner's request follows the same
path: the API's `POST /v1/install/delete` with the install id, or `DELETE /v1/account` with the key.
Both remove the rows and the files. A ban record of the install id stays, so deleting the data does
not reset its strikes.

## Lifting a ban

On the dashboard (`/admin`), the bans table has an Unban button per active ban. The strikes stay: the
next offence is the next step (24 h, 7 days, for good).

## Load test

Start a local server with `trust_proxy = true` and a temporary data folder, then:

    cargo run --release -p razdor-server --example load -- 127.0.0.1:8080 2000 8 64 <server-pid>

The tool prints requests per second, the latency and the server's peak RAM (VmHWM of the pid). It
spreads its fake installs over fake client addresses through `X-Real-IP`, so the per-IP limits do
not trip on the loopback address; that needs `trust_proxy`.

## Logs and privacy

The service logs its own events (startup, bans with their reason, retention counts) and no client
addresses. The IP address is used in memory for the rate limits and hashed (with a salt kept in
`ip_salt` in the data folder) for the IP bans. nginx's access log is off for the API host.
