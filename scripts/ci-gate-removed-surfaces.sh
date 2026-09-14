#!/usr/bin/env bash
# ci-gate-removed-surfaces.sh
#
# CI gate: fail if any forbidden strings (removed surfaces) reappear in the
# codebase, or if forbidden modules exist in op-web.
#
# Forbidden strings:
#   - ai.gateway.lovable.dev
#   - Lovable-API-Key
#   - LOVABLE_API_KEY
#   - createLovableAiGatewayProvider
#   - @ai-sdk/openai-compatible
#   - /api/llm/provider   (POST runtime-switching route; read-only /api/llm/* is OK)
#   - /api/llm/model       (POST runtime-switching route; read-only /api/llm/* is OK)
#
# Forbidden modules in op-web:
#   - handlers::gemma
#   - handlers::ui_gallery
#
# Excludes:
#   - .specs/ directory
#   - this script itself
#
# Exits non-zero on any violation.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Path to this script, for self-exclusion.
SELF_SCRIPT="scripts/ci-gate-removed-surfaces.sh"

# Build the ripgrep exclude glob list.
# -g '!<glob>' negates (excludes) paths matching the glob.
EXCLUDES=(
  -g '!.specs/**'
  -g '!scripts/**'
  -g "!$SELF_SCRIPT"
  -g '!*.jsonl'
  -g '!*.xml'
  -g '!*.txt'
  -g '!*.md'
  -g '!assistant-work*'
  -g '!schema-work*'
  -g '!hypervisor*'
  -g '!repomix*'
  -g '!md-xml/**'
  -g '!knowledge/**'
  -g '!crates/audits/**'
  -g '!*.log'
  -g '!chat/**'
  -g '!archive/**'
  -g '!0/**'
)

VIOLATIONS=()

# --- Check forbidden literal strings -----------------------------------------
# These are exact strings with no substring ambiguity.
FORBIDDEN_LITERALS=(
  "ai.gateway.lovable.dev"
  "Lovable-API-Key"
  "LOVABLE_API_KEY"
  "createLovableAiGatewayProvider"
  "@ai-sdk/openai-compatible"
)

for s in "${FORBIDDEN_LITERALS[@]}"; do
  matches=$(rg --fixed-strings "$s" "${EXCLUDES[@]}" --no-heading --line-number . 2>/dev/null || true)
  if [ -n "$matches" ]; then
    while IFS= read -r line; do
      VIOLATIONS+=("forbidden string: \"$s\" -> $line")
    done <<< "$matches"
  fi
done

# --- Check forbidden route strings (regex, avoid plural false positives) -----
# /api/llm/provider and /api/llm/model are the singular POST runtime-switching
# routes. The plural GET routes (/api/llm/providers, /api/llm/models) are OK.
# Use regex [^s] boundary to avoid matching the plural forms.
FORBIDDEN_ROUTE_REGEXES=(
  '/api/llm/provider([^s]|$)'
  '/api/llm/model([^s]|$)'
)

for pat in "${FORBIDDEN_ROUTE_REGEXES[@]}"; do
  matches=$(rg "$pat" "${EXCLUDES[@]}" --no-heading --line-number . 2>/dev/null || true)
  if [ -n "$matches" ]; then
    while IFS= read -r line; do
      VIOLATIONS+=("forbidden route pattern: \"$pat\" -> $line")
    done <<< "$matches"
  fi
done

# --- Check forbidden modules in op-web --------------------------------------
# Ensure handlers::gemma and handlers::ui_gallery modules don't exist.
FORBIDDEN_MODULE_NAMES=(
  "gemma"
  "ui_gallery"
)

for mod_name in "${FORBIDDEN_MODULE_NAMES[@]}"; do
  # Search for `mod gemma` or `mod ui_gallery` declarations in op-web source.
  matches=$(rg --fixed-strings "mod ${mod_name}" -g 'crates/op-web/src/**' --no-heading --line-number . 2>/dev/null || true)
  if [ -n "$matches" ]; then
    while IFS= read -r line; do
      VIOLATIONS+=("forbidden module: \"handlers::${mod_name}\" -> $line")
    done <<< "$matches"
  fi
done

# --- Report ------------------------------------------------------------------
if [ "${#VIOLATIONS[@]}" -gt 0 ]; then
  echo "ERROR: ci-gate-removed-surfaces.sh failed. Forbidden surfaces detected:"
  echo
  for v in "${VIOLATIONS[@]}"; do
    echo "  - $v"
  done
  echo
  exit 1
fi

echo "OK: ci-gate-removed-surfaces.sh passed. No forbidden surfaces found."
exit 0
