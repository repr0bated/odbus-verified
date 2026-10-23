#!/usr/bin/env bash
# Bind the already prepared chatbot sprout. No format, member change or start.
set -euo pipefail
fail() { printf 'bind-instance-root: %s\n' "$*" >&2; exit 1; }
secure_path() (
    path=$1
    [[ $(realpath -e -- "$path") == "$path" ]] || fail "noncanonical path: $path"
    while :; do
        [[ ! -L $path && $(stat -c %u -- "$path") == 0 ]] &&
            (( (8#$(stat -c %a -- "$path") & 022) == 0 )) || fail "unsafe owner/mode: $path"
        [[ $path != / ]] || break
        path=$(dirname -- "$path")
    done
)
[[ $# == 1 && $1 =~ ^(3tchedCtlChatbot|3tchedChatbot|ID-[1-9][0-9]*)$ && ${#1} -le 63 && $(id -u) == 0 ]] || fail 'usage: bind-instance-root NAME (existing configured chatbot; host root)'
umask 077
name=$1

sid=b0c777e3-0955-cd8b-3d01-ca12998cae28
key=[REDACTED:opaque-base64]
client=${THREETCHED_FS:-/usr/local/libexec/op-dbus/bin/3tched-fs}
control_mount=${OP_3TCHEDFS_MOUNT:-/run/mounts/3tchedFS}
# This checks the mount's caller, never selects or creates a backend identity.
operator=${OP_3TCHEDFS_EXPECTED_SESSION-414fbc56-03fb-20ab-758a-bcc7a7cbdc0e}
[[ -x $client && $operator =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] || fail 'mounted client or canonical expected caller session absent'
command -v timeout >/dev/null || fail 'timeout is required for bounded control-plane calls'
pool=/var/lib/incus/storage-pools/3tched-pool
secure_path "$pool"
[[ $(stat -c %u:%g "$pool") == 0:0 &&
   $(findmnt -rn -M "$pool" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$pool" -o FSROOT) == / &&
   $(findmnt -rn -M "$pool" -o UUID) == a1ab7b42-12a8-437c-8f65-d0469892c8dd &&
   $(findmnt -rn -M / -o UUID) != a1ab7b42-12a8-437c-8f65-d0469892c8dd ]] ||
    fail 'canonical loop pool is not mounted on its pinned filesystem'
secure_path "$pool/.provisioning"
[[ -d $pool/.provisioning && $(stat -c %u:%g:%a "$pool/.provisioning") == 0:0:700 ]] ||
    fail 'preparation receipt directory must be private'
disk=$pool/.provisioning/$name.json
receipt=$pool/.provisioning/$name.incus.json
[[ ! -L $disk && -f $disk && ! -L $receipt && -f $receipt ]] || fail 'operator preparation receipts required'
for file in "$disk" "$receipt"; do
    secure_path "$file"
    [[ $(stat -c %u:%g:%a "$file") == 0:0:600 ]] || fail "unsafe preparation receipt: $file"
done
[[ ! -L $pool/.provisioning/$name.lock ]] || fail 'unsafe instance lock'
exec 9>"$pool/.provisioning/$name.lock"
flock -n 9 || fail 'instance preparation active'
jq -e --arg name "$name" '.name==$name and .phase=="filesystems-prepared" and .filesystem_prepared==true' "$disk" >/dev/null || fail 'filesystems not prepared'
# A numbered name is only a selector, never identity or allocation authority.
# The host preparation receipt and exact existing native UUID/key must agree.
jq -e --arg name "$name" --arg sid "$sid" --arg key "$key" '
    .version==1 and .name==$name and .session_id==$sid and
    .phase=="container-defined" and .identity_registered==false and .root_bound==false and
    .instance.name==$name and .instance.type=="container" and
    .instance.config["user.opdbus.session_id"]==$sid and
    .instance.config["user.opdbus.wireguard_public_key"]==$key and
    (.instance.config["volatile.uuid"]|type=="string" and test("^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$"))
' "$receipt" >/dev/null || fail 'configured chatbot preparation binding differs; reconcile existing intent'
native_uuid=$(jq -er '.instance.config["volatile.uuid"]' "$receipt")
root=$pool/.mounts/$name/root
replica=$pool/.mounts/$name/upgrade
view=$pool/.mounts/$name/root-view
seed_layer=$pool/.mounts/$name/seed-layer
upgrade_layer=$pool/.mounts/$name/upgrade-layer
container_root=/var/lib/incus/storage-pools/identity-roots/containers/$name/rootfs
image=$pool/$name.img
[[ $(jq -r .image "$disk") == "$image" && -f $image && ! -L $image &&
   $(stat -c %i "$image") == "$(jq -r .image_inode "$disk")" &&
   $(stat -c %s "$image") == "$(jq -r .image_bytes "$disk")" &&
   $(jq -r .root_mount "$disk") == "$root" &&
   $(findmnt -rn -M "$root" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$root" -o FSROOT) == / ]] || fail 'prepared root/image pins differ'
root_source=$(findmnt -rn -M "$root" -o SOURCE)
[[ $(blkid -p -s UUID -o value "$root_source") == "$(jq -r .root_fsid "$disk")" ]] || fail 'wrong sprout filesystem'
jq -e '(.root_fsid|type)=="string" and (.seed_fsid|type)=="string" and
    (.root_fsid|length)>0 and (.seed_fsid|length)>0 and .root_fsid!=.seed_fsid' "$disk" >/dev/null || fail 'refusing to edit the shared seed'
pair=$pool/.provisioning/$name.upgrade.json
[[ -f $pair && ! -L $pair ]] || fail 'upgrade pair must be established during provisioning'
jq -e --arg replica "$(jq -r .upgrade_fsid "$disk")" '.phase=="paired" and
    .host_fsid=="7ac6c955-4028-439c-80b4-c55a647abcb4" and .replica_fsid==$replica and
    .direction=="host-to-container"' "$pair" >/dev/null || fail 'upgrade pair differs'
[[ $(findmnt -rn -M "$replica" -o FSTYPE) == btrfs &&
   $(findmnt -rn -M "$replica" -o FSROOT) == / ]] || fail 'upgrade filesystem not mounted'
for layer in "$view" "$seed_layer" "$upgrade_layer"; do
    [[ ! -L $layer && -z $(findmnt -rn -M "$layer") ]] || fail 'retained OS view requires reconciliation'
done
mount_pin=$(timeout --kill-after=5s 30s jq -ce --arg session "$operator" '
    select(.format=="3tchedfs/1" and .session_id==$session and
        .mount_identity=={kind:"host_session",session:$session} and
        (.instance_uuid|type)=="string" and (.instance_uuid|length)>0)
    ' "$control_mount/.3tchedfs.json") || fail 'mounted caller identity unavailable or mismatched'
[[ $(findmnt -rn -M "$control_mount" -o FSTYPE) == fuse.3tchedFS ]] || fail 'control namespace is not a 3tchedFS mount'
mount_id=$(findmnt -rn -M "$control_mount" -o ID)
[[ -n $mount_id ]] || fail 'control mount ID absent'
verify_control_mount() {
    local current
    current=$(timeout --kill-after=5s 30s jq -ce . "$control_mount/.3tchedfs.json") || fail 'control mount metadata unavailable'
    [[ $current == "$mount_pin" && $(findmnt -rn -M "$control_mount" -o ID) == "$mount_id" ]] || fail 'control mount changed; reconciliation required'
}
read_instance() {
    # Incus public_state deliberately omits names/config/devices; mounted
    # value.json cannot verify this private binding and no get/list is declared.
    # Keep the existing explicit host-maintenance observation. After a mutation
    # it runs only after invoke succeeds: never an admission-denial fallback.
    verify_control_mount
    instance=$(timeout --kill-after=5s 30s incus --force-local query "/1.0/instances/$name?project=default") || fail 'bounded host-maintenance instance readback failed'
    verify_control_mount
    jq -e --arg name "$name" --arg sid "$sid" --arg key "$key" --arg uuid "$native_uuid" '
       .name==$name and .type=="container" and .status=="Stopped" and
       .status_code==102 and .config["user.opdbus.session_id"]==$sid and
       .config["user.opdbus.wireguard_public_key"]==$key and .config["volatile.uuid"]==$uuid and
       .config["security.privileged"]=="false" and .config["security.protection.shift"]=="true" and
       .devices.root.pool=="identity-roots" and .profiles==["3tched-lungs"] and
       (.devices|type)=="object" and (.expanded_devices|type)=="object"' <<<"$instance" >/dev/null || fail 'stopped container binding differs'
}
read_instance
# Canonical preparation creates the exact guest FUSE device. Do not
# route drift through generic mutations: the lifecycle guard intentionally
# denies pre-adoption device/config changes for identity candidates.
if [[ $name == ID-* || $name == 3tchedCtlChatbot ]]; then
    jq -e 'all(.devices,.expanded_devices;
        .fuse=={type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"} and
        (has("3tchedfs")|not) and (has("system-bus")|not))' <<<"$instance" >/dev/null ||
        fail 'canonical device definition drifted; no device mutation fallback'
fi
# A stopped, never-started candidate has only idmap.next. Never start it to
# populate current, and never accept an impending shift between two mappings.
select_idmap() {
    jq -er '
        def mapping:
            fromjson |
            select(type=="array" and length==2 and
                all(.[]; .Hostid==1000000 and .Nsid==0 and .Maprange==1000000000) and
                any(.[]; .Isuid==true and .Isgid==false) and
                any(.[]; .Isgid==true and .Isuid==false)) |
            sort_by(.Isuid, .Isgid);
        .config as $config |
        [$config | to_entries[] |
            select(.key=="volatile.idmap.current" or .key=="volatile.idmap.next") |
            .value] as $maps |
        select(($maps|length)>0) |
        [$maps[] | mapping] as $checked |
        select(($checked|length)==($maps|length) and
            all($checked[]; .==$checked[0])) |
        if $config|has("volatile.idmap.current")
        then $config["volatile.idmap.current"]
        else $config["volatile.idmap.next"] end
    ' <<<"$instance"
}
idmap=$(select_idmap) || fail 'unsupported or differing current/next container idmap'
[[ ! -L $container_root && -d $container_root && $(realpath -e "$container_root") == "$container_root" &&
   -z $(findmnt -rn -M "$container_root") && ! -L $root/@/etc/fstab && -f $root/@/etc/fstab &&
   ! -L $root/@/etc/hostname && -f $root/@/etc/hostname ]] || fail 'unexpected root mount or guest configuration'
# Preserve unrelated guest mounts and comments. Only disable physical/host
# storage and paths already mounted by the external root/agent mount owner.
# The guest helper accepts exactly these six options (3tchedfs.fstab); its
# singular, lowercase guest mountpoint is distinct from the host control mount.
guest_fstab() {
    local mode=$1 file=$2
    [[ -f $file && ! -L $file && $(realpath -e "$file") == "$file" ]] || fail "unsafe or absent guest fstab: $file"
    awk -v mode="$mode" '
        /^[[:space:]]*#/ || NF==0 { if (mode=="filter") print; next }
        $1 ~ /^(UUID|LABEL|PARTUUID|PARTLABEL)=/ || $1 ~ /^\/dev\// ||
        $3=="btrfs" || $3=="swap" || $2=="/" || $2 ~ /^\/boot(\/|$)/ ||
        $2 ~ /^\/agent(\/|$)/ || $2=="/opt/run-mounts/persist" {
            if (mode=="filter") print "# bind-instance-root: disabled host-storage entry: " $0
            else bad=1
            next
        }
        {
            if ($1=="3tchedFS" || $2=="/run/mount/3tchedfs" || $3=="fuse.3tchedFS") {
                count++
                if ($1!="3tchedFS" || $2!="/run/mount/3tchedfs" || $3!="fuse.3tchedFS" ||
                    $5!="0" || $6!="0" || (NF>6 && $7 !~ /^#/)) bad=1
                n=split($4, options, ",")
                if (n!=6) bad=1
                for (i=1; i<=n; i++) {
                    if (options[i] !~ /^(noauto|rw|nosuid|nodev|noexec|noatime)$/ || seen[options[i]]++) bad=1
                }
            }
            if (mode=="filter") print
        }
        END { if (bad || count!=1) exit 1 }
    ' "$file" || fail "guest fstab must retain one supported FUSE entry and no active host storage: $file"
}
guest_fstab filter "$root/@/etc/fstab" >/dev/null
# An upgrade fstab takes precedence over the sprout. Never edit the received
# upgrade baseline to hide drift; the merged view is checked again below.
if [[ -e $replica/@/etc/fstab || -L $replica/@/etc/fstab ]]; then
    guest_fstab verify "$replica/@/etc/fstab"
fi
for member in memory rules shared skills soul user; do
    [[ -d $root/@/agent/$member && ! -L $root/@/agent/$member &&
       -d $root/@agent_$member && ! -L $root/@agent_$member ]] || fail 'missing ordinary mountpoint or sibling'
    btrfs subvolume show "$root/@agent_$member" >/dev/null
done
# Keep the Incus empty-root scaffolding underneath the bind; never delete it.
for backup in "$pool/.provisioning/$name.before-root.json" "$pool/.provisioning/$name.fstab.before"; do
    [[ ! -e $backup && ! -L $backup ]] || fail 'retained root backup requires reconciliation'
done
printf '%s\n' "$instance" > "$pool/.provisioning/$name.before-root.json"
cp -a "$root/@/etc/fstab" "$pool/.provisioning/$name.fstab.before"
sync -f "$pool/.provisioning/$name.fstab.before"
work=$(mktemp -d "$pool/.provisioning/$name.bind.XXXXXX")
printf 'Private root-binding evidence: %s\n' "$work"
mutate_device() {
    local method=$1 device=$2 args=$3
    [[ $name != ID-* && $name != 3tchedCtlChatbot ]] || fail 'canonical candidate device mutations are not permitted; reconcile canonical preparation'
    # Invoke directly: installed preparation helpers need not ship a sibling
    # shell wrapper. The supported client validates schema and same-FD receipt.
    # Canonical dispatch validates full native devices (incus.rs) and publishes
    # authoritative SHM (incus_observer.rs) before returning success. The CLI
    # returns the mount receipt, then engine envelope, then device result.
    verify_control_mount
    if ! timeout --kill-after=5s 30s "$client" invoke --mount "$control_mount" \
        --expected-session "$operator" --confirm-side-effects --args "$args" \
        "plugins/incus/operations/$method/call" > "$work/$method-$device.receipt.json" \
        2> "$work/$method-$device.diagnostic"; then
        cat "$work/$method-$device.diagnostic" >&2
        fail "incus/$method failed or outcome is unknown; retain intent/evidence, do not retry or bypass admission"
    fi
    jq -e --arg method "$method" --arg name "$name" --arg device "$device" '
        .plugin_id=="incus" and .method==$method and
        .result.success==true and .result.plugin_id=="incus" and .result.method==$method and
        .result.result.instance_name==$name and .result.result.device_name==$device
    ' "$work/$method-$device.receipt.json" >/dev/null || fail 'device receipt target differs'
    verify_control_mount
    read_instance
    printf '%s\n' "$instance" > "$work/$method-$device.instance.json"
}
jq '.phase="root-bind-intent"' "$receipt" > "$receipt.new"
sync -f "$receipt.new"
mv -T "$receipt.new" "$receipt"
for device in 3tchedfs system-bus; do
    if jq -e --arg device "$device" '.devices|has($device)' <<<"$instance" >/dev/null; then
        args=$(jq -cn --arg name "$name" --arg device "$device" '{instance_name:$name,device_name:$device}')
        mutate_device remove_device "$device" "$args"
    fi
    jq -e --arg device "$device" 'all(.devices,.expanded_devices; has($device)|not)' <<<"$instance" >/dev/null || fail "host device still present: $device"
done
if jq -e '.devices|has("fuse")' <<<"$instance" >/dev/null; then
    jq -e '.devices.fuse=={type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"}' <<<"$instance" >/dev/null || fail 'different fuse device'
else
    args=$(jq -cn --arg name "$name" '{instance_name:$name,device_name:"fuse",device:{type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"}}')
    mutate_device add_device fuse "$args"
fi
jq -e 'all(.devices,.expanded_devices; .fuse=={type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"})' <<<"$instance" >/dev/null || fail 'guest fuse device readback differs'
fstab_tmp=$(mktemp "$root/@/etc/.fstab.XXXXXX")
guest_fstab filter "$pool/.provisioning/$name.fstab.before" > "$fstab_tmp"
chmod --reference="$root/@/etc/fstab" "$fstab_tmp"
chown --reference="$root/@/etc/fstab" "$fstab_tmp"
sync -f "$fstab_tmp"
mv -T "$fstab_tmp" "$root/@/etc/fstab"
printf '%s\n' "$sid" > "$root/@/etc/hostname"
# Provider-owned volatile state is not accepted by generic Incus config
# mutation. This remains explicit host maintenance, never a denial fallback.
timeout --kill-after=5s 30s incus --force-local --project default config set "$name" "volatile.last_state.idmap=$idmap" || fail 'host idmap maintenance failed; reconcile before retry'
sync -f "$root"
mkdir -p "$view" "$seed_layer" "$upgrade_layer"
mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$root/@" "$seed_layer"
mount --bind -o ro,X-mount.idmap=b:0:1000000:1000000000 "$replica/@" "$upgrade_layer"
mount -t overlay overlay -o "ro,lowerdir=$upgrade_layer:$seed_layer" "$view"
guest_fstab verify "$view/etc/fstab"
mount --bind "$view" "$container_root"
for member in memory rules shared skills soul user; do
    mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_$member" "$container_root/agent/$member"
    case "$member" in shared|soul) mount -o remount,bind,ro "$container_root/agent/$member" ;; esac
done
mount --bind -o X-mount.idmap=b:0:1000000:1000000000 "$root/@agent_user/persist" "$container_root/opt/run-mounts/persist"
read_instance
[[ $(select_idmap) == "$idmap" ]] || fail 'container idmap changed during binding'
jq -e --arg idmap "$idmap" '.config["volatile.last_state.idmap"]==$idmap and
    all(.devices,.expanded_devices; .fuse=={type:"unix-char",source:"/dev/fuse",path:"/dev/fuse"} and
        (has("3tchedfs")|not) and (has("system-bus")|not))' <<<"$instance" >/dev/null || fail 'final guest device/idmap readback differs'
[[ $(findmnt -rn -M "$container_root" -o FSTYPE) == overlay ]] || fail 'effective guest root is not the prepared overlay'
guest_fstab verify "$container_root/etc/fstab"
jq --arg root "$container_root" --arg view "$view" --argjson instance "$instance" \
   '.phase="root-bound"|.root_bound=true|.threetched_root=$root|.instance=$instance|
    .root_view=$view|.root_view_readonly=true|.upgrade_paired=true|
    .guest_fuse_configured=true|.guest_fuse_mounted=false|.host_fuse_mirrored=false' "$receipt" > "$receipt.new"
sync -f "$receipt.new"
mv -T "$receipt.new" "$receipt"
sync -f "$pool/.provisioning"
printf 'Existing root bound; container stopped, identity adoption and live guest FUSE remain pending.\n'
