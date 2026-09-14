#!/usr/bin/env bash
# ci-gate-dbus-leaves.sh
#
# Determinism gate: keep the projected D-Bus plugin tree (org.opdbus.v1.plugins)
# the single source of truth by FAILING if forbidden D-Bus addressing forms
# reappear in code. Only the stable root anchor may be hardcoded; plugin leaves
# must be derived (op_plugins::canonical::plugin_path / op_core::config::PLUGIN_BASE_PATH)
# or discovered by introspecting the tree.
#
# Rule 1 — Legacy alias path (the deprecated double "/plugin/plugins" spelling)
#   is forbidden everywhere except the canonical normalization layer that exists
#   precisely to accept and rewrite it.
#       forbidden: /org/opdbus/v1/plugin/plugins
#
# Rule 2 — Missing-"org" path (/opdbus/v1/plugins, lacking the org prefix) is
#   forbidden in the addressing-sensitive crates op-web and op-grpc-bridge, where
#   it silently resolves to a non-existent object path (a real bug class). The
#   same malformed form also appears as schema *data* values in other crates;
#   migrating those (they feed the footprint hash) is tracked in SIGNALS OD-31
#   and is intentionally out of this gate's scope for now.
#
# Exits non-zero on any violation. Mirrors scripts/ci-gate-removed-surfaces.sh.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

VIOLATIONS=()

# --- Rule 1: legacy "/plugin/plugins" alias ----------------------------------
# Allowed only in the normalization layer (the code whose job is to accept and
# rewrite the legacy alias).
LEGACY_EXCLUDES=(
  -g '*.rs'
  -g '!**/canonical.rs'
  -g '!**/default_registry.rs'
  -g '!crates/op-dbus-mirror/src/lib.rs'
  -g '!crates/op-plugins/src/state_plugins/freedesktop.rs'
)
matches=$(rg --fixed-strings "/org/opdbus/v1/plugin/plugins" "${LEGACY_EXCLUDES[@]}" \
  --no-heading --line-number crates 2>/dev/null || true)
if [ -n "$matches" ]; then
  while IFS= read -r line; do
    VIOLATIONS+=("legacy /plugin/plugins alias (derive via canonical::plugin_path): $line")
  done <<< "$matches"
fi

# --- Rule 2: missing-"org" path in addressing-sensitive crates ---------------
# The [^g] / ^ boundary keeps the canonical /org/opdbus/... form from matching.
for crate in op-web op-grpc-bridge; do
  matches=$(rg '(^|[^g])/opdbus/v1/plugins' -g '*.rs' \
    --no-heading --line-number "crates/$crate" 2>/dev/null || true)
  if [ -n "$matches" ]; then
    while IFS= read -r line; do
      VIOLATIONS+=("missing-org /opdbus path in $crate (use op_core::config::PLUGIN_BASE_PATH): $line")
    done <<< "$matches"
  fi
done

# --- Report ------------------------------------------------------------------
if [ "${#VIOLATIONS[@]}" -gt 0 ]; then
  echo "ERROR: ci-gate-dbus-leaves.sh failed. Forbidden D-Bus addressing detected:"
  echo
  for v in "${VIOLATIONS[@]}"; do
    echo "  - $v"
  done
  echo
  echo "Only org.opdbus.v1.plugins / /org/opdbus/v1/plugins may be hardcoded."
  echo "Derive leaves via op_plugins::canonical::plugin_path() /"
  echo "op_core::config::PLUGIN_BASE_PATH, or discover them by introspecting the tree."
  exit 1
fi

echo "OK: ci-gate-dbus-leaves.sh passed. No forbidden D-Bus addressing found."
exit 0
