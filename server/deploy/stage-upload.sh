#!/usr/bin/env bash
# Builds the Razdor VPS files (unless told not to) and uploads them to the deploy user's staging folder,
# /home/razdor/stage/, over ONE ssh connection: the host blocks more than four ssh connections a minute, so
# this never loops. Run it on the workstation as the owner (the deploy key is ~/.ssh/razdor_vps_ed25519).
#
#   server/deploy/stage-upload.sh                 # build (build.sh: musl binaries and the API image), then upload
#   SKIP_BUILD=1 server/deploy/stage-upload.sh    # upload what is already built
#
# Then, as root on the VPS (the one-time setup, or a later change to the deploy files):
#   ssh root@<vps> bash -s < server/deploy/root-setup-2026-11-24.sh
# A routine update of the binaries needs no root script: see server/deploy/README.md, "Routine update".
#
# Env (defaults in brackets): RAZDOR_VPS_IP [217.117.29.101], RAZDOR_VPS_USER [razdor],
# RAZDOR_VPS_KEY [~/.ssh/razdor_vps_ed25519], RAZDOR_STAGE [/home/razdor/stage],
# RAZDOR_BUILD_OUT [<repo>/target/x86_64-unknown-linux-musl/release] (where build.sh put the binaries).
# On a shared machine run it pinned and at low priority: nice -n 19 taskset -c 8-19 server/deploy/stage-upload.sh
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
IP="${RAZDOR_VPS_IP:-217.117.29.101}"
USER_NAME="${RAZDOR_VPS_USER:-razdor}"
KEY="${RAZDOR_VPS_KEY:-$HOME/.ssh/razdor_vps_ed25519}"
STAGE="${RAZDOR_STAGE:-/home/razdor/stage}"
OUT="${RAZDOR_BUILD_OUT:-$REPO/target/x86_64-unknown-linux-musl/release}"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/server/Cargo.toml" | head -n 1)"
IMAGE="localhost/razdor-api:$VERSION"

if [ "${SKIP_BUILD:-0}" != 1 ]; then
  "$HERE/build.sh"
fi
[ -f "$OUT/razdor-mirror" ] && [ -f "$OUT/razdor-server" ] \
  || { echo "stage-upload: no musl binaries in $OUT (run build.sh, or set RAZDOR_BUILD_OUT)" >&2; exit 1; }
[ -f "$KEY" ] || { echo "stage-upload: no key at $KEY" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$OUT/razdor-mirror" "$OUT/razdor-server" "$WORK/"
podman save -o "$WORK/razdor-api.tar" "$IMAGE"
# The deploy files, under the names the root script expects (the same names as in this folder).
for f in nginx.conf razdor-mirror.toml razdor-mirror-sync.service razdor-mirror-sync.timer \
  razdor-mirror-alert.service razdor-geo-update.service razdor-geo-update.timer \
  razdor-mirror-sync.polkit.rules razdor-hub.logrotate razdor-api.container sudoers-razdor; do
  cp "$HERE/$f" "$WORK/$f"
done
cp "$HERE/geo-update.sh" "$WORK/geo-update.sh"
echo "staged in $WORK: $(ls "$WORK" | wc -l) files, image $IMAGE"

# One connection: the mkdir and the tar stream run in the same ssh command.
echo "uploading to $USER_NAME@$IP:$STAGE (one ssh connection)"
tar -C "$WORK" -cf - . | ssh -i "$KEY" -o IdentitiesOnly=yes \
  -o ControlMaster=auto -o "ControlPath=$HOME/.ssh/cm-%r@%h:%p" -o ControlPersist=10m \
  "$USER_NAME@$IP" "mkdir -p -m 0750 '$STAGE' && tar -C '$STAGE' --no-same-owner -xf - && ls -l '$STAGE' | wc -l"
echo "uploaded. Next (as root on the VPS): ssh root@$IP bash -s < $HERE/root-setup-2026-11-24.sh"
