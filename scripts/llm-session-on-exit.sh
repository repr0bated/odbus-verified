#!/usr/bin/env bash
# Fire after a CLI exits (or from shell EXIT trap).
# For persistent sessions (factory/droid, opencode, kilo) this is *not* "session
# closed" — it re-exports any sessions whose content hash changed since last run.
#
# Usage:
#   llm-session-on-exit.sh                 # all
#   llm-session-on-exit.sh opencode        # hint only (still scans all; cheap via hash)
#   llm-session-on-exit.sh factory --only factory
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd 2>/dev/null || true)"
EXPORTER="${ROOT:-}/scripts/export-llm-sessions-to-notebooklm-sources.py"
if [[ ! -f "${EXPORTER}" ]]; then
  EXPORTER="${HOME}/.local/bin/export-sessions-by-model.py"
fi
CLI="${1:-}"
shift || true
ONLY_ARGS=()
case "$CLI" in
  opencode|kilo|codex|agy|antigravity|cursor|factory|droid|grok|claude|agent)
    # map droid → factory
    [[ "$CLI" == "droid" ]] && CLI=factory
    [[ "$CLI" == "agent" ]] && CLI=cursor
    ONLY_ARGS=(--only "$CLI")
    ;;
  ""|all) ;;
  *)
    # unknown first arg: pass through
    set -- "$CLI" "$@"
    ;;
esac
# Incremental by default (hash watermark). Silent unless -v in args.
python3 "$EXPORTER" "${ONLY_ARGS[@]}" "$@" || true
# Roll new session files into ≤300 _bundle_*.md sources (append-only)
if command -v notebook-sources-cleanup >/dev/null 2>&1; then
  notebook-sources-cleanup --archive-sessions >/dev/null 2>&1 || true
elif [[ -f "${ROOT:-/srv/git/odbus}/scripts/notebook-sources-cleanup.py" ]]; then
  python3 "${ROOT:-/srv/git/odbus}/scripts/notebook-sources-cleanup.py" --archive-sessions >/dev/null 2>&1 || true
fi
