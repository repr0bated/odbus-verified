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
#     sbin/                      control scripts + systemd compat layer
#     usr/bin/zeroclaw           pinned tched_router native dependency
#     usr/share/op-dbus/         native and UI provenance manifests
#     usr/local/share/op-dbus/dashboard/  pinned production UI assets at root
#     sv/<service>/run           runit service definitions
#     etc/                       environment defaults, pacman hooks
#     var/lib/opdbus/plugin-blobs/  persisted sealed plugin catalog
#     MANIFEST                   commit, build time, sha256 per binary
#
# Usage:
#   build-golden.sh                 # prepare and validate golden (default)
#   build-golden.sh --golden-only   # compatibility alias for the default
#   build-golden.sh --dry-run
#
# Retired options `--live-only` and `--no-restart` fail explicitly. Requires
# root to populate the btrfs subvolume. Never builds: run
# `CXXFLAGS="-include cstdint" cargo build --workspace --release` first (the
# vendored RocksDB in cozorocks needs that flag on modern GCC).
# `OP_PERSISTED_CATALOG_SOURCE` may name an isolated prepared catalog; it
# defaults to `/var/lib/opdbus/plugin-blobs` and is never modified here.
set -eu

OPDBUS_ROOT=${OPDBUS_ROOT:-/opt/op-dbus}
GOLDEN_DIR="$OPDBUS_ROOT/golden"

DRY_RUN=0

SCRIPT_PATH=$(readlink -f "$0" 2>/dev/null || printf '%s' "$0")
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd)
if [ -f "$SCRIPT_DIR/../../Cargo.toml" ]; then
    PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
else
    PROJECT_ROOT=${OP_DBUS_ROOT:-$(pwd)}
fi
RELEASE_DIR="$PROJECT_ROOT/target/release"
MAIL_ARTIFACT_NAME=mail-3tched-x86_64-linux-gnu-bundle.tar
MAIL_ARTIFACT_PATH="$PROJECT_ROOT/target/container-artifacts/$MAIL_ARTIFACT_NAME"
GRANTS_SOURCE="$SCRIPT_DIR/../security/capability-grants.json"
MCP_AUDIENCE_POLICY_SOURCE="$SCRIPT_DIR/../config/mcp-audience-policy.json"
MCP_TOOLSETS_SOURCE="$SCRIPT_DIR/../config/mcp-toolsets.json"
HOST_RUNTIME_PACKAGES_SOURCE="$SCRIPT_DIR/../config/host-runtime-packages.txt"
PERSISTED_CATALOG_DIR=${OP_PERSISTED_CATALOG_SOURCE:-/var/lib/opdbus/plugin-blobs}
RETIRED_SERVICES_FILE="$SCRIPT_DIR/retired-services"
RETIRED_BINARIES_FILE="$SCRIPT_DIR/retired-binaries"
EMQX_VERSION_FILE="$SCRIPT_DIR/../config/emqx.version"
EMQX_CONFIG_DIR="$SCRIPT_DIR/../emqx"
EMQX_ARTIFACT_CACHE_DIR=${EMQX_ARTIFACT_CACHE_DIR:-/var/cache/op-dbus/source-artifacts}
MCP_PROVIDER_ARTIFACT_CACHE_DIR=${MCP_PROVIDER_ARTIFACT_CACHE_DIR:-/var/cache/op-dbus/source-artifacts}
NOTEBOOKLM_MCP_VERSION_FILE="$SCRIPT_DIR/../config/notebooklm-mcp.version"
MONGODB_MCP_VERSION_FILE="$SCRIPT_DIR/../config/mongodb-mcp-server.version"
EXECUTION_PYTHON_LOCK="$SCRIPT_DIR/../config/execution-python-requirements.lock"
EXECUTION_PYTHON_PREPARE="$SCRIPT_DIR/prepare-execution-python.sh"
NLM_CLI_ROOT=/opt/opdbus
TCHED_ROUTER_SOURCE_PIN="$PROJECT_ROOT/vendor/tched-router/source.pin"
TCHED_ROUTER_VENDORED_SOURCE="$PROJECT_ROOT/vendor/tched-router/source.tar.gz"
TCHED_ROUTER_ARTIFACT_DIR="$PROJECT_ROOT/target/native-artifacts/tched-router"
TCHED_ROUTER_NATIVE_BIN="$TCHED_ROUTER_ARTIFACT_DIR/zeroclaw"
TCHED_ROUTER_NATIVE_MANIFEST="$TCHED_ROUTER_ARTIFACT_DIR/manifest.json"
TCHED_ROUTER_LAG_PATCH="$PROJECT_ROOT/vendor/tched-router/patches/0001-close-native-event-stream-on-lag.patch"
TCHED_ROUTER_GENERATION_PATCH="$PROJECT_ROOT/vendor/tched-router/patches/0002-stateless-provider-generation.patch"
TCHED_ROUTER_PATCH_INVENTORY="$PROJECT_ROOT/vendor/tched-router/patch-inventory"
TCHED_ROUTER_BASE_MANIFEST="$PROJECT_ROOT/vendor/tched-router/base-sha256sums"
TCHED_ROUTER_BOOTSTRAP_CONFIG="$SCRIPT_DIR/../config/zeroclaw-runtime.toml"
TCHED_ROUTER_BOOTSTRAP_SKILLS="$SCRIPT_DIR/../config/zeroclaw-skills/default"
OPENCODE_RUNTIME_CONFIG="$SCRIPT_DIR/../config/opencode-runtime.json"
OPENCODE_BINARY_SOURCE=${OPENCODE_BINARY_SOURCE:-/usr/bin/opencode}
UI_SOURCE_DIR="$PROJECT_ROOT/vendor/vercel-json-render-ui"
UI_SOURCE_PIN="$UI_SOURCE_DIR/source.pin"
UI_SOURCE_ARCHIVE="$UI_SOURCE_DIR/source.tar.gz"
UI_SOURCE_MANIFEST="$UI_SOURCE_DIR/source-sha256sums"
UI_ARTIFACT_DIR="$PROJECT_ROOT/target/ui-artifacts/vercel-json-render-ui"
UI_ASSET_ARCHIVE="$UI_ARTIFACT_DIR/assets.tar.gz"
UI_ASSET_MANIFEST="$UI_ARTIFACT_DIR/manifest.json"

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

apply_provider_patch() {
    patch_root=$1
    patch_source=$2
    if [ "$DRY_RUN" = 1 ]; then
        printf '  would patch: %s with %s\n' "$patch_root" "$patch_source"
    else
        patch --batch --silent -d "$patch_root" -p1 < "$patch_source" ||
            die "failed to apply provider patch: $patch_source"
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
    chmod 0755 "$wrapper_stage"
    mv -Tf -- "$wrapper_stage" "$dest"
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
            install -d -m 0755 "$dest_root$NLM_CLI_ROOT"
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
    run chmod 0755 "$venv"
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

[ -r "$MONGODB_MCP_VERSION_FILE" ] ||
    die "missing MongoDB MCP artifact declaration: $MONGODB_MCP_VERSION_FILE"
# shellcheck disable=SC1090 -- release-owned, fixed-key declaration validated below.
. "$MONGODB_MCP_VERSION_FILE"
case "${MONGODB_MCP_VERSION:-}" in *[!0-9.]*|'') die "invalid MONGODB_MCP_VERSION" ;; esac
case "${MONGODB_MCP_ARTIFACT:-}" in *[!A-Za-z0-9._-]*|'') die "invalid MONGODB_MCP_ARTIFACT" ;; esac
case "${MONGODB_MCP_SHA256:-}" in *[!0-9a-f]*|'') die "invalid MONGODB_MCP_SHA256" ;; esac
[ "${#MONGODB_MCP_SHA256}" -eq 64 ] ||
    die "MONGODB_MCP_SHA256 must contain 64 lowercase hex characters"
MONGODB_MCP_ARTIFACT_PATH="$MCP_PROVIDER_ARTIFACT_CACHE_DIR/$MONGODB_MCP_ARTIFACT"
MONGODB_MCP_INSTALL_NAME="mongodb-mcp-server-$MONGODB_MCP_VERSION-$MONGODB_MCP_SHA256"

[ -r "$TCHED_ROUTER_SOURCE_PIN" ] ||
    die "missing tched_router native source pin: $TCHED_ROUTER_SOURCE_PIN"
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
        --live-only)   die "--live-only is retired; deploy only with btrfs send/receive of a prepared snapshot" ;;
        --no-restart)  die "--no-restart is retired because this script never installs or restarts live services" ;;
        --dry-run)     DRY_RUN=1 ;;
        -h|--help)     sed -n '2,35p' "$SCRIPT_PATH"; exit 0 ;;
        *)             die "unknown option: $1" ;;
    esac
    shift
done

# ── Preconditions ───────────────────────────────────────────────────────────
[ -d "$RELEASE_DIR" ] || die "no $RELEASE_DIR — build first:
  CXXFLAGS=\"-include cstdint\" cargo build --workspace --release"

BINARIES=$(find "$RELEASE_DIR" -maxdepth 1 -type f -executable ! -name '*.d' ! -name '*.so' | sort |
    while IFS= read -r bin; do
        # Container and decoy owners have separate offline release artifacts.
        case "$(basename "$bin")" in op-mail-admin|op-decoy-network) continue ;; esac
        is_retired_binary "$(basename "$bin")" || printf '%s\n' "$bin"
    done)
[ -n "$BINARIES" ] || die "$RELEASE_DIR contains no executables"
[ -x "$RELEASE_DIR/opblob" ] || die "release is missing opblob"
[ -x "$RELEASE_DIR/op-mail-admin" ] || die "build op-mail-admin before preparing the release"
[ -f "$MAIL_ARTIFACT_PATH" ] || die "missing mail bundle; run deploy/mail-3tched/build-container-artifact.sh after the release build"
tar -tf "$MAIL_ARTIFACT_PATH" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad = 1 }
    END { exit bad ? 1 : 0 }
' || die "mail bundle contains an unsafe path"
tar -tvf "$MAIL_ARTIFACT_PATH" | awk 'substr($1,1,1) !~ /[-d]/ { bad=1 } END { exit bad ? 1 : 0 }' ||
    die "mail bundle must contain only regular files and directories"
for required in MANIFEST apply-rootfs.sh rootfs/usr/local/bin/op-mail-admin rootfs/usr/local/lib/op-mail-admin/op-mail-admin.bin rootfs/usr/local/lib/op-mail-admin/ld-linux-x86-64.so.2 rootfs/etc/init.d/op-mail-admin rootfs/etc/postfix/opdbus-main.cf rootfs/etc/postfix/opdbus-domains rootfs/etc/dovecot/opdbus.conf; do
    tar -tf "$MAIL_ARTIFACT_PATH" | grep -qx "$required" || die "mail bundle is missing $required"
done
for declaration in 'rust-target: x86_64-unknown-linux-gnu' 'elf-runtime: private-glibc-loader-bundle'; do
    tar -xOf "$MAIL_ARTIFACT_PATH" MANIFEST | grep -Fqx "$declaration" ||
        die "mail bundle does not declare $declaration"
done
[ "$(tar -xOf "$MAIL_ARTIFACT_PATH" rootfs/usr/local/lib/op-mail-admin/op-mail-admin.bin | sha256sum | cut -d' ' -f1)" = "$(sha256sum "$RELEASE_DIR/op-mail-admin" | cut -d' ' -f1)" ] ||
    die "mail bundle does not contain the current release binary"
MAIL_ARTIFACT_SHA256=$(sha256sum "$MAIL_ARTIFACT_PATH" | cut -d' ' -f1)
[ -x "$TCHED_ROUTER_NATIVE_BIN" ] ||
    die "missing tched_router native binary; run vendor/tched-router/build-native.sh --cache-dir /var/cache/op-dbus-build/tched-router --output-dir target/native-artifacts/tched-router"
[ -f "$TCHED_ROUTER_NATIVE_MANIFEST" ] ||
    die "missing tched_router native provenance manifest: $TCHED_ROUTER_NATIVE_MANIFEST"
TCHED_ROUTER_NATIVE_SHA256=$(sha256sum "$TCHED_ROUTER_NATIVE_BIN" | cut -d' ' -f1)
TCHED_ROUTER_MANIFEST_SHA256=$(sha256sum "$TCHED_ROUTER_NATIVE_MANIFEST" | cut -d' ' -f1)
TCHED_ROUTER_NATIVE_BYTES=$(wc -c < "$TCHED_ROUTER_NATIVE_BIN" | tr -d ' ')
TCHED_ROUTER_LAG_PATCH_SHA256=$(sha256sum "$TCHED_ROUTER_LAG_PATCH" | cut -d' ' -f1)
TCHED_ROUTER_GENERATION_PATCH_SHA256=$(sha256sum "$TCHED_ROUTER_GENERATION_PATCH" | cut -d' ' -f1)
TCHED_ROUTER_PATCH_INVENTORY_SHA256=$(sha256sum "$TCHED_ROUTER_PATCH_INVENTORY" | cut -d' ' -f1)
TCHED_ROUTER_BASE_MANIFEST_SHA256=$(sha256sum "$TCHED_ROUTER_BASE_MANIFEST" | cut -d' ' -f1)
jq -e \
    --arg url "$UPSTREAM_URL" \
    --arg tag "$UPSTREAM_TAG" \
    --arg commit "$UPSTREAM_COMMIT" \
    --arg source_sha "$SOURCE_ARCHIVE_SHA256" \
    --arg vendored_source_gzip_sha "$VENDORED_SOURCE_GZIP_SHA256" \
    --arg binary_sha "$TCHED_ROUTER_NATIVE_SHA256" \
    --argjson binary_bytes "$TCHED_ROUTER_NATIVE_BYTES" \
    --arg lag_patch_sha "$TCHED_ROUTER_LAG_PATCH_SHA256" \
    --arg generation_patch_sha "$TCHED_ROUTER_GENERATION_PATCH_SHA256" \
    --arg inventory_sha "$TCHED_ROUTER_PATCH_INVENTORY_SHA256" \
    --arg base_manifest_sha "$TCHED_ROUTER_BASE_MANIFEST_SHA256" \
    '.format == "opdbus.tched-router-native.v1" and
     .upstream.url == $url and
     .upstream.tag == $tag and
     .upstream.commit == $commit and
     .upstream.source_archive_sha256 == $source_sha and
     .upstream.vendored_source_gzip_sha256 == $vendored_source_gzip_sha and
     .artifact.name == "zeroclaw" and
     .artifact.sha256 == $binary_sha and
     .artifact.bytes == $binary_bytes and
     (.artifact.rust_target | type == "string" and length > 0) and
     .patches.lagged_sse_sha256 == $lag_patch_sha and
     .patches.stateless_generation_sha256 == $generation_patch_sha and
     .patches.inventory_sha256 == $inventory_sha and
     .patches.base_manifest_sha256 == $base_manifest_sha and
     (.patches.patched_cargo_lock_sha256 | type == "string" and test("^[0-9a-f]{64}$"))' \
    "$TCHED_ROUTER_NATIVE_MANIFEST" >/dev/null ||
    die "tched_router native provenance does not match its pinned source and binary"
[ "$("$TCHED_ROUTER_NATIVE_BIN" --version)" = "zeroclaw ${UPSTREAM_TAG#v}" ] ||
    die "tched_router native candidate reports an unexpected version"
strings "$TCHED_ROUTER_NATIVE_BIN" | grep -Fq 'tched_router.generation.v1' ||
    die "tched_router native candidate lacks the patched generation protocol"
[ -r "$OPENCODE_RUNTIME_CONFIG" ] ||
    die "missing OpenCode runtime contract: $OPENCODE_RUNTIME_CONFIG"
[ -x "$OPENCODE_BINARY_SOURCE" ] ||
    die "missing OpenCode binary: $OPENCODE_BINARY_SOURCE"
OPENCODE_VERSION=$(jq -er '.version | select(type == "string" and length > 0)' "$OPENCODE_RUNTIME_CONFIG")
OPENCODE_SHA256=$(jq -er '.sha256 | select(test("^[0-9a-f]{64}$"))' "$OPENCODE_RUNTIME_CONFIG")
OPENCODE_BINARY_VERSION=$($OPENCODE_BINARY_SOURCE --version)
[ "$OPENCODE_BINARY_VERSION" = "$OPENCODE_VERSION" ] ||
    die "OpenCode binary version does not match runtime contract"
[ "$(sha256sum "$OPENCODE_BINARY_SOURCE" | cut -d' ' -f1)" = "$OPENCODE_SHA256" ] ||
    die "OpenCode binary digest does not match runtime contract"
jq -e '
    .format == "opdbus.opencode-runtime.v1" and
    .binary == "/usr/bin/opencode" and
    .user == "jeremy" and .host == "127.0.0.1" and
    (.port | type == "number" and floor == . and . >= 1024 and . <= 65535) and
    .password_file == "/etc/op-dbus/secrets/opencode-server-password" and
    .auth_file == "/home/jeremy/.local/share/opencode/auth.json" and
    .agent == "opdbus-completion" and
    .config.share == "disabled" and .config.autoshare == false and
    (.config.mcp | type == "object" and length == 0) and
    .config.agent["opdbus-completion"].tools["*"] == false and
    .config.agent["opdbus-completion"].permission["*"] == "deny"' \
    "$OPENCODE_RUNTIME_CONFIG" >/dev/null ||
    die "OpenCode runtime contract is invalid"
OPENCODE_RUNTIME_CONFIG_SHA256=$(sha256sum "$OPENCODE_RUNTIME_CONFIG" | cut -d' ' -f1)
[ -f "$TCHED_ROUTER_BOOTSTRAP_CONFIG" ] ||
    die "missing tched_router bootstrap configuration: $TCHED_ROUTER_BOOTSTRAP_CONFIG"
[ -d "$TCHED_ROUTER_BOOTSTRAP_SKILLS" ] ||
    die "missing tched_router bootstrap skill bundle: $TCHED_ROUTER_BOOTSTRAP_SKILLS"
[ -z "$(find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type l -print -quit)" ] ||
    die "tched_router bootstrap skill bundle must not contain symlinks"
TCHED_ROUTER_BOOTSTRAP_SKILL_COUNT=$(find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type f | wc -l)
[ "$TCHED_ROUTER_BOOTSTRAP_SKILL_COUNT" -gt 0 ] ||
    die "tched_router bootstrap skill bundle is empty"
TCHED_ROUTER_BOOTSTRAP_CONFIG_SHA256=$(sha256sum "$TCHED_ROUTER_BOOTSTRAP_CONFIG" | cut -d' ' -f1)
TCHED_ROUTER_BOOTSTRAP_SKILLS_SHA256=$(
    find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type f -print | sort |
        while IFS= read -r skill; do
            relative=${skill#"$TCHED_ROUTER_BOOTSTRAP_SKILLS"/}
            printf '%s  %s\n' "$(sha256sum "$skill" | cut -d' ' -f1)" "$relative"
        done | sha256sum | cut -d' ' -f1
)
[ -f "$UI_SOURCE_PIN" ] && [ -f "$UI_SOURCE_ARCHIVE" ] && [ -f "$UI_SOURCE_MANIFEST" ] ||
    die "missing pinned UI source inputs under $UI_SOURCE_DIR"
[ -f "$UI_ASSET_ARCHIVE" ] && [ -f "$UI_ASSET_MANIFEST" ] ||
    die "missing UI build artifact; run vendor/vercel-json-render-ui/build-assets.sh"
ui_pin() {
    sed -n "s/^$1=//p" "$UI_SOURCE_PIN"
}
UI_SOURCE_HEAD=$(ui_pin source_head)
UI_SOURCE_WORKTREE=$(ui_pin source_worktree)
UI_SOURCE_ARCHIVE_SHA256=$(sha256sum "$UI_SOURCE_ARCHIVE" | cut -d' ' -f1)
UI_SOURCE_CONTENT_SHA256=$(sha256sum "$UI_SOURCE_MANIFEST" | cut -d' ' -f1)
[ "$UI_SOURCE_ARCHIVE_SHA256" = "$(ui_pin source_archive_sha256)" ] ||
    die "pinned UI source archive hash mismatch"
[ "$UI_SOURCE_CONTENT_SHA256" = "$(ui_pin content_manifest_sha256)" ] ||
    die "pinned UI source content-manifest hash mismatch"
UI_PACKAGE_LOCK_SHA256=$(tar -xOzf "$UI_SOURCE_ARCHIVE" vercel-json-render-ui/package-lock.json | sha256sum | cut -d' ' -f1)
UI_ASSET_SHA256=$(sha256sum "$UI_ASSET_ARCHIVE" | cut -d' ' -f1)
UI_ASSET_BYTES=$(wc -c < "$UI_ASSET_ARCHIVE" | tr -d ' ')
UI_ASSET_MANIFEST_SHA256=$(sha256sum "$UI_ASSET_MANIFEST" | cut -d' ' -f1)
jq -e \
    --arg source_head "$UI_SOURCE_HEAD" \
    --arg source_worktree "$UI_SOURCE_WORKTREE" \
    --arg source_archive_sha256 "$UI_SOURCE_ARCHIVE_SHA256" \
    --arg content_manifest_sha256 "$UI_SOURCE_CONTENT_SHA256" \
    --arg package_lock_sha256 "$UI_PACKAGE_LOCK_SHA256" \
    --arg asset_sha256 "$UI_ASSET_SHA256" \
    --argjson asset_bytes "$UI_ASSET_BYTES" \
    '.format == "opdbus.vercel-json-render-ui-assets.v1" and
     .source.head == $source_head and
     .source.worktree == $source_worktree and
     .source.archive_sha256 == $source_archive_sha256 and
     .source.content_manifest_sha256 == $content_manifest_sha256 and
     .source.package_lock_sha256 == $package_lock_sha256 and
     .build.base == "/" and
     .build.door == "https://10.0.0.3:8090" and
     (.build.node_version | type == "string" and length > 0) and
     (.build.npm_version | type == "string" and length > 0) and
     .artifact.name == "assets.tar.gz" and
     .artifact.sha256 == $asset_sha256 and
     .artifact.bytes == $asset_bytes' \
    "$UI_ASSET_MANIFEST" >/dev/null ||
    die "UI asset provenance does not match pinned source and artifact"
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
    done
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

[ -f "$MONGODB_MCP_ARTIFACT_PATH" ] ||
    die "missing pinned MongoDB MCP artifact: $MONGODB_MCP_ARTIFACT_PATH"
[ "$(sha256sum "$MONGODB_MCP_ARTIFACT_PATH" | cut -d' ' -f1)" = "$MONGODB_MCP_SHA256" ] ||
    die "MongoDB MCP artifact digest does not match deploy/config/mongodb-mcp-server.version"
tar -tzf "$MONGODB_MCP_ARTIFACT_PATH" | awk '
    /^\// || /(^|\/)\.\.($|\/)/ { bad = 1 }
    END { exit bad ? 1 : 0 }
' || die "MongoDB MCP artifact contains an unsafe path"
tar -tzf "$MONGODB_MCP_ARTIFACT_PATH" |
    grep -qx 'node_modules/mongodb-mcp-server/dist/esm/index.js' ||
    die "MongoDB MCP artifact is missing its provider entry point"

if [ -f "$GRANTS_SOURCE" ]; then
    [ -x "$RELEASE_DIR/op-grants-materializer" ] ||
        die "release is missing op-grants-materializer"
    "$RELEASE_DIR/op-grants-materializer" validate "$GRANTS_SOURCE" >/dev/null ||
        die "capability grants are not principal-only and valid"
fi
[ -x "$RELEASE_DIR/op-grants-materializer" ] ||
    die "release is missing op-grants-materializer"
"$RELEASE_DIR/op-grants-materializer" validate-audience "$MCP_AUDIENCE_POLICY_SOURCE" >/dev/null ||
    die "MCP audience policy is invalid"
"$RELEASE_DIR/op-grants-materializer" validate-toolsets "$MCP_TOOLSETS_SOURCE" >/dev/null ||
    die "MCP tool-set manifest is invalid"
[ -r "$HOST_RUNTIME_PACKAGES_SOURCE" ] ||
    die "missing host runtime package manifest: $HOST_RUNTIME_PACKAGES_SOURCE"
HOST_RUNTIME_PACKAGES=$(grep -Ev '^[[:space:]]*(#|$)' "$HOST_RUNTIME_PACKAGES_SOURCE")
[ -n "$HOST_RUNTIME_PACKAGES" ] || die "host runtime package manifest is empty"
[ -r "$EXECUTION_PYTHON_LOCK" ] || die "missing execution Python requirements lock"
[ -x "$EXECUTION_PYTHON_PREPARE" ] || die "execution Python preparation helper is not executable"
EXECUTION_PYTHON_LOCK_SHA256=$(sha256sum "$EXECUTION_PYTHON_LOCK" | cut -d' ' -f1)
command -v pacman >/dev/null 2>&1 || die "pacman is required to verify host runtime packages"
missing_host_packages=$(pacman -T $HOST_RUNTIME_PACKAGES 2>/dev/null || true)
[ -z "$missing_host_packages" ] ||
    die "missing host runtime packages: $(printf '%s' "$missing_host_packages" | tr '\n' ' ')"
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

    # /opt must be btrfs for subvolumes; refuse rather than silently making a
    # plain directory that cannot be snapshotted or sent.
    fstype=$(stat -f -c %T "$OPDBUS_ROOT" 2>/dev/null || stat -f -c %T "$(dirname "$OPDBUS_ROOT")")
    case "$fstype" in
        btrfs) ;;
        *) die "$OPDBUS_ROOT is on '$fstype', not btrfs — snapshots and send need btrfs" ;;
    esac

    run mkdir -p "$OPDBUS_ROOT"

    if [ ! -d "$GOLDEN_DIR" ]; then
        run btrfs subvolume create "$GOLDEN_DIR"
        ok "created subvolume $GOLDEN_DIR"
    else
        log "reusing existing subvolume $GOLDEN_DIR"
    fi

    run mkdir -p "$GOLDEN_DIR/bin" "$GOLDEN_DIR/sbin" "$GOLDEN_DIR/sv" \
        "$GOLDEN_DIR/etc" "$GOLDEN_DIR/opt"
    run install -d -m 0700 -o root -g root \
        "$GOLDEN_DIR/var/lib/op-dbus/identity-cozo"
    run env \
        OP_EXECUTION_PYTHON_ROOT="$GOLDEN_DIR/opt/op-dbus/execution-python" \
        OP_EXECUTION_PYTHON_REPO="$PROJECT_ROOT" \
        OP_EXECUTION_PYTHON_LOCK="$EXECUTION_PYTHON_LOCK" \
        OP_EXECUTION_PYTHON_CACHE="${OP_EXECUTION_PYTHON_CACHE:-/var/cache/op-dbus/execution-python}" \
        "$EXECUTION_PYTHON_PREPARE"
    run test -x "$GOLDEN_DIR/opt/op-dbus/execution-python/bin/ruff"
    run test -x "$GOLDEN_DIR/opt/op-dbus/execution-python/bin/mypy"
    run "$GOLDEN_DIR/opt/op-dbus/execution-python/bin/python" -m pytest --version
    ok "staged pinned Python execution environment ($EXECUTION_PYTHON_LOCK_SHA256)"

    # Binaries. A reused golden subvolume must not retain listener executables
    # that were retired when their implementation moved behind the bridge.
    for name in op-mail-admin op-decoy-network; do
        [ ! -e "$GOLDEN_DIR/bin/$name" ] || run rm -- "$GOLDEN_DIR/bin/$name"
    done
    if [ -f "$RETIRED_BINARIES_FILE" ]; then
        grep -Ev '^[[:space:]]*(#|$)' "$RETIRED_BINARIES_FILE" | while IFS= read -r name; do
            [ -n "$name" ] || continue
            [ ! -e "$GOLDEN_DIR/bin/$name" ] || run rm -- "$GOLDEN_DIR/bin/$name"
        done
    fi
    printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
        run install -Dm755 "$bin" "$GOLDEN_DIR/bin/$(basename "$bin")"
    done
    ok "staged $BIN_COUNT binaries into golden/bin"
    # The upstream-named binary is the native implementation behind the
    # tched_router plugin. Its runit definition verifies this separately
    # staged provenance manifest before exec.
    run install -Dm755 "$TCHED_ROUTER_NATIVE_BIN" \
        "$GOLDEN_DIR/usr/bin/zeroclaw"
    run install -Dm644 "$TCHED_ROUTER_NATIVE_MANIFEST" \
        "$GOLDEN_DIR/usr/share/op-dbus/native-manifests/tched-router.json"
    ok "staged pinned tched_router native binary and provenance manifest"
    run install -Dm755 "$OPENCODE_BINARY_SOURCE" "$GOLDEN_DIR/usr/bin/opencode"
    run install -Dm644 "$OPENCODE_RUNTIME_CONFIG" \
        "$GOLDEN_DIR/etc/op-dbus/opencode-runtime.json"
    ok "staged OpenCode ${OPENCODE_VERSION} runtime and provenance contract"
    ui_destination="$GOLDEN_DIR/usr/local/share/op-dbus/dashboard"
    run install -d -m 0755 "$ui_destination"
    [ ! -e "$ui_destination/assets" ] || run rm -r -- "$ui_destination/assets"
    [ ! -e "$ui_destination/index.html" ] || run rm -- "$ui_destination/index.html"
    run tar -xzf "$UI_ASSET_ARCHIVE" -C "$ui_destination"
    run install -Dm644 "$UI_ASSET_MANIFEST" \
        "$GOLDEN_DIR/usr/share/op-dbus/ui-manifests/vercel-json-render-ui.json"
    ok "staged pinned vercel-json-render UI assets and provenance manifest"
    # Offline rootfs preparation consumes this archive before snapshot/send.
    # Never install the GNU daemon directly into the running Alpine container.
    run install -Dm644 "$MAIL_ARTIFACT_PATH" "$GOLDEN_DIR/opt/op-dbus/container-artifacts/$MAIL_ARTIFACT_NAME"

    # The sealed blob is the plugin. Stage exactly the persisted committed
    # catalog, with the manifest copied last, so first boot does not regenerate
    # schemas from a different runtime environment.
    golden_catalog="$GOLDEN_DIR/var/lib/opdbus/plugin-blobs"
    [ ! -e "$golden_catalog" ] || run rm -r -- "$golden_catalog"
    run install -d -m 0755 "$golden_catalog"
    jq -r '.plugins | to_entries[] | [.key, .value] | @tsv' \
        "$PERSISTED_CATALOG_DIR/.manifest.json" |
        while IFS="$(printf '\t')" read -r plugin_id schema_hash; do
            blob="$PERSISTED_CATALOG_DIR/$plugin_id.$(printf '%s' "$schema_hash" | cut -c1-16).blob"
            run install -Dm644 "$blob" "$golden_catalog/$(basename "$blob")"
        done
    run install -Dm644 "$PERSISTED_CATALOG_DIR/.manifest.json" \
        "$golden_catalog/.manifest.json"
    ok "staged committed sealed catalog ($CATALOG_PLUGIN_COUNT plugins, generation $CATALOG_GENERATION)"

    # Control scripts + the systemd compatibility layer.
    for script in systemd-unit-to-runit op-convert-systemd-units systemctl-shim \
                  build-golden.sh; do
        [ -f "$SCRIPT_DIR/$script" ] &&
            run install -Dm755 "$SCRIPT_DIR/$script" "$GOLDEN_DIR/sbin/$script"
    done
    [ -f "$SCRIPT_DIR/../agent-runit-guard.sh" ] &&
        run install -Dm755 "$SCRIPT_DIR/../agent-runit-guard.sh" \
            "$GOLDEN_DIR/sbin/agent-runit-guard.sh"
    for helper in xray-config-mount-up emqx-prepare mcp-provider-probe opdbus-rundirs-up runit-ready-signal; do
        [ -f "$SCRIPT_DIR/libexec-3tched/$helper" ] &&
            run install -Dm755 "$SCRIPT_DIR/libexec-3tched/$helper" \
                "$GOLDEN_DIR/libexec/3tched/$helper"
    done
    ok "staged control scripts into golden/sbin"

    # Standalone EMQX is part of the release artifact and is never copied from
    # a running Netmaker container. Reused golden trees are accepted only when
    # their pinned source digest still matches.
    golden_emqx="$GOLDEN_DIR/opt/$EMQX_INSTALL_NAME"
    golden_emqx_marker="$golden_emqx/.opdbus-artifact-sha256"
    golden_emqx_current=0
    if [ -f "$golden_emqx_marker" ] &&
       [ "$(sed -n '1p' "$golden_emqx_marker")" = "$EMQX_SHA256" ]; then
        golden_emqx_current=1
    fi
    if [ "$golden_emqx_current" != 1 ]; then
        [ ! -e "$golden_emqx" ] || run rm -r -- "$golden_emqx"
        run mkdir -p "$golden_emqx"
        run tar -xzf "$EMQX_ARTIFACT_PATH" -C "$golden_emqx"
        if [ "$DRY_RUN" != 1 ]; then
            printf '%s\n' "$EMQX_SHA256" > "$golden_emqx_marker"
        fi
    fi
    run chmod 0755 "$golden_emqx"
    run mkdir -p "$GOLDEN_DIR/etc/emqx"
    if [ -d "$golden_emqx/etc" ] && [ ! -L "$golden_emqx/etc" ]; then
        run cp -a "$golden_emqx/etc/." "$GOLDEN_DIR/etc/emqx/"
        run mv -- "$golden_emqx/etc" "$golden_emqx/etc.dist"
        run ln -s ../../etc/emqx "$golden_emqx/etc"
    fi
    if [ ! -L "$GOLDEN_DIR/opt/emqx" ] ||
       [ "$(readlink "$GOLDEN_DIR/opt/emqx" 2>/dev/null || true)" != "$EMQX_INSTALL_NAME" ]; then
        [ ! -e "$GOLDEN_DIR/opt/emqx" ] || run rm -r -- "$GOLDEN_DIR/opt/emqx"
        run ln -s "$EMQX_INSTALL_NAME" "$GOLDEN_DIR/opt/emqx"
    fi
    for config_name in emqx.conf base.hocon acl.conf; do
        run install -Dm644 "$EMQX_CONFIG_DIR/$config_name" \
            "$GOLDEN_DIR/etc/emqx/$config_name"
    done
    ok "staged pinned standalone EMQX $EMQX_VERSION into golden/opt"

    # Provider packages are immutable, digest-pinned release inputs. Their
    # runit services talk to these versioned trees through stable local links;
    # neither provider creates another externally reachable MCP endpoint.
    stage_golden_mcp_provider() {
        provider_alias=$1
        provider_install_name=$2
        provider_artifact_path=$3
        provider_sha256=$4
        provider_entry=$5
        provider_overlay_source=${6:-}
        provider_overlay_target=${7:-}
        provider_patch_source=${8:-}
        provider_patch_target=${9:-}
        provider_root="$GOLDEN_DIR/opt/op-mcp-providers/$provider_install_name"
        provider_marker="$provider_root/.opdbus-artifact-sha256"
        provider_current=0
        if [ -f "$provider_marker" ] &&
           [ "$(sed -n '1p' "$provider_marker")" = "$provider_sha256" ]; then
            provider_current=1
            if [ -n "$provider_overlay_source" ] &&
               ! cmp -s "$provider_overlay_source" "$provider_root/$provider_overlay_target"; then
                die "$provider_alias content-addressed overlay does not match its digest marker"
            fi
            if [ -n "$provider_patch_source" ] &&
               ! patch --batch --silent --dry-run --reverse \
                    -d "$provider_root" -p1 < "$provider_patch_source" >/dev/null; then
                die "$provider_alias content-addressed auth patch does not match its digest marker"
            fi
        fi
        if [ "$provider_current" != 1 ]; then
            [ ! -e "$provider_root" ] || run rm -r -- "$provider_root"
            run mkdir -p "$provider_root"
            run tar -xzf "$provider_artifact_path" -C "$provider_root"
            if [ -n "$provider_overlay_source" ]; then
                run install -Dm644 "$provider_overlay_source" \
                    "$provider_root/$provider_overlay_target"
            fi
            if [ -n "$provider_patch_source" ]; then
                apply_provider_patch "$provider_root" "$provider_patch_source"
            fi
            if [ "$DRY_RUN" != 1 ]; then
                [ -f "$provider_root/$provider_entry" ] ||
                    die "$provider_alias staged tree is missing $provider_entry"
                if [ -n "$provider_overlay_source" ]; then
                    cmp -s "$provider_overlay_source" \
                        "$provider_root/$provider_overlay_target" ||
                        die "$provider_alias staged overlay verification failed"
                fi
                if [ -n "$provider_patch_source" ]; then
                    patch --batch --silent --dry-run --reverse \
                        -d "$provider_root" -p1 < "$provider_patch_source" >/dev/null ||
                        die "$provider_alias staged auth patch verification failed"
                    /usr/bin/node --check "$provider_root/$provider_patch_target" >/dev/null ||
                        die "$provider_alias patched auth manager is not valid JavaScript"
                fi
                printf '%s\n' "$provider_sha256" > "$provider_marker"
            fi
        fi
        run chmod 0755 "$provider_root"
        provider_link="$GOLDEN_DIR/opt/op-mcp-providers/$provider_alias"
        if [ ! -L "$provider_link" ] ||
           [ "$(readlink "$provider_link" 2>/dev/null || true)" != "$provider_install_name" ]; then
            if [ -e "$provider_link" ] || [ -L "$provider_link" ]; then
                run rm -r -- "$provider_link"
            fi
            run ln -s "$provider_install_name" "$provider_link"
        fi
    }
    run mkdir -p "$GOLDEN_DIR/opt/op-mcp-providers"
    install_nlm_cli "$GOLDEN_DIR" "$GOLDEN_DIR/bin/nlm"
    stage_golden_mcp_provider \
        mongodb-mcp-server "$MONGODB_MCP_INSTALL_NAME" \
        "$MONGODB_MCP_ARTIFACT_PATH" "$MONGODB_MCP_SHA256" \
        node_modules/mongodb-mcp-server/dist/esm/index.js
    run mkdir -p \
        "$GOLDEN_DIR/var/log/runit/mongodb-mcp-server"
    ok "staged pinned nlm CLI and MongoDB MCP provider into golden/opt"

    # Runit service definitions tracked in the repo. Explicit retirement is
    # subtractive so a reused golden subvolume cannot resurrect an old unit.
    if [ -f "$RETIRED_SERVICES_FILE" ]; then
        grep -Ev '^[[:space:]]*(#|$)' "$RETIRED_SERVICES_FILE" | while IFS= read -r svc; do
            [ -n "$svc" ] || continue
            [ ! -e "$GOLDEN_DIR/sv/$svc" ] || run rm -r -- "$GOLDEN_DIR/sv/$svc"
        done
    fi
    for svc_dir in "$SCRIPT_DIR"/*/; do
        [ -f "${svc_dir}run" ] || continue
        svc=$(basename "$svc_dir")
        is_retired_service "$svc" && continue
        run mkdir -p "$GOLDEN_DIR/sv/$svc"
        run install -Dm755 "${svc_dir}run" "$GOLDEN_DIR/sv/$svc/run"
        [ -f "${svc_dir}log/run" ] &&
            run install -Dm755 "${svc_dir}log/run" "$GOLDEN_DIR/sv/$svc/log/run"
        [ -f "${svc_dir}finish" ] &&
            run install -Dm755 "${svc_dir}finish" "$GOLDEN_DIR/sv/$svc/finish"
        [ -f "${svc_dir}check" ] &&
            run install -Dm755 "${svc_dir}check" "$GOLDEN_DIR/sv/$svc/check"
    done
    ok "staged runit service definitions into golden/sv"

    # Config the target needs alongside the binaries.
    [ -f "$SCRIPT_DIR/../environment.default" ] &&
        run install -Dm644 "$SCRIPT_DIR/../environment.default" \
            "$GOLDEN_DIR/etc/environment.default"
    [ -f "$SCRIPT_DIR/99-systemd-unit-to-runit.hook" ] &&
        run install -Dm644 "$SCRIPT_DIR/99-systemd-unit-to-runit.hook" \
            "$GOLDEN_DIR/etc/pacman-hooks/99-systemd-unit-to-runit.hook"
    [ -f "$SCRIPT_DIR/../99-agent-runit-guard.hook" ] &&
        run install -Dm644 "$SCRIPT_DIR/../99-agent-runit-guard.hook" \
            "$GOLDEN_DIR/etc/pacman-hooks/99-agent-runit-guard.hook"
    [ -f "$GRANTS_SOURCE" ] &&
        run install -Dm600 "$GRANTS_SOURCE" \
            "$GOLDEN_DIR/etc/opdbus/capability-grants.json"
    run install -Dm600 "$MCP_AUDIENCE_POLICY_SOURCE" \
        "$GOLDEN_DIR/etc/opdbus/mcp-audience-policy.json"
    run install -Dm600 "$MCP_TOOLSETS_SOURCE" \
        "$GOLDEN_DIR/etc/opdbus/mcp-toolsets.json"
    run install -Dm644 "$PROJECT_ROOT/deploy/dbus/org.dbusmcp.conf" \
        "$GOLDEN_DIR/etc/dbus-1/system.d/org.dbusmcp.conf"
    run install -Dm644 "$HOST_RUNTIME_PACKAGES_SOURCE" \
        "$GOLDEN_DIR/etc/opdbus/host-runtime-packages.txt"
    run install -Dm644 "$TCHED_ROUTER_BOOTSTRAP_CONFIG" \
        "$GOLDEN_DIR/etc/op-dbus/zeroclaw-runtime.toml"
    golden_tched_router_skills="$GOLDEN_DIR/etc/op-dbus/zeroclaw-skills/default"
    [ ! -e "$golden_tched_router_skills" ] || run rm -r -- "$golden_tched_router_skills"
    run install -d -m 0755 "$golden_tched_router_skills"
    run cp -a "$TCHED_ROUTER_BOOTSTRAP_SKILLS/." "$golden_tched_router_skills/"

    # Network configuration shipped by this release. The paths are relative
    # to the golden release tree and are activated only after snapshot receipt.
    for mapping in \
        "nftables.conf:nftables.conf" \
        "iptables.rules:iptables/iptables.rules" \
        "network.conf:op-dbus/network.conf" \
        "openflow-static-flows.json:op-dbus/openflow-static-flows.json" \
        "sshd/sshd_config:ssh/sshd_config" \
        "sshd/10-loopback-only.conf:ssh/sshd_config.d/10-loopback-only.conf" \
        "sshd/15-ghostbridge-vip.conf:ssh/sshd_config.d/15-ghostbridge-vip.conf" \
        "sshd/90-password-public.conf:ssh/sshd_config.d/90-singleuser-password.conf"
    do
        src=${mapping%%:*}
        rel=${mapping#*:}
        [ -f "$SCRIPT_DIR/../config/$src" ] &&
            run install -Dm644 "$SCRIPT_DIR/../config/$src" "$GOLDEN_DIR/etc/$rel"
    done

    # MANIFEST: staged release identity and hashes for verification after the
    # read-only snapshot is received. This script does not inspect a live host.
    if [ "$DRY_RUN" != 1 ]; then
        {
            printf 'golden-build: %s\n' "$STAMP"
            printf 'commit: %s\n' "$COMMIT"
            printf 'source: %s\n' "$PROJECT_ROOT"
            printf 'init: runit (sv)\n'
            printf 'binaries: %s\n' "$BIN_COUNT"
            printf 'mail-artifact: %s\n' "$MAIL_ARTIFACT_NAME"
            printf 'mail-sha256: %s\n' "$MAIL_ARTIFACT_SHA256"
            printf 'tched-router-native-sha256: %s\n' "$TCHED_ROUTER_NATIVE_SHA256"
            printf 'tched-router-manifest-sha256: %s\n' "$TCHED_ROUTER_MANIFEST_SHA256"
            printf 'tched-router-bootstrap-config-sha256: %s\n' "$TCHED_ROUTER_BOOTSTRAP_CONFIG_SHA256"
            printf 'tched-router-bootstrap-skills-sha256: %s\n' "$TCHED_ROUTER_BOOTSTRAP_SKILLS_SHA256"
            printf 'opencode-version: %s\n' "$OPENCODE_VERSION"
            printf 'opencode-sha256: %s\n' "$OPENCODE_SHA256"
            printf 'opencode-runtime-config-sha256: %s\n' "$OPENCODE_RUNTIME_CONFIG_SHA256"
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
            printf 'mongodb-mcp-artifact: %s\n' "$MONGODB_MCP_ARTIFACT"
            printf 'mongodb-mcp-sha256: %s\n' "$MONGODB_MCP_SHA256"
            printf 'execution-python-lock-sha256: %s\n' "$EXECUTION_PYTHON_LOCK_SHA256"
            printf '\n[sha256]\n'
            printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
                printf '%s  %s\n' "$(sha256sum "$bin" | cut -d' ' -f1)" "$(basename "$bin")"
            done
        } > "$GOLDEN_DIR/MANIFEST"
    fi
    ok "wrote golden/MANIFEST (commit $COMMIT)"
    log "golden is ready; the read-only snapshot and btrfs send happen at deploy time"
}

# ═══ Run ════════════════════════════════════════════════════════════════════
build_golden

log "commit $COMMIT, build $STAMP"
log "prepared subvolume: $GOLDEN_DIR"
log "next deployment boundary: create a read-only snapshot, then transfer it with btrfs send/receive"
ok "golden preparation complete"
