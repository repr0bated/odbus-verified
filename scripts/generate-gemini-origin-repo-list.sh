#!/usr/bin/env bash
set -euo pipefail

repo_root="${1:-/home/jeremy/git}"

declare -A canonical_uri_by_uri=()
declare -A source_kind_by_uri=()
declare -A source_host_by_uri=()
declare -A owner_repo_by_uri=()
declare -A sources_by_uri=()
declare -a uris=()

normalize_origin_uri() {
    local raw="$1"

    case "$raw" in
        https://github.com/*)
            raw="${raw%.git}"
            printf 'github|github.com|%s.git\n' "$raw"
            ;;
        git@github.com:*)
            raw="${raw#git@github.com:}"
            raw="${raw%.git}"
            printf 'github|github.com|https://github.com/%s.git\n' "$raw"
            ;;
        https://gitlab.com/*)
            raw="${raw%.git}"
            printf 'gitlab|gitlab.com|%s.git\n' "$raw"
            ;;
        git@gitlab.com:*)
            raw="${raw#git@gitlab.com:}"
            raw="${raw%.git}"
            printf 'gitlab|gitlab.com|https://gitlab.com/%s.git\n' "$raw"
            ;;
        https://community.opengroup.org/*)
            raw="${raw%.git}"
            printf 'gitlab-self-hosted|community.opengroup.org|%s.git\n' "$raw"
            ;;
        git@community.opengroup.org:*)
            raw="${raw#git@community.opengroup.org:}"
            raw="${raw%.git}"
            printf 'gitlab-self-hosted|community.opengroup.org|https://community.opengroup.org/%s.git\n' "$raw"
            ;;
        *)
            return 1
            ;;
    esac
}

link_name_from_uri() {
    local uri="$1"
    local without_scheme="${uri#https://}"
    local without_suffix="${without_scheme%.git}"
    printf '%s\n' "$without_suffix" \
        | tr '[:upper:]' '[:lower:]' \
        | tr '/._' '-' \
        | tr -cs 'a-z0-9-' '-' \
        | sed 's/^-*//; s/-*$//'
}

owner_repo_from_uri() {
    local uri="$1"
    case "$uri" in
        https://github.com/*)
            printf '%s\n' "${uri#https://github.com/}" | sed 's/\.git$//'
            ;;
        https://gitlab.com/*)
            printf '%s\n' "${uri#https://gitlab.com/}" | sed 's/\.git$//'
            ;;
        https://community.opengroup.org/*)
            printf '%s\n' "${uri#https://community.opengroup.org/}" | sed 's/\.git$//'
            ;;
        *)
            printf ''
            ;;
    esac
}

append_source() {
    local uri="$1"
    local source="$2"
    if [[ -n "${sources_by_uri[$uri]:-}" ]]; then
        sources_by_uri["$uri"]+=$'\n'
    fi
    sources_by_uri["$uri"]+="$source"
}

indexability_for_kind() {
    local source_kind="$1"
    if [[ "$source_kind" == "github" ]]; then
        printf 'true|Indexable with the current GitHub Developer Connect connection.'
    else
        printf 'false|Not indexable with the current GitHub-only Developer Connect connection.'
    fi
}

for repo_dir in "$repo_root"/*; do
    [[ -d "$repo_dir/.git" ]] || continue

    clone_uri_raw="$(git -C "$repo_dir" remote get-url origin 2>/dev/null || true)"
    [[ -n "$clone_uri_raw" ]] || continue

    if ! normalized="$(normalize_origin_uri "$clone_uri_raw")"; then
        continue
    fi

    source_kind="${normalized%%|*}"
    remainder="${normalized#*|}"
    source_host="${remainder%%|*}"
    canonical_uri="${remainder#*|}"

    if [[ -z "${canonical_uri_by_uri[$canonical_uri]:-}" ]]; then
        canonical_uri_by_uri["$canonical_uri"]="$canonical_uri"
        source_kind_by_uri["$canonical_uri"]="$source_kind"
        source_host_by_uri["$canonical_uri"]="$source_host"
        owner_repo_by_uri["$canonical_uri"]="$(owner_repo_from_uri "$canonical_uri")"
        uris+=("$canonical_uri")
    fi

    append_source "$canonical_uri" "$(basename "$repo_dir")"
done

printf '[\n'
for i in "${!uris[@]}"; do
    uri="${uris[$i]}"
    source_kind="${source_kind_by_uri[$uri]}"
    source_host="${source_host_by_uri[$uri]}"
    owner_repo="${owner_repo_by_uri[$uri]}"
    link_name="$(link_name_from_uri "$uri")"
    indexability="$(indexability_for_kind "$source_kind")"
    indexable_with_current_connection="${indexability%%|*}"
    indexing_notes="${indexability#*|}"

    printf '  {\n'
    printf '    "link_name": "%s",\n' "$link_name"
    printf '    "source_kind": "%s",\n' "$source_kind"
    printf '    "source_host": "%s",\n' "$source_host"
    printf '    "owner_repo": "%s",\n' "$owner_repo"
    printf '    "clone_uri": "%s",\n' "$uri"
    printf '    "indexable_with_current_connection": %s,\n' "$indexable_with_current_connection"
    printf '    "indexing_notes": "%s",\n' "$indexing_notes"
    printf '    "local_sources": ['

    mapfile -t local_sources < <(printf '%s\n' "${sources_by_uri[$uri]}" | sort -u)
    for j in "${!local_sources[@]}"; do
        [[ "$j" -gt 0 ]] && printf ', '
        printf '"%s"' "${local_sources[$j]}"
    done

    printf ']\n'
    if [[ "$i" -eq $((${#uris[@]} - 1)) ]]; then
        printf '  }\n'
    else
        printf '  },\n'
    fi
done
printf ']\n'
