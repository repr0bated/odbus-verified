#!/usr/bin/env bash
# Install public LiteLLM assets into an authorized, unsealed offline image.
# Never mount, format, seal, change membership, execute a guest, or start services.
set -euo pipefail
fail() { printf 'prepare-litellm: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 && $(id -u) == 0 ]] || fail 'usage (host root): TOP ROOT_CONTROLLED_PAYLOAD'
top=$1 payload=$2
[[ $top == /* && $top != / && $(realpath -e "$top") == "$top" ]] || fail 'noncanonical image top'
[[ $(findmnt -rn -M "$top" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$top" -o FSROOT) == / ]] || fail 'existing Btrfs top-level mount required'
case ",$(findmnt -rn -M "$top" -o OPTIONS)," in *,rw,*) ;; *) fail 'image is not writable' ;; esac
[[ $payload == /opt/op-dbus/release-send/* && $(realpath -e "$payload") == "$payload" ]] || fail 'protected preparation payload required'
parent=$payload
while :; do
    [[ $(stat -c %u "$parent") == 0 ]] || fail 'payload path is not root-owned'
    mode=$(stat -c %a "$parent")
    (( (8#$mode & 022) == 0 )) || fail 'payload path writable by another owner'
    [[ $parent == / ]] && break
    parent=$(dirname -- "$parent")
done
[[ -z $(find "$payload" \( -type l -o ! -user root -o -perm /022 \) -print -quit) ]] || fail 'unsafe payload contents'
jq -e '.format == "opdbus.seed-litellm.v1" and .litellm_version == "1.105.1" and
    .python_version == "3.14" and .python_platform == "x86_64-manylinux_2_28" and
    .credentials_included == false and .guest_executed == false and .activated == false' \
    "$payload/preparation.json" >/dev/null || fail 'unsupported preparation receipt'
[[ $(sha256sum "$payload/SHA256SUMS" | cut -d ' ' -f1) == $(jq -er .manifest_sha256 "$payload/preparation.json") ]] || fail 'manifest digest mismatch'
(
    cd -- "$payload"
    sha256sum --strict --quiet -c SHA256SUMS
    sha256sum --strict --quiet -c runtime.sha256
) || fail 'payload digest mismatch'
[[ -f $payload/runtime/litellm-1.105.1.dist-info/METADATA ]] || fail 'missing runtime'
[[ -z $(find "$payload/runtime" \( -type d ! -perm -0005 -o -type f ! -perm -0004 \) -print -quit) ]] || fail 'runtime is unreadable by guest service user'
guest=$top/@
persist=$top/@agent_user/persist
for path in "$top" "$guest" "$top/@agent_user" "$persist"; do
    [[ -d $path && ! -L $path && $(realpath -e "$path") == "$path" && $(stat -c %u "$path") == 0 ]] || fail "unsafe image directory: $path"
    mode=$(stat -c %a "$path")
    (( (8#$mode & 022) == 0 )) || fail "image directory writable by another owner: $path"
done
mode=$(stat -c %a "$guest")
(( (8#$mode & 001) != 0 )) || fail 'guest root is not traversable by service user'
for path in "$guest" "$top/@agent_user"; do
    btrfs subvolume show "$path" >/dev/null || fail 'existing guest and user subvolumes required'
    [[ $(btrfs property get "$path" ro) == ro=false ]] || fail 'image subvolume is not writable'
done
[[ $(stat -c '%u:%g:%a' "$persist") == 0:0:700 && $(stat -c %i "$persist") != 256 &&
   $(findmnt -rn -T "$persist" -o TARGET) == "$top" ]] || fail 'existing private ordinary persist directory required'
[[ ! -e $guest/usr/share/op-dbus/image && ! -L $guest/usr/share/op-dbus/image ]] || fail 'image metadata already prepared; do not alter a finalized seed'
[[ $(readlink "$guest/usr/bin/python3") == python3.14 && -x $guest/usr/bin/python3.14 ]] || fail 'guest CPython3.14 required'
[[ -x $guest/usr/lib/systemd/systemd && -f $guest/usr/bin/dash ]] || fail 'guest systemd and shell required'
[[ $(readlink "$guest/bin") == usr/bin && $(readlink "$guest/usr/bin/sh") == dash ]] || fail 'unsupported guest shell layout'
for path in "$guest/usr/bin/python3.14" "$guest/usr/bin/dash"; do
    mode=$(stat -c %a "$path")
    (( (8#$mode & 005) == 005 )) || fail 'guest interpreter is not readable/executable by service user'
done
grep -q '^nobody:x:65534:65534:' "$guest/etc/passwd" || fail 'guest nobody account absent'
grep -q '^nogroup:x:65534:' "$guest/etc/group" || fail 'guest nogroup absent'
targets=(usr/local/lib/op-dbus/litellm usr/local/lib/op-dbus/.litellm-preparing
    usr/local/libexec/op-dbus/litellm-key-present etc/op-dbus/litellm
    usr/lib/systemd/system/litellm.service etc/systemd/system/litellm.service
    etc/systemd/system/multi-user.target.wants/litellm.service usr/share/op-dbus/litellm)
for relative in "${targets[@]}"; do
    [[ ! -e $guest/$relative && ! -L $guest/$relative ]] || fail "target already exists: $relative"
done
[[ ! -e $persist/litellm && ! -L $persist/litellm ]] || fail 'activation backing already exists'
required=$(du -sb "$payload/runtime" | cut -f1)
available=$(df -B1 --output=avail "$guest" | tail -n1)
(( available > required + 268435456 )) || fail 'insufficient image headroom'
dirs=(usr usr/bin usr/local usr/local/lib usr/local/lib/op-dbus usr/local/libexec
    usr/local/libexec/op-dbus usr/lib usr/lib/systemd usr/lib/systemd/system
    usr/share usr/share/op-dbus etc etc/op-dbus etc/systemd etc/systemd/system
    etc/systemd/system/multi-user.target.wants)
# Inspect every existing component before changing anything in the image.
for relative in "${dirs[@]}"; do
    path=$guest/$relative
    [[ ! -L $path ]] || fail "symlink directory: $relative"
    if [[ -e $path ]]; then
        [[ -d $path && $(stat -c %u "$path") == 0 ]] || fail "unsafe directory: $relative"
        mode=$(stat -c %a "$path")
        (( (8#$mode & 022) == 0 )) || fail "writable directory: $relative"
        (( (8#$mode & 001) != 0 )) || fail "directory is not traversable by service user: $relative"
    fi
done
for relative in "${dirs[@]}"; do
    [[ -d $guest/$relative ]] || install -d -m 0755 "$guest/$relative"
done
cp -a -- "$payload/runtime" "$guest/usr/local/lib/op-dbus/.litellm-preparing"
mv -- "$guest/usr/local/lib/op-dbus/.litellm-preparing" "$guest/usr/local/lib/op-dbus/litellm"
install -m 0755 "$payload/litellm-key-present" "$guest/usr/local/libexec/op-dbus/litellm-key-present"
install -d -m 0755 "$guest/etc/op-dbus/litellm" "$guest/usr/share/op-dbus/litellm"
install -m 0644 "$payload/config.yaml" "$guest/etc/op-dbus/litellm/config.yaml"
install -m 0644 "$payload/litellm.service" "$guest/usr/lib/systemd/system/litellm.service"
install -m 0644 "$payload/requirements.lock" "$payload/runtime.sha256" "$payload/SHA256SUMS" "$guest/usr/share/op-dbus/litellm/"
install -d -m 0700 "$persist/litellm"
ln -s /usr/lib/systemd/system/litellm.service "$guest/etc/systemd/system/multi-user.target.wants/litellm.service"
jq --arg prepared_at "$(date -u +%FT%TZ)" \
    --arg source "$payload" --arg filesystem_uuid "$(findmnt -rn -M "$top" -o UUID)" \
    '. + {prepared_at:$prepared_at,source_payload:$source,filesystem_uuid:$filesystem_uuid,
      status:"staged",enabled_offline:true,guest_boot_verified:false,
      activation_file:"/opt/run-mounts/persist/litellm/activation.env",
      activation_file_present:false,missing_key_action:"skip_without_failure",
      listen:"127.0.0.1:4000"}' "$payload/preparation.json" > "$guest/usr/share/op-dbus/litellm/preparation.json"
chmod 0644 "$guest/usr/share/op-dbus/litellm/preparation.json"
sync -f "$guest"
# Bounded installation readback; never execute anything from the image.
cmp "$payload/config.yaml" "$guest/etc/op-dbus/litellm/config.yaml"
cmp "$payload/litellm.service" "$guest/usr/lib/systemd/system/litellm.service"
cmp "$payload/litellm-key-present" "$guest/usr/local/libexec/op-dbus/litellm-key-present"
(
    cd -- "$guest/usr/local/lib/op-dbus/litellm"
    sed 's|  runtime/|  |' "$payload/runtime.sha256" | sha256sum --strict --quiet -c -
)
[[ -z $(find "$persist/litellm" -mindepth 1 -print -quit) ]] || fail 'activation backing unexpectedly populated'
[[ $(readlink "$guest/etc/systemd/system/multi-user.target.wants/litellm.service") == /usr/lib/systemd/system/litellm.service ]] || fail 'enablement mismatch'
cat "$guest/usr/share/op-dbus/litellm/preparation.json"
