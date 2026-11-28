#!/bin/sh
# Secret guard (owner rule 2026-11-22: no secret may ever be committed or pushed).
# Called by .githooks/pre-commit (--staged) and .githooks/pre-push (--range A..B).
# Fails closed. Prints only path, line and rule, never a matched value.
set -u
mode=${1:---staged}; range=${2:-}
top=$(git rev-parse --show-toplevel) || exit 1
cd "$top" || exit 1
fail=0

# 1. Paths that must never be tracked (keys, env files, store/secrets dirs).
deny='(\.(key|pem|p12|pfx|jks|keystore|age|sealed)$)|((^|/)id_(rsa|dsa|ecdsa|ed25519)(\.pub)?$)|((^|/)\.env(\..*)?$)|((^|/)secrets?/)|((^|/)etc/op-dbus/env$)|((^|/)wg[0-9]*\.conf$)|((^|/)(private\.key|mcp_token|zeroclaw-gateway\.token|.*\.token)$)|((^|/)(client_secret[^/]*\.json|wgcf-profile\.conf|credentials\.json)$)'
allow='(^|/)\.env\.example$'
case "$mode" in
  --staged) names=$(git diff --cached --name-only --diff-filter=ACMR) ;;
  --range)  names=$(git diff --name-only --diff-filter=ACMR "$range") ;;
  *) echo "secret-guard: usage: secret-guard.sh --staged | --range A..B" >&2; exit 2 ;;
esac
bad=$(printf '%s\n' "$names" | grep -E "$deny" | grep -Ev "$allow")
if [ -n "$bad" ]; then
  printf 'secret-guard: refused path (keys/env/secrets never go in git):\n%s\n' "$bad" >&2
  fail=1
fi

# 2. Content scan: gitleaks when installed, plus a built-in high-signal scan that
#    always runs, so the guard holds on any machine (terminal, Grok Bot, cloud desktop).
GL=$(command -v gitleaks 2>/dev/null || { [ -x /usr/local/bin/gitleaks ] && echo /usr/local/bin/gitleaks; })
cfg=; [ -f .gitleaks.toml ] && cfg="--config .gitleaks.toml"
if [ -n "$GL" ]; then
  if [ "$mode" = --staged ]; then
    "$GL" git --pre-commit --staged --redact --no-banner $cfg . >&2 || fail=1
  else
    "$GL" git --redact --no-banner $cfg --log-opts="$range" . >&2 || fail=1
  fi
fi
if [ "$mode" = --staged ]; then diff=$(git diff --cached -U0 --no-color)
else diff=$(git diff -U0 --no-color "$range"); fi
hits=$(printf '%s\n' "$diff" | awk -v OFS=: '
  /^\+\+\+ b\//{f=substr($0,7); next}
  /^@@/{split($3,a,","); n=substr(a[1],2)+0; next}
  /^\+/{
    l=substr($0,2); r=""
    if (l ~ /-----BEGIN [A-Z ]*PRIVATE KEY-----/) r="private-key-pem"
    else if (l ~ /ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{50,}|gho_[A-Za-z0-9]{36}/) r="github-token"
    else if (l ~ /xox[abposr]-[A-Za-z0-9-]{10,}/) r="slack-token"
    else if (l ~ /AKIA[0-9A-Z]{16}/) r="aws-key"
    else if (l ~ /sk-(proj|ant|svcacct|or-v1)-[A-Za-z0-9_-]{20,}/) r="llm-api-key"
    else if (l ~ /[Pp]rivate[Kk]ey *= *[A-Za-z0-9+\/]{43}=/) r="wireguard-private-key"
    if (r != "") print f, n, r
    n++; next }
  /^ /{n++}')
if [ -n "$hits" ]; then
  printf 'secret-guard: secret-like content (path:line:rule):\n%s\n' "$hits" >&2
  fail=1
fi
if [ "$fail" -ne 0 ]; then
  echo "secret-guard: commit/push refused. Keep keys and env in the store (/etc/op-dbus/secrets), never in git. Do not bypass with --no-verify." >&2
  exit 1
fi
exit 0
