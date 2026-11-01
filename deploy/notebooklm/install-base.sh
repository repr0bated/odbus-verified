#!/bin/sh
# install-base.sh — install jacob-bd notebooklm-mcp-cli on the base system.
#
#   sudo deploy/notebooklm/install-base.sh
#
# NotebookLM is independent of releases. Long-lived agent sessions launch its
# MCP server, so running it from a release overlay pins that slot and makes
# the next slot reuse fail with EBUSY. It therefore lives outside every
# release target, at a content-addressed root:
#
#   /usr/local/lib/opdbus-notebooklm/nlm-cli-<version>-<sha256>/   venv
#   /usr/local/lib/opdbus-notebooklm/bin/nlm                       CLI wrapper
#   /usr/local/lib/opdbus-notebooklm/bin/notebooklm-mcp            MCP wrapper
#
# The wheel pin is deploy/config/notebooklm-mcp.version; the wheel is read from
# the same artifact cache golden uses. Never touches /usr/bin/nlm (an unrelated
# Node tool) and never installs into /usr/local/bin.
set -eu
PATH=/usr/sbin:/usr/bin:/sbin:/bin
export PATH
umask 022

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$(readlink -f "$0")")" && pwd)
VERSION_FILE="$SCRIPT_DIR/../config/notebooklm-mcp.version"
ARTIFACT_CACHE_DIR=${OP_ARTIFACT_CACHE_DIR:-/var/cache/op-dbus/source-artifacts}
ROOT=/usr/local/lib/opdbus-notebooklm

die() { printf 'install-base: %s\n' "$*" >&2; exit 1; }
[ "$(id -u)" = 0 ] || die "run as root"

. "$VERSION_FILE"
case "${NOTEBOOKLM_MCP_VERSION:-}" in *[!0-9.]*|'') die "invalid NOTEBOOKLM_MCP_VERSION" ;; esac
case "${NOTEBOOKLM_MCP_ARTIFACT:-}" in *[!A-Za-z0-9._-]*|'') die "invalid NOTEBOOKLM_MCP_ARTIFACT" ;; esac
case "${NOTEBOOKLM_MCP_SHA256:-}" in *[!0-9a-f]*|'') die "invalid NOTEBOOKLM_MCP_SHA256" ;; esac
[ "${#NOTEBOOKLM_MCP_SHA256}" -eq 64 ] || die "NOTEBOOKLM_MCP_SHA256 must be 64 hex characters"
wheel="$ARTIFACT_CACHE_DIR/$NOTEBOOKLM_MCP_ARTIFACT"
[ -f "$wheel" ] || die "missing pinned wheel: $wheel"
[ "$(sha256sum "$wheel" | cut -d' ' -f1)" = "$NOTEBOOKLM_MCP_SHA256" ] || die "wheel digest mismatch: $wheel"

name="nlm-cli-$NOTEBOOKLM_MCP_VERSION-$NOTEBOOKLM_MCP_SHA256"
venv="$ROOT/$name"
install -d -o root -g root -m 0755 "$ROOT" "$ROOT/bin"

if [ -f "$venv/.opdbus-artifact-sha256" ] &&
   [ "$(sed -n 1p "$venv/.opdbus-artifact-sha256")" = "$NOTEBOOKLM_MCP_SHA256" ] &&
   [ -x "$venv/bin/python" ]; then
    printf 'install-base: %s already installed\n' "$name"
else
    [ ! -e "$venv" ] && [ ! -L "$venv" ] || die "content-addressed root exists but is invalid; preserving it: $venv"
    stage=$(mktemp -d "$ROOT/.nlm-stage.XXXXXX")
    python3 -m venv "$stage" || die "python3 -m venv failed; retained $stage"
    "$stage/bin/python" -I -m pip install --no-cache-dir --disable-pip-version-check "$wheel" ||
        die "pip install failed; retained $stage"
    "$stage/bin/python" -I -m notebooklm_tools.cli.main --version >/dev/null ||
        die "nlm entry point validation failed; retained $stage"
    printf '%s\n' "$NOTEBOOKLM_MCP_SHA256" > "$stage/.opdbus-artifact-sha256"
    chmod 0755 "$stage"
    mv -T -- "$stage" "$venv"
    printf 'install-base: installed %s\n' "$venv"
fi

# Venv console-script shebangs hold the staging path; invoke modules instead.
write_wrapper() {
    tmp=$(mktemp "$ROOT/bin/.wrapper.XXXXXX")
    printf '#!/bin/sh\nexec "%s/bin/python" -I -m %s "$@"\n' "$venv" "$2" > "$tmp"
    chmod 0755 "$tmp"
    mv -f -- "$tmp" "$ROOT/bin/$1"
}
write_wrapper nlm notebooklm_tools.cli.main
write_wrapper notebooklm-mcp notebooklm_tools.mcp.server
"$ROOT/bin/nlm" --version
