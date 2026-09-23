#!/bin/sh
set -eu

package_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
base=${1:?usage: $0 BASE-DIR MODIFIED-DIR}
modified=${2:?usage: $0 BASE-DIR MODIFIED-DIR}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM

existing='Cargo.lock
crates/zeroclaw-api/src/model_provider.rs
crates/zeroclaw-gateway/src/a2a.rs
crates/zeroclaw-gateway/src/api_sections.rs
crates/zeroclaw-gateway/Cargo.toml
crates/zeroclaw-gateway/src/lib.rs
crates/zeroclaw-gateway/src/sse.rs
crates/zeroclaw-gateway/src/ws.rs
crates/zeroclaw-providers/src/anthropic.rs
crates/zeroclaw-providers/src/compatible.rs
crates/zeroclaw-providers/src/gemini.rs
crates/zeroclaw-providers/src/lib.rs
crates/zeroclaw-providers/src/ollama.rs
crates/zeroclaw-providers/src/openai.rs
crates/zeroclaw-providers/src/openrouter.rs
crates/zeroclaw-providers/src/vision_override.rs
crates/zeroclaw-runtime/src/quickstart/mod.rs'
new='crates/zeroclaw-gateway/src/ws_stateless.rs
crates/zeroclaw-gateway/src/ws_stateless_tests.rs
crates/zeroclaw-providers/src/opencode_session.rs
crates/zeroclaw-providers/src/sled_identity.rs
crates/zeroclaw-providers/src/stateless.rs'

printf '%s\n' "$existing" | while IFS= read -r path; do
    test -f "$base/$path" || { echo "missing base path: $path" >&2; exit 2; }
    test -f "$modified/$path" || { echo "missing modified path: $path" >&2; exit 2; }
    mkdir -p "$work/$(dirname "$path")"
    cp "$base/$path" "$work/$path"
done
printf '%s\n' "$new" | while IFS= read -r path; do
    test -f "$modified/$path" || { echo "missing new modified path: $path" >&2; exit 2; }
    test ! -e "$base/$path" && test ! -L "$base/$path" || { echo "new path exists in base: $path" >&2; exit 2; }
done
patch --batch --fuzz=0 -d "$work" -p1 < "$package_dir/patches/0001-close-native-event-stream-on-lag.patch" >/dev/null

patch_out="$work/0002-stateless-provider-generation.patch"
manifest_out="$work/base-sha256sums"
inventory_out="$work/patch-inventory"
: > "$patch_out"
printf '%s\n' "$existing" | while IFS= read -r path; do
    diff -u --label "a/$path" --label "b/$path" "$work/$path" "$modified/$path" >> "$patch_out" || test $? -eq 1
done
printf '%s\n' "$new" | while IFS= read -r path; do
    diff -u --label "a/$path" --label "b/$path" /dev/null "$modified/$path" >> "$patch_out" || test $? -eq 1
done
# GNU patch accepts an empty physical line for blank context. Normalize those
# lines so the checked-in patch file itself has no trailing whitespace.
sed 's/^ $//' "$patch_out" > "$patch_out.normalized"
mv "$patch_out.normalized" "$patch_out"

printf '%s\n' "$existing" | while IFS= read -r path; do
    (cd "$base" && sha256sum "$path")
done > "$manifest_out"
printf '%s\n' '# Existing files whose pinned-base contents are verified before patching.' > "$inventory_out"
printf '%s\n' "$existing" >> "$inventory_out"
printf '%s\n' '# New files that must be absent before patching.' >> "$inventory_out"
printf '%s\n' "$new" >> "$inventory_out"
cp "$patch_out" "$package_dir/patches/0002-stateless-provider-generation.patch"
cp "$manifest_out" "$package_dir/base-sha256sums"
cp "$inventory_out" "$package_dir/patch-inventory"
