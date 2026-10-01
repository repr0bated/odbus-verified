#!/bin/sh
# Exercise the actual helper invocation with a multiline value containing '='.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
helper=$repo/deploy/runit/libexec-3tched/op-identity-root
seed_name=fixture
hook_selection='lxc.hook.version = 1
lxc.hook.start-host = /fixture/release/libexec/3tched/op-identity-lungs-hook'
invocation=$(awk '/^[[:space:]]*incus config set .*raw.lxc/ {print; n++} END {if (n != 1) exit 1}' "$helper")
called=false
incus() {
    [ "$#" -eq 4 ]
    [ "$1" = config ] && [ "$2" = set ] && [ "$3" = "$seed_name" ]
    [ "$4" = "raw.lxc=$hook_selection" ]
    called=true
}
# Only the repository's trusted command line is evaluated; input is expanded
# as quoted data, never evaluated as code.
eval "$invocation"
[ "$called" = true ]
printf '%s\n' 'PASS: hook config is a single modern key=value argument'
