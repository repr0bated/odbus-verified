#!/usr/bin/env bash
# Invoke one plugin method through a mounted 3tchedFS namespace (write `call`, read
# `result` / `error`). The mount must already be bound to the caller's identity
# (e.g. /run/mount/3tchedFS + /run/opdbus/lungs.sock).
#
# Capability grants and op-grants-materializer are not exposed here; durable policy
# stays under /etc/opdbus/local/ with owner-approved deltas only.
#
# Usage:
#   deploy/scripts/3tchedfs-invoke.sh <plugin> <method> [json-args]
#   OP_3TCHEDFS_MOUNT=/run/mount/3tchedFS deploy/scripts/3tchedfs-invoke.sh tched_router config_get '{"path":"agents.dashboard.model_provider"}'
set -euo pipefail

die() { echo "3tchedfs-invoke: $*" >&2; exit 1; }

MOUNT="${OP_3TCHEDFS_MOUNT:-/run/mount/3tchedFS}"
PLUGIN="${1:?plugin id}"
METHOD="${2:?method name}"
shift 2
ARGS="${1:-'{}'}"
if [[ $# -gt 1 ]]; then
  die "pass json-args as one quoted argument"
fi
TIMEOUT_SEC="${OP_3TCHEDFS_INVOKE_TIMEOUT:-30}"

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

# Clear stale receipt files and write fresh call
>"$OP/result" 2>/dev/null || true
>"$OP/error" 2>/dev/null || true
printf '%s' "$ARGS" >"$OP/call"

deadline=$((SECONDS + TIMEOUT_SEC))
while ((SECONDS < deadline)); do
  if [[ -s "$OP/error" ]]; then
    cat "$OP/error" >&2
    exit 1
  fi
  if [[ -s "$OP/result" ]]; then
    jq . "$OP/result"
    exit 0
  fi
  sleep 0.2
done

die "timed out after ${TIMEOUT_SEC}s waiting for $PLUGIN/$METHOD"
