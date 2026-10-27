#!/usr/bin/env bash
# Prepare public wheels and guest assets only. No guest execution or credentials.
set -euo pipefail
umask 022
fail() { printf 'build-litellm-runtime: %s\n' "$*" >&2; exit 1; }
[[ $# == 1 && $(id -u) != 0 ]] || fail 'usage (unprivileged): NEW_OUTPUT_DIRECTORY'
output=$1
[[ $output == /* && ! -e $output && ! -L $output ]] || fail 'output must be a new absolute path'
assets=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/litellm" && pwd -P)
uv_bin=$(command -v uv) || fail 'uv is required'
mkdir -m 0700 -- "$output"
output=$(realpath -e "$output")
mkdir -- "$output/runtime"
# Empty environment prevents personal indexes, keys and package settings leaking
# into resolution. --target leaves both host and guest interpreters untouched.
# The generated entrypoints name /usr/bin/python3, also the guest interpreter.
env -i PATH=/usr/bin:/bin "$uv_bin" --no-config pip install \
    --cache-dir "$output/.wheel-cache" --default-index https://pypi.org/simple \
    --python /usr/bin/python3 --python-version 3.14 \
    --python-platform x86_64-manylinux_2_28 --only-binary :all: \
    --require-hashes --link-mode copy --target "$output/runtime" \
    --requirement "$assets/requirements.lock"
[[ -z $(find "$output/runtime" -type l -print -quit) ]] || fail 'unexpected runtime symlink'
[[ -f $output/runtime/litellm-1.105.1.dist-info/METADATA ]] || fail 'pinned LiteLLM absent'
# Public code must remain readable/traversable by the unprivileged guest service,
# even under a restrictive caller umask or wheel mode. Preserve executable files.
chmod -R u+rwX,go+rX,go-w "$output/runtime"
for asset in requirements.in requirements.lock config.yaml litellm.service litellm-key-present; do
    install -m 0644 "$assets/$asset" "$output/$asset"
done
chmod 0755 "$output/litellm-key-present"
# The cache is not part of the payload or its manifest.
rm -rf -- "$output/.wheel-cache"
(
    cd -- "$output"
    find runtime -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum > runtime.sha256
    sha256sum requirements.in requirements.lock config.yaml litellm.service litellm-key-present runtime.sha256 > SHA256SUMS
)
jq -n --arg created_at "$(date -u +%FT%TZ)" \
    --arg uv_version "$("$uv_bin" --version)" \
    --arg manifest_sha256 "$(sha256sum "$output/SHA256SUMS" | cut -d ' ' -f1)" \
    '{format:"opdbus.seed-litellm.v1",litellm_version:"1.105.1",python_version:"3.14",
      python_platform:"x86_64-manylinux_2_28",created_at:$created_at,uv:$uv_version,
      manifest_sha256:$manifest_sha256,credentials_included:false,
      guest_executed:false,activated:false}' > "$output/preparation.json"
printf 'Prepared %s\n' "$output"
