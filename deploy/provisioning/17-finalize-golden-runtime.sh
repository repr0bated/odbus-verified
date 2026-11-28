#!/bin/bash
# Finalize an unused private writable seed only AFTER the fresh source build
# and explicit opblob seal completed. Never reseals or edits an existing guest.
# Usage: NAME UUID ARTIFACT_DIR BRIDGE_SHA256 CATALOG_DIR MANIFEST_SHA256 RECEIPT_DIR
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 077
[[ $# == 7 && $(id -u) == 0 ]] || exit 2
name=$1 uuid=$2 artifacts=$3 bridge_sha=$4 catalog=$5 catalog_sha=$6 receipt_dir=$7
[[ $name =~ ^@golden-[a-zA-Z0-9-]+$ && $bridge_sha =~ ^[a-f0-9]{64}$ && $catalog_sha =~ ^[a-f0-9]{64}$ && $receipt_dir == /var/lib/opdbus-release/* ]] || exit 2
seed=/var/lib/incus/storage-pools/3tched-pool/images/$name
pool=/var/lib/incus/storage-pools/3tched-pool
uuid_of() { btrfs subvolume show "$1" | awk '$1=="UUID:" {print $2}'; }
[[ $(findmnt -rn -M "$pool" -o UUID) == b6a13d79-ed2d-4057-8d0c-252fb7947f94 && $(uuid_of "$seed") == "$uuid" ]] || exit 1
[[ $(btrfs property get -ts "$seed" ro) == ro=false && -z $(btrfs subvolume list -o "$seed") ]] || exit 1
case $artifacts in /mnt/@opdbus-system-a/opt/op-build/target/release|/mnt/@opdbus-system-a/opt/op-build/target/x86_64-unknown-linux-gnu/release) ;; *) exit 1;; esac
[[ $(findmnt -rn -T "$artifacts" -o UUID) == 69ee351a-e4a4-44db-a318-b4574a2439ae ]] || exit 1
[[ $(sha256sum "$artifacts/op-grpc-bridge" | cut -d' ' -f1) == "$bridge_sha" && -f $catalog/.manifest.json && ! -L $catalog/.manifest.json && $(sha256sum "$catalog/.manifest.json" | cut -d' ' -f1) == "$catalog_sha" ]] || exit 1
# A seed already referenced by any retained native root must never be changed.
for instance in $(incus --force-local list --format csv -c n); do
  root=$pool/containers/$instance
  [[ ! -d $root ]] || [[ $(btrfs subvolume show "$root" | awk '$1=="Parent" && $2=="UUID:" {print $3}') != "$uuid" ]] || exit 1
done
install -d -m 0700 "$receipt_dir"
exec 9>"$receipt_dir/lock"; flock -n 9
R=$seed/rootfs
[[ -f $R/usr/share/op-dbus/golden-runtime.json ]] || exit 1
manifest='{}'
for binary in 3tched-fs op-grpc-bridge op-tunnel-door op-identity-accept; do
  [[ -f $artifacts/$binary && -x $artifacts/$binary && ! -L $artifacts/$binary ]] || exit 1
  case $binary in op-identity-accept) path=/usr/local/bin/$binary;; *) path=/usr/local/libexec/op-dbus/bin/$binary;; esac
  install -m 0755 "$artifacts/$binary" "$R$path.new"
  sha=$(sha256sum "$artifacts/$binary" | cut -d' ' -f1)
  [[ $(sha256sum "$R$path.new" | cut -d' ' -f1) == "$sha" ]] || exit 1
  mv -T "$R$path.new" "$R$path"
  manifest=$(jq -c --arg name "$binary" --arg path "$path" --arg sha "$sha" '. + {($name):{path:$path,sha256:$sha}}' <<<"$manifest")
done
# Copy only the authored sealed catalog files. opblob remains the sole sealer.
staged=$R/usr/share/op-dbus/plugin-blobs
[[ ! -e $staged ]] || { echo 'catalog staging exists; reconcile exact prior attempt' >&2; exit 1; }
install -d -m 0755 "$staged"
while IFS= read -r -d '' file; do
  [[ -f $file && ! -L $file ]] || exit 1
  case ${file##*/} in .manifest.json|*.blob) install -m 0644 "$file" "$staged/";; *) echo 'unexpected catalog member' >&2; exit 1;; esac
done < <(find "$catalog" -mindepth 1 -maxdepth 1 -print0)
[[ $(sha256sum "$staged/.manifest.json" | cut -d' ' -f1) == "$catalog_sha" ]] || exit 1
jq --argjson binaries "$manifest" --arg catalog_sha "$catalog_sha" --arg at "$(date -u +%FT%TZ)" \
  '.binaries=$binaries | .catalog_manifest_sha256=$catalog_sha | .runtime_staged=true | .payload_prepared=true | .finalized_at=$at | .guest_readiness_verified=false' \
  "$R/usr/share/op-dbus/golden-runtime.json" >"$R/usr/share/op-dbus/golden-runtime.json.new"
mv -T "$R/usr/share/op-dbus/golden-runtime.json.new" "$R/usr/share/op-dbus/golden-runtime.json"
sync -f "$seed"
btrfs property set -ts "$seed" ro true
[[ $(btrfs property get -ts "$seed" ro) == ro=true ]] || exit 1
jq -n --arg name "$name" --arg path "$seed" --arg uuid "$uuid" --arg id "$(btrfs inspect-internal rootid "$seed")" \
  --arg manifest_sha "$(sha256sum "$R/usr/share/op-dbus/golden-runtime.json" | cut -d' ' -f1)" --arg catalog_sha "$catalog_sha" --arg metadata_sha "$(sha256sum "$seed/metadata.yaml" | cut -d' ' -f1)" --argjson binaries "$manifest" \
  '{phase:"finalized",seed:$path,seed_id:$id,seed_uuid:$uuid,seed_metadata_sha256:$metadata_sha,pool_fsid:"b6a13d79-ed2d-4057-8d0c-252fb7947f94",seed_location:{incus_dir:"/var/lib/incus",pool:"3tched-pool",seed_dir:"images",seed_name:$name},runtime_staged:true,runtime_manifest_sha256:$manifest_sha,catalog_manifest_sha256:$catalog_sha,binaries:$binaries,guest_readiness_verified:false}' >"$receipt_dir/receipt.json"
sync -f "$receipt_dir/receipt.json"; sync -f "$receipt_dir"
cat "$receipt_dir/receipt.json"
