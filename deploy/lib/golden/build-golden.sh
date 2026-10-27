#!/bin/sh
# build-golden.sh — prepare the golden btrfs subvolume from one release build.
#
# Populate the `golden` btrfs subvolume with the complete deployable content.
# Taking a read-only snapshot, transferring it with `btrfs send/receive`, and
# activating the received filesystem belong to the deployment process. This
# preparation script performs none of those deployment steps and never writes
# into the running host's installation or restarts a service.
#
# Layout created under $OPDBUS_ROOT (default /opt/op-dbus):
#
#   golden/                      the deployable subvolume
#     bin/                       release binaries
#     libexec/3tched/             application and explicit provisioning helpers/assets
#     usr/bin/threetched          pinned threetched dependency
#     usr/share/op-dbus/          immutable config, catalog and provenance
#     usr/local/share/op-dbus/dashboard/  pinned production UI assets at root
#     sv/<service>/run            enabled application runit definitions
#     opt/                       immutable broker and provider runtimes
# Host secrets, grants, network/bootstrap services and writable data are excluded.
#     MANIFEST                   commit, build time, sha256 per binary
#
# Usage:
#   build-golden.sh                 # prepare and validate golden (default)
#   build-golden.sh --golden-only   # compatibility alias for the default
#   build-golden.sh --dry-run
#   build-golden.sh --system    # also stage the declared base-system payload
#
# Retired options `--live-only` and `--no-restart` fail explicitly. Requires
# root to populate the btrfs subvolume. Never builds: run
# `CXXFLAGS="-include cstdint" cargo build --workspace --release` first (the
# vendored RocksDB in cozorocks needs that flag on modern GCC).
# `OP_PERSISTED_CATALOG_SOURCE` may name an isolated prepared catalog; it
# defaults to `/var/lib/opdbus/plugin-blobs` and is never modified here.
# `OP_RELEASE_DIR` selects the same prepared optimized artifact directory as
# activate-golden.sh; it defaults to the workspace's target/release.
set -eu

OPDBUS_ROOT=${OPDBUS_ROOT:-/opt/op-dbus}
OPDBUS_ROOT=$(realpath -e -- "$OPDBUS_ROOT")
GOLDEN_DIR="$OPDBUS_ROOT/golden"

DRY_RUN=0
PREPARE_SYSTEM=0

SCRIPT_PATH=$(readlink -f "$0" 2>/dev/null || printf '%s' "$0")
# Internal helper of ./activate-golden.sh; every relative input lives in deploy/runit.
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/../../runit" && pwd)
if [ -f "$SCRIPT_DIR/../../Cargo.toml" ]; then
    PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
else
    PROJECT_ROOT=${OP_DBUS_ROOT:-$(pwd)}
fi
RELEASE_DIR=$(realpath -e -- "${OP_RELEASE_DIR:-"$PROJECT_ROOT/target/release"}")
MCP_AUDIENCE_POLICY_SOURCE="$SCRIPT_DIR/../config/mcp-audience-policy.json"
HOST_RUNTIME_PACKAGES_SOURCE="$SCRIPT_DIR/../config/host-runtime-packages.txt"
PERSISTED_CATALOG_DIR=${OP_PERSISTED_CATALOG_SOURCE:-/var/lib/opdbus/plugin-blobs}
PERSISTED_CATALOG_DIR=$(realpath -e -- "$PERSISTED_CATALOG_DIR")
case "$PERSISTED_CATALOG_DIR" in
    "$GOLDEN_DIR"|"$GOLDEN_DIR"/*)
        printf '%s\n' "catalog source must be outside the golden preparation tree" >&2
        exit 1
        ;;
esac
RETIRED_SERVICES_FILE="$SCRIPT_DIR/retired-services"
RETIRED_BINARIES_FILE="$SCRIPT_DIR/retired-binaries"
EMQX_VERSION_FILE="$SCRIPT_DIR/../config/emqx.version"
EMQX_CONFIG_DIR="$SCRIPT_DIR/../emqx"
EMQX_ARTIFACT_CACHE_DIR=${EMQX_ARTIFACT_CACHE_DIR:-/var/cache/op-dbus/source-artifacts}
MCP_PROVIDER_ARTIFACT_CACHE_DIR=${MCP_PROVIDER_ARTIFACT_CACHE_DIR:-/var/cache/op-dbus/source-artifacts}
NOTEBOOKLM_MCP_VERSION_FILE="$SCRIPT_DIR/../config/notebooklm-mcp.version"
EXECUTION_PYTHON_LOCK="$SCRIPT_DIR/../config/execution-python-requirements.lock"
EXECUTION_PYTHON_PREPARE="$SCRIPT_DIR/prepare-execution-python.sh"
NLM_CLI_ROOT=/opt/opdbus
TCHED_ROUTER_SOURCE_PIN="$PROJECT_ROOT/vendor/tched-router/source.pin"
TCHED_ROUTER_VENDORED_SOURCE="$PROJECT_ROOT/vendor/tched-router/source.tar.gz"
TCHED_ROUTER_ARTIFACT_DIR="$PROJECT_ROOT/target/threetched-artifacts/tched-router"
THREETCHED_BIN="$TCHED_ROUTER_ARTIFACT_DIR/threetched"
THREETCHED_MANIFEST="$TCHED_ROUTER_ARTIFACT_DIR/manifest.json"
TCHED_ROUTER_BOOTSTRAP_CONFIG="$SCRIPT_DIR/../config/threetched-runtime.toml"
TCHED_ROUTER_BOOTSTRAP_SKILLS="$SCRIPT_DIR/../config/threetched-skills/default"
AGENT_BUNDLE_SOURCE="$SCRIPT_DIR/../config/agents"
UI_SOURCE_DIR="$PROJECT_ROOT/vendor/3tched-json-render-ui"
UI_SOURCE_PIN="$UI_SOURCE_DIR/source.pin"
UI_SOURCE_ARCHIVE="$UI_SOURCE_DIR/source.tar.gz"
UI_SOURCE_MANIFEST="$UI_SOURCE_DIR/source-sha256sums"
UI_ASSET_BUILD_HELPER="$UI_SOURCE_DIR/build-assets.sh"
UI_ARTIFACT_DIR="$PROJECT_ROOT/target/ui-artifacts/3tched-json-render-ui"
UI_ASSET_ARCHIVE="$UI_ARTIFACT_DIR/assets.tar.gz"
UI_ASSET_MANIFEST="$UI_ARTIFACT_DIR/manifest.json"

# Explicit source -> release-name inventory, relative to deploy/. Never copy
# the whole host helper directory: network/storage bootstrap stays independent.
# RELEASE_ROOTS maps libexec/3tched to /usr/local/libexec/op-dbus/helpers, NOT
# the base-owned /usr/local/libexec/3tched. Guest files are inert image inputs;
# staging them does not install/enable a host or guest service or prepare a seed.
RELEASE_HELPER_INPUTS='0755 runit/libexec-3tched/emqx-prepare emqx-prepare
0755 runit/libexec-3tched/runit-ready-signal runit-ready-signal
0755 runit/libexec-3tched/prepare-salad-control-plane prepare-salad-control-plane
0755 runit/libexec-3tched/opdbus-rundirs-up opdbus-rundirs-up
0755 runit/libexec-3tched/mount-3tchedfs mount-3tchedfs
0755 scripts/provision-workspace-subscriber.sh provision-workspace-subscriber
0755 scripts/prepare-instance-disk.sh prepare-instance-disk
0755 scripts/sync-instance-upgrade.sh sync-instance-upgrade
0755 scripts/prepare-chatbot-adoption.sh prepare-chatbot-adoption
0755 scripts/bind-instance-root.sh bind-instance-root
0755 scripts/3tchedfs-invoke.sh 3tchedfs-invoke
0755 runit/libexec-3tched/op-identity-root op-identity-root
0755 runit/libexec-3tched/op-identity-image op-identity-image
0755 runit/libexec-3tched/attach-host-workspace-folders attach-host-workspace-folders
0755 runit/libexec-3tched/restore-chatbot-root restore-chatbot-root
0755 workspace-image/prepare-lungs-client.sh prepare-lungs-client
0755 workspace-image/3tchedfs-mount-preflight 3tchedfs-mount-preflight
0755 workspace-image/mount.fuse.3tchedFS mount.fuse.3tchedFS
0644 workspace-image/3tchedfs.service 3tchedfs.service
0644 workspace-image/threetchedfs.service threetchedfs.service
0644 workspace-image/3tchedfs.fstab 3tchedfs.fstab
0644 workspace-image/3tched-lungs.conf 3tched-lungs.conf'

is_application_service() {
    [ -f "$SCRIPT_DIR/$1/run" ] &&
        grep -qx '# op-dbus-release: application' "$SCRIPT_DIR/$1/run" &&
        grep -Ev '^[[:space:]]*(#|$)' "$SCRIPT_DIR/enabled-services" | grep -qx "$1" &&
        ! is_retired_service "$1"
}

# Only this already-provisioned, root-owned preparation tree may be changed.
# Never follow a stale destination link into host state.
ensure_directory() (
    directory=$1
    case "$directory" in "$GOLDEN_DIR"|"$GOLDEN_DIR"/*) ;; *) die "destination outside golden: $directory" ;; esac
    [ ! -L "$directory" ] || die "symlink directory in golden: $directory"
    if [ "$directory" != "$GOLDEN_DIR" ]; then
        ensure_directory "$(dirname "$directory")"
    fi
    if [ ! -e "$directory" ]; then
        run install -d -m 0755 -o root -g root "$directory"
    else
        [ -d "$directory" ] || die "not a directory: $directory"
        [ "$(stat -c %u:%g "$directory")" = 0:0 ] || die "non-root staging directory: $directory"
        [ "$directory" = "$GOLDEN_DIR" ] || {
            [ "$(stat -c %a "$directory")" = 755 ] || run chmod 0755 "$directory"
        }
    fi
)

remove_owned() (
    destination=$1
    parent=$(dirname "$destination")
    while [ "$parent" != "$GOLDEN_DIR" ]; do
        case "$parent" in "$GOLDEN_DIR"/*) ;; *) die "removal outside golden: $destination" ;; esac
        [ ! -L "$parent" ] || die "symlink parent in golden: $parent"
        parent=$(dirname "$parent")
    done
    run rm -r -- "$destination"
)

# Equal bytes retain the inode and extents; metadata-only changes do not copy.
install_file() (
    mode=$1 source=$2 destination=$3
    ensure_directory "$(dirname "$destination")"
    [ ! -L "$destination" ] || die "refusing destination symlink: $destination"
    [ ! -e "$destination" ] || [ -f "$destination" ] || die "not a regular destination: $destination"
    if [ -f "$destination" ] && cmp -s "$source" "$destination"; then
        [ "$(stat -c %u:%g "$destination")" = 0:0 ] || run chown root:root "$destination"
        [ "$(stat -c %a "$destination")" = "${mode#0}" ] || run chmod "$mode" "$destination"
        exit 0
    fi
    if [ "$DRY_RUN" = 1 ]; then
        printf '  would install changed file: %s -> %s\n' "$source" "$destination"
        exit 0
    fi
    temporary=$(mktemp "$(dirname "$destination")/.install.XXXXXX")
    trap 'rm -f -- "$temporary"' EXIT HUP INT TERM
    cp --reflink=auto -- "$source" "$temporary"
    chown root:root "$temporary"
    chmod "$mode" "$temporary"
    mv -Tf -- "$temporary" "$destination"
)

# For wholly owned, regular-file source trees only. No unconditional recopy.
sync_tree() (
    source=$1 destination=$2
    [ -z "$(find "$source" ! -type f ! -type d -print -quit)" ] ||
        die "unsupported source entry in $source"
    ensure_directory "$destination"
    if [ -d "$destination" ]; then
        find "$destination" -depth -mindepth 1 -print | while IFS= read -r entry; do
            relative=${entry#"$destination"/}
            if [ ! -e "$source/$relative" ]; then
                if [ -d "$entry" ] && [ ! -L "$entry" ]; then
                    run rmdir -- "$entry"
                else
                    run rm -- "$entry"
                fi
            fi
        done
    fi
    find "$source" -type d -print | while IFS= read -r entry; do
        [ "$entry" = "$source" ] || ensure_directory "$destination/${entry#"$source"/}"
    done
    find "$source" -type f -print | while IFS= read -r entry; do
        mode=0644
        [ ! -x "$entry" ] || mode=0755
        install_file "$mode" "$entry" "$destination/${entry#"$source"/}"
    done
)
is_retired_service() {
    svc=$1
    [ -f "$RETIRED_SERVICES_FILE" ] &&
        grep -Ev '^[[:space:]]*(#|$)' "$RETIRED_SERVICES_FILE" | grep -qx "$svc"
}

is_retired_binary() {
    binary=$1
    [ -f "$RETIRED_BINARIES_FILE" ] &&
        grep -Ev '^[[:space:]]*(#|$)' "$RETIRED_BINARIES_FILE" | grep -qx "$binary"
}

log()  { printf '\033[0;34m[golden]\033[0m %s\n' "$*"; }
ok()   { printf '\033[0;32m[ ok  ]\033[0m %s\n' "$*"; }
die()  { printf '\033[0;31m[fail ]\033[0m %s\n' "$*" >&2; exit 1; }

run() {
    if [ "$DRY_RUN" = 1 ]; then
        printf '  would run: %s\n' "$*"
    else
        "$@"
    fi
}

write_nlm_wrapper() {
    dest=$1
    runtime_venv=$2
    if [ "$DRY_RUN" = 1 ]; then
        printf '  would write nlm wrapper: %s -> %s\n' "$dest" "$runtime_venv"
        return 0
    fi
    # Never follow an existing wrapper symlink or truncate a running wrapper.
    [ ! -L "$dest" ] || die "refusing to overwrite a symlink at $dest"
    wrapper_stage=$(mktemp "$(dirname "$dest")/.nlm-wrapper.XXXXXX")
    # Invoke the module: venv console-script shebangs contain the temporary
    # build path and are not relocatable. This is always the target's path,
    # never a path beneath the golden staging prefix.
    printf '#!/bin/sh\nexec "%s/bin/python" -I -m notebooklm_tools.cli.main "$@"\n' \
        "$runtime_venv" > "$wrapper_stage"
    install_file 0755 "$wrapper_stage" "$dest"
    rm -- "$wrapper_stage"
}

install_nlm_cli() {
    dest_root=$1
    wrapper=$2
    venv="$dest_root$NLM_CLI_ROOT/$NOTEBOOKLM_MCP_INSTALL_NAME"
    marker="$venv/.opdbus-artifact-sha256"
    if [ -f "$marker" ] && [ "$(sed -n '1p' "$marker")" = "$NOTEBOOKLM_MCP_SHA256" ] &&
       [ -x "$venv/bin/python" ]; then
        :
    else
        [ ! -e "$venv" ] && [ ! -L "$venv" ] ||
            die "content-addressed nlm root exists but is invalid; preserving it: $venv"
        if [ "$DRY_RUN" = 1 ]; then
            printf '  would venv+pip: %s from %s\n' "$venv" "$NOTEBOOKLM_MCP_ARTIFACT_PATH"
        else
            ensure_directory "$dest_root$NLM_CLI_ROOT"
            nlm_stage=$(mktemp -d "$dest_root$NLM_CLI_ROOT/.nlm-stage.XXXXXX")
            python3 -m venv "$nlm_stage" || die "python3 -m venv failed; retained $nlm_stage"
            "$nlm_stage/bin/python" -I -m pip install --no-cache-dir --disable-pip-version-check \
                "$NOTEBOOKLM_MCP_ARTIFACT_PATH" ||
                die "pip install notebooklm-mcp-cli failed; retained $nlm_stage"
            "$nlm_stage/bin/python" -I -m notebooklm_tools.cli.main --version >/dev/null ||
                die "nlm entry point validation failed; retained $nlm_stage"
            printf '%s\n' "$NOTEBOOKLM_MCP_SHA256" > "$nlm_stage/.opdbus-artifact-sha256"
            chmod 0755 "$nlm_stage"
            mv -T -- "$nlm_stage" "$venv"
        fi
    fi
    ensure_directory "$venv"
    write_nlm_wrapper "$wrapper" "$NLM_CLI_ROOT/$NOTEBOOKLM_MCP_INSTALL_NAME"
}

[ -r "$EMQX_VERSION_FILE" ] || die "missing EMQX artifact declaration: $EMQX_VERSION_FILE"
# shellcheck disable=SC1090 -- release-owned, fixed-key declaration validated below.
. "$EMQX_VERSION_FILE"
case "${EMQX_VERSION:-}" in *[!0-9.]*|'') die "invalid EMQX_VERSION" ;; esac
case "${EMQX_ARTIFACT:-}" in *[!A-Za-z0-9._-]*|'') die "invalid EMQX_ARTIFACT" ;; esac
case "${EMQX_SHA256:-}" in *[!0-9a-f]*|'') die "invalid EMQX_SHA256" ;; esac
[ "${#EMQX_SHA256}" -eq 64 ] || die "EMQX_SHA256 must contain 64 lowercase hex characters"
EMQX_ARTIFACT_PATH="$EMQX_ARTIFACT_CACHE_DIR/$EMQX_ARTIFACT"
EMQX_INSTALL_NAME="emqx-$EMQX_VERSION-$EMQX_SHA256"

[ -r "$NOTEBOOKLM_MCP_VERSION_FILE" ] ||
    die "missing NotebookLM nlm wheel declaration: $NOTEBOOKLM_MCP_VERSION_FILE"
# shellcheck disable=SC1090 -- release-owned, fixed-key declaration validated below.
. "$NOTEBOOKLM_MCP_VERSION_FILE"
case "${NOTEBOOKLM_MCP_VERSION:-}" in *[!0-9.]*|'') die "invalid NOTEBOOKLM_MCP_VERSION" ;; esac
case "${NOTEBOOKLM_MCP_ARTIFACT:-}" in *[!A-Za-z0-9._-]*|'') die "invalid NOTEBOOKLM_MCP_ARTIFACT" ;; esac
case "${NOTEBOOKLM_MCP_SHA256:-}" in *[!0-9a-f]*|'') die "invalid NOTEBOOKLM_MCP_SHA256" ;; esac
[ "${#NOTEBOOKLM_MCP_SHA256}" -eq 64 ] ||
    die "NOTEBOOKLM_MCP_SHA256 must contain 64 lowercase hex characters"
NOTEBOOKLM_MCP_ARTIFACT_PATH="$MCP_PROVIDER_ARTIFACT_CACHE_DIR/$NOTEBOOKLM_MCP_ARTIFACT"
NOTEBOOKLM_MCP_INSTALL_NAME="nlm-cli-$NOTEBOOKLM_MCP_VERSION-$NOTEBOOKLM_MCP_SHA256"

[ -r "$TCHED_ROUTER_SOURCE_PIN" ] ||
    die "missing threetched source pin: $TCHED_ROUTER_SOURCE_PIN"
# shellcheck disable=SC1090 -- release-owned, fixed-key declaration validated below.
. "$TCHED_ROUTER_SOURCE_PIN"
case "${UPSTREAM_TAG:-}" in v[0-9]*.[0-9]*.[0-9]*) ;; *) die "invalid tched_router UPSTREAM_TAG" ;; esac
case "${UPSTREAM_COMMIT:-}" in *[!0-9a-f]*|'') die "invalid tched_router UPSTREAM_COMMIT" ;; esac
[ "${#UPSTREAM_COMMIT}" -eq 40 ] || die "tched_router UPSTREAM_COMMIT must contain 40 lowercase hex characters"
case "${SOURCE_ARCHIVE_SHA256:-}" in *[!0-9a-f]*|'') die "invalid tched_router SOURCE_ARCHIVE_SHA256" ;; esac
[ "${#SOURCE_ARCHIVE_SHA256}" -eq 64 ] ||
    die "tched_router SOURCE_ARCHIVE_SHA256 must contain 64 lowercase hex characters"
case "${VENDORED_SOURCE_GZIP_SHA256:-}" in *[!0-9a-f]*|'') die "invalid tched_router VENDORED_SOURCE_GZIP_SHA256" ;; esac
[ "${#VENDORED_SOURCE_GZIP_SHA256}" -eq 64 ] ||
    die "tched_router VENDORED_SOURCE_GZIP_SHA256 must contain 64 lowercase hex characters"
[ -f "$TCHED_ROUTER_VENDORED_SOURCE" ] ||
    die "missing vendored tched_router source archive: $TCHED_ROUTER_VENDORED_SOURCE"
[ "$(sha256sum "$TCHED_ROUTER_VENDORED_SOURCE" | cut -d' ' -f1)" = "$VENDORED_SOURCE_GZIP_SHA256" ] ||
    die "vendored tched_router source archive does not match source.pin"

while [ $# -gt 0 ]; do
    case "$1" in
        --golden-only) ;;
        --system) PREPARE_SYSTEM=1 ;;
        --live-only)   die "--live-only is retired; deploy only with btrfs send/receive of a prepared snapshot" ;;
        --no-restart)  die "--no-restart is retired because this script never installs or restarts live services" ;;
        --dry-run)     DRY_RUN=1 ;;
        -h|--help)     sed -n '2,35p' "$SCRIPT_PATH"; exit 0 ;;
        *)             die "unknown option: $1" ;;
    esac
    shift
done

# ── Preconditions ───────────────────────────────────────────────────────────
printf '%s\n' "$RELEASE_HELPER_INPUTS" | while read -r mode source helper; do
    input="$SCRIPT_DIR/../$source"
    [ -f "$input" ] && [ ! -L "$input" ] && [ -r "$input" ] ||
        die "missing or unsafe release helper input: $input"
    # Some guest script sources are deliberately non-executable in Git.
    # install_file applies the declared release mode, not the checkout mode.
done
[ -d "$RELEASE_DIR" ] || die "no $RELEASE_DIR — build first:
  CXXFLAGS=\"-include cstdint\" cargo build --workspace --release"

BINARIES=$(find "$RELEASE_DIR" -maxdepth 1 -type f -executable ! -name '*.d' ! -name '*.so' | sort |
    while IFS= read -r bin; do
        # Container and decoy owners have separate offline release artifacts.
        case "$(basename "$bin")" in
            op-mail-admin|op-decoy-*|op-wgcf*|op-ovs*|op-rtnetlink*|op-storage*|op-release*) continue ;;
        esac
        is_retired_binary "$(basename "$bin")" || printf '%s\n' "$bin"
    done)
[ -n "$BINARIES" ] || die "$RELEASE_DIR contains no executables"
[ -f "$RELEASE_DIR/3tched-fs" ] && [ -x "$RELEASE_DIR/3tched-fs" ] ||
    die "release is missing 3tched-fs required by identity provisioning"
printf '%s\n' "$BINARIES" | grep -Fxq "$RELEASE_DIR/3tched-fs" ||
    die "3tched-fs is absent from the release binary inventory"
[ -x "$RELEASE_DIR/opblob" ] || die "release is missing opblob"
[ -x "$RELEASE_DIR/system_graph_inventory" ] ||
    die "release is missing system_graph_inventory; rebuild the workspace"
[ -x "$THREETCHED_BIN" ] ||
    die "missing threetched binary; run vendor/tched-router/build-threetched.sh --cache-dir /var/cache/op-dbus-build/tched-router --output-dir target/threetched-artifacts/tched-router"
[ -f "$THREETCHED_MANIFEST" ] ||
    die "missing threetched provenance manifest: $THREETCHED_MANIFEST"
THREETCHED_SHA256=$(sha256sum "$THREETCHED_BIN" | cut -d' ' -f1)
TCHED_ROUTER_MANIFEST_SHA256=$(sha256sum "$THREETCHED_MANIFEST" | cut -d' ' -f1)
THREETCHED_BYTES=$(wc -c < "$THREETCHED_BIN" | tr -d ' ')
TCHED_ROUTER_LOCK_SHA256=$(tar -xOzf "$TCHED_ROUTER_VENDORED_SOURCE" Cargo.lock | sha256sum | cut -d' ' -f1)
jq -e \
    --arg url "$UPSTREAM_URL" \
    --arg tag "$UPSTREAM_TAG" \
    --arg commit "$UPSTREAM_COMMIT" \
    --arg source_sha "$SOURCE_ARCHIVE_SHA256" \
    --arg vendored_source_gzip_sha "$VENDORED_SOURCE_GZIP_SHA256" \
    --arg lock_sha "$TCHED_ROUTER_LOCK_SHA256" \
    --arg binary_sha "$THREETCHED_SHA256" \
    --argjson binary_bytes "$THREETCHED_BYTES" \
    '.format == "opdbus.threetched.v2" and
     .source.mode == "unmodified-upstream" and
     .source.cargo_lock_sha256 == $lock_sha and
     (has("patches") | not) and
     .upstream.url == $url and
     .upstream.tag == $tag and
     .upstream.commit == $commit and
     .upstream.source_archive_sha256 == $source_sha and
     .upstream.vendored_source_gzip_sha256 == $vendored_source_gzip_sha and
     .artifact.name == "threetched" and
     .artifact.upstream_binary == "zeroclaw" and
     .artifact.sha256 == $binary_sha and
     .artifact.bytes == $binary_bytes and
     .artifact.rust_target == "x86_64-unknown-linux-gnu"' \
    "$THREETCHED_MANIFEST" >/dev/null ||
    die "threetched provenance does not match its pinned source and binary"
[ "$("$THREETCHED_BIN" --version)" = "zeroclaw ${UPSTREAM_TAG#v}" ] ||
    die "threetched candidate reports an unexpected version"
strings "$THREETCHED_BIN" | grep -Fq 'tched_router.generation.v1' &&
    die "threetched candidate still carries the retired patched generation protocol"
[ -f "$TCHED_ROUTER_BOOTSTRAP_CONFIG" ] ||
    die "missing tched_router bootstrap configuration: $TCHED_ROUTER_BOOTSTRAP_CONFIG"
[ -d "$TCHED_ROUTER_BOOTSTRAP_SKILLS" ] ||
    die "missing tched_router bootstrap skill bundle: $TCHED_ROUTER_BOOTSTRAP_SKILLS"
[ -z "$(find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type l -print -quit)" ] ||
    die "tched_router bootstrap skill bundle must not contain symlinks"
TCHED_ROUTER_BOOTSTRAP_SKILL_COUNT=$(find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type f | wc -l)
[ "$TCHED_ROUTER_BOOTSTRAP_SKILL_COUNT" -gt 0 ] ||
    die "tched_router bootstrap skill bundle is empty"
[ -f "$AGENT_BUNDLE_SOURCE/AGENTS.md" ] ||
    die "missing shared agent bundle instructions: $AGENT_BUNDLE_SOURCE/AGENTS.md"
[ -z "$(find "$AGENT_BUNDLE_SOURCE" ! -type f ! -type d -print -quit)" ] ||
    die "shared agent bundle must contain only regular files and directories"
TCHED_ROUTER_BOOTSTRAP_CONFIG_SHA256=$(sha256sum "$TCHED_ROUTER_BOOTSTRAP_CONFIG" | cut -d' ' -f1)
TCHED_ROUTER_BOOTSTRAP_SKILLS_SHA256=$(
    find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type f -print | sort |
        while IFS= read -r skill; do
            relative=${skill#"$TCHED_ROUTER_BOOTSTRAP_SKILLS"/}
            printf '%s  %s\n' "$(sha256sum "$skill" | cut -d' ' -f1)" "$relative"
        done | sha256sum | cut -d' ' -f1
)
[ -x "$UI_ASSET_BUILD_HELPER" ] || die "missing pinned UI asset builder: $UI_ASSET_BUILD_HELPER"
[ -f "$UI_SOURCE_PIN" ] && [ -f "$UI_SOURCE_ARCHIVE" ] && [ -f "$UI_SOURCE_MANIFEST" ] ||
    die "pinned UI source archive, pin, or content manifest is missing under $UI_SOURCE_DIR"
[ -f "$UI_ASSET_ARCHIVE" ] && [ -f "$UI_ASSET_MANIFEST" ] || die "missing pinned UI asset artifact; build it with:
  $UI_ASSET_BUILD_HELPER --npm-cache /var/cache/op-dbus-build/npm --output-dir $UI_ARTIFACT_DIR"

ui_pin() {
    key=$1
    sed -n "s/^${key}=//p" "$UI_SOURCE_PIN"
}
UI_PIN_FORMAT=$(ui_pin format)
UI_PIN_HEAD=$(ui_pin source_head)
UI_PIN_WORKTREE=$(ui_pin source_worktree)
UI_PIN_ARCHIVE_SHA256=$(ui_pin source_archive_sha256)
UI_PIN_CONTENT_MANIFEST_SHA256=$(ui_pin content_manifest_sha256)
UI_PIN_ASSET_BASE=$(ui_pin asset_base)
UI_PIN_DOOR=$(ui_pin door_default)
[ "$UI_PIN_FORMAT" = opdbus.3tched-json-render-ui-source.v1 ] || die "unsupported pinned UI source format: $UI_PIN_FORMAT"
case "$UI_PIN_HEAD" in *[!0-9a-f]*|'') die "invalid pinned UI source revision" ;; esac
[ "${#UI_PIN_HEAD}" -eq 40 ] || die "pinned UI source revision must contain 40 lowercase hex characters"
case "$UI_PIN_WORKTREE" in clean|modified) ;; *) die "invalid pinned UI worktree state" ;; esac
[ "$UI_PIN_ASSET_BASE" = / ] || die "pinned UI asset base must be the dashboard root"
[ -n "$UI_PIN_DOOR" ] || die "pinned UI production door is empty"
case "$UI_PIN_ARCHIVE_SHA256" in *[!0-9a-f]*|'') die "invalid pinned UI archive digest" ;; esac
[ "${#UI_PIN_ARCHIVE_SHA256}" -eq 64 ] || die "pinned UI archive digest must contain 64 lowercase hex characters"
case "$UI_PIN_CONTENT_MANIFEST_SHA256" in *[!0-9a-f]*|'') die "invalid pinned UI content manifest digest" ;; esac
[ "${#UI_PIN_CONTENT_MANIFEST_SHA256}" -eq 64 ] || die "pinned UI content manifest digest must contain 64 lowercase hex characters"
[ "$(sha256sum "$UI_SOURCE_ARCHIVE" | cut -d' ' -f1)" = "$UI_PIN_ARCHIVE_SHA256" ] ||
    die "pinned UI source archive does not match source.pin"
[ "$(sha256sum "$UI_SOURCE_MANIFEST" | cut -d' ' -f1)" = "$UI_PIN_CONTENT_MANIFEST_SHA256" ] ||
    die "pinned UI source manifest does not match source.pin"
tar -tzf "$UI_SOURCE_ARCHIVE" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad=1 }
    !/^3tched-json-render-ui\// { bad=1 }
    END { exit bad ? 1 : 0 }
' || die "pinned UI source archive contains an unsafe or unexpected path"
tar -tzf "$UI_SOURCE_ARCHIVE" | grep -qx '3tched-json-render-ui/package-lock.json' ||
    die "pinned UI source archive is missing package-lock.json"
"$UI_ASSET_BUILD_HELPER" --verify-only || die "pinned UI source archive failed verification"

UI_SOURCE_HEAD=$(jq -er 'select(.format == "opdbus.3tched-json-render-ui-assets.v1") | .source.head | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest has an invalid format or source head"
UI_SOURCE_WORKTREE=$(jq -er '.source.worktree | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing source worktree provenance"
UI_SOURCE_ARCHIVE_SHA256=$(jq -er '.source.archive_sha256 | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing source archive provenance"
UI_SOURCE_CONTENT_MANIFEST_SHA256=$(jq -er '.source.content_manifest_sha256 | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing source content manifest provenance"
UI_SOURCE_CONTENT_SHA256=$(jq -er '.source.package_lock_sha256 | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing package-lock provenance"
UI_PIN_PACKAGE_LOCK_SHA256=$(tar -xOzf "$UI_SOURCE_ARCHIVE" 3tched-json-render-ui/package-lock.json | sha256sum | cut -d' ' -f1)
UI_ASSET_DOOR=$(jq -er '.build.door | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing the production door"
UI_ASSET_BASE=$(jq -er '.build.base | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing the UI asset base"
UI_ASSET_SHA256=$(jq -er '.artifact.sha256 | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing the artifact digest"
UI_ASSET_BYTES=$(jq -er '.artifact.bytes | numbers' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing the artifact size"
UI_ASSET_NAME=$(jq -er '.artifact.name | strings' "$UI_ASSET_MANIFEST") ||
    die "UI asset manifest is missing the artifact name"
[ "$UI_SOURCE_HEAD" = "$UI_PIN_HEAD" ] && [ "$UI_SOURCE_WORKTREE" = "$UI_PIN_WORKTREE" ] ||
    die "UI asset manifest source revision does not match source.pin"
[ "$UI_SOURCE_ARCHIVE_SHA256" = "$UI_PIN_ARCHIVE_SHA256" ] ||
    die "UI asset manifest source archive digest does not match source.pin"
[ "$UI_SOURCE_CONTENT_MANIFEST_SHA256" = "$UI_PIN_CONTENT_MANIFEST_SHA256" ] ||
    die "UI asset manifest content manifest digest does not match source.pin"
[ "$UI_SOURCE_CONTENT_SHA256" = "$UI_PIN_PACKAGE_LOCK_SHA256" ] ||
    die "UI asset manifest package-lock digest does not match the pinned source archive"
[ "$UI_ASSET_BASE" = "$UI_PIN_ASSET_BASE" ] && [ "$UI_ASSET_BASE" = / ] ||
    die "UI asset manifest base does not match the pinned dashboard root"
[ "$UI_ASSET_DOOR" = "$UI_PIN_DOOR" ] || die "UI asset manifest door does not match source.pin"
[ "$UI_ASSET_NAME" = assets.tar.gz ] || die "UI asset manifest names an unexpected artifact"
case "$UI_ASSET_SHA256" in *[!0-9a-f]*|'') die "invalid UI asset digest in manifest" ;; esac
[ "${#UI_ASSET_SHA256}" -eq 64 ] || die "UI asset digest must contain 64 lowercase hex characters"
[ "$(sha256sum "$UI_ASSET_ARCHIVE" | cut -d' ' -f1)" = "$UI_ASSET_SHA256" ] ||
    die "UI asset archive does not match its provenance manifest"
[ "$(wc -c < "$UI_ASSET_ARCHIVE" | tr -d ' ')" = "$UI_ASSET_BYTES" ] ||
    die "UI asset archive size does not match its provenance manifest"
UI_ASSET_MANIFEST_SHA256=$(sha256sum "$UI_ASSET_MANIFEST" | cut -d' ' -f1)
tar -tzf "$UI_ASSET_ARCHIVE" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad=1 }
    END { exit bad ? 1 : 0 }
' || die "UI asset archive contains an unsafe path"
tar -tvzf "$UI_ASSET_ARCHIVE" | awk 'substr($1,1,1) !~ /[-d]/ {bad=1} END {exit bad ? 1 : 0}' ||
    die "UI asset archive must contain only files and directories"
tar -tzf "$UI_ASSET_ARCHIVE" | grep -qx './index.html' ||
    die "UI asset archive is missing index.html"
[ -f "$PERSISTED_CATALOG_DIR/.manifest.json" ] ||
    die "missing persisted sealed catalog; run the freshly built opblob seal-shm and persist preparation steps first"
jq -e '
    (.generation | type == "number" and floor == . and . >= 1) and
    (.catalog_hash | type == "string" and test("^[0-9a-f]{64}$")) and
    (.plugins | type == "object" and length > 0) and
    all(.plugins | to_entries[];
        (.key | test("^[a-z0-9_-]+$")) and
        (.value | type == "string" and test("^[0-9a-f]{64}$")))
' "$PERSISTED_CATALOG_DIR/.manifest.json" >/dev/null ||
    die "persisted sealed catalog manifest is invalid"
jq -e '.plugins.threetched | type == "string"' "$PERSISTED_CATALOG_DIR/.manifest.json" >/dev/null ||
    die "persisted sealed catalog is missing threetched; seal its selected capture inputs with opblob first"
CATALOG_GENERATION=$(jq -r '.generation' "$PERSISTED_CATALOG_DIR/.manifest.json")
CATALOG_HASH=$(jq -r '.catalog_hash' "$PERSISTED_CATALOG_DIR/.manifest.json")
CATALOG_MANIFEST_SHA256=$(sha256sum "$PERSISTED_CATALOG_DIR/.manifest.json" | cut -d' ' -f1)
CATALOG_PLUGIN_COUNT=$(jq -r '.plugins | length' "$PERSISTED_CATALOG_DIR/.manifest.json")
ACTUAL_CATALOG_BLOB_COUNT=$(find "$PERSISTED_CATALOG_DIR" -maxdepth 1 -type f -name '*.blob' | wc -l)
[ "$ACTUAL_CATALOG_BLOB_COUNT" -eq "$CATALOG_PLUGIN_COUNT" ] ||
    die "persisted sealed catalog blob count does not match its manifest"
jq -r '.plugins | to_entries[] | [.key, .value] | @tsv' \
    "$PERSISTED_CATALOG_DIR/.manifest.json" |
    while IFS="$(printf '\t')" read -r plugin_id schema_hash; do
        blob="$PERSISTED_CATALOG_DIR/$plugin_id.$(printf '%s' "$schema_hash" | cut -c1-16).blob"
        [ -f "$blob" ] || die "persisted sealed catalog is missing $blob"
        inspected=$("$RELEASE_DIR/opblob" inspect "$blob") ||
            die "persisted sealed catalog contains an invalid blob: $blob"
        [ "$(printf '%s\n' "$inspected" | awk '$1 == "plugin" { print $2; exit }')" = "$plugin_id" ] ||
            die "persisted sealed catalog blob plugin id does not match its manifest: $blob"
        [ "$(printf '%s\n' "$inspected" | awk '$1 == "schema" && $2 == "sha256" { print $3; exit }')" = "$schema_hash" ] ||
            die "persisted sealed catalog blob schema hash does not match its manifest: $blob"
        if [ "$plugin_id" = threetched ]; then
            printf '%s\n' "$inspected" | awk '$1 == "method" {found=1} END {exit found ? 0 : 1}' ||
                die "sealed threetched blob has no method inventory"
            printf '%s\n' "$inspected" | awk '$1 == "descriptor" && /services=\["/ {found=1} END {exit found ? 0 : 1}' ||
                die "sealed threetched blob has no method service descriptor"
        fi
    done
# Use the existing schema projection to inspect the sealed plugin example.
# This only reads the supplied catalog into a disposable declaration-only view;
# opblob remains the sole sealer and the backing binary is never recaptured here.
(
    threetched_validation=$(mktemp -d "${TMPDIR:-/tmp}/threetched-release-catalog.XXXXXX") || exit 1
    trap 'rm -rf -- "$threetched_validation"' EXIT HUP INT TERM
    mkdir "$threetched_validation/state" || exit 1
    "$RELEASE_DIR/3tched-fs" --store "$threetched_validation/store" capture --schema-only \
        --catalog "$PERSISTED_CATALOG_DIR" --state "$threetched_validation/state" >/dev/null || exit 1
    "$RELEASE_DIR/3tched-fs" --store "$threetched_validation/store" inspect --plugin threetched |
        jq -e --arg binary_sha "$THREETCHED_SHA256" --arg manifest_sha "$TCHED_ROUTER_MANIFEST_SHA256" '
            .plugin_id == "threetched" and .schema.name == "threetched" and
            (.schema.methods | type == "object" and length > 0) and
            .schema.example.binary == "/usr/local/libexec/op-dbus/threetched/threetched" and
            .schema.example.manifest == "/usr/share/op-dbus/threetched-manifests/tched-router.json" and
            .schema.example.binary_sha256 == $binary_sha and
            .schema.example.manifest_sha256 == $manifest_sha
        ' >/dev/null
) || die "sealed threetched capture provenance does not match the prepared release binary and manifest"
[ -f "$EMQX_ARTIFACT_PATH" ] || die "missing pinned EMQX artifact: $EMQX_ARTIFACT_PATH"
[ "$(sha256sum "$EMQX_ARTIFACT_PATH" | cut -d' ' -f1)" = "$EMQX_SHA256" ] ||
    die "EMQX artifact digest does not match deploy/config/emqx.version"
tar -tzf "$EMQX_ARTIFACT_PATH" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad = 1 }
    END { exit bad ? 1 : 0 }
' || die "EMQX artifact contains an unsafe path"
for required in bin/emqx etc/emqx.conf etc/base.hocon; do
    tar -tzf "$EMQX_ARTIFACT_PATH" | grep -qx "$required" ||
        die "EMQX artifact is missing $required"
done
for required_config in emqx.conf base.hocon acl.conf; do
    [ -f "$EMQX_CONFIG_DIR/$required_config" ] ||
        die "missing tracked EMQX config: $EMQX_CONFIG_DIR/$required_config"
done
[ -f "$NOTEBOOKLM_MCP_ARTIFACT_PATH" ] ||
    die "missing pinned jacob-bd nlm wheel: $NOTEBOOKLM_MCP_ARTIFACT_PATH"
[ "$(sha256sum "$NOTEBOOKLM_MCP_ARTIFACT_PATH" | cut -d' ' -f1)" = "$NOTEBOOKLM_MCP_SHA256" ] ||
    die "nlm wheel digest does not match deploy/config/notebooklm-mcp.version"
python3 - "$NOTEBOOKLM_MCP_ARTIFACT_PATH" <<'PY' ||
import sys, zipfile
wheel = zipfile.ZipFile(sys.argv[1])
names = wheel.namelist()
if any(n.startswith("/") or "/../" in n or n.startswith("../") for n in names):
    raise SystemExit("unsafe path in nlm wheel")
if not any(n.endswith("entry_points.txt") for n in names):
    raise SystemExit("nlm wheel is missing entry_points.txt")
text = wheel.read(next(n for n in names if n.endswith("entry_points.txt"))).decode()
if "nlm =" not in text and "nlm=" not in text:
    raise SystemExit("nlm wheel does not declare the nlm console script")
PY
    die "pinned nlm wheel failed integrity checks"

[ -x "$RELEASE_DIR/op-grants-materializer" ] ||
    die "release is missing op-grants-materializer"
"$RELEASE_DIR/op-grants-materializer" validate-audience "$MCP_AUDIENCE_POLICY_SOURCE" >/dev/null ||
    die "MCP audience policy is invalid"
[ -r "$HOST_RUNTIME_PACKAGES_SOURCE" ] ||
    die "missing host runtime package manifest: $HOST_RUNTIME_PACKAGES_SOURCE"
HOST_RUNTIME_PACKAGES=$(grep -Ev '^[[:space:]]*(#|$)' "$HOST_RUNTIME_PACKAGES_SOURCE")
[ -n "$HOST_RUNTIME_PACKAGES" ] || die "host runtime package manifest is empty"
[ -r "$EXECUTION_PYTHON_LOCK" ] || die "missing execution Python requirements lock"
[ -x "$EXECUTION_PYTHON_PREPARE" ] || die "execution Python preparation helper is not executable"
EXECUTION_PYTHON_LOCK_SHA256=$(sha256sum "$EXECUTION_PYTHON_LOCK" | cut -d' ' -f1)
HOST_RUNTIME_PACKAGE_CHECK="$SCRIPT_DIR/check-host-runtime-packages.sh"
[ -x "$HOST_RUNTIME_PACKAGE_CHECK" ] || die "missing host runtime package checker: $HOST_RUNTIME_PACKAGE_CHECK"
if ! missing_host_packages=$("$HOST_RUNTIME_PACKAGE_CHECK" "$HOST_RUNTIME_PACKAGES_SOURCE"); then
    die "host runtime package verification failed: $(printf '%s' "$missing_host_packages" | tr '\n' ' ')"
fi
BIN_COUNT=$(printf '%s\n' "$BINARIES" | wc -l)
log "$BIN_COUNT release binaries in $RELEASE_DIR"

if [ "$DRY_RUN" != 1 ] && [ "$(id -u)" != 0 ]; then
    die "must run as root to prepare the golden btrfs subvolume"
fi

COMMIT=$(git -C "$PROJECT_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)
STAMP=$(date -u '+%Y%m%dT%H%M%SZ')

# ═══ Path 1: golden subvolume ═══════════════════════════════════════════════
build_golden() {
    log "building golden subvolume at $GOLDEN_DIR"

    [ -d "$GOLDEN_DIR" ] && [ ! -L "$GOLDEN_DIR" ] ||
        die "golden must already exist; use op-release provision-slots"
    [ "$(stat -f -c %T "$GOLDEN_DIR")" = btrfs ] &&
        [ "$(stat -c %i "$GOLDEN_DIR")" = 256 ] ||
        die "golden is not a Btrfs subvolume; use op-release provision-slots"
    [ "$(stat -c %u:%g "$GOLDEN_DIR")" = 0:0 ] ||
        die "golden must be root-owned"
    case "$(stat -c %a "$GOLDEN_DIR")" in 700|750|755) ;; *) die "unsafe golden root mode" ;; esac
    btrfs subvolume show "$GOLDEN_DIR" >/dev/null ||
        die "cannot inspect the provisioned golden subvolume"
    [ "$(btrfs property get -ts "$GOLDEN_DIR" ro)" = ro=false ] ||
        die "golden must be writable preparation state"
    # Refuse nested filesystem ownership before any subtractive cutover.
    [ -z "$(find "$GOLDEN_DIR" -mindepth 1 -type d -inum 256 -print -quit)" ] ||
        die "golden contains a nested Btrfs subvolume"
    if findmnt -rn -o TARGET | awk -v root="$GOLDEN_DIR/" \
        'index($0, root) == 1 { found=1 } END { exit found ? 0 : 1 }'; then
        die "golden contains a nested mount"
    fi

    ensure_directory "$GOLDEN_DIR/bin"
    ensure_directory "$GOLDEN_DIR/sv"
    ensure_directory "$GOLDEN_DIR/usr/share/op-dbus"

    # These were preparation-owned paths in the old root-shaped golden.
    # Never remove their live host counterparts or any unrecognized base files.
    for legacy in sbin/systemd-unit-to-runit sbin/op-convert-systemd-units \
        sbin/systemctl-shim sbin/ban-caddy-nginx sbin/build-golden.sh sbin/agent-runit-guard.sh \
        libexec/op-warp-ssh share/op-dbus/warp-ssh \
        share/op-dbus/notebook-sync var/lib/opdbus/plugin-blobs \
        etc/environment.default etc/pacman-hooks/99-systemd-unit-to-runit.hook \
        etc/pacman-hooks/99-ban-caddy-nginx.hook etc/pacman-hooks/99-agent-runit-guard.hook \
        etc/opdbus/capability-grants.json etc/opdbus/mcp-audience-policy.json \
        etc/opdbus/mcp-toolsets.json etc/opdbus/host-runtime-packages.txt \
        etc/opdbus/enabled-services etc/opdbus/retired-services etc/opdbus/retired-binaries \
        etc/op-dbus/threetched-runtime.toml etc/op-dbus/model-providers.toml \
        etc/op-dbus/threetched-skills etc/op-dbus/opencode-runtime.json \
        etc/dbus-1/system.d/org.dbusmcp.conf etc/pam.d/op-dbus-elevate \
        etc/nftables.conf etc/iptables/iptables.rules etc/op-dbus/network.conf \
        etc/op-dbus/openflow-static-flows.json etc/ssh/sshd_config \
        etc/ssh/sshd_config.d/10-loopback-only.conf \
        etc/ssh/sshd_config.d/15-ghostbridge-vip.conf \
        etc/ssh/sshd_config.d/90-singleuser-password.conf etc/emqx \
        opt/op-mcp-providers opt/op-dbus/container-artifacts/mail-3tched-x86_64-linux-gnu-bundle.tar \
        usr/share/op-dbus/etc/opdbus/mcp-toolsets.json \
        usr/share/op-dbus/etc/op-dbus/model-providers.toml \
        usr/share/op-dbus/etc/rust-network-mgr \
        usr/bin/opencode; do
        if [ -e "$GOLDEN_DIR/$legacy" ] || [ -L "$GOLDEN_DIR/$legacy" ]; then
            remove_owned "$GOLDEN_DIR/$legacy"
        fi
    done
    for legacy_root in etc var share sbin; do
        [ ! -d "$GOLDEN_DIR/$legacy_root" ] || {
            [ ! -L "$GOLDEN_DIR/$legacy_root" ] || die "symlink legacy root"
            if [ "$DRY_RUN" != 1 ]; then
                find "$GOLDEN_DIR/$legacy_root" -depth -type d -empty -delete
                [ ! -e "$GOLDEN_DIR/$legacy_root" ] ||
                    die "unrecognized legacy content remains; preserved $GOLDEN_DIR/$legacy_root"
            fi
        }
    done
    # All binary entries here belonged to the prior release builder.
    find "$GOLDEN_DIR/bin" -mindepth 1 -maxdepth 1 -print | while IFS= read -r old; do
        [ "$(basename "$old")" = nlm ] && continue
        printf '%s\n' "$BINARIES" | grep -Fxq "$RELEASE_DIR/$(basename "$old")" ||
            run rm -- "$old"
    done
    printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
        install_file 0755 "$bin" "$GOLDEN_DIR/bin/$(basename "$bin")"
    done
    install_file 0755 "$THREETCHED_BIN" "$GOLDEN_DIR/usr/bin/threetched"
    install_file 0644 "$THREETCHED_MANIFEST" \
        "$GOLDEN_DIR/usr/share/op-dbus/threetched-manifests/tched-router.json"

    ui_destination="$GOLDEN_DIR/usr/local/share/op-dbus/dashboard"
    ui_manifest="$GOLDEN_DIR/usr/share/op-dbus/ui-manifests/3tched-ui.json"
    if [ ! -f "$ui_manifest" ] || ! cmp -s "$UI_ASSET_MANIFEST" "$ui_manifest" ||
       [ ! -f "$ui_destination/index.html" ]; then
        if [ "$DRY_RUN" = 1 ]; then
            log "would synchronize changed UI archive"
        else
            (
                ui_stage=$(mktemp -d)
                trap 'rm -rf -- "$ui_stage"' EXIT HUP INT TERM
                tar -xzf "$UI_ASSET_ARCHIVE" --no-same-owner -C "$ui_stage"
                sync_tree "$ui_stage" "$ui_destination"
            )
        fi
    fi
    install_file 0644 "$UI_ASSET_MANIFEST" "$ui_manifest"

    golden_catalog="$GOLDEN_DIR/usr/share/op-dbus/plugin-blobs"
    ensure_directory "$golden_catalog"
    find "$golden_catalog" -maxdepth 1 -type f -name '*.blob' -print |
        while IFS= read -r old; do
            [ -f "$PERSISTED_CATALOG_DIR/$(basename "$old")" ] || run rm -- "$old"
        done
    jq -r '.plugins | to_entries[] | [.key, .value] | @tsv' "$PERSISTED_CATALOG_DIR/.manifest.json" |
        while IFS="$(printf '\t')" read -r plugin_id schema_hash; do
            blob="$plugin_id.$(printf '%s' "$schema_hash" | cut -c1-16).blob"
            install_file 0644 "$PERSISTED_CATALOG_DIR/$blob" "$golden_catalog/$blob"
        done
    install_file 0644 "$PERSISTED_CATALOG_DIR/.manifest.json" "$golden_catalog/.manifest.json"

    # One release-owned helper target. Only the explicit provisioning closure
    # above joins the existing application helpers; base service definitions,
    # host-installed helper copies, receipts and storage remain untouched.
    ensure_directory "$GOLDEN_DIR/libexec/3tched"
    for old in "$GOLDEN_DIR/libexec/3tched"/*; do
        [ -e "$old" ] || [ -L "$old" ] || continue
        if printf '%s\n' "$RELEASE_HELPER_INPUTS" | cut -d ' ' -f3 | grep -Fxq "$(basename "$old")"; then
            continue
        fi
        case "$(basename "$old")" in
            xray-config-mount-up|mcp-provider-probe|op-identity-lungs-hook|op-lungs-check) ;;
            *) die "unrecognized helper preserved: $old" ;;
        esac
        run rm -- "$old"
    done
    printf '%s\n' "$RELEASE_HELPER_INPUTS" | while read -r mode source helper; do
        install_file "$mode" "$SCRIPT_DIR/../$source" "$GOLDEN_DIR/libexec/3tched/$helper"
    done
    for helper in notebook-sync notebook-sources-sync-up notebook-sources-upload \
                  export-llm-sessions-to-notebooklm-sources.py uv-tools/notebook_sync.py; do
        install_file 0755 "$PROJECT_ROOT/scripts/$helper" \
            "$GOLDEN_DIR/usr/share/op-dbus/notebook-sync/scripts/$helper"
    done

    # Preserve unchanged content-addressed installations. Only the writable
    # data/log paths escape the read-only broker target; config is immutable.
    golden_emqx="$GOLDEN_DIR/opt/$EMQX_INSTALL_NAME"
    ensure_directory "$GOLDEN_DIR/opt"
    [ ! -L "$golden_emqx" ] || die "symlink broker installation in golden"
    [ ! -d "$golden_emqx" ] || ensure_directory "$golden_emqx"
    golden_emqx_marker="$golden_emqx/.opdbus-artifact-sha256"
    if [ ! -f "$golden_emqx_marker" ]; then
        [ ! -e "$golden_emqx" ] && [ ! -L "$golden_emqx" ] ||
            die "unmarked EMQX directory: $golden_emqx"
        if [ "$DRY_RUN" = 1 ]; then
            log "would extract pinned EMQX $EMQX_INSTALL_NAME"
        else
            ensure_directory "$golden_emqx"
            tar -xzf "$EMQX_ARTIFACT_PATH" --no-same-owner -C "$golden_emqx"
            [ -d "$golden_emqx/etc" ] && [ ! -L "$golden_emqx/etc" ] ||
                die "EMQX artifact lacks a regular static config directory"
            mv -- "$golden_emqx/etc" "$golden_emqx/etc.dist"
            printf '%s\n' "$EMQX_SHA256" > "$golden_emqx_marker"
            chmod 0644 "$golden_emqx_marker"
        fi
    else
        [ "$(cat "$golden_emqx_marker")" = "$EMQX_SHA256" ] ||
            die "EMQX content-address marker mismatch"
    fi
    # esockd ships a public demonstration private key, not runtime credentials.
    # Never publish it in the immutable broker target. Real TLS keys remain
    # under the external /etc/emqx/certs boundary declared by emqx.conf.
    for demo_key in "$golden_emqx"/lib/esockd-*/priv/demo.key; do
        [ -e "$demo_key" ] || [ -L "$demo_key" ] || continue
        [ -f "$demo_key" ] && [ ! -L "$demo_key" ] ||
            die "unexpected esockd demonstration key type"
        run rm -- "$demo_key"
    done
    if [ "$DRY_RUN" != 1 ]; then
        [ -d "$golden_emqx/etc.dist" ] && [ ! -L "$golden_emqx/etc.dist" ] ||
            die "EMQX static config template unavailable"
        emqx_config_marker="$GOLDEN_DIR/usr/share/op-dbus/emqx-etc/.opdbus-artifact-sha256"
        if [ ! -f "$emqx_config_marker" ] || ! cmp -s "$golden_emqx_marker" "$emqx_config_marker"; then
            sync_tree "$golden_emqx/etc.dist" "$GOLDEN_DIR/usr/share/op-dbus/emqx-etc"
            install_file 0644 "$golden_emqx_marker" "$emqx_config_marker"
        fi
    fi
    for config_name in emqx.conf base.hocon acl.conf; do
        install_file 0644 "$EMQX_CONFIG_DIR/$config_name" \
            "$GOLDEN_DIR/usr/share/op-dbus/emqx-etc/$config_name"
    done
    for mapping in etc:/usr/share/op-dbus/emqx-etc data:/var/lib/emqx log:/var/log/emqx; do
        name=${mapping%%:*} target=${mapping#*:}
        if [ -L "$golden_emqx/$name" ]; then
            [ "$(readlink "$golden_emqx/$name")" = "$target" ] ||
                run rm -- "$golden_emqx/$name"
        elif [ -e "$golden_emqx/$name" ]; then
            # These are vendor archive defaults, never host data.
            run rm -r -- "$golden_emqx/$name"
        fi
        [ -L "$golden_emqx/$name" ] || run ln -s "$target" "$golden_emqx/$name"
    done
    # The target is the actual pinned installation, not this old selector.
    if [ -L "$GOLDEN_DIR/opt/emqx" ]; then
        run rm -- "$GOLDEN_DIR/opt/emqx"
    elif [ -e "$GOLDEN_DIR/opt/emqx" ]; then
        die "unexpected non-symlink legacy EMQX selector"
    fi
    for old in "$GOLDEN_DIR"/opt/emqx-*; do
        [ -e "$old" ] || continue
        [ "$old" = "$golden_emqx" ] && continue
        [ -f "$old/.opdbus-artifact-sha256" ] || die "unowned old EMQX tree: $old"
        run rm -r -- "$old"
    done
    install_nlm_cli "$GOLDEN_DIR" "$GOLDEN_DIR/bin/nlm"
    for old in "$GOLDEN_DIR"/opt/opdbus/nlm-cli-*; do
        [ -e "$old" ] || continue
        [ "$old" = "$GOLDEN_DIR/opt/opdbus/$NOTEBOOKLM_MCP_INSTALL_NAME" ] && continue
        [ -f "$old/.opdbus-artifact-sha256" ] || die "unowned old provider tree: $old"
        run rm -r -- "$old"
    done
    ensure_directory "$GOLDEN_DIR/opt/op-dbus"
    [ ! -L "$GOLDEN_DIR/opt/op-dbus/execution-python" ] ||
        die "symlink execution environment in golden"
    run env OP_EXECUTION_PYTHON_ROOT="$GOLDEN_DIR/opt/op-dbus/execution-python" \
        OP_EXECUTION_PYTHON_REPO="$PROJECT_ROOT" OP_EXECUTION_PYTHON_LOCK="$EXECUTION_PYTHON_LOCK" \
        OP_EXECUTION_PYTHON_CACHE="${OP_EXECUTION_PYTHON_CACHE:-/var/cache/op-dbus/execution-python}" \
        OP_EXECUTION_PYTHON_BUNDLE="${OP_EXECUTION_PYTHON_BUNDLE:-/home/jeremy/.local/share/uv/python/cpython-3.14.7-linux-x86_64-gnu}" \
        "$EXECUTION_PYTHON_PREPARE"

    # One service inventory. Full declarations remain visible, but only marked
    # enabled application definitions enter this snapshot/restart domain.
    for old in "$GOLDEN_DIR/sv"/*; do
        [ -e "$old" ] || [ -L "$old" ] || continue
        svc=$(basename "$old")
        is_application_service "$svc" && continue
        [ -f "$SCRIPT_DIR/$svc/run" ] || is_retired_service "$svc" ||
            die "unrecognized service definition preserved: $old"
        remove_owned "$old"
    done
    grep -Ev '^[[:space:]]*(#|$)' "$SCRIPT_DIR/enabled-services" |
        while IFS= read -r svc; do
            is_application_service "$svc" || continue
            for component in run finish check log/run; do
                if [ -f "$SCRIPT_DIR/$svc/$component" ]; then
                    install_file 0755 "$SCRIPT_DIR/$svc/$component" "$GOLDEN_DIR/sv/$svc/$component"
                elif [ -f "$GOLDEN_DIR/sv/$svc/$component" ]; then
                    run rm -- "$GOLDEN_DIR/sv/$svc/$component"
                fi
            done
        done
    install_file 0644 "$SCRIPT_DIR/../environment.default" "$GOLDEN_DIR/usr/share/op-dbus/etc/environment.default"
    install_file 0600 "$MCP_AUDIENCE_POLICY_SOURCE" "$GOLDEN_DIR/usr/share/op-dbus/etc/opdbus/mcp-audience-policy.json"
    install_file 0644 "$HOST_RUNTIME_PACKAGES_SOURCE" "$GOLDEN_DIR/usr/share/op-dbus/etc/opdbus/host-runtime-packages.txt"
    for declaration in enabled-services retired-services retired-binaries; do
        install_file 0644 "$SCRIPT_DIR/$declaration" "$GOLDEN_DIR/usr/share/op-dbus/etc/opdbus/$declaration"
    done
    install_file 0644 "$TCHED_ROUTER_BOOTSTRAP_CONFIG" "$GOLDEN_DIR/usr/share/op-dbus/etc/op-dbus/threetched-runtime.toml"
    sync_tree "$TCHED_ROUTER_BOOTSTRAP_SKILLS" "$GOLDEN_DIR/usr/share/op-dbus/etc/op-dbus/threetched-skills/default"
    sync_tree "$AGENT_BUNDLE_SOURCE" "$GOLDEN_DIR/usr/share/op-dbus/agents"

    # MANIFEST: staged release identity and hashes for verification after the
    # read-only snapshot is received. This script does not inspect a live host.
    if [ "$DRY_RUN" != 1 ]; then
        {
            printf 'golden-build: %s\n' "$STAMP"
            printf 'commit: %s\n' "$COMMIT"
            printf 'source: %s\n' "$PROJECT_ROOT"
            printf 'init: runit (sv)\n'
            printf 'binaries: %s\n' "$BIN_COUNT"
            printf 'threetched-sha256: %s\n' "$THREETCHED_SHA256"
            printf 'tched-router-manifest-sha256: %s\n' "$TCHED_ROUTER_MANIFEST_SHA256"
            printf 'tched-router-bootstrap-config-sha256: %s\n' "$TCHED_ROUTER_BOOTSTRAP_CONFIG_SHA256"
            printf 'tched-router-bootstrap-skills-sha256: %s\n' "$TCHED_ROUTER_BOOTSTRAP_SKILLS_SHA256"
            printf 'ui-source-head: %s\n' "$UI_SOURCE_HEAD"
            printf 'ui-source-worktree: %s\n' "$UI_SOURCE_WORKTREE"
            printf 'ui-source-archive-sha256: %s\n' "$UI_SOURCE_ARCHIVE_SHA256"
            printf 'ui-source-content-sha256: %s\n' "$UI_SOURCE_CONTENT_SHA256"
            printf 'ui-assets-sha256: %s\n' "$UI_ASSET_SHA256"
            printf 'ui-assets-manifest-sha256: %s\n' "$UI_ASSET_MANIFEST_SHA256"
            printf 'plugin-catalog-generation: %s\n' "$CATALOG_GENERATION"
            printf 'plugin-catalog-hash: %s\n' "$CATALOG_HASH"
            printf 'plugin-catalog-manifest-sha256: %s\n' "$CATALOG_MANIFEST_SHA256"
            printf 'emqx-artifact: %s\n' "$EMQX_ARTIFACT"
            printf 'emqx-sha256: %s\n' "$EMQX_SHA256"
            printf 'nlm-cli-artifact: %s\n' "$NOTEBOOKLM_MCP_ARTIFACT"
            printf 'nlm-cli-sha256: %s\n' "$NOTEBOOKLM_MCP_SHA256"
            printf 'execution-python-lock-sha256: %s\n' "$EXECUTION_PYTHON_LOCK_SHA256"
            printf '\n[sha256]\n'
            printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
                printf '%s  %s\n' "$(sha256sum "$bin" | cut -d' ' -f1)" "$(basename "$bin")"
            done
        } > "$GOLDEN_DIR/MANIFEST"
    fi
    ok "wrote golden/MANIFEST (commit $COMMIT)"
    if [ "$PREPARE_SYSTEM" = 1 ]; then
        # shellcheck source=system-payload.sh
        . "$(dirname -- "$SCRIPT_PATH")/system-payload.sh"
        stage_system_payload
        ok "staged explicit system payload"
    fi
    # Derive inventory from the finished prepared tree. The graph directory and
    # MANIFEST digest are excluded by the collector to prevent self-reference.
    # This is preparation only: importing into live Cozo is an authenticated
    # plugin mutation after activation, never a direct database write here.
    run install -d -m 0755 "$GOLDEN_DIR/usr/share/op-dbus/graphs"
    run "$RELEASE_DIR/system_graph_inventory" \
        --prepared "$GOLDEN_DIR" \
        --output "$GOLDEN_DIR/usr/share/op-dbus/graphs/release.json" \
        --namespace "release:$COMMIT-$STAMP" \
        --generated-at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    if [ "$DRY_RUN" != 1 ]; then
        printf '\nsystem-graph-path: usr/share/op-dbus/graphs/release.json\nsystem-graph-sha256: %s\n' \
            "$(sha256sum "$GOLDEN_DIR/usr/share/op-dbus/graphs/release.json" | cut -d' ' -f1)" \
            >> "$GOLDEN_DIR/MANIFEST"
    fi
    ok "validated prepared-release system graph"
    log "golden is ready; the read-only snapshot and btrfs send happen at deploy time"
}

# ═══ Run ════════════════════════════════════════════════════════════════════
build_golden

log "commit $COMMIT, build $STAMP"
log "prepared subvolume: $GOLDEN_DIR"
log "next deployment boundary: create a read-only snapshot, then transfer it with btrfs send/receive"
ok "golden preparation complete"
