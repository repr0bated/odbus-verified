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
export NATIVE_NAME=3tchedChatbot
export OPERATOR=414fbc56-03fb-20ab-758a-bcc7a7cbdc0e
export KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=
export FSID=806e2801-8be0-4b0d-919c-3e319452667b
export PATH="$T/bin:/usr/bin:/bin"
# Rewrite only the mount/socket paths in a disposable copy. Production has no
# override allowing an arbitrary directory to impersonate the mounted lungs.
sed -e "s|^LUNGS_MOUNT=/run/mount/3tchedFS$|LUNGS_MOUNT=$T/lungs|" \
    -e 's/\[\[ -S $INCUS_SOCKET \]\]/[[ -f $FIX\/socket-fixture ]]/' \
    "$PROVISIONER" > "$T/provisioner"
chmod 700 "$T/provisioner"
PROVISIONER="$T/provisioner"
: > "$T/socket-fixture"
printf '{"catalog_hash":"fixture-catalog"}\n' > "$T/catalog/.manifest.json"
mkdir -p "$T/lungs/plugins/identity_sled/operations"
for method in inspect_identity_inventory inspect_provision_preflight provision_container get_identity touch_session; do
 mkdir "$T/lungs/plugins/identity_sled/operations/$method"
 for file in input.schema.json call result error; do
  : > "$T/lungs/plugins/identity_sled/operations/$method/$file"
 done
done
cat > "$T/bin/findmnt" <<'STUB'
#!/usr/bin/env bash
[[ "$*" == "-rn -M $FIX/lungs -o FSTYPE" ]] || exit 99
[[ ${MOUNT_ABSENT:-0} == 0 ]] || exit 1
printf 'fuse.3tchedFS\n'
STUB
cat > "$T/bin/jq" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
last=${!#}
if [[ $last == "$FIX/lungs/plugins/identity_sled/operations/"*/result ]]; then
 operation=${last%/result}; method=${operation##*/}
 "$FIX/bin/operation-result" "$method" "$operation/call" > "$FIX/response.json"
 /usr/bin/jq -c --arg method "$method" '{plugin_id:"identity_sled",method:$method,result:.result.structuredContent}' "$FIX/response.json" > "$last"
fi
exec /usr/bin/jq "$@"
STUB
cat > "$T/bin/fs" <<'STUB'
#!/usr/bin/env bash
exit 99 # No MCP or schema-only capture in the operator provisioning path.
STUB
cat > "$T/bin/incus" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
[[ $1 == --force-local ]] || exit 99
shift
printf 'incus:%s\n' "$1" >> "$EVENT_LOG"
case "$1" in
query)
 case "$2" in
  '/1.0/instances?recursion=1')
   if [[ ${EXISTING:-0} == 1 ]]; then jq -cn --arg name "$NATIVE_NAME" '[{name:$name}]'; else printf '[]\n'; fi ;;
  /1.0/storage-pools/identity-roots) printf '{"name":"identity-roots","driver":"dir","status":"Created","config":{"source":"/fixture/pool"}}\n' ;;
  "/1.0/instances/$NATIVE_NAME")
   jq -cn --arg sid "$SID" --arg name "$NATIVE_NAME" --arg status "${NATIVE_STATUS_OVERRIDE:-$(cat "$FIX/native-status")}" '{name:$name,status:$status,config:{"user.opdbus.session_id":$sid}}' ;;
  "/1.0/instances/${INSTANCE_NAME_OVERRIDE:-$SID}")
   status=Stopped
   [[ ${PREVIOUS_RUNNING:-0} == 0 ]] || status=Running
   jq -cn --arg name "${INSTANCE_NAME_OVERRIDE:-$SID}" --arg status "$status" \
    '{name:$name,type:"container",status:$status,config:{"volatile.uuid":"prior-native-uuid"}}' ;;
  *) exit 99 ;;
 esac ;;
init)
 [[ $2 == --empty && $3 == "$NATIVE_NAME" ]] || exit 99
 [[ "$*" == *'--storage identity-roots --profile 3tched-lungs'* ]] || exit 99
 [[ "$*" == *'security.syscalls.deny_default=true'* ]] || exit 99
 [[ "$*" == *'security.protection.shift=true'* ]] || exit 99
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
[[ $THREETCHED_FS == "$FIX/bin/fs" && $OP_IDENTITY_SEED_CATALOG == "$FIX/catalog" ]] || exit 99
[[ ${HELPER_FAIL:-} != "$1" ]] || exit 21
case "$1" in
seed-create)
 [[ "$2" == "$NATIVE_NAME" && "$3" == "$SID" && ! -e "$4" ]] || exit 99
 mkdir -m 700 -p "$OP_IDENTITY_SEED_STATE/$NATIVE_NAME/volume"
 : > "$4"; chmod 600 "$4"
 jq -cn --arg sid "$SID" --arg name "$NATIVE_NAME" --arg leaf "$4" --arg fsid "$FSID" \
  --arg layout "${RECEIPT_LAYOUT:-shared-partition-siblings-v2}" \
  '{version:1,layout:$layout,name:$name,session:$sid,phase:"ready",leaf:$leaf,sprout_fsid:$fsid,
    seed_fsid:"b6561d54-3c69-4897-a2f3-79b867e33601",seed_generation:"40",leaf_inode:"123",leaf_size:"1073741824",
    members:{"@":{id:"256",uuid:"fixture-root"},"@agent_memory":{id:"257",uuid:"fixture-cache"},
     "@agent_rules":{id:"258",uuid:"fixture-log"},"@agent_shared":{id:"259",uuid:"fixture-state"},
     "@agent_skills":{id:"260",uuid:"fixture-timing"},"@agent_user":{id:"261",uuid:"fixture-user"},
     "@agent_soul":{id:"262",uuid:"fixture-vector"}},persist:{inode:"456"},root:"/fixture/root"}' > "$OP_IDENTITY_SEED_STATE/$NATIVE_NAME/state.json"
 chmod 600 "$OP_IDENTITY_SEED_STATE/$NATIVE_NAME/state.json" ;;
seed-preflight|seed-profile) [[ $# == 1 ]] || exit 99 ;;
seed-check|seed-lungs) [[ "$2" == "$NATIVE_NAME" ]] || exit 99 ;;
seed-enable-boot)
 [[ "$2" == "$NATIVE_NAME" && ( "$3" == true || "$3" == false ) ]] || exit 99
 [[ "$(cat "$FIX/native-status")" == "$([[ $3 == true ]] && printf Running || printf Stopped)" ]] || exit 99
 printf '%s\n' "$3" > "$FIX/boot-policy" ;;
*) exit 99 ;;
esac
STUB
cat > "$T/bin/operation-result" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
tool=identity_sled__$1
line=$(jq -cn --arg name "$tool" --slurpfile args "$2" '{params:{name:$name,arguments:$args[0]}}')
printf 'lungs:%s\n' "$tool" >> "$EVENT_LOG"
[[ ${MCP_FAIL:-} != "$tool" ]] || { printf '{"result":{"structuredContent":{"success":false}}}\n'; exit 0; }
  case "$tool" in
  identity_sled__inspect_identity_inventory)
   printf '{"id":2,"result":{"structuredContent":{"success":true,"result":{"counts":{},"entries":[]}}}}\n' ;;
  identity_sled__inspect_provision_preflight)
   [[ ${PREVIOUS_RUNNING:-0} == 0 ]] || exit 21
   revision=7; operation=prior-op
   if [[ ${CHANGE_DURING_PREP:-0} == 1 && -e "$FIX/native-status" ]]; then revision=8; fi
   if [[ ${CHANGE_OPERATION_DURING_PREP:-0} == 1 && -e "$FIX/native-status" ]]; then operation=changed-op; fi
   jq -cn --argjson revision "$revision" --arg operation "$operation" --arg key "$KEY" --arg sid "$SID" \
    '{id:2,result:{structuredContent:{success:true,result:{session_id:$sid,wireguard_pubkey:$key,
      expected_revision:$revision,expected_revision_operation_id:$operation,
      previous_instance:(if env.DELETED=="1" then null else {name:env.SID,type:"container"} end),
      previous_instance_etag:(if env.DELETED=="1" then "" else "\"prior-etag\"" end),
      previous_native_instance:(if env.DELETED=="1" then null else {name:env.SID,type:"container",status:"Stopped"} end)}}}}' ;;
  identity_sled__get_identity)

   index=0
   if [[ ${CHANGE_DURING_PREP:-0} == 1 && -e "$FIX/native-status" ]]; then index=8; fi
   jq -cn --argjson index "$index" --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{identity:($i[0]|del(.sealed_id)|.mutation_index=$index)}}}}' ;;
  identity_sled__provision_container)
   plan="$OP_IDENTITY_PROVISION_PLAN_ROOT/$SID.json"
   [[ -f $plan && ! -L $plan && $(stat -c %a "$plan") == 600 ]] || exit 99
   digest=$(sha256sum "$plan"); digest=${digest%% *}
   jq -e --arg key "$KEY" --arg digest "$digest" \
    '.params.arguments=={wireguard_pubkey:$key,plan_sha256:$digest}' >/dev/null <<<"$line" || exit 99
   jq -e --arg sid "$SID" --arg key "$KEY" --arg name "$NATIVE_NAME" '
    (keys|sort)==(["version","session_id","wireguard_pubkey","expected_revision","expected_revision_operation_id",
     "binding","seed_device","seed_fsid","seed_generation","leaf_inode","leaf_size","members","persist_inode",
     "native_root","pool_name","pool_source","instance_etag","instance","previous_instance_etag","previous_instance"]|sort)
    and .version==(if env.DELETED=="1" then 3 else 2 end) and .session_id==$sid and .wireguard_pubkey==$key and .expected_revision==7
    and .expected_revision_operation_id=="prior-op" and .binding.cozo_id==$sid and .binding.attached==true
    and .instance.name==$name and .instance.status=="Stopped" and .instance_etag=="\"fresh-etag\""
    and (if env.DELETED=="1" then .previous_instance==null and .previous_instance_etag==""
         else .previous_instance.name==env.SID and .previous_instance.status=="Stopped"
              and .previous_instance_etag=="\"prior-etag\"" end)
    and .pool_source=="/fixture/pool" and (.members|length)==7
    and .instance.devices.lungs.type=="disk" and .instance.devices.workspace.type=="disk"
    and .seed_device==env.OP_IDENTITY_SEED_DEVICE and .leaf_inode==123 and .persist_inode==456' "$plan" >/dev/null || exit 99
   ! grep -E 'SECRET|keep-interface|100.69.0.10|KEEP_OWNER_KEY' "$plan" >/dev/null || exit 99
   jq -n --arg sid "$SID" --arg key "$KEY" --arg name "$NATIVE_NAME" --slurpfile plan "$plan" '{
    session_id:$sid,wireguard_pubkey:$key,principal_kind:"service",instance:{name:$name,type:"container"},
    genesis:"FRESH_GENESIS_SECRET_SENTINEL",arrival_timestamp:2,chain_head_at_arrival:"FRESH_CHAIN_SECRET_SENTINEL",
    interface:"",peer_ip:null,active:false,mutation_index:0,btrfs_device:$plan[0].binding}' > "$FIX/current.json"
   [[ ${BAD_ADOPTION_DIGEST:-0} == 0 ]] || digest=ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
   jq -cn --arg digest "$digest" --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{
    identity:$i[0],plan_sha256:$digest,operation_id:"fresh-op",committed_revision:8,replayed:false}}}}' ;;
  identity_sled__touch_session)
   if jq -e '.active==false' "$FIX/current.json" >/dev/null; then printf Running > "$FIX/native-status"; fi
   jq '.active=true' "$FIX/current.json" > "$FIX/change.json"; mv "$FIX/change.json" "$FIX/current.json"
   jq -cn --slurpfile i "$FIX/current.json" '{id:2,result:{structuredContent:{success:true,result:{identity:($i[0]|del(.sealed_id))}}}}' ;;
  *) exit 99 ;;
  esac
STUB
cat > "$T/bin/host-folders-helper" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
[[ $1 == "$NATIVE_NAME" && $(cat "$FIX/native-status") == Stopped ]] || exit 99
printf 'host-folders\n' >> "$EVENT_LOG"
[[ ${HOST_FOLDERS_FAIL:-0} == 0 ]] || exit 22
STUB
cat > "$T/bin/curl" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
[[ ${CURL_FAIL:-0} == 0 ]] || exit 23
[[ "$*" == *'--unix-socket /var/lib/incus/unix.socket'* && "$*" == *'--noproxy *'* ]] || exit 99
headers= response=
url=${!#}
while (( $# )); do
 case "$1" in --dump-header) headers=$2; shift;; --output) response=$2; shift;; esac
 shift
done
[[ -n $headers && -n $response ]] || exit 99
previous_url=http://unix.socket/1.0/instances/${INSTANCE_NAME_OVERRIDE:-$SID}
if [[ $url == "$previous_url" && $url != "http://unix.socket/1.0/instances/$NATIVE_NAME" ]]; then
 printf 'curl:previous-pin\n' >> "$EVENT_LOG"
 printf 'HTTP/1.1 200 OK\r\n' > "$headers"
 etag='"prior-etag"'
 if [[ ${CHANGE_OPERATION_DURING_PREP:-0} == 1 && -e "$FIX/native-status" ]]; then etag='"changed-etag"'; fi
 printf 'ETag: %s\r\n' "$etag" >> "$headers"
 jq -cn --arg name "${INSTANCE_NAME_OVERRIDE:-$SID}" '{type:"sync",status_code:200,metadata:{name:$name,type:"container",status:"Stopped"}}' > "$response"
 exit 0
fi
printf 'curl:native-pin\n' >> "$EVENT_LOG"
[[ $url == "http://unix.socket/1.0/instances/$NATIVE_NAME" && $(cat "$FIX/native-status") == Stopped ]] || exit 99
printf 'HTTP/1.1 200 OK\r\n' > "$headers"
[[ ${MISSING_ETAG:-0} == 1 ]] || printf 'ETag: "fresh-etag"\r\n' >> "$headers"
[[ ${AMBIGUOUS_ETAG:-0} == 0 ]] || printf 'ETag: "second-etag"\r\n' >> "$headers"
jq -cn --arg sid "$SID" --arg name "$NATIVE_NAME" --arg status "${PIN_STATUS:-Stopped}" '{type:"sync",status_code:200,metadata:{
 name:$name,type:"container",status:$status,profiles:["3tched-lungs"],
 config:{"user.opdbus.session_id":$sid},devices:{root:{type:"disk",path:"/",pool:"identity-roots"},
  lungs:{type:"disk",path:"/run/opdbus/lungs"},workspace:{type:"disk",path:"/workspace"}}}}' > "$response"
STUB
chmod 755 "$T/bin/"*

export OP_IDENTITY_ROOT_HELPER="$T/bin/physical-helper" THREETCHED_FS="$T/bin/fs"
export OP_IDENTITY_HOST_FOLDERS_HELPER="$T/bin/host-folders-helper"
export OP_IDENTITY_SEED_CATALOG="$T/catalog"
: > "$T/seed"
reset_fixture() {
 rm -rf -- "$T/state" "$T/receipts" "$T/leaf" "$T/native-status" "$T/boot-policy" "$T/plans"
 mkdir -m 700 "$T/leaf"
 export NATIVE_NAME=${NATIVE_NAME_OVERRIDE:-3tchedChatbot}
 export OP_IDENTITY_SEED_STATE="$T/state" OP_IDENTITY_PROVISION_RECEIPTS="$T/receipts"
 export OP_IDENTITY_PROVISION_PLAN_ROOT="$T/plans" OP_IDENTITY_SEED_DEVICE="$T/seed"
 : > "$EVENT_LOG"
 jq -cn --arg sid "${MOUNT_SESSION_OVERRIDE:-$OPERATOR}" --arg hash "${MOUNT_CATALOG_OVERRIDE:-fixture-catalog}" \
  '{session_id:$sid,source_catalog_hash:$hash}' > "$T/lungs/.3tchedfs.json"
 jq -n --arg sid "$SID" --arg key "$KEY" '{
  session_id:$sid,wireguard_pubkey:$key,sealed_id:"SID1_SECRET_SENTINEL",genesis:"GENESIS_SECRET_SENTINEL",
  arrival_timestamp:1,chain_head_at_arrival:"CHAIN_SECRET_SENTINEL",
  private_key:"PRIVATE_SECRET_SENTINEL",interface:"keep-interface",peer_ip:"100.69.0.10/32",
  principal_kind:(env.SERVICE_KIND_OVERRIDE // "service"),
  instance:{name:(env.INSTANCE_NAME_OVERRIDE // env.SID),type:"container",config:{"volatile.uuid":"prior-native-uuid"}},active:false,mutation_index:0,btrfs_device:{device_path:"/old/leaf",mount_point:"/old/mount",
   btrfs_uuid:"old-fsid",cozo_id:"KEEP_OWNER_KEY",attached:true}
 }' > "$T/current.json"
}
run_flow() {
 local -a args=(owner --session-id "${SESSION_ID_OVERRIDE:-$SID}" --pubkey "$KEY"
  --operator-session "$OPERATOR" --leaf-path "$T/leaf/new.img")
 [[ -z ${NATIVE_NAME_OVERRIDE:-} ]] || args+=(--container-name "$NATIVE_NAME")
 "$PROVISIONER" "${args[@]}" "$@"
}
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
grep -qx host-folders "$EVENT_LOG" || fail 'host folders not attached'
jq -e '.btrfs_device.attached==true and .btrfs_device.cozo_id==env.SID and .interface=="" and .peer_ip==null and .mutation_index==0 and .active==false' "$T/current.json" >/dev/null
! grep -R -E 'SID1_SECRET|GENESIS_SECRET|PRIVATE_SECRET|CHAIN_SECRET' "$T/receipts" "$T/plans" "$T/output" >/dev/null || fail 'persisted secret'
[[ $(cat "$T/native-status") == Stopped ]] || fail '--no-start changed state'
[[ $(cat "$T/boot-policy") == false ]] || fail '--no-start boot policy starts guest'
reset_fixture
jq '.active=true' "$T/current.json" > "$T/change.json"; mv "$T/change.json" "$T/current.json"
run_flow --no-start > "$T/output" 2>&1
jq -e '.active==false' "$T/current.json" >/dev/null || fail '--no-start retained prior active metadata'
assert_no 'lungs:identity_sled__touch_session|incus:start'
reset_fixture; export PREVIOUS_RUNNING=1
expect_failure; assert_no 'incus:init|helper:|lungs:identity_sled__provision'; unset PREVIOUS_RUNNING
# Same-key read before native mutation, mounted native ETag before adoption, adoption before activation.
reset_fixture
jq '.active=false' "$T/current.json" > "$T/change.json"; mv "$T/change.json" "$T/current.json"
run_flow > "$T/output" 2>&1
jq -e '.active==true' "$T/current.json" >/dev/null || fail 'canonical activation left identity inactive'
[[ $(cat "$T/boot-policy") == true ]] || fail 'normal boot policy does not start guest'
awk '/lungs:identity_sled__inspect_identity_inventory/{gate=NR}
 /lungs:identity_sled__inspect_provision_preflight/{if (!before) before=NR; else recheck=NR}
 /helper:seed-preflight/{preflight=NR} /helper:seed-profile/{profile=NR} /incus:init/{init=NR} /host-folders/{folders=NR} /helper:seed-lungs/{lungs=NR}
 /curl:native-pin/{pin=NR}
 /lungs:identity_sled__provision_container/{adopt=NR}
 /lungs:identity_sled__touch_session/{start=NR} /helper:seed-enable-boot/{boot=NR}
 END{exit !(gate<before && before<preflight && preflight<profile && profile<init && init<folders && folders<lungs && lungs<pin && pin<recheck && recheck<adopt && adopt<start && start<boot)}' "$EVENT_LOG" || fail 'unsafe ordering'
reset_fixture
export HOST_FOLDERS_FAIL=1
expect_failure; assert_no 'helper:seed-lungs|lungs:identity_sled__provision_container|lungs:identity_sled__touch_session|incus:start'
unset HOST_FOLDERS_FAIL
assert_no 'incus:start'
reset_fixture; export MCP_FAIL=identity_sled__inspect_identity_inventory
expect_failure; assert_no 'incus:|helper:|lungs:identity_sled__provision'; unset MCP_FAIL
reset_fixture; export EXISTING=1
expect_failure; assert_no 'incus:init|helper:|lungs:identity_sled__provision'; unset EXISTING
reset_fixture
export NATIVE_NAME_OVERRIDE=3tchedChatbot INSTANCE_NAME_OVERRIDE="$SID" SERVICE_KIND_OVERRIDE=service
reset_fixture
run_flow --no-start > "$T/output" 2>&1
assert_no 'incus:start'
grep -qx 'incus:init' "$EVENT_LOG" || fail 'existing service SID alias was not initialized under its native alias'
grep -qx 'helper:seed-preflight' "$EVENT_LOG" || fail 'seed preflight missing for alias'
jq -e --arg sid "$SID" --arg name "$NATIVE_NAME" '.name==$name and .session==$sid and .layout=="shared-partition-siblings-v2"' \
 "$T/state/$NATIVE_NAME/state.json" >/dev/null || fail 'alias receipt path/name/session mismatch'
[[ ! -e "$T/state/$SID/state.json" ]] || fail 'alias receipt was placed under the session instead of native name'
[[ $(cat "$FIX/native-status") == Stopped && $(cat "$T/boot-policy") == false ]] || fail 'alias no-start changed native power or boot policy'
unset NATIVE_NAME_OVERRIDE INSTANCE_NAME_OVERRIDE SERVICE_KIND_OVERRIDE
export NATIVE_NAME_OVERRIDE="$SID"
reset_fixture
expect_failure; assert_no 'capture|incus:|helper:|lungs:'; unset NATIVE_NAME_OVERRIDE
export NATIVE_NAME_OVERRIDE=3tchedChatbot-old-20261005 INSTANCE_NAME_OVERRIDE=3tchedChatbot-old-20261005 SERVICE_KIND_OVERRIDE=service
reset_fixture
expect_failure; assert_no 'incus:|helper:|lungs:'; unset NATIVE_NAME_OVERRIDE INSTANCE_NAME_OVERRIDE SERVICE_KIND_OVERRIDE
export NATIVE_NAME_OVERRIDE=3tchedChatbot SESSION_ID_OVERRIDE=aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa INSTANCE_NAME_OVERRIDE=3tchedChatbot SERVICE_KIND_OVERRIDE=service
reset_fixture
expect_failure; assert_no 'incus:|helper:|lungs:identity_sled__provision'; unset NATIVE_NAME_OVERRIDE SESSION_ID_OVERRIDE INSTANCE_NAME_OVERRIDE SERVICE_KIND_OVERRIDE
reset_fixture; export HELPER_FAIL=seed-preflight
expect_failure; assert_no 'incus:init|helper:seed-create|lungs:identity_sled__provision'; unset HELPER_FAIL
reset_fixture; export HELPER_FAIL=seed-profile
expect_failure; assert_no 'incus:init|helper:seed-create|lungs:identity_sled__provision'; unset HELPER_FAIL
reset_fixture
mkdir -m 700 "$T/receipts"
umask 077; : > "$T/receipts/$SID.lock"; chmod 600 "$T/receipts/$SID.lock"
exec {held_lock}>>"$T/receipts/$SID.lock"
flock -n "$held_lock" || fail 'fixture failed to hold identity lock'
expect_failure; assert_no 'incus:|helper:|lungs:'
exec {held_lock}>&-
reset_fixture; export HELPER_FAIL=seed-check
expect_failure; assert_no 'lungs:identity_sled__provision|incus:start'; unset HELPER_FAIL
reset_fixture; export HELPER_FAIL=seed-lungs
expect_failure; assert_no 'lungs:identity_sled__provision|lungs:identity_sled__touch_session|incus:start'; unset HELPER_FAIL
reset_fixture; export RECEIPT_LAYOUT=shared-partition-siblings-v1
expect_failure; assert_no 'lungs:identity_sled__provision|lungs:identity_sled__touch_session|incus:start'; unset RECEIPT_LAYOUT
reset_fixture; export CHANGE_DURING_PREP=1
expect_failure; assert_no 'lungs:identity_sled__provision|incus:start'; unset CHANGE_DURING_PREP
for tool in identity_sled__provision_container identity_sled__get_identity identity_sled__touch_session; do
 reset_fixture; export MCP_FAIL="$tool"
 expect_failure; assert_no 'incus:start'; unset MCP_FAIL
 [[ ! -e "$T/boot-policy" ]] || fail 'failed operation enabled boot'
done
reset_fixture; export INIT_FAIL=1
expect_failure; assert_no 'helper:seed-create|lungs:identity_sled__provision|incus:start'; unset INIT_FAIL
reset_fixture; export NATIVE_STATUS_OVERRIDE=Broken
expect_failure; assert_no 'helper:seed-enable-boot'; unset NATIVE_STATUS_OVERRIDE
reset_fixture; export HELPER_FAIL=seed-enable-boot
expect_failure; unset HELPER_FAIL
reset_fixture
for option in --recreate --private-key-file --identity-dir --ghostbridge; do
 if "$PROVISIONER" owner "$option" ignored > "$T/output" 2>&1; then fail "accepted $option"; fi
 [[ ! -s "$EVENT_LOG" ]] || fail 'legacy option had effects'
done
reset_fixture; export BAD_ADOPTION_DIGEST=1
expect_failure
awk '/lungs:identity_sled__provision_container/{seen=1} seen && /lungs:identity_sled__get_identity/{bad=1} END{exit bad}' "$EVENT_LOG" || fail 'bad adoption continued to identity readback'
assert_no 'lungs:identity_sled__touch_session'
unset BAD_ADOPTION_DIGEST
reset_fixture; export CURL_FAIL=1
expect_failure; assert_no 'lungs:identity_sled__provision_container|lungs:identity_sled__touch_session'; unset CURL_FAIL
reset_fixture; export MISSING_ETAG=1
expect_failure; assert_no 'lungs:identity_sled__provision_container|lungs:identity_sled__touch_session'; unset MISSING_ETAG
reset_fixture; export AMBIGUOUS_ETAG=1
expect_failure; assert_no 'lungs:identity_sled__provision_container|lungs:identity_sled__touch_session'; unset AMBIGUOUS_ETAG
reset_fixture; export PIN_STATUS=Running
expect_failure; assert_no 'lungs:identity_sled__provision_container|lungs:identity_sled__touch_session'; unset PIN_STATUS
reset_fixture; export CHANGE_OPERATION_DURING_PREP=1
expect_failure; assert_no 'lungs:identity_sled__provision_container|lungs:identity_sled__touch_session'; unset CHANGE_OPERATION_DURING_PREP
reset_fixture
mkdir -m 700 "$T/plans"
ln -s "$T/current.json" "$T/plans/$SID.json"
expect_failure; assert_no 'lungs:identity_sled__provision_container|lungs:identity_sled__touch_session'
assert_no 'lungs:identity_sled__write_identity|lungs:identity_sled__attach_btrfs_device'

reset_fixture; export DELETED=1
run_flow --no-start > "$T/output" 2>&1
jq -e '.version==3 and .previous_instance==null' "$T/plans/$SID.json" >/dev/null || fail 'deleted-row plan invalid'
unset DELETED
for flag in MOUNT_ABSENT MOUNT_SESSION_OVERRIDE MOUNT_CATALOG_OVERRIDE; do
 export "$flag=1"
 reset_fixture
 expect_failure; assert_no 'incus:|helper:|lungs:'
 unset "$flag"
done

printf 'PASS: same-key clean adoption, private plans, local pins, guards, order and redaction\n'
