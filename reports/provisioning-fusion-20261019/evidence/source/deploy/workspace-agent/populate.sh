#!/usr/bin/env bash
# Populate an unaccepted, externally prepared base. Caller owns storage resealing.
set -euo pipefail
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
[[ $# == 2 && $(id -u) == 0 ]] || fail 'usage (root): populate.sh BASE_TOPLEVEL PREPARED_ASSETS'
base=$(realpath -e -- "$1")
assets=$(realpath -e -- "$2")
[[ $base != / && -f $base/@/etc/os-release ]] || fail 'invalid base'
[[ $(findmnt -rn -M "$base" -o FSTYPE) == btrfs && $(findmnt -rn -M "$base" -o FSROOT) == / ]] || fail 'requires mounted Btrfs top level'
[[ $(findmnt -rn -M "$base" -o OPTIONS) == *rw,* ]] || fail 'caller must prepare writable base'
for member in memory rules shared skills soul user; do
    target=$base/@agent_$member
    [[ -d $target && ! -L $target ]] || fail "missing sibling: $member"
    btrfs subvolume show "$target" >/dev/null
    [[ -z $(find "$target" -mindepth 1 -maxdepth 1 -print -quit) ]] || fail "refusing nonempty sibling: $member"
    [[ -d $assets/$member && ! -L $assets/$member ]] || fail "missing assets: $member"
    [[ -z $(find "$assets/$member" -type l -print -quit) ]] || fail 'asset symlinks must be authored explicitly'
    [[ -d $base/@/agent/$member && ! -L $base/@/agent/$member ]] || fail 'unsafe guest mountpoint'
    [[ -z $(find "$base/@/agent/$member" -mindepth 1 -maxdepth 1 -print -quit) ]] || fail 'guest mountpoint must remain empty'
done
for entry in AGENTS.md SOUL.md USER.md MEMORY.md; do
    [[ ! -e $base/@/agent/$entry && ! -L $base/@/agent/$entry ]] || fail "existing guest entry: $entry"
done
[[ ! -L $base/@/etc/3tched ]] || fail 'unsafe host-folder mountpoint'
if [[ -e $base/@/etc/3tched ]]; then
    [[ -d $base/@/etc/3tched && -z $(find "$base/@/etc/3tched" -mindepth 1 -print -quit) ]] || fail 'host-folder mountpoint must be empty'
fi
for member in memory rules shared skills soul user; do
    cp -a "$assets/$member/." "$base/@agent_$member/"
    chown -R root:root "$base/@agent_$member"
    find "$base/@agent_$member" -type d -exec chmod 0755 {} +
    find "$base/@agent_$member" -type f -exec chmod 0644 {} +
    diff -qr "$assets/$member" "$base/@agent_$member"
done
chmod 0700 "$base/@agent_memory" "$base/@agent_rules" "$base/@agent_user"
chmod 0700 "$base/@agent_rules/rules" "$base/@agent_user/user"
chmod 0600 "$base/@agent_rules/rules/rules.md" "$base/@agent_user/user/user.md"
install -d -m 0700 "$base/@agent_user/identity"
for method in memory_store memory_recall memory_retrieve memory_query memory_delete memory_list_namespaces memory_semantic_query; do
    ln -s "/run/mount/3tchedfs/plugins/cognitive/operations/$method" "$base/@agent_memory/$method"
done
ln -s rules/AGENTS.md "$base/@/agent/AGENTS.md"
ln -s soul/SOUL.md "$base/@/agent/SOUL.md"
ln -s user/USER.md "$base/@/agent/USER.md"
ln -s memory/MEMORY.md "$base/@/agent/MEMORY.md"
ln -s /run/mount/3tchedfs/plugins/cognitive "$base/@agent_shared/cognitive"
ln -s /run/mount/3tchedfs/plugins/agent_config "$base/@agent_shared/agent-config"
ln -s /run/mount/3tchedfs/plugins/threetched "$base/@agent_shared/threetched-plugins"
ln -s /run/mount/3tchedfs/plugins/tched_router "$base/@agent_skills/runtime"
ln -s /run/mount/3tchedfs/plugins/threetched "$base/@agent_skills/skills"
install -d -m 0755 "$base/@/etc/3tched"
for member in skills rules user shared soul memory; do
    ln -s "/etc/3tched/$member" "$base/@agent_$member/host"
done
manifest=$base/@/usr/share/op-dbus/agents/workspace
install -d -m 0755 "$manifest"
(
    cd "$base"
    find @agent_memory @agent_rules @agent_shared @agent_skills @agent_soul @agent_user -type f -print0 |
        sort -z | xargs -0 sha256sum
) > "$manifest/SHA256SUMS"
printf '%s\n' 'Shared templates only. No identity database or private data initialized. Runtime loading requires explicit integration.' > "$manifest/README"
sync -f "$base"
printf 'Populated six agent siblings; kept guest mountpoints empty and authored workspace links.\n'
