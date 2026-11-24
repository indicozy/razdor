#!/bin/sh
# /usr/local/sbin/razdor-install-bin (root:root 0755): the only root step the razdor deploy user has for a
# routine binary update. Installed by server/deploy/root-setup-2026-11-24.sh. The sudo rule allows exactly:
#   /usr/local/sbin/razdor-install-bin mirror
#   /usr/local/sbin/razdor-install-bin server
# It installs /home/razdor/stage/razdor-mirror or razdor-server as /usr/local/bin/razdor-mirror or
# razdor-server. The staging folder is razdor-owned, so the file is never used in place: it is copied
# without following links into a root-owned folder, and the COPY is checked (a regular file, not a link,
# a sane size, the ELF magic). The check is on the copy, so a swap after the check changes nothing.
# Exit codes: 2 wrong arguments; 1 refused (the message says why); 0 installed (or, when not root in a
# test, checked only).
#
# Env overrides exist for the test suite (server/deploy/test_deploy.py) and are honoured ONLY when the
# caller is not root. The sudo rule runs this as root, so they cannot reach a real install.
umask 077
PATH=/usr/bin:/bin:/usr/sbin:/sbin
export PATH

STAGE=/home/razdor/stage
DEST_DIR=/usr/local/bin
WORK_ROOT=/var/lib/razdor-install
MAX_BYTES=209715200   # 200 MiB: far above a static Razdor binary
OWN="-o root -g root"

if [ "$(id -u)" != 0 ]; then
    STAGE="${RZ_INSTALL_STAGE:-$STAGE}"
    DEST_DIR="${RZ_INSTALL_DEST:-$DEST_DIR}"
    WORK_ROOT="${RZ_INSTALL_WORK:-$WORK_ROOT}"
    OWN=""
fi

die() {
    echo "razdor-install-bin: refused: $*" >&2
    exit 1
}

[ $# -eq 1 ] || { echo "usage: razdor-install-bin mirror|server" >&2; exit 2; }
case "$1" in
    mirror|server) NAME="razdor-$1" ;;
    *) echo "razdor-install-bin: the target is mirror or server" >&2; exit 2 ;;
esac

[ ! -L "$STAGE" ] || die "the staging folder $STAGE is a symlink"
[ -d "$STAGE" ] || die "the staging folder $STAGE is missing"
[ -d "$WORK_ROOT" ] && [ ! -L "$WORK_ROOT" ] || die "the work folder $WORK_ROOT is missing"
[ ! -L "$DEST_DIR" ] || die "$DEST_DIR is a symlink"

[ ! -L "$STAGE/$NAME" ] || die "$NAME in the staging folder is a symlink"
[ -f "$STAGE/$NAME" ] || die "$NAME is not in the staging folder"

WORK="$(mktemp -d "$WORK_ROOT/bin.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
COPY="$WORK/$NAME"

# cp -P: a link is copied as a link (then refused below), never followed.
( ulimit -f 409600 2>/dev/null; cp -P -- "$STAGE/$NAME" "$COPY" ) || die "the copy of $NAME failed"

[ -f "$COPY" ] && [ ! -L "$COPY" ] || die "$NAME is not a regular file (a link or a special file)"
SIZE="$(stat -c %s "$COPY")"
[ "$SIZE" -gt 0 ] && [ "$SIZE" -le "$MAX_BYTES" ] || die "$NAME has size $SIZE, outside 1..$MAX_BYTES bytes"
MAGIC="$(od -An -tx1 -N4 "$COPY" | tr -d ' \n')"
[ "$MAGIC" = 7f454c46 ] || die "$NAME is not an ELF file"
SHA="$(sha256sum "$COPY" | cut -d' ' -f1)"

# shellcheck disable=SC2086 # $OWN is two words or empty on purpose
install -m 0755 $OWN "$COPY" "$DEST_DIR/$NAME"
echo "razdor-install-bin: installed $DEST_DIR/$NAME, $SIZE bytes, sha256 $SHA"
