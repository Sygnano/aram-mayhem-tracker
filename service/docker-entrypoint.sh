#!/bin/sh
# Railway mounts the volume at runtime, owned by root:root — which replaces whatever ownership the
# image set at build time. A container running as an unprivileged user then cannot create the SQLite
# file, and the service crash-loops on "unable to open database file".
#
# Railway's own answer is RAILWAY_RUN_UID=0, i.e. run everything as root. Instead we start as root,
# hand the database directory to the service account, and immediately drop to it, so the service
# itself never runs privileged.
set -eu

DB_PATH="${DATABASE_PATH:-/data/cache.db}"
DB_DIR=$(dirname "$DB_PATH")
APP_UID=10001
APP_GID=10001

if [ "$(id -u)" = "0" ]; then
    mkdir -p "$DB_DIR"
    # Recursive, so a database written during an earlier root-only deploy is adopted rather than
    # left unreadable.
    chown -R "$APP_UID:$APP_GID" "$DB_DIR"
    # setpriv comes from util-linux, which is Essential in Debian and present in bookworm-slim.
    exec setpriv --reuid="$APP_UID" --regid="$APP_GID" --clear-groups "$@"
fi

# Already unprivileged — for instance if RAILWAY_RUN_UID is set to something other than 0. Nothing
# to fix up; run as we are and let the service report a clear error if the volume is not writable.
exec "$@"
