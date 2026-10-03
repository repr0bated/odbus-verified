# Source this file to make the ordinary Cargo command delegate to a cheap worker.
# Explicit overrides: CARGO_CALLER_FAMILY=gpt|grok|claude.
_opdbus_cargo_dispatch=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/cargo-dispatch.sh
export OPDBUS_CARGO_DISPATCH="$_opdbus_cargo_dispatch"
unset _opdbus_cargo_dispatch
cargo() { "$OPDBUS_CARGO_DISPATCH" "$@"; }
export -f cargo
