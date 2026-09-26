#!/usr/bin/env bash
# Export the reproducible OP-DBUS source closure.
set -Eeuo pipefail

usage() { echo "usage: $0 SOURCE DEST" >&2; exit 2; }
[ "$#" -eq 2 ] || usage
source_dir=$(realpath "$1")
dest_dir=$(realpath -m "$2")
[ -f "$source_dir/Cargo.toml" ] || { echo "source has no Cargo.toml: $source_dir" >&2; exit 1; }
[ ! -e "$dest_dir" ] && [ ! -L "$dest_dir" ] || { echo "destination already exists: $dest_dir" >&2; exit 1; }
case "$dest_dir/" in "$source_dir/"*) echo "destination must be outside source" >&2; exit 1;; esac

# Cargo metadata is authoritative for active package roots and target sources.
metadata=$(cd "$source_dir" && cargo metadata --locked --no-deps --format-version 1)
mapfile -t package_manifests < <(printf '%s' "$metadata" | jq -r '.packages[].manifest_path')
[ "${#package_manifests[@]}" -gt 0 ] || { echo "Cargo metadata returned no packages" >&2; exit 1; }
for manifest in "${package_manifests[@]}"; do
    case "$manifest" in "$source_dir/"*) ;; *) echo "external package manifest: $manifest" >&2; exit 1;; esac
    [ -f "$manifest" ] || { echo "metadata target missing: $manifest" >&2; exit 1; }
done
mapfile -t target_sources < <(printf '%s' "$metadata" | jq -r '.packages[].targets[]?.src_path')
for target in "${target_sources[@]}"; do
    [ -f "$target" ] || { echo "metadata target source missing: $target" >&2; exit 1; }
done

stage=$(mktemp -d "${TMPDIR:-/tmp}/odbus-verified.XXXXXX")
finished=0
cleanup() { status=$?; if [ "$finished" -eq 0 ]; then rm -rf -- "$stage"; fi; exit "$status"; }
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

excluded() {
    case "$1" in
        .git/*|target/*|*/target/*|*/node_modules/*|*/.venv/*|*/__pycache__/*) return 0;;
        .antigravitycli/*|.codex/*|.gemini/*|.cache/*|.local/*|*/.antigravitycli/*|*/.codex/*|*/.gemini/*) return 0;;
        docs/conversation-*|docs/.consolidation-staging/*|docs/inspector-gadget/*) return 0;;
        */all-specs-archive/*|*/logs/*|*/sessions/*|docs/*repomix*|*.blob|*.sqlite|*.sqlite3) return 0;;
        crates/op-waypipe-grpc/*|crates/op-grpc-adapters/*) return 0;;
        # Historical provider setup material contains credential-shaped literals
        # and is not part of the active build, runtime, or verification closure.
        scripts/antigravity-full-setup.md|scripts/antigravity-manager-explained.md|scripts/complete-antigravity-setup.sh|scripts/configure-antigravity-gemini.md|scripts/configure-ide-user-agent.md|scripts/quick-test-gemini-proxy.sh|scripts/setup-gemini-api-proxy.sh|scripts/setup-gemini-in-antigravity.md|scripts/test-antigravity-gemini.py|scripts/vectorize_rust_repos.py|scripts/get-cloudcode-credentials.sh) return 0;;
        .env|.env.*|*/.env|*/.env.*|*.env|*.pem|*.key|*.p12|*.pfx|*/xray_config.json|*/mcp-config.json) return 0;;
        *) return 1;;
    esac
}

copy_one() {
    rel=$1
    excluded "$rel" && return 0
    src=$source_dir/$rel
    [ -e "$src" ] || [ -L "$src" ] || { echo "selected input missing: $rel" >&2; exit 1; }
    mkdir -p "$stage/$(dirname "$rel")"
    if [ -L "$src" ]; then
        resolved=$(realpath "$src")
        case "$resolved/" in
            "$source_dir/"*) ;;
            *) echo "escaping symlink in selected input: $rel -> $resolved" >&2; exit 1;;
        esac
        target_rel=${resolved#"$source_dir/"}
        excluded "$target_rel" && { echo "symlink targets excluded input: $rel -> $target_rel" >&2; exit 1; }
        # Absolute links inside the original checkout must become relative
        # links inside the copy. Final validation also requires their targets.
        link_target=$(realpath --relative-to="$(dirname "$src")" "$resolved")
        ln -sfn -- "$link_target" "$stage/$rel"
    else
        cp -a -- "$src" "$stage/$rel"
    fi
}

for rel in Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml clippy.toml build.rs .gitignore AGENTS.md README.md LICENSE LICENSE-APACHE LICENSE-MIT SIGNALS.md WISHLIST.md; do
    [ -e "$source_dir/$rel" ] && copy_one "$rel"
done
for rel in .agents/skills/grpc-expert/SKILL.md .agents/skills/grpc-protocol-expert/SKILL.md \
           .agents/skills/json-render/SKILL.md .agents/skills/ovs-db-analysis/SKILL.md; do
    copy_one "$rel"
done

declare -A seen=()
for manifest in "${package_manifests[@]}"; do
    rel_manifest=${manifest#"$source_dir/"}
    package_root=$(dirname "$rel_manifest")
    # The workspace root is also a package. Never use it as a recursive
    # source root: that would import arbitrary private/ignored host material.
    [ "$package_root" = . ] && continue
    [ -n "${seen[$package_root]+x}" ] && continue
    seen[$package_root]=1
    while IFS= read -r -d '' src; do
        copy_one "${src#"$source_dir/"}"
    done < <(find "$source_dir/$package_root" \
        \( -path '*/target' -o -path '*/node_modules' -o -path '*/.venv' -o -path '*/__pycache__' \
           -o -path '*/.git' -o -path '*/waypipe*' -o -path '*/.antigravitycli' \
           -o -path '*/.codex' -o -path '*/.gemini' -o -path '*/.cache' -o -path '*/.local' \) -prune -o \
        \( -type f -o -type l \) -print0)
done

for root in src examples .cargo deploy docs scripts vendor; do
    [ -d "$source_dir/$root" ] || continue
    while IFS= read -r -d '' src; do
        copy_one "${src#"$source_dir/"}"
    done < <(find "$source_dir/$root" \
        \( -path '*/target' -o -path '*/node_modules' -o -path '*/.venv' -o -path '*/__pycache__' \
           -o -path '*/.git' -o -path '*/waypipe*' -o -path '*/.antigravitycli' \
           -o -path '*/.codex' -o -path '*/.gemini' -o -path '*/.cache' -o -path '*/.local' \) -prune -o \
        \( -type f -o -type l \) -print0)
done

for target in "${package_manifests[@]}" "${target_sources[@]}"; do
    rel=${target#"$source_dir/"}
    [ -f "$stage/$rel" ] || { echo "export omits required Cargo input: $rel" >&2; exit 1; }
done
while IFS= read -r -d '' link; do
    resolved=$(realpath -e "$link") || { echo "export contains dangling link: ${link#"$stage/"}" >&2; exit 1; }
    case "$resolved/" in "$stage/"*) ;; *) echo "export link escapes staging: ${link#"$stage/"}" >&2; exit 1;; esac
done < <(find "$stage" -type l -print0)

# Admission guard: reject recognizable provider/GitHub token prefixes and
# private-key blocks without printing matching content. Runtime secret reads,
# empty templates, and ordinary fixture credentials remain allowed.
credential_literal_gate() {
    local found=0 file
    command -v rg >/dev/null 2>&1 || {
        printf '%s\n' 'credential gate requires rg' >&2
        return 2
    }
    while IFS= read -r -d '' file; do
        set +e
        rg -q -I -e 'sk-(ant|or|proj)-[A-Za-z0-9._~-]{16,}' \
            -e 'sk-[a-f0-9]{32}' \
            -e 'pa-[A-Za-z0-9._~-]{16,}' \
            -e 'gh[pousr]_[A-Za-z0-9_]{16,}' \
            -e 'github_pat_[A-Za-z0-9_]{16,}' \
            -e 'AIza[A-Za-z0-9_-]{20,}' \
            -e '-----BEGIN ((OPENSSH|RSA|EC|DSA|ENCRYPTED) PRIVATE|PRIVATE) KEY-----' "$file"
        local status=$?
        set -e
        case "$status" in
          0)
            printf 'credential-shaped literal in exported input: %s\n' "${file#"$stage/"}" >&2
            found=1
            ;;
          1) ;;
          *)
            printf 'credential gate could not inspect exported input: %s\n' "${file#"$stage/"}" >&2
            return 2
            ;;
        esac
    done < <(find "$stage" -type f -print0)
    return "$found"
}
credential_literal_gate

# -T avoids nesting into a destination created concurrently; -n prevents
# replacement. A skipped move leaves the stage present and is a failure.
mv -Tn -- "$stage" "$dest_dir"
[ ! -d "$stage" ] || { echo "destination appeared during export: $dest_dir" >&2; exit 1; }
finished=1
trap - EXIT
printf '%s\n' "exported source closure to $dest_dir"
printf '%s\n' "pinned active UI source artifact: vendor/3tched-json-render-ui/source.tar.gz"
printf '%s\n' "UI source/build verifier: vendor/3tched-json-render-ui/build-assets.sh"
printf '%s\n' "UI build output is separate until its manifest is admitted to the golden snapshot"
printf '%s\n' "pinned tched_router source and patch recipe: vendor/tched-router/build-native.sh"
printf '%s\n' "native build output is separate until its manifest is admitted to the golden snapshot"
printf '%s\n' "this export is source input, not a verified build or deployment"
