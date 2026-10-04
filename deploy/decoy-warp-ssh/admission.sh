#!/bin/bash
# Apply only this transport's admissions in the existing firewall base chains.
set -euo pipefail
umask 077
state=${OP_WARP_SSH_STATE_DIR:-/run/op-warp-ssh}
rules=${OP_WARP_SSH_ADMISSION:-/etc/op-dbus/warp-ssh-admission.nft}
mkdir -p "$state"
exec 9>"$state/admission.lock"
flock -n 9
receipt=$state/admission.json
readback() {
 nft -j list ruleset | jq '[.nftables[].rule? | select((.comment // "") | startswith("op-warp-ssh/")) |
   del(.handle) | walk(if type == "object" and has("counter") then .counter={} else . end)]'
}
check() {
 [ -s "$receipt" ] || { echo 'No firewall admission receipt' >&2; exit 1; }
 readback > "$state/admission-current.json"
 cmp -s "$receipt" "$state/admission-current.json" || { echo 'Transport admissions changed externally' >&2; exit 1; }
}
case ${1:-check} in
 install)
  if [ -f "$receipt" ]; then check; exit 0; fi
  [ "$(readback | jq length)" = 0 ] || { echo 'Existing admissions have no receipt' >&2; exit 1; }
  nft --check -f "$rules"
  nft -f "$rules"
  readback > "$receipt"
  [ "$(jq length "$receipt")" -gt 0 ]
  ;;
 check) check ;;
 remove)
  check
  nft -j list ruleset | jq -r '.nftables[].rule? | select((.comment // "") | startswith("op-warp-ssh/")) |
    "delete rule \(.family) \(.table) \(.chain) handle \(.handle)"' > "$state/admission-remove.nft"
  nft --check -f "$state/admission-remove.nft"
  nft -f "$state/admission-remove.nft"
  rm "$receipt"
  ;;
 *) echo 'usage: admission.sh install|check|remove' >&2; exit 1 ;;
esac
