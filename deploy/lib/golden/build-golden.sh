#!/bin/sh
# build-golden.sh — prepare the golden btrfs subvolume from one release build.
#
# Rewritten 2026-10-29 (owner-directed, from scratch). Contract unchanged:
# populate $OPDBUS_ROOT/golden (default /opt/op-dbus/golden) with the complete
# deployable content, then let the deployment process snapshot/send/activate
# it. This script never builds Rust, never seals the catalog, never writes
# into the running host's installation and never restarts a service.
#
# Inputs (all must already exist; build/seal happen before this script):
#   target/release/                 CXXFLAGS="-include cstdint" cargo build --workspace --release
#   $OP_PERSISTED_CATALOG_SOURCE    sealed catalog from `opblob seal-shm && opblob persist`
#                                   (default /var/lib/opdbus/plugin-blobs, never modified here)
#   target/threetched-artifacts/    pinned threetched binary + provenance manifest
#   /srv/git/3tched.com             the UI repository itself — Svelte/SvelteKit only.
#                                   There is no vendored UI archive; assets are built
#                                   from this repo (overridable with OP_UI_REPO).
#   /var/cache/op-dbus/...          pinned EMQX and NotebookLM wheel artifacts.
#
# Layout created under golden/:
#   bin/                            release binaries
#   libexec/3tched/                 explicit provisioning helpers/assets
#   usr/bin/threetched              pinned threetched dependency
#   usr/share/op-dbus/              immutable config, catalog, graphs, provenance
#   usr/local/share/op-dbus/dashboard/  UI assets built from the UI repo, at root
#   sv/<service>/run                enabled application runit definitions
#   opt/                            immutable broker, provider, python runtimes
#   MANIFEST                        commit, build time, sha256 per binary, graph pin
# Host secrets, grants, network/bootstrap services and writable data are excluded.
#
# Usage:
#   build-golden.sh                 # prepare and validate golden (default)
#   build-golden.sh --golden-only   # compatibility alias for the default
#   build-golden.sh --dry-run       # validate inputs and print planned changes
# Retired options --system, --live-only and --no-restart fail explicitly.
# Requires root for the live run (the UI build itself drops to the repo owner).
set -eu

OPDBUS_ROOT=${OPDBUS_ROOT:-/opt/op-dbus}
OPDBUS_ROOT=$(realpath -e -- "$OPDBUS_ROOT")
GOLDEN_DIR="$OPDBUS_ROOT/golden"
DRY_RUN=0

SCRIPT_PATH=$(readlink -f "$0" 2>/dev/null || printf '%s' "$0")
# Internal helper of ./activate-golden.sh; every relative input lives in deploy/runit.
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")/../../runit" && pwd)
if [ -f "$SCRIPT_DIR/../../Cargo.toml" ]; then
    PROJECT_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
else
    PROJECT_ROOT=${OP_DBUS_ROOT:-$(pwd)}
fi
RELEASE_DIR=$(realpath -e -- "${OP_RELEASE_DIR:-"$PROJECT_ROOT/target/release"}")
PERSISTED_CATALOG_DIR=${OP_PERSISTED_CATALOG_SOURCE:-/var/lib/opdbus/plugin-blobs}
PERSISTED_CATALOG_DIR=$(realpath -e -- "$PERSISTED_CATALOG_DIR")
case "$PERSISTED_CATALOG_DIR" in
    "$GOLDEN_DIR"|"$GOLDEN_DIR"/*)
        printf '%s\n' "catalog source must be outside the golden preparation tree" >&2
        exit 1 ;;
esac

UI_REPO=${OP_UI_REPO:-/srv/git/3tched.com}
UI_REPO=$(realpath -e -- "$UI_REPO")
UI_DIST="$UI_REPO/dist"
THREETCHED_ARTIFACT_DIR="$PROJECT_ROOT/target/threetched-artifacts/tched-router"
THREETCHED_BIN="$THREETCHED_ARTIFACT_DIR/threetched"
THREETCHED_MANIFEST="$THREETCHED_ARTIFACT_DIR/manifest.json"
TCHED_ROUTER_SOURCE_PIN="$PROJECT_ROOT/vendor/tched-router/source.pin"
EMQX_VERSION_FILE="$SCRIPT_DIR/../config/emqx.version"
EMQX_CONFIG_DIR="$SCRIPT_DIR/../emqx"
ARTIFACT_CACHE_DIR=${OP_ARTIFACT_CACHE_DIR:-/var/cache/op-dbus/source-artifacts}
MCP_AUDIENCE_POLICY_SOURCE="$SCRIPT_DIR/../config/mcp-audience-policy.json"
HOST_RUNTIME_PACKAGES_SOURCE="$SCRIPT_DIR/../config/host-runtime-packages.txt"
EXECUTION_PYTHON_LOCK="$SCRIPT_DIR/../config/execution-python-requirements.lock"
EXECUTION_PYTHON_PREPARE="$SCRIPT_DIR/prepare-execution-python.sh"
HOST_RUNTIME_PACKAGE_CHECK="$SCRIPT_DIR/check-host-runtime-packages.sh"
TCHED_ROUTER_BOOTSTRAP_CONFIG="$SCRIPT_DIR/../config/threetched-runtime.toml"
TCHED_ROUTER_BOOTSTRAP_SKILLS="$SCRIPT_DIR/../config/threetched-skills/default"
AGENT_BUNDLE_SOURCE="$SCRIPT_DIR/../config/agents"
RETIRED_SERVICES_FILE="$SCRIPT_DIR/retired-services"
RETIRED_BINARIES_FILE="$SCRIPT_DIR/retired-binaries"

# Explicit source -> release-name inventory, relative to deploy/. Never copy
# the whole host helper directory: network/storage bootstrap stays independent.
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

log() { printf '\033[0;34m[golden]\033[0m %s\n' "$*"; }
ok()  { printf '\033[0;32m[ ok  ]\033[0m %s\n' "$*"; }
die() { printf '\033[0;31m[fail ]\033[0m %s\n' "$*" >&2; exit 1; }
run() {
    if [ "$DRY_RUN" = 1 ]; then printf '  would run: %s\n' "$*"; else "$@"; fi
}

# Only this already-provisioned, root-owned preparation tree may be changed.
# Never follow a stale destination link into host state.
ensure_directory() (
    directory=$1
    case "$directory" in "$GOLDEN_DIR"|"$GOLDEN_DIR"/*) ;; *) die "destination outside golden: $directory" ;; esac
    [ ! -L "$directory" ] || die "symlink directory in golden: $directory"
    if [ "$directory" != "$GOLDEN_DIR" ]; then ensure_directory "$(dirname "$directory")"; fi
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
                if [ -d "$entry" ] && [ ! -L "$entry" ]; then run rmdir -- "$entry"; else run rm -- "$entry"; fi
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
    [ -f "$RETIRED_SERVICES_FILE" ] &&
        grep -Ev '^[[:space:]]*(#|$)' "$RETIRED_SERVICES_FILE" | grep -qx "$1"
}
is_retired_binary() {
    [ -f "$RETIRED_BINARIES_FILE" ] &&
        grep -Ev '^[[:space:]]*(#|$)' "$RETIRED_BINARIES_FILE" | grep -qx "$1"
}
is_application_service() {
    [ -f "$SCRIPT_DIR/$1/run" ] &&
        grep -qx '# op-dbus-release: application' "$SCRIPT_DIR/$1/run" &&
        grep -Ev '^[[:space:]]*(#|$)' "$SCRIPT_DIR/enabled-services" | grep -qx "$1" &&
        ! is_retired_service "$1"
}


while [ $# -gt 0 ]; do
    case "$1" in
        --golden-only) ;;
        --dry-run) DRY_RUN=1 ;;
        --system) die "--system is retired; the base-system payload is staged by its own owner" ;;
        --live-only) die "--live-only is retired; deploy only with btrfs send/receive of a prepared snapshot" ;;
        --no-restart) die "--no-restart is retired because this script never installs or restarts live services" ;;
        -h|--help) sed -n '2,34p' "$SCRIPT_PATH"; exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done

# ── Declared artifact pins ──────────────────────────────────────────────────
[ -r "$EMQX_VERSION_FILE" ] || die "missing EMQX artifact declaration: $EMQX_VERSION_FILE"
# shellcheck disable=SC1090 -- release-owned, fixed-key declaration validated below.
. "$EMQX_VERSION_FILE"
case "${EMQX_VERSION:-}" in *[!0-9.]*|'') die "invalid EMQX_VERSION" ;; esac
case "${EMQX_ARTIFACT:-}" in *[!A-Za-z0-9._-]*|'') die "invalid EMQX_ARTIFACT" ;; esac
case "${EMQX_SHA256:-}" in *[!0-9a-f]*|'') die "invalid EMQX_SHA256" ;; esac
[ "${#EMQX_SHA256}" -eq 64 ] || die "EMQX_SHA256 must contain 64 lowercase hex characters"
EMQX_ARTIFACT_PATH="$ARTIFACT_CACHE_DIR/$EMQX_ARTIFACT"
EMQX_INSTALL_NAME="emqx-$EMQX_VERSION-$EMQX_SHA256"

# ── Preconditions ───────────────────────────────────────────────────────────
printf '%s\n' "$RELEASE_HELPER_INPUTS" | while read -r mode source helper; do
    input="$SCRIPT_DIR/../$source"
    [ -f "$input" ] && [ ! -L "$input" ] && [ -r "$input" ] ||
        die "missing or unsafe release helper input: $input"
done
[ -d "$RELEASE_DIR" ] || die "no $RELEASE_DIR — build first:
  CXXFLAGS=\"-include cstdint\" cargo build --workspace --release"

BINARIES=$(find "$RELEASE_DIR" -maxdepth 1 -type f -executable ! -name '*.d' ! -name '*.so' | sort |
    while IFS= read -r bin; do
        # Container, decoy, storage and release owners have separate artifacts.
        case "$(basename "$bin")" in
            op-mail-admin|op-decoy-*|op-wgcf*|op-ovs*|op-rtnetlink*|op-storage*|op-release*) continue ;;
        esac
        is_retired_binary "$(basename "$bin")" || printf '%s\n' "$bin"
    done)
[ -n "$BINARIES" ] || die "$RELEASE_DIR contains no executables"
printf '%s\n' "$BINARIES" | grep -Fxq "$RELEASE_DIR/3tched-fs" ||
    die "release is missing 3tched-fs required by identity provisioning"
[ -x "$RELEASE_DIR/opblob" ] || die "release is missing opblob"
[ -x "$RELEASE_DIR/system_graph_inventory" ] ||
    die "release is missing system_graph_inventory; rebuild the workspace"

# threetched: pinned upstream binary staged as usr/bin/threetched.
[ -f "$TCHED_ROUTER_SOURCE_PIN" ] || die "missing threetched source pin: $TCHED_ROUTER_SOURCE_PIN"
# shellcheck disable=SC1090 -- release-owned, fixed-key declaration validated in the manifest check below.
. "$TCHED_ROUTER_SOURCE_PIN"
[ -x "$THREETCHED_BIN" ] || die "missing threetched binary; run vendor/tched-router/build-threetched.sh"
[ -f "$THREETCHED_MANIFEST" ] || die "missing threetched provenance manifest: $THREETCHED_MANIFEST"
THREETCHED_SHA256=$(sha256sum "$THREETCHED_BIN" | cut -d' ' -f1)
TCHED_ROUTER_MANIFEST_SHA256=$(sha256sum "$THREETCHED_MANIFEST" | cut -d' ' -f1)
jq -e --arg sha "$THREETCHED_SHA256" --arg tag "${UPSTREAM_TAG:-}" \
    '.format == "opdbus.threetched.v2" and .source.mode == "unmodified-upstream" and
     (has("patches") | not) and .artifact.name == "threetched" and
     .artifact.sha256 == $sha and .upstream.tag == $tag' \
    "$THREETCHED_MANIFEST" >/dev/null ||
    die "threetched provenance does not match its pinned source and binary"
[ "$("$THREETCHED_BIN" --version)" = "zeroclaw ${UPSTREAM_TAG#v}" ] ||
    die "threetched candidate reports an unexpected version"
[ -f "$TCHED_ROUTER_BOOTSTRAP_CONFIG" ] || die "missing threetched bootstrap config"
[ -d "$TCHED_ROUTER_BOOTSTRAP_SKILLS" ] && [ -n "$(find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type f -print -quit)" ] ||
    die "missing or empty threetched bootstrap skill bundle"
[ -f "$AGENT_BUNDLE_SOURCE/AGENTS.md" ] || die "missing shared agent bundle: $AGENT_BUNDLE_SOURCE"

# UI: the repository itself, Svelte only. Dependencies may list React
# (Vercel AI SDK and Chat SDK peers). The Svelte app must not import it.
[ -f "$UI_REPO/package.json" ] || die "missing UI repository package.json: $UI_REPO"
grep -q '"@json-render/svelte"' "$UI_REPO/package.json" && grep -q '"svelte"' "$UI_REPO/package.json" ||
    die "UI repository is not the Svelte/@json-render/svelte app: $UI_REPO"
react_use=$(git -C "$UI_REPO" grep -n -E \
    -e "from ['\"]react['\"/]" \
    -e "from ['\"]react-dom['\"/]" \
    -e "require\\(['\"]react" \
    HEAD -- src || true)
if [ -n "$react_use" ]; then
    printf '%s\n' "$react_use" >&2
    die "UI source imports React; dependencies may be present, src/ may not use react or react-dom"
fi
UI_HEAD=$(git -C "$UI_REPO" rev-parse HEAD 2>/dev/null) || die "UI repository has no git HEAD: $UI_REPO"
UI_WORKTREE=clean-head
UI_TMP=
cleanup_ui() { [ -z "$UI_TMP" ] || rm -rf -- "$UI_TMP"; }
trap cleanup_ui EXIT HUP INT TERM
if [ "$DRY_RUN" = 1 ]; then
    [ -f "$UI_DIST/index.html" ] || die "UI dist missing; build $UI_REPO first (npm run build) or run without --dry-run"
    UI_STAGE="$UI_DIST"
else
    # Build from a clean HEAD checkout: the live UI worktree may be dirty or
    # conflicted and is never modified here. Deps are shared read-only via the
    # repo's existing node_modules; the temp checkout is removed on exit.
    ui_owner=$(stat -c %U "$UI_REPO")
    UI_TMP=$(mktemp -d)
    git -C "$UI_REPO" archive HEAD | tar -x -C "$UI_TMP" ||
        die "cannot export clean UI HEAD from $UI_REPO"
    [ -d "$UI_REPO/node_modules" ] || die "UI repo node_modules missing: $UI_REPO/node_modules"
    ln -s "$UI_REPO/node_modules" "$UI_TMP/node_modules"
    # Untracked build inputs (e.g. generated sources) live only in the
    # worktree; tracked files always come from HEAD above, never the worktree.
    if [ -n "$(git -C "$UI_REPO" ls-files --others --exclude-standard)" ]; then
        (CDPATH= cd -- "$UI_REPO" && git ls-files --others --exclude-standard -z |
            tar --null -T - -cf -) | tar -x -C "$UI_TMP" ||
            die "cannot overlay untracked UI inputs from $UI_REPO"
    fi
    log "building UI assets from clean HEAD ${UI_HEAD} of $UI_REPO (owner $ui_owner)"
    # /usr/local/bin/node is v22 (SvelteKit postbuild needs Promise.withResolvers);
    # restricted caller PATHs (activate-golden.sh) would otherwise pick v20.
    if [ "$(id -u)" = 0 ] && [ "$ui_owner" != root ]; then
        chown -R "$ui_owner" "$UI_TMP"
        sudo -n -u "$ui_owner" -- env "HOME=$(getent passwd "$ui_owner" | cut -d: -f6)" \
            "PATH=/usr/local/bin:$PATH" "SVELTEKIT_BASE_PATH=" npm --prefix "$UI_TMP" run build >/dev/null ||
            die "UI build failed for clean HEAD of $UI_REPO"
    else
        PATH="/usr/local/bin:$PATH" SVELTEKIT_BASE_PATH= npm --prefix "$UI_TMP" run build >/dev/null ||
            die "UI build failed for clean HEAD of $UI_REPO"
    fi
    [ -f "$UI_TMP/dist/index.html" ] || die "UI build produced no dist/index.html for clean HEAD of $UI_REPO"
    UI_STAGE="$UI_TMP/dist"
fi
UI_TREE_SHA256=$(find "$UI_STAGE" -type f -print | sort |
    while IFS= read -r f; do sha256sum "$f"; done | sha256sum | cut -d' ' -f1)

# Sealed catalog: copied, never resealed here.
[ -f "$PERSISTED_CATALOG_DIR/.manifest.json" ] ||
    die "missing persisted sealed catalog; run the freshly built opblob seal-shm and persist first"
jq -e '(.generation | type == "number" and . >= 1) and
       (.catalog_hash | type == "string" and test("^[0-9a-f]{64}$")) and
       (.plugins | type == "object" and length > 0) and (.plugins.threetched | type == "string")' \
    "$PERSISTED_CATALOG_DIR/.manifest.json" >/dev/null ||
    die "persisted sealed catalog manifest is invalid or missing threetched"
CATALOG_GENERATION=$(jq -r '.generation' "$PERSISTED_CATALOG_DIR/.manifest.json")
CATALOG_HASH=$(jq -r '.catalog_hash' "$PERSISTED_CATALOG_DIR/.manifest.json")
CATALOG_MANIFEST_SHA256=$(sha256sum "$PERSISTED_CATALOG_DIR/.manifest.json" | cut -d' ' -f1)
CATALOG_PLUGIN_COUNT=$(jq -r '.plugins | length' "$PERSISTED_CATALOG_DIR/.manifest.json")
[ "$(find "$PERSISTED_CATALOG_DIR" -maxdepth 1 -type f -name '*.blob' | wc -l)" -eq "$CATALOG_PLUGIN_COUNT" ] ||
    die "persisted sealed catalog blob count does not match its manifest"

[ -f "$EMQX_ARTIFACT_PATH" ] && [ "$(sha256sum "$EMQX_ARTIFACT_PATH" | cut -d' ' -f1)" = "$EMQX_SHA256" ] ||
    die "missing or mismatched pinned EMQX artifact: $EMQX_ARTIFACT_PATH"
[ -x "$RELEASE_DIR/op-grants-materializer" ] || die "release is missing op-grants-materializer"
"$RELEASE_DIR/op-grants-materializer" validate-audience "$MCP_AUDIENCE_POLICY_SOURCE" >/dev/null ||
    die "MCP audience policy is invalid"
[ -r "$HOST_RUNTIME_PACKAGES_SOURCE" ] && [ -x "$HOST_RUNTIME_PACKAGE_CHECK" ] ||
    die "missing host runtime package manifest or checker"
if ! missing_host_packages=$("$HOST_RUNTIME_PACKAGE_CHECK" "$HOST_RUNTIME_PACKAGES_SOURCE"); then
    die "host runtime package verification failed: $(printf '%s' "$missing_host_packages" | tr '\n' ' ')"
fi
[ -r "$EXECUTION_PYTHON_LOCK" ] && [ -x "$EXECUTION_PYTHON_PREPARE" ] ||
    die "missing execution Python lock or preparation helper"
EXECUTION_PYTHON_LOCK_SHA256=$(sha256sum "$EXECUTION_PYTHON_LOCK" | cut -d' ' -f1)
TCHED_ROUTER_BOOTSTRAP_CONFIG_SHA256=$(sha256sum "$TCHED_ROUTER_BOOTSTRAP_CONFIG" | cut -d' ' -f1)
TCHED_ROUTER_BOOTSTRAP_SKILLS_SHA256=$(
    find "$TCHED_ROUTER_BOOTSTRAP_SKILLS" -type f -print | sort |
        while IFS= read -r skill; do sha256sum "$skill"; done | sha256sum | cut -d' ' -f1)
BIN_COUNT=$(printf '%s\n' "$BINARIES" | wc -l)
log "$BIN_COUNT release binaries in $RELEASE_DIR"
if [ "$DRY_RUN" != 1 ] && [ "$(id -u)" != 0 ]; then
    die "must run as root to prepare the golden btrfs subvolume"
fi
COMMIT=$(git -C "$PROJECT_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)
STAMP=$(date -u '+%Y%m%dT%H%M%SZ')

# ═══ Golden subvolume ═══════════════════════════════════════════════════════
build_golden() {
    log "building golden subvolume at $GOLDEN_DIR"
    [ -d "$GOLDEN_DIR" ] && [ ! -L "$GOLDEN_DIR" ] ||
        die "golden must already exist; use op-release provision-slots"
    [ "$(stat -f -c %T "$GOLDEN_DIR")" = btrfs ] && [ "$(stat -c %i "$GOLDEN_DIR")" = 256 ] ||
        die "golden is not a Btrfs subvolume; use op-release provision-slots"
    [ "$(stat -c %u:%g "$GOLDEN_DIR")" = 0:0 ] || die "golden must be root-owned"
    case "$(stat -c %a "$GOLDEN_DIR")" in 700|750|755) ;; *) die "unsafe golden root mode" ;; esac
    btrfs subvolume show "$GOLDEN_DIR" >/dev/null || die "cannot inspect the provisioned golden subvolume"
    [ "$(btrfs property get -ts "$GOLDEN_DIR" ro)" = ro=false ] || die "golden must be writable preparation state"
    [ -z "$(find "$GOLDEN_DIR" -mindepth 1 -type d -inum 256 -print -quit)" ] ||
        die "golden contains a nested Btrfs subvolume"
    if findmnt -rn -o TARGET | awk -v root="$GOLDEN_DIR/" \
        'index($0, root) == 1 { found=1 } END { exit found ? 0 : 1 }'; then
        die "golden contains a nested mount"
    fi

    ensure_directory "$GOLDEN_DIR/bin"
    ensure_directory "$GOLDEN_DIR/sv"
    ensure_directory "$GOLDEN_DIR/usr/share/op-dbus"

    # Preparation-owned paths from older golden layouts. Never remove their
    # live host counterparts or any unrecognized base files.
    for legacy in sbin/systemd-unit-to-runit sbin/op-convert-systemd-units \
        sbin/systemctl-shim sbin/ban-caddy-nginx sbin/build-golden.sh sbin/agent-runit-guard.sh \
        libexec/op-warp-ssh share/op-dbus/warp-ssh \
        share/op-dbus/notebook-sync var/lib/opdbus/plugin-blobs \
        etc/environment.default etc/opdbus etc/op-dbus etc/dbus-1 etc/pam.d \
        etc/nftables.conf etc/iptables etc/ssh etc/emqx \
        opt/op-mcp-providers usr/share/op-dbus/etc/rust-network-mgr usr/bin/opencode; do
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

    # Binaries: stage exactly the filtered release inventory.
    find "$GOLDEN_DIR/bin" -mindepth 1 -maxdepth 1 -print | while IFS= read -r old; do
        printf '%s\n' "$BINARIES" | grep -Fxq "$RELEASE_DIR/$(basename "$old")" || run rm -- "$old"
    done
    printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
        install_file 0755 "$bin" "$GOLDEN_DIR/bin/$(basename "$bin")"
    done
    install_file 0755 "$THREETCHED_BIN" "$GOLDEN_DIR/usr/bin/threetched"
    install_file 0644 "$THREETCHED_MANIFEST" \
        "$GOLDEN_DIR/usr/share/op-dbus/threetched-manifests/tched-router.json"

    # UI: assets built from the UI repository, staged at the dashboard root.
    sync_tree "$UI_STAGE" "$GOLDEN_DIR/usr/local/share/op-dbus/dashboard"
    ui_manifest_tmp=$(mktemp)
    jq -n --arg head "$UI_HEAD" --arg worktree "$UI_WORKTREE" --arg repo "$UI_REPO" \
        --arg tree "$UI_TREE_SHA256" \
        '{format:"opdbus.3tched-ui-repo.v1", source:{repo:$repo, head:$head, worktree:$worktree},
          artifact:{tree_sha256:$tree, base:"/"}}' > "$ui_manifest_tmp"
    install_file 0644 "$ui_manifest_tmp" "$GOLDEN_DIR/usr/share/op-dbus/ui-manifests/3tched-ui.json"
    rm -f -- "$ui_manifest_tmp"

    # Sealed catalog.
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

    # Helpers: only the explicit provisioning closure above.
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

    # EMQX: pinned content-addressed broker installation.
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
        [ "$(cat "$golden_emqx_marker")" = "$EMQX_SHA256" ] || die "EMQX content-address marker mismatch"
    fi
    # esockd ships a public demonstration private key, not runtime credentials.
    for demo_key in "$golden_emqx"/lib/esockd-*/priv/demo.key; do
        [ -e "$demo_key" ] || [ -L "$demo_key" ] || continue
        [ -f "$demo_key" ] && [ ! -L "$demo_key" ] || die "unexpected esockd demonstration key type"
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
            [ "$(readlink "$golden_emqx/$name")" = "$target" ] || run rm -- "$golden_emqx/$name"
        elif [ -e "$golden_emqx/$name" ]; then
            run rm -r -- "$golden_emqx/$name"
        fi
        [ -L "$golden_emqx/$name" ] || run ln -s "$target" "$golden_emqx/$name"
    done
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

    # NotebookLM is independent of releases (deploy/notebooklm/install-base.sh
    # installs it on the base system); agent sessions running it from an
    # overlay pin retired slots. Golden ships only its MCP setting.
    for old in "$GOLDEN_DIR"/opt/opdbus/nlm-cli-*; do
        [ -e "$old" ] || continue
        [ -f "$old/.opdbus-artifact-sha256" ] || die "unowned old provider tree: $old"
        run rm -r -- "$old"
    done
    install_file 0644 "$SCRIPT_DIR/../config/agent-mcp.json" "$GOLDEN_DIR/usr/share/op-dbus/agent-mcp.json"

    ensure_directory "$GOLDEN_DIR/opt/op-dbus"
    [ ! -L "$GOLDEN_DIR/opt/op-dbus/execution-python" ] || die "symlink execution environment in golden"
    run env OP_EXECUTION_PYTHON_ROOT="$GOLDEN_DIR/opt/op-dbus/execution-python" \
        OP_EXECUTION_PYTHON_REPO="$PROJECT_ROOT" OP_EXECUTION_PYTHON_LOCK="$EXECUTION_PYTHON_LOCK" \
        OP_EXECUTION_PYTHON_CACHE="${OP_EXECUTION_PYTHON_CACHE:-/var/cache/op-dbus/execution-python}" \
        OP_EXECUTION_PYTHON_BUNDLE="${OP_EXECUTION_PYTHON_BUNDLE:-/home/jeremy/.local/share/uv/python/cpython-3.14.7-linux-x86_64-gnu}" \
        "$EXECUTION_PYTHON_PREPARE"

    # Services: one inventory; only enabled application definitions enter.
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

    # Immutable shared config.
    install_file 0644 "$SCRIPT_DIR/../environment.default" "$GOLDEN_DIR/usr/share/op-dbus/etc/environment.default"
    install_file 0600 "$MCP_AUDIENCE_POLICY_SOURCE" "$GOLDEN_DIR/usr/share/op-dbus/etc/opdbus/mcp-audience-policy.json"
    install_file 0644 "$HOST_RUNTIME_PACKAGES_SOURCE" "$GOLDEN_DIR/usr/share/op-dbus/etc/opdbus/host-runtime-packages.txt"
    for declaration in enabled-services retired-services retired-binaries; do
        install_file 0644 "$SCRIPT_DIR/$declaration" "$GOLDEN_DIR/usr/share/op-dbus/etc/opdbus/$declaration"
    done
    install_file 0644 "$TCHED_ROUTER_BOOTSTRAP_CONFIG" "$GOLDEN_DIR/usr/share/op-dbus/etc/op-dbus/threetched-runtime.toml"
    sync_tree "$TCHED_ROUTER_BOOTSTRAP_SKILLS" "$GOLDEN_DIR/usr/share/op-dbus/etc/op-dbus/threetched-skills/default"
    sync_tree "$AGENT_BUNDLE_SOURCE" "$GOLDEN_DIR/usr/share/op-dbus/agents"

    # MANIFEST: staged release identity and hashes for post-receive verification.
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
            printf 'ui-source-repo: %s\n' "$UI_REPO"
            printf 'ui-source-head: %s\n' "$UI_HEAD"
            printf 'ui-source-worktree: %s\n' "$UI_WORKTREE"
            printf 'ui-assets-tree-sha256: %s\n' "$UI_TREE_SHA256"
            printf 'plugin-catalog-generation: %s\n' "$CATALOG_GENERATION"
            printf 'plugin-catalog-hash: %s\n' "$CATALOG_HASH"
            printf 'plugin-catalog-manifest-sha256: %s\n' "$CATALOG_MANIFEST_SHA256"
            printf 'emqx-artifact: %s\n' "$EMQX_ARTIFACT"
            printf 'emqx-sha256: %s\n' "$EMQX_SHA256"
            printf 'execution-python-lock-sha256: %s\n' "$EXECUTION_PYTHON_LOCK_SHA256"
            printf '\n[sha256]\n'
            printf '%s\n' "$BINARIES" | while IFS= read -r bin; do
                printf '%s  %s\n' "$(sha256sum "$bin" | cut -d' ' -f1)" "$(basename "$bin")"
            done
        } > "$GOLDEN_DIR/MANIFEST"
    fi
    ok "wrote golden/MANIFEST (commit $COMMIT)"

    # Derive inventory from the finished prepared tree; the graph directory and
    # MANIFEST digest are excluded by the collector to prevent self-reference.
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

build_golden
log "commit $COMMIT, build $STAMP"
log "prepared subvolume: $GOLDEN_DIR"
log "next deployment boundary: create a read-only snapshot, then transfer it with btrfs send/receive"
ok "golden preparation complete"
