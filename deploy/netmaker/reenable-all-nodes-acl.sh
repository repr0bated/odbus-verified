#!/bin/bash
# Re-enable Netmaker default ACL 3tched.all-nodes via local API.
# Run on the VPS host where Incus container NetMaker is present:
#   ./deploy/netmaker/reenable-all-nodes-acl.sh
set -euo pipefail

API="${NETMAKER_API_BASE:-http://127.0.0.1:8081}"
API="${API%/}"

if [[ -n "${NETMAKER_MASTER_KEY:-}" ]]; then
  MK="$NETMAKER_MASTER_KEY"
elif [[ -n "${MASTER_KEY:-}" ]]; then
  MK="$MASTER_KEY"
else
  MK="$(sudo incus exec NetMaker -- grep '^MASTER_KEY=' /etc/netmaker/netmaker.env | cut -d= -f2-)"
fi

if [[ -z "$MK" ]]; then
  echo "MASTER_KEY not found (set NETMAKER_MASTER_KEY or check NetMaker env)" >&2
  exit 1
fi

BODY='{"id":"3tched.all-nodes","name":"All Nodes","network_id":"3tched","policy_type":"device-policy","src_type":[{"id":"tag","value":"*"}],"dst_type":[{"id":"tag","value":"*"}],"protocol":"all","type":"Any","ports":[],"allowed_traffic_direction":1,"enabled":true,"default":true}'

curl -sS -X PUT \
  -H "Authorization: Bearer ${MK}" \
  -H "Content-Type: application/json" \
  -d "${BODY}" \
  "${API}/api/v1/acls"
echo
