#!/usr/bin/env bash
# Invoke one plugin method through the supported same-handle mounted client.
# The mount must already be bound to the caller's identity
# (e.g. /run/mounts/3tchedFS + /run/opdbus/lungs/lungs.sock).
#
# Capability grants and op-grants-materializer are not exposed here; durable policy
# stays under /etc/opdbus/local/ with owner-approved deltas only.
#
# Usage:
#   deploy/scripts/3tchedfs-invoke.sh <plugin> <method> [json-args] [--confirm-side-effects]
#   OP_3TCHEDFS_MOUNT=/run/mounts/3tchedFS deploy/scripts/3tchedfs-invoke.sh tched_router config_get '{"path":"agents.dashboard.model_provider"}'
set -euo pipefail

die() { echo "3tchedfs-invoke: $*" >&2; exit 1; }

MOUNT="${OP_3TCHEDFS_MOUNT:-/run/mounts/3tchedFS}"
CLIENT="${THREETCHED_FS:-/usr/local/libexec/op-dbus/bin/3tched-fs}"
PLUGIN="${1:?plugin id}"
METHOD="${2:?method name}"
shift 2
ARGS='{}'
if [[ $# -gt 0 && $1 != --confirm-side-effects ]]; then ARGS=$1; shift; fi
options=()
if [[ ${1:-} == --confirm-side-effects ]]; then options+=(--confirm-side-effects); shift; fi
[[ $# == 0 ]] || die 'pass one JSON argument and optional --confirm-side-effects'
[[ $PLUGIN =~ ^[a-zA-Z0-9_-]+$ && $METHOD =~ ^[a-zA-Z0-9_-]+$ ]] || die 'unsafe plugin or method name'
[[ -x $CLIENT ]] || die "mounted client absent: $CLIENT"
if [[ -n ${OP_3TCHEDFS_EXPECTED_SESSION:-} ]]; then
  options+=(--expected-session "$OP_3TCHEDFS_EXPECTED_SESSION")
fi

[[ -f "$MOUNT/.3tchedfs.json" ]] || die "not a 3tchedFS root: $MOUNT"
case "$(findmnt -n -o FSTYPE -T "$MOUNT" 2>/dev/null || true)" in
  fuse|fuse.3tchedFS) ;;
  *) die "namespace is not a fuse 3tchedFS mount: $MOUNT" ;;
esac

OP="$MOUNT/plugins/$PLUGIN/operations/$METHOD"
[[ -d "$OP" ]] || die "operation absent: $PLUGIN/$METHOD"
[[ -f "$OP/input.schema.json" ]] || die "missing input schema: $OP"

if ! printf '%s' "$ARGS" | jq -e . >/dev/null 2>&1; then
  die "json-args must be valid JSON"
fi

# invoke validates the sealed schema and pinned mount, fsyncs and reads the
# receipt on the same descriptor. Preserve its diagnostic and exit status;
# never retry an uncertain mutation or fall back to a different transport.
exec "$CLIENT" invoke --mount "$MOUNT" "${options[@]}" --args "$ARGS" \
  "plugins/$PLUGIN/operations/$METHOD/call"
