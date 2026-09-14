#!/usr/bin/env bash
# Completion checks for zcall against the live D-Bus tree.
#
# Requires the plugin tree to be up (org.opdbus.v1.plugins on
# unix:path=/run/opdbus/session-bus.sock) and the sealed blob catalog present.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$repo_root/bin:$PATH"

source "$repo_root/completions/zcall.bash"

get_completions() {
    local line="$1"
    local -a words=()

    COMP_LINE="$line"
    COMP_POINT=${#COMP_LINE}

    # shellcheck disable=SC2206
    words=($line)
    if [[ "${line: -1}" == " " ]]; then
        words+=("")
    fi

    COMP_WORDS=("${words[@]}")
    COMP_CWORD=$((${#COMP_WORDS[@]} - 1))
    COMPREPLY=()
    _zcall
    printf '%s\n' "${COMPREPLY[@]}" | LC_ALL=C sort
}

assert_lines() {
    local name="$1"
    local actual="$2"
    local expected="$3"

    if [[ "$actual" != "$expected" ]]; then
        printf 'FAIL %s\nexpected:\n%s\nactual:\n%s\n' "$name" "$expected" "$actual" >&2
        exit 1
    fi
    printf 'PASS %s\n' "$name"
}

plugins="$(zcall --complete plugins)"
subcommands="$(zcall --complete subcommands)"
top_level="$(printf '%s\n%s\n' "$plugins" "$subcommands" | LC_ALL=C sort)"

[[ -n "$plugins" ]] || {
    printf 'SKIP: no plugin objects on the live tree\n' >&2
    exit 0
}

assert_lines \
    "top-level offers tree plugins and subcommands" \
    "$(get_completions 'zcall ')" \
    "$top_level"

assert_lines \
    "prefix a filters the tree" \
    "$(get_completions 'zcall a')" \
    "$(grep '^a' <<<"$top_level")"

assert_lines \
    "unix_socket declared methods" \
    "$(get_completions 'zcall unix-socket ')" \
    $'accept\nbind\nclose\nlisten'

assert_lines \
    "help completes methods" \
    "$(get_completions 'zcall help unix-socket ')" \
    $'accept\nbind\nclose\nlisten'

assert_lines \
    "methods subcommand completes plugins" \
    "$(get_completions 'zcall methods unix')" \
    "$(grep '^unix' <<<"$plugins")"

assert_lines \
    "bind args" \
    "$(get_completions 'zcall unix-socket bind ')" \
    $'--name\n--path\n--ports\n--protocol'

assert_lines \
    "bind arg prefix" \
    "$(get_completions 'zcall unix-socket bind --p')" \
    $'--path\n--ports\n--protocol'

assert_lines \
    "expand shifts one position" \
    "$(get_completions 'zcall expand unix-socket ')" \
    $'accept\nbind\nclose\nlisten'
