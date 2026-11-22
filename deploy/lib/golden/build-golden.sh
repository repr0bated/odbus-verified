#!/bin/sh
# Front door for the permanent writable @opdbus-system-a build OS and
# immutable @upper release snapshots. Source edits and rollback trees are
# retained; there is no Incus/ublk build-root fallback.
# Dry-run is the default. See deploy/lib/golden/README.md.
# The previous application-payload stager is build-golden-payload.sh.
set -eu
DIR=$(CDPATH='' cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
exec "$DIR/golden.sh" "$@"
