#!/usr/bin/env bash
# Disposable files and mocked storage operations only; no host storage effects.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../../.." && pwd)
helper=$repo/deploy/runit/libexec-3tched/op-identity-root
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
seed_fail() { printf '%s\n' "$*" >&2; exit 1; }
seed_secure_path() { [[ ! -L $1 ]] || seed_fail 'symlink'; }
seed_private_dir() { [[ -e $1 ]] || mkdir -m 700 "$1"; }
id() { printf '0\n'; }
for name in seed_boot_name seed_image_helper_ready seed_image_metadata; do
    eval "$(sed -n "/^$name() {/,/^}/p" "$helper")"
done
for name in seed_inspect seed_preflight; do
    eval "$(sed -n "/^$name() (/,/^)/p" "$helper")"
done
# Each canonical identity is valid; the single alias is pinned to one SID.
seed_session=b0c777e3-0955-cd8b-3d01-ca12998cae28
seed_name=$seed_session; seed_boot_name
seed_name=3tchedCtlbot; seed_boot_name
seed_session=00000000-0000-4000-8000-000000000001
if (seed_boot_name) 2>/dev/null; then exit 1; fi
seed_name=another-alias
if (seed_boot_name) 2>/dev/null; then exit 1; fi
# Shared lock algorithm uses a disposable path, never /run in this fixture.
eval "$(sed -n '/^seed_shared_lock() {/,/^}/p' "$helper" | sed "s|/run/opdbus-shared-seed|$fixture/shared|g")"
seed_shared_lock
seed_shared_lock # same process can reuse it
if (exec 8>&-; seed_shared_locked=false; seed_shared_lock) 2>/dev/null; then exit 1; fi
exec 8>&-
seed_shared_locked=false; seed_shared_lock; exec 8>&-
# The image contract accepts only safe immutable release pins.
mkdir "$fixture/volume"
seed_dir=$fixture
OP_IDENTITY_IMAGE_HELPER=$fixture/image-helper
cat > "$OP_IDENTITY_IMAGE_HELPER" <<'SCRIPT'
#!/bin/sh
case "$1" in verify|verify-metadata) ;; *) exit 1;; esac
[ -d "$2" ] || exit 1
printf '{"release_id":"fixture-v1","layout":"shared-partition-siblings-v2","manifest_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}\n'
SCRIPT
chmod 700 "$OP_IDENTITY_IMAGE_HELPER"
seed_image_metadata | jq -e '.release_id=="fixture-v1"' >/dev/null
printf 'exit 1\n' >> "$OP_IDENTITY_IMAGE_HELPER"
if (seed_image_metadata) >/dev/null 2>&1; then exit 1; fi
sed -i '$d' "$OP_IDENTITY_IMAGE_HELPER"
# Preflight must fail before any fixture state path is created.
seed_verify_source() { [[ ${BAD_SOURCE:-false} == false ]] || seed_fail source; source_fsid=source; source_generation=7; }
seed_no_direct_mount() { [[ ${DIRECT:-false} == false ]] || seed_fail direct; }
OP_IDENTITY_SEED_STATE=$fixture/absent
seed_preflight | jq -e '.block_readonly and .standalone_mount==false' >/dev/null
[[ ! -e $OP_IDENTITY_SEED_STATE ]]
BAD_SOURCE=true
if (seed_preflight) 2>/dev/null; then exit 1; fi
BAD_SOURCE=false; DIRECT=true
if (seed_preflight) 2>/dev/null; then exit 1; fi
DIRECT=false
[[ ! -e $OP_IDENTITY_SEED_STATE ]]
# Incomplete and ready receipts can be inspected without source/API calls.
mkdir -m 700 "$OP_IDENTITY_SEED_STATE" "$OP_IDENTITY_SEED_STATE/3tchedCtlbot"
receipt=$OP_IDENTITY_SEED_STATE/3tchedCtlbot/state.json
jq -n '{version:1,layout:"shared-partition-siblings-v2",name:"3tchedCtlbot",session:"b0c777e3-0955-cd8b-3d01-ca12998cae28",seed_fsid:"b6561d54-3c69-4897-a2f3-79b867e33601",instance_uuid:"00000000-0000-4000-8000-000000000001",boot_id:"00000000-0000-4000-8000-000000000002",leaf:"/private/leaf",root:"/native/root",phase:"device-add-intent",unexpected_secret:"never-output"}' > "$receipt"
chmod 600 "$receipt"
seed_verify_source() { exit 91; }
seed_inspect 3tchedCtlbot > "$fixture/inspection"
jq -e '.phase=="device-add-intent" and (.recovery|contains("Do not retry")) and (has("unexpected_secret")|not)' "$fixture/inspection" >/dev/null
before=$(sha256sum "$receipt")
seed_inspect 3tchedCtlbot >/dev/null
[[ $(sha256sum "$receipt") == "$before" && ! -e $OP_IDENTITY_SEED_STATE/3tchedCtlbot/lock ]]
jq '.phase="ready"' "$receipt" > "$fixture/ready"; cat "$fixture/ready" > "$receipt"
seed_inspect 3tchedCtlbot | jq -e '.phase=="ready" and (.verification|contains("not been verified"))' >/dev/null
printf 'PASS: shared lock, pinned chatbot alias, image pins, observational preflight/intent inspection\n'
