#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
HELPER=$(realpath -e -- "$SCRIPT_DIR/../../runit/libexec-3tched/op-identity-image")
TMP=$(mktemp -d "${TMPDIR:-/tmp}/op-identity-image-test.XXXXXXXX")
trap 'rm -rf -- "$TMP"' EXIT
MOCK_BIN=$TMP/bin
mkdir -p -- "$MOCK_BIN"

cat > "$MOCK_BIN/btrfs" <<'MOCK'
#!/usr/bin/env bash
set -eu
[[ ${1-} == subvolume && ${2-} == list ]] || exit 2
cat <<'EOF'
ID 256 gen 1 top level 5 path @
ID 257 gen 1 top level 5 path @agent_memory
ID 258 gen 1 top level 5 path @agent_rules
ID 259 gen 1 top level 5 path @agent_shared
ID 260 gen 1 top level 5 path @agent_skills
ID 261 gen 1 top level 5 path @agent_soul
ID 262 gen 1 top level 5 path @agent_user
EOF
if [[ ${BTRFS_EXTRA_MEMBER:-0} == 1 ]]; then
    printf '%s\n' 'ID 263 gen 1 top level 5 path unexpected'
fi
MOCK
chmod 0755 -- "$MOCK_BIN/btrfs"
export PATH="$MOCK_BIN:$PATH"

fail() { printf 'not ok: %s\n' "$*" >&2; exit 1; }
ok() { printf 'ok: %s\n' "$*"; }
expect_fail() {
    local label=$1
    shift
    if "$@" >"$TMP/stdout" 2>"$TMP/stderr"; then
        fail "$label unexpectedly succeeded"
    fi
}

new_fixture() {
    local name=$1 top=$TMP/$1 root=$TMP/$1/@
    mkdir -p -- "$top" "$root/etc" "$root/usr/lib/systemd" "$root/usr/sbin" \
        "$root/usr/lib" "$root/var/lib/dpkg" \
        "$top/@agent_memory" "$top/@agent_rules" "$top/@agent_shared" \
        "$top/@agent_skills" "$top/@agent_soul" "$top/@agent_user"
    printf '%s\n' 'ID=ubuntu' 'VERSION_ID="26.04"' > "$root/usr/lib/os-release"
    ln -s ../usr/lib/os-release "$root/etc/os-release"
    printf '%s\n' 'proc /proc proc defaults 0 0' > "$root/etc/fstab"
    printf '%s\n' fixture-systemd > "$root/usr/lib/systemd/systemd"
    ln -s ../lib/systemd/systemd "$root/usr/sbin/init"
    : > "$root/etc/machine-id"
    printf '%s\n' ubuntu > "$root/etc/hostname"
    cat > "$root/var/lib/dpkg/status" <<'EOF'
Package: base-files
Status: install ok installed
Version: 1.0

Package: dpkg
Status: install ok installed
Version: 1.0

Package: systemd
Status: install ok installed
Version: 1.0

Package: systemd-sysv
Status: install ok installed
Version: 1.0
EOF
    cat > "$TMP/$name.provenance.json" <<'EOF'
{"archive_name":"ubuntu-26.04-server-cloudimg-amd64-root.tar.xz","archive_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","archive_url":"https://cloud-images.ubuntu.com/releases/resolute/release-20260927/ubuntu-26.04-server-cloudimg-amd64-root.tar.xz","checksum_url":"https://cloud-images.ubuntu.com/releases/resolute/release-20260927/SHA256SUMS","signature_url":"https://cloud-images.ubuntu.com/releases/resolute/release-20260927/SHA256SUMS.gpg","signer_primary_fingerprint":"D2EB44626FDDC30B513D5BB71A5D6C4C7DB87C81","verified_at_utc":"2026-10-05T00:00:00Z"}
EOF
}

fixture=$TMP/base
new_fixture base
TOP=$(realpath -e -- "$fixture")
"$HELPER" prepare "$TOP" ubuntu-26.04-fixture "$TMP/base.provenance.json" >/dev/null
receipt=$("$HELPER" verify "$TOP")
[[ $(jq -r .release_id <<<"$receipt") == ubuntu-26.04-fixture ]] || fail 'full verify receipt has wrong release ID'
[[ $(jq -r .layout <<<"$receipt") == shared-partition-siblings-v2 ]] || fail 'full verify receipt has wrong layout'
[[ $(jq -r .manifest_sha256 <<<"$receipt") =~ ^[0-9a-f]{64}$ ]] || fail 'manifest digest is malformed'
metadata_receipt=$("$HELPER" verify-metadata "$TOP")
[[ $(jq -S . <<<"$receipt") == "$(jq -S . <<<"$metadata_receipt")" ]] || fail 'metadata-only receipt differs from full receipt'
"$HELPER" inspect "$TOP" >/dev/null
[[ $(wc -l < "$TOP/@/usr/share/op-dbus/image/packages.tsv" | tr -d ' ') == 4 ]] || fail 'package inventory is incomplete'
ok 'prepare, full verify, metadata verify, and inspect return pinned metadata'

printf '%s\n' modified-systemd > "$TOP/@/usr/lib/systemd/systemd"
expect_fail 'changed critical file full verification' "$HELPER" verify "$TOP"
"$HELPER" verify-metadata "$TOP" >/dev/null
ok 'metadata-only verification permits normal guest file updates'

printf '%s\n' changed > "$TOP/@/usr/share/op-dbus/image/release.json"
expect_fail 'tampered metadata artifact verification' "$HELPER" verify-metadata "$TOP"
ok 'metadata-only verification detects metadata drift'

new_fixture extra
TOP_EXTRA=$(realpath -e -- "$TMP/extra")
BTRFS_EXTRA_MEMBER=1 "$HELPER" prepare "$TOP_EXTRA" extra-release "$TMP/extra.provenance.json" >/dev/null 2>&1 && fail 'extra Btrfs sibling unexpectedly accepted'
ok 'prepare rejects a non-exact Btrfs sibling set'

new_fixture identity
TOP_ID=$(realpath -e -- "$TMP/identity")
printf '%s\n' nonneutral > "$TOP_ID/@/etc/machine-id"
expect_fail 'non-neutral machine ID during prepare' "$HELPER" prepare "$TOP_ID" identity-release "$TMP/identity.provenance.json"
ok 'prepare refuses personalized machine identity'

new_fixture symlink
TOP_SYM=$(realpath -e -- "$TMP/symlink")
mkdir -p -- "$TOP_SYM/@/usr/share/op-dbus"
ln -s "$TMP/outside" "$TOP_SYM/@/usr/share/op-dbus/image"
expect_fail 'symlink metadata target during prepare' "$HELPER" prepare "$TOP_SYM" symlink-release "$TMP/symlink.provenance.json"
ok 'prepare refuses to adopt a symlink metadata target'
