#!/bin/sh
# Verify the pinned portable UI source and emit a build artifact. This helper
# never installs dashboard files or writes to the external UI checkout.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SOURCE_ARCHIVE="$SCRIPT_DIR/source.tar.gz"
OUTPUT_DIR=target/ui-artifacts/3tched-json-render-ui
NPM_CACHE=${NPM_CACHE:-/var/cache/op-dbus-build/npm}
VERIFY_ONLY=0

usage() {
    cat <<'EOF'
Usage: vendor/3tched-json-render-ui/build-assets.sh [options]
  --source-archive PATH  source archive to verify (default: pinned archive)
  --output-dir PATH      artifact directory
  --npm-cache PATH       npm download cache
  --verify-only          verify provenance and contents without npm/build
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --source-archive) SOURCE_ARCHIVE=$2; shift 2 ;;
        --output-dir) OUTPUT_DIR=$2; shift 2 ;;
        --npm-cache) NPM_CACHE=$2; shift 2 ;;
        --verify-only) VERIFY_ONLY=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

case "$OUTPUT_DIR" in
    /*) ;;
    *) OUTPUT_DIR=$(pwd)/$OUTPUT_DIR ;;
esac
case "$NPM_CACHE" in
    /*) ;;
    *) NPM_CACHE=$(pwd)/$NPM_CACHE ;;
esac

for command in awk find gzip jq npm node sed sha256sum tar wc; do
    command -v "$command" >/dev/null 2>&1 || {
        printf 'required command is unavailable: %s\n' "$command" >&2
        exit 1
    }
done
[ -f "$SOURCE_ARCHIVE" ] || { printf 'source archive is absent: %s\n' "$SOURCE_ARCHIVE" >&2; exit 1; }
[ -f "$SCRIPT_DIR/source.pin" ] || { printf 'source pin is absent\n' >&2; exit 1; }
[ -f "$SCRIPT_DIR/source-sha256sums" ] || { printf 'source manifest is absent\n' >&2; exit 1; }

pin() {
    key=$1
    sed -n "s/^${key}=//p" "$SCRIPT_DIR/source.pin"
}

[ "$(pin format)" = opdbus.3tched-json-render-ui-source.v1 ] || {
    printf 'unsupported UI source pin format\n' >&2
    exit 1
}
expected_archive=$(pin source_archive_sha256)
actual_archive=$(sha256sum "$SOURCE_ARCHIVE" | awk '{print $1}')
[ "$actual_archive" = "$expected_archive" ] || {
    printf 'UI source archive does not match the pinned dirty-worktree artifact\n' >&2
    exit 1
}
expected_manifest=$(pin content_manifest_sha256)
actual_manifest=$(sha256sum "$SCRIPT_DIR/source-sha256sums" | awk '{print $1}')
[ "$actual_manifest" = "$expected_manifest" ] || {
    printf 'UI source content manifest does not match its pin\n' >&2
    exit 1
}
tar -tzf "$SOURCE_ARCHIVE" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad=1 }
    !/^3tched-json-render-ui\// { bad=1 }
    END { exit bad ? 1 : 0 }
' || { printf 'UI source archive has an unsafe or unexpected path\n' >&2; exit 1; }
tar -tvzf "$SOURCE_ARCHIVE" | awk 'substr($1,1,1) !~ /[-d]/ {bad=1} END {exit bad ? 1 : 0}' || {
    printf 'UI source archive must contain only files and directories\n' >&2
    exit 1
}

stage=$(mktemp -d "${TMPDIR:-/var/tmp}/op-ui-build.XXXXXX")
tar -xzf "$SOURCE_ARCHIVE" -C "$stage"
source_root="$stage/3tched-json-render-ui"
asset_base=$(pin asset_base)
door_default=$(pin door_default)
issuer_default=$(pin issuer_default)
[ "$asset_base" = "/" ] || {
    printf 'production UI asset base must be the dashboard root: %s\n' "$asset_base" >&2
    exit 1
}
[ "$door_default" = "https://10.0.0.2:8090" ] || {
    printf 'production UI door default is not the pinned :8090 door: %s\n' "$door_default" >&2
    exit 1
}
(cd "$source_root" && sha256sum -c "$SCRIPT_DIR/source-sha256sums" >/dev/null) || {
    printf 'UI source files do not match the pinned content manifest\n' >&2
    exit 1
}
listed=$(wc -l < "$SCRIPT_DIR/source-sha256sums" | tr -d ' ')
actual=$(find "$source_root" -type f | wc -l | tr -d ' ')
[ "$listed" = "$actual" ] || {
    printf 'UI source archive contains an unmanifested file\n' >&2
    exit 1
}
[ ! -e "$source_root/.env" ] && [ ! -e "$source_root/.git" ] &&
    [ ! -e "$source_root/node_modules" ] && [ ! -e "$source_root/dist" ] || {
        printf 'UI source archive contains a forbidden generated/private path\n' >&2
        exit 1
    }

if [ "$VERIFY_ONLY" = 1 ]; then
    printf 'verified UI source archive %s (%s files)\n' "$actual_archive" "$actual"
    exit 0
fi

node_version=$(node --version | sed 's/^v//')
node_major=$(printf '%s' "$node_version" | awk -F. '{print $1}')
node_minor=$(printf '%s' "$node_version" | awk -F. '{print $2}')
node_patch=$(printf '%s' "$node_version" | awk -F. '{print $3}' | sed 's/[^0-9].*$//')
if [ "$node_major" -lt 22 ] ||
   { [ "$node_major" -eq 22 ] && [ "$node_minor" -lt 22 ]; } ||
   { [ "$node_major" -eq 22 ] && [ "$node_minor" -eq 22 ] && [ "$node_patch" -lt 2 ]; }; then
    printf 'Node %s does not satisfy the pinned >=22.22.2 engine\n' "$node_version" >&2
    exit 1
fi

mkdir -p "$NPM_CACHE" "$OUTPUT_DIR"
export NPM_CONFIG_CACHE="$NPM_CACHE"
export NPM_CONFIG_USERCONFIG=/dev/null
(
    cd "$source_root"
    npm ci
    # Build before deployment; the owner runs type-check, lint and the full
    # test suite during post-deployment downtime, not as preparation gates.
    VITE_OPDBUS_DOOR="$door_default" VITE_OPDBUS_ISSUER="$issuer_default" npm run build -- --base "$asset_base"
)
[ -f "$source_root/dist/index.html" ] || { printf 'Vite omitted dist/index.html\n' >&2; exit 1; }
grep -Fq '/assets/' "$source_root/dist/index.html" || {
    printf 'Vite output does not contain root-relative dashboard assets\n' >&2
    exit 1
}
[ -z "$(find "$source_root/dist" -type l -print -quit)" ] || {
    printf 'Vite output contains a symlink\n' >&2
    exit 1
}

asset_archive="$OUTPUT_DIR/assets.tar.gz"
(cd "$source_root/dist" &&
    tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner \
        --mode='u+rwX,go+rX,go-w' -cf - . | gzip -n -9 > "$asset_archive")
asset_sha=$(sha256sum "$asset_archive" | awk '{print $1}')
asset_bytes=$(wc -c < "$asset_archive" | tr -d ' ')
lock_sha=$(sha256sum "$source_root/package-lock.json" | awk '{print $1}')
npm_version=$(npm --version)
jq -n \
    --arg source_head "$(pin source_head)" \
    --arg source_worktree "$(pin source_worktree)" \
    --arg source_archive_sha256 "$actual_archive" \
    --arg content_manifest_sha256 "$actual_manifest" \
    --arg package_lock_sha256 "$lock_sha" \
    --arg node_version "$node_version" \
    --arg npm_version "$npm_version" \
    --arg base "$asset_base" \
    --arg door "$door_default" \
    --arg issuer "$issuer_default" \
    --arg asset_sha256 "$asset_sha" \
    --argjson asset_bytes "$asset_bytes" \
    '{format:"opdbus.3tched-json-render-ui-assets.v1",
      source:{head:$source_head,worktree:$source_worktree,archive_sha256:$source_archive_sha256,content_manifest_sha256:$content_manifest_sha256,package_lock_sha256:$package_lock_sha256},
      build:{node_version:$node_version,npm_version:$npm_version,base:$base,door:$door,issuer:$issuer},
      artifact:{name:"assets.tar.gz",sha256:$asset_sha256,bytes:$asset_bytes}}' \
    > "$OUTPUT_DIR/manifest.json"
printf 'built UI assets: %s\n' "$OUTPUT_DIR"
