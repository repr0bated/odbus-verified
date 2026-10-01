#!/usr/bin/env bash
# Root-owned disposable stubs only; never connects to Incus, Cozo or live lungs.
set -euo pipefail
[[ $(id -u) == 0 ]] || { printf 'Run with sudo bash %s\n' "$0" >&2; exit 1; }
PROVISIONER=$(cd "$(dirname "$0")/.." && pwd)/provision-workspace-subscriber.sh
T=$(mktemp -d /root/provision-identity-test.XXXXXX)
trap 'rm -rf -- "$T"' EXIT
mkdir -m 700 "$T/bin" "$T/catalog"
export FIX="$T" EVENT_LOG="$T/events"
export SID=b0c777e3-0955-cd8b-3d01-ca12998cae28
export OPERATOR=414fbc56-03fb-20ab-758a-bcc7a7cbdc0e
export KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
export FSID=806e2801-8be0-4b0d-919c-3e319452667b
export PATH="$T/bin:/usr/bin:/bin"
cat > "$T/bin/incus" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'incus:%s\n' "$1" >> "$EVENT_LOG"
case "$1" in
query)
 case "$2" in
  '/1.0/instances?recursion=1')
   if [[ ${EXISTING:-0} == 1 ]]; then jq -cn --arg sid "$SID" '[{name:$sid}]'; else printf '[]\n'; fi ;;
  /1.0/storage-pools/identity-roots) printf '{"name":"identity-roots","driver":"dir","status":"Created"}\n' ;;
  "/1.0/instances/$SID")
   jq -cn --arg sid "$SID" --arg status "${NATIVE_STATUS_OVERRIDE:-$(cat "$FIX/native-status")}" '{name:$sid,status:$status,config:{"user.opdbus.session_id":$sid}}' ;;
  *) exit 99 ;;
 esac ;;
init)
 [[ $2 == --empty && $3 == "$SID" ]] || exit 99
 [[ "$*" == *'--storage identity-roots --no-profiles'* ]] || exit 99
 [[ "$*" == *'security.syscalls.deny_default=true'* ]] || exit 99
 for request in 0x4008941a 0x5000940f 0x5000943f; do
   [[ "$*" == *"ioctl errno 1 [1,$request,SCMP_CMP_MASKED_EQ,0xffffffff]"* ]] || exit 99
 done
 [[ "$*" != *ghostbridge* && "$*" != *private.key* ]] || exit 99
 [[ ${INIT_FAIL:-0} == 0 ]] || exit 12
 printf Stopped > "$FIX/native-status" ;;
start) [[ ${START_FAIL:-0} == 0 ]] || exit 13; printf Running > "$FIX/native-status" ;;
*) exit 99 ;;
esac
STUB
cat > "$T/bin/physical-helper" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
printf 'helper:%s\n' "$1" >> "$EVENT_LOG"
[[ $THREETCHED_FS == "$FIX/bin/fs" && $OP_IDENTITY_SEED_CATALOG == "$FIX/catalog" &&
   $OP_IDENTITY_SEED_LUNGS_HOOK == "$FIX/bin/op-identity-lungs-hook" ]] || exit 99
[[ ${HELPER_FAIL:-} != "$1" ]] || exit 21
case "$1" in
seed-create)
 [[ "$2" == "$SID" && "$3" == "$SID" && ! -e "$4" ]] || exit 99
 mkdir -m 700 -p "$OP_IDENTITY_SEED_STATE/$SID/volume"
 : > "$4"; chmod 600 "$4"
 jq -cn --arg sid "$SID" --arg leaf "$4" --arg fsid "$FSID" \
  '{version:1,name:$sid,session:$sid,phase:"ready",leaf:$leaf,sprout_fsid:$fsid}' > "$OP_IDENTITY_SEED_STATE/$SID/state.json"
 chmod 600 "$OP_IDENTITY_SEED_STATE/$SID/state.json" ;;
seed-check|seed-lungs) [[ "$2" == "$SID" ]] || exit 99 ;;
seed-enable-boot)
 [[ "$2" == "$SID" && ( "$3" == true || "$3" == false ) ]] || exit 99
 [[ "$(cat "$FIX/native-status")" == "$([[ $3 == true ]] && printf Running || printf Stopped)" ]] || exit 99
 printf '%s\n' "$3" > "$FIX/boot-policy" ;;
*) exit 99 ;;
esac
STUB
cat > "$T/bin/fs" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
if [[ $3 == capture ]]; then
 [[ "$*" == *--state* && "$*" == *empty-state* ]] || exit 99
 printf 'capture\n' >> "$EVENT_LOG"; exit 0
fi
[[ $3 == mcp && "$*" == *"--lungs-session $OPERATOR"* && "$*" == *'--lungs-socket /run/opdbus/lungs.sock'* ]] || exit 99
while IFS= read -r line; do
 case "$(jq -r .method <<<"$line")" in
 initialize) printf '{"jsonrpc":"2.0","id":1,"result":{}}\n' ;;
 tools/call)
  tool=$(jq -r .params.name <<<"$line")
  printf 'mcp:%s\n' "$tool" >> "$EVENT_LOG"
  [[ ${MCP_FAIL:-} != "$tool" ]] || { printf '{"id":2,"result":{"isError":true,"content":[{"type":"text","text":"SECRET_ERROR_SENTINEL"}]}}\n'; continue; }
  case "$tool" in
  identity_sled__inspect_identity_inventory)
   printf '{"id":2,"result":{"structuredContent":{"success":true,"result":{"counts":{},"entries":[]}}}}\n' ;;
  identity_sled__get_identity)
   if [[ ${BAD_PAIR:-0} == 1 ]]; then
     jq '.session_id="wrong"' "$FIX/current.json" > "$FIX/change.json"; mv "$FIX/change.json" "$FIX/current.json"
   fi
   if [[ ${CHANGE_DURING_PREP:-0} == 1 && -e "$FIX/native-status" ]]; then
     jq '.mutation_index=12' "$FIX/current.json" > "$FIX/change.json"; mv "$FIX/change.json" "$FIX/current.json"
   fi
   jq -cn --arg owner "$OPERATOR" --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{identity:($i[0]|if .session_id==$owner then . else del(.sealed_id) end)}}}}' ;;
  identity_sled__write_identity)
   jq -e '.params.arguments | (keys|sort)==(["wireguard_pubkey","btrfs_device"]|sort) and .btrfs_device.attached==false' >/dev/null <<<"$line" || exit 99
   binding=$(jq -c '.params.arguments.btrfs_device' <<<"$line")
   jq --argjson b "$binding" '.btrfs_device=$b' "$FIX/current.json" > "$FIX/change.json"; mv "$FIX/change.json" "$FIX/current.json"
   jq -cn --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{identity:($i[0]|del(.sealed_id))}}}}' ;;
  identity_sled__attach_btrfs_device)
   jq '.btrfs_device.attached=true' "$FIX/current.json" > "$FIX/change.json"; mv "$FIX/change.json" "$FIX/current.json"
   jq -cn --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{identity:($i[0]|del(.sealed_id))}}}}' ;;
  identity_sled__touch_session)
   if jq -e '.active==false' "$FIX/current.json" >/dev/null; then printf Running > "$FIX/native-status"; fi
   jq '.active=true' "$FIX/current.json" > "$FIX/change.json"; mv "$FIX/change.json" "$FIX/current.json"
   jq -cn --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{identity:($i[0]|del(.sealed_id))}}}}' ;;
  *) exit 99 ;;
  esac ;;
 esac
done
STUB
printf '#!/bin/sh\nexit 0\n' > "$T/bin/op-identity-lungs-hook"
chmod 755 "$T/bin/"*
export OP_IDENTITY_ROOT_HELPER="$T/bin/physical-helper" THREETCHED_FS="$T/bin/fs"
export OP_IDENTITY_SEED_CATALOG="$T/catalog"
reset_fixture() {
 rm -rf -- "$T/state" "$T/receipts" "$T/leaf" "$T/native-status" "$T/boot-policy"
 mkdir -m 700 "$T/leaf"
 export OP_IDENTITY_SEED_STATE="$T/state" OP_IDENTITY_PROVISION_RECEIPTS="$T/receipts"
 : > "$EVENT_LOG"
 jq -n --arg sid "$SID" --arg key "$KEY" '{
  session_id:$sid,wireguard_pubkey:$key,sealed_id:"SID1_SECRET_SENTINEL",genesis:"GENESIS_SECRET_SENTINEL",
  arrival_timestamp:1,chain_head_at_arrival:"CHAIN_SECRET_SENTINEL",
  private_key:"PRIVATE_SECRET_SENTINEL",interface:"keep-interface",peer_ip:"100.69.0.10/32",
  instance:{name:$sid,type:"container"},active:false,mutation_index:0,btrfs_device:{device_path:"/old/leaf",mount_point:"/old/mount",
   btrfs_uuid:"old-fsid",cozo_id:"KEEP_OWNER_KEY",attached:true}
 }' > "$T/current.json"
}
run_flow() { "$PROVISIONER" owner --session-id "$SID" --pubkey "$KEY" --operator-session "$OPERATOR" --leaf-path "$T/leaf/new.img" "$@"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
assert_no() { ! grep -E "$1" "$EVENT_LOG" >/dev/null || fail "unexpected call $1"; }
expect_failure() {
 if run_flow "$@" > "$T/output" 2>&1; then fail 'flow unexpectedly succeeded'; fi
 ! grep -E 'SECRET_.*SENTINEL' "$T/output" >/dev/null || fail 'error leaked secret'
 [[ ! -e "$T/boot-policy" ]] || fail 'failed operation enabled boot'
}
reset_fixture
run_flow --no-start > "$T/output" 2>&1
assert_no 'incus:start'
jq -e '.btrfs_device.attached==true and .btrfs_device.cozo_id=="KEEP_OWNER_KEY" and .interface=="keep-interface" and .peer_ip=="100.69.0.10/32"' "$T/current.json" >/dev/null
! grep -R -E 'SID1_SECRET|GENESIS_SECRET|PRIVATE_SECRET|CHAIN_SECRET' "$T/receipts" "$T/output" >/dev/null || fail 'persisted secret'
[[ $(cat "$T/native-status") == Stopped ]] || fail '--no-start changed state'
[[ $(cat "$T/boot-policy") == false ]] || fail '--no-start boot policy starts guest'
reset_fixture
jq '.active=true' "$T/current.json" > "$T/change.json"; mv "$T/change.json" "$T/current.json"
run_flow --no-start > "$T/output" 2>&1
jq -e '.active==true' "$T/current.json" >/dev/null || fail '--no-start changed prior active metadata'
assert_no 'mcp:identity_sled__touch_session|incus:start'
reset_fixture
jq '.active=true' "$T/current.json" > "$T/change.json"; mv "$T/change.json" "$T/current.json"
expect_failure; assert_no 'incus:|helper:|mcp:identity_sled__write'
# Inventory before native mutation, root verification before write, attach before start.
reset_fixture
jq '.active=false' "$T/current.json" > "$T/change.json"; mv "$T/change.json" "$T/current.json"
run_flow > "$T/output" 2>&1
jq -e '.active==true' "$T/current.json" >/dev/null || fail 'canonical activation left identity inactive'
[[ $(cat "$T/boot-policy") == true ]] || fail 'normal boot policy does not start guest'
awk '/mcp:identity_sled__inspect_identity_inventory/{gate=NR}
 /incus:init/{init=NR} /helper:seed-lungs/{lungs=NR}
 /mcp:identity_sled__write_identity/{write=NR} /mcp:identity_sled__attach_btrfs_device/{attach=NR}
 /mcp:identity_sled__touch_session/{start=NR} /helper:seed-enable-boot/{boot=NR}
 END{exit !(gate<init && init<lungs && lungs<write && write<attach && attach<start && start<boot)}' "$EVENT_LOG" || fail 'unsafe ordering'
assert_no 'incus:start'
reset_fixture; export MCP_FAIL=identity_sled__inspect_identity_inventory
expect_failure; assert_no 'incus:|helper:|mcp:identity_sled__write'; unset MCP_FAIL
reset_fixture; export BAD_PAIR=1
expect_failure; assert_no 'incus:|helper:|mcp:identity_sled__write'; unset BAD_PAIR
for invalid_anchor in 'del(.arrival_timestamp)' '.arrival_timestamp=0' '.chain_head_at_arrival=""' 'del(.chain_head_at_arrival)' '.genesis=""' 'del(.instance)' '.instance.name="wrong"'; do
 reset_fixture
 jq "$invalid_anchor" "$T/current.json" > "$T/change.json"; mv "$T/change.json" "$T/current.json"
 expect_failure; assert_no 'incus:|helper:|mcp:identity_sled__write'
done
reset_fixture; export EXISTING=1
expect_failure; assert_no 'incus:init|helper:|mcp:identity_sled__write'; unset EXISTING
reset_fixture; export HELPER_FAIL=seed-check
expect_failure; assert_no 'mcp:identity_sled__write|incus:start'; unset HELPER_FAIL
reset_fixture; export CHANGE_DURING_PREP=1
expect_failure; assert_no 'mcp:identity_sled__write|incus:start'; unset CHANGE_DURING_PREP
for tool in identity_sled__write_identity identity_sled__attach_btrfs_device identity_sled__touch_session; do
 reset_fixture; export MCP_FAIL="$tool"
 expect_failure; assert_no 'incus:start'; unset MCP_FAIL
 [[ ! -e "$T/boot-policy" ]] || fail 'failed operation enabled boot'
done
reset_fixture; export INIT_FAIL=1
expect_failure; assert_no 'helper:|mcp:identity_sled__write|incus:start'; unset INIT_FAIL
reset_fixture; export NATIVE_STATUS_OVERRIDE=Broken
expect_failure; assert_no 'helper:seed-enable-boot'; unset NATIVE_STATUS_OVERRIDE
reset_fixture; export HELPER_FAIL=seed-enable-boot
expect_failure; unset HELPER_FAIL
reset_fixture
for option in --recreate --private-key-file --identity-dir --ghostbridge; do
 if "$PROVISIONER" owner "$option" ignored > "$T/output" 2>&1; then fail "accepted $option"; fi
 [[ ! -s "$EVENT_LOG" ]] || fail 'legacy option had effects'
done
printf 'PASS: existing-identity physical-root guards, order, redaction and preservation\n'
