#!/usr/bin/env bash
# No storage privileges needed: every storage/Incus effect is a shell mock.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../../.." && pwd)
helper=$repo/deploy/runit/libexec-3tched/op-identity-root
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT
for name in valid_session ensure_soul seed_write seed_member_pin seed_layout_pins seed_persist_pin seed_verify_layout seed_bind_member seed_mount_guest seed_unmount_guest seed_write_hostname seed_create seed_read seed_verify_source; do
    eval "$(sed -n "/^$name() {/,/^}/p" "$helper")"
done
seed_fail() { printf '%s\n' "$*" >&2; exit 1; }
log() { :; }
seed_secure_path() { [[ ! -L $1 ]] || seed_fail 'symlink'; }
seed_instance() { printf 'instance\n' >> "$events"; }
seed_empty_native_root() { :; }
seed_attach() { seed_view=/mock/seed-loop; leaf_loop=/mock/leaf-loop; }
seed_shared_lock() { printf 'shared-lock\n' >> "$events"; }
seed_image_metadata() { printf 'image:%s\n' "${1:-verify-metadata}" >> "$events"; [[ ${BAD_IMAGE:-false} == false ]] || return 1; jq -cn '{release_id:"fixture",layout:"shared-partition-siblings-v2",manifest_sha256:("a"*64)}'; }
blockdev() { [[ $1 == --getro && $2 == /mock/device ]]; printf '%s\n' "${BLOCK_RO:-1}"; }
seed_verify_volume() { :; }
seed_no_direct_mount() { [[ ${DIRECT_MOUNT:-false} == false ]] || seed_fail 'direct source mount'; }
seed_super() { printf '%s\n' "$sprout_uuid"; }
seed_verify_boot_marker() { :; }
sync() { :; }
# Resolve fake bind mounts through a JSON map. No mount syscall is invoked.
mountpoint() { jq -e --arg p "$2" 'has($p)' "$mounts" >/dev/null; }
mount() {
    printf 'mount:%s\n' "$*" >> "$events"
    if [[ $1 == --bind ]]; then
        jq --arg p "$3" --arg s "$2" '. + {($p):{source:$s,ro:false}}' "$mounts" > "$mounts.tmp"
        mv "$mounts.tmp" "$mounts"
    elif [[ $1 == -o ]]; then
        jq --arg p "$3" '.[$p].ro=true' "$mounts" > "$mounts.tmp"
        mv "$mounts.tmp" "$mounts"
    fi
}
umount() {
    printf 'umount:%s\n' "$1" >> "$events"
    jq --arg p "$1" 'del(.[$p])' "$mounts" > "$mounts.tmp"; mv "$mounts.tmp" "$mounts"
}
stat() {
    if [[ $1 == -c && $2 == %d && ${BAD_PERSIST_DEVICE:-false} == true && $3 == "$seed_dir/volume/@agent_user/persist" ]]; then
        printf '%s\n' "$(( $(command stat -c %d "$3") + 1 ))"
    elif [[ $1 == -c && $2 == %d:%i ]]; then
        p=$3; mapped=$(jq -r --arg p "$p" '.[$p].source // empty' "$mounts")
        [[ -z $mapped ]] || p=$mapped
        command stat -c %d:%i "$p"
    elif [[ $1 == -c && $3 == /mock/device ]]; then
        case "$2" in %u) printf '0\n';; %a) printf '600\n';; *) exit 90;; esac
    else command stat "$@"; fi
}
findmnt() {
    p=$3; field=$5
    case "$field" in
        FSTYPE) printf 'btrfs\n';;
        FSROOT) source=$(jq -r --arg p "$p" '.[$p].source' "$mounts"); printf '/%s\n' "${source#"$seed_dir/volume/"}";;
        VFS-OPTIONS) jq -r --arg p "$p" 'if .[$p].ro then "ro" else "rw" end' "$mounts";;
        *) exit 91;;
    esac
}
btrfs() {
    printf 'btrfs:%s\n' "$*" >> "$events"
    case "$1 $2" in
        'subvolume list')
            for s in @ @agent_memory @agent_rules @agent_shared @agent_skills @agent_soul @agent_user; do
                printf 'ID 256 gen 7 top level 5 path %s\n' "$s"
            done
            [[ -z ${EXTRA_SUBVOL:-} ]] || printf 'ID 300 gen 7 top level 256 path %s\n' "$EXTRA_SUBVOL";;
        'subvolume show')
            p=$3; base=${p##*/}
            case "$base" in
                @) n=256;; @agent_memory) n=257;; @agent_rules) n=258;; @agent_shared) n=259;;
                @agent_skills) n=260;; @agent_soul) n=261;; @agent_user) n=262;;
                persist) [[ ${BAD_PERSIST_SUBVOL:-false} == true ]] || return 1; n=263;;
                *) return 1;;
            esac
            [[ ${BAD_PIN:-false} == false || $base != @agent_rules ]] || n=256
            printf 'Subvolume ID: %s\nUUID: 00000000-0000-4000-8000-%012d\n' "$n" "$n";;
        'subvolume create'|'subvolume snapshot') printf 'forbidden new subvolume\n' >&2; exit 92;;
        'property get')
            jq -r --arg p "$4" '"ro=" + ((.[$p] // false)|tostring)' "$properties";;
        'property set')
            jq --arg p "$4" --argjson ro "$6" '. + {($p):$ro}' "$properties" > "$properties.tmp"
            mv "$properties.tmp" "$properties";;
        'device add') :;;
        'inspect-internal dump-super')
            printf 'fsid %s\ngeneration %s\n' "${SUPER_UUID:-$source_uuid}" "${SUPER_GENERATION:-7}"
            [[ ${SEEDING:-true} == false ]] || printf 'flags SEEDING\n';;
        *) printf 'forbidden storage command\n' >&2; exit 92;;
    esac
}
reset() {
    rm -rf "$fixture/case"; mkdir -m 700 "$fixture/case"
    seed_dir=$fixture/case; events=$seed_dir/events; mounts=$seed_dir/mounts; properties=$seed_dir/properties
    : > "$events"; printf '{}\n' > "$mounts"; printf '{}\n' > "$properties"
    mkdir "$seed_dir/volume" "$seed_dir/native" "$seed_dir/volume/@"
    for sibling in memory rules shared skills soul user; do
        mkdir "$seed_dir/volume/@agent_$sibling"
    done
    mkdir -p "$seed_dir/volume/@/etc" "$seed_dir/native/agent" "$seed_dir/native/opt/run-mounts/persist"
    for member in memory rules shared skills soul user; do mkdir "$seed_dir/native/agent/$member"; done
    native_root=$seed_dir/native; leaf=$seed_dir/leaf.img; seed_name=test
    seed_session=b0c777e3-0955-cd8b-3d01-ca12998cae28
    source_uuid=b6561d54-3c69-4897-a2f3-79b867e33601; source_fsid=$source_uuid; source_generation=7
    sprout_uuid=00000000-0000-4000-8000-000000000099
    native_uuid=00000000-0000-4000-8000-000000000098
    boot_id=00000000-0000-4000-8000-000000000097
    seed_device=/mock/device; mode=seed-create; BAD_PIN=false; DIRECT_MOUNT=false
    SEEDING=true; SUPER_UUID=$source_uuid; SUPER_GENERATION=7
    BLOCK_RO=1; EXTRA_SUBVOL=; BAD_IMAGE=false
}
reset
seed_create "$seed_session" "$leaf" 1073741824
jq -e '.version==1 and .layout=="shared-partition-siblings-v2" and .phase=="ready" and (.members|length)==7 and .persist.path=="@agent_user/persist" and .persist.parent==.members["@agent_user"] and .persist.fsid==.sprout_fsid and (.persist.inode|length)>0' "$seed_dir/state.json" >/dev/null
[[ $(cat "$seed_dir/volume/@/etc/hostname") == "$seed_session" ]]
[[ $(rg -c '^btrfs:device add ' "$events") == 1 ]]
[[ $(rg -c '^image:verify$' "$events") == 1 ]]
rg -q '^image:verify-metadata$' "$events"
! rg 'btrfs:(subvolume (create|snapshot)|send|receive)|identity-base|overlay' "$events"
[[ ! -e $seed_dir/volume/@persist && $(command stat -c %a "$seed_dir/volume/@agent_user/persist") == 700 ]]
mode=seed-check
seed_mount_guest
mode=seed-up
seed_mount_guest
[[ $(rg -c '^btrfs:device add ' "$events") == 1 ]]
# Altered subvolume identity must fail without another mount.
before=$(rg -c '^mount:' "$events"); BAD_PIN=true
if (seed_mount_guest) >/dev/null 2>&1; then exit 1; fi
[[ $(rg -c '^mount:' "$events") == "$before" ]]; BAD_PIN=false
# Persistence replacement, subvolume conversion, and symlinks are rejected.
persist=$seed_dir/volume/@agent_user/persist
mv "$persist" "$persist.original"; mkdir -m 700 "$persist"
if (seed_mount_guest) >/dev/null 2>&1; then exit 1; fi
rmdir "$persist"; mv "$persist.original" "$persist"
BAD_PERSIST_SUBVOL=true
if (seed_mount_guest) >/dev/null 2>&1; then exit 1; fi
BAD_PERSIST_SUBVOL=false
BAD_PERSIST_DEVICE=true
if (seed_mount_guest) >/dev/null 2>&1; then exit 1; fi
BAD_PERSIST_DEVICE=false
mv "$persist" "$persist.original"; ln -s persist.original "$persist"
if (seed_mount_guest) >/dev/null 2>&1; then exit 1; fi
rm "$persist"; mv "$persist.original" "$persist"
[[ $(rg -c '^mount:' "$events") == "$before" ]]
mode=seed-down
seed_mount_guest
seed_unmount_guest
sed -n 's/^umount://p' "$events" > "$seed_dir/actual-down"
printf '%s\n' "$native_root/opt/run-mounts/persist" "$native_root/agent/user" "$native_root/agent/soul" "$native_root/agent/skills" "$native_root/agent/shared" "$native_root/agent/rules" "$native_root/agent/memory" "$native_root" > "$seed_dir/expected-down"
tail -n 8 "$seed_dir/actual-down" | cmp -s - "$seed_dir/expected-down"
# Previous extra-subvolume receipts and missing layouts are not adopted.
jq '.layout="shared-partition-siblings-v1"' "$seed_dir/state.json" > "$seed_dir/old.json"
if (cp "$seed_dir/old.json" "$seed_dir/state.json"; seed_read) >/dev/null 2>&1; then exit 1; fi
jq 'del(.layout)' "$seed_dir/state.json" > "$seed_dir/old.json"
mv "$seed_dir/old.json" "$seed_dir/state.json"
if (seed_read) >/dev/null 2>&1; then exit 1; fi
for failure in missing duplicate source_generation symlink direct old_persist extra nested writable_block image; do
    reset
    case "$failure" in
        missing) rmdir "$seed_dir/volume/@agent_rules";;
        duplicate) BAD_PIN=true;;
        source_generation) SUPER_GENERATION=8;;
        symlink) rmdir "$seed_dir/volume/@agent_rules"; ln -s "$seed_dir/volume/@agent_soul" "$seed_dir/volume/@agent_rules";;
        direct) DIRECT_MOUNT=true;;
        old_persist) mkdir "$seed_dir/volume/@persist";;
        extra) EXTRA_SUBVOL=@extra;;
        nested) EXTRA_SUBVOL=@agent_memory/nested;;
        writable_block) BLOCK_RO=0;;
        image) BAD_IMAGE=true;;
    esac
    if (seed_create "$seed_session" "$leaf" 1073741824) >/dev/null 2>&1; then exit 1; fi
    ! rg '^btrfs:device add ' "$events"
done
# Historical soul reconciliation must fail rather than allocate missing data.
reset
IDENTITIES=$seed_dir/legacy
incus() { printf 'incus:%s\n' "$*" >> "$events"; [[ $1 == info ]]; }
if (ensure_soul "$seed_session") >/dev/null 2>&1; then exit 1; fi
mkdir -p "$IDENTITIES/$seed_session/device/cozo"
jq --arg p "$IDENTITIES/$seed_session/device" '. + {($p):{source:$p,ro:false}}' "$mounts" > "$mounts.tmp"
mv "$mounts.tmp" "$mounts"
if (ensure_soul "$seed_session") >/dev/null 2>&1; then exit 1; fi
! rg 'btrfs:subvolume (create|snapshot)|incus:config device (add|set)' "$events"
# Source preflight rejects an ordinary filesystem or wrong expected UUID.
# Intercept only fake block-device/realpath operations; no host device is read.
function [ { if [[ ${1:-} == -b && ${2:-} == /mock/device ]]; then return 0; fi; builtin [ "$@"; }
readlink() { [[ $1 == -f && $3 == /mock/device ]] && printf '/mock/device\n'; }
reset
OP_IDENTITY_SEED_DEVICE=/mock/device OP_IDENTITY_SEED_EXPECTED_UUID=$source_uuid RACCOON_UUID=$source_uuid
seed_verify_source
BLOCK_RO=0
if (seed_verify_source) >/dev/null 2>&1; then exit 1; fi
BLOCK_RO=1
SEEDING=false
if (seed_verify_source) >/dev/null 2>&1; then exit 1; fi
SEEDING=true; OP_IDENTITY_SEED_EXPECTED_UUID=00000000-0000-4000-8000-000000000088
if (seed_verify_source) >/dev/null 2>&1; then exit 1; fi
[[ ! -e $leaf ]]
printf 'PASS: shared sibling seed creation, pins, reverse unmount and preflight; no native effects\n'
