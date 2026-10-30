#!/bin/sh
# Sourced by build-golden.sh; uses its bounded install_file/remove_owned helpers.
# Reads prepared artifacts and source inputs only, never live destinations.
validate_system_inputs() {
    for input in "$RELEASE_DIR/op-release" \
        "$PROJECT_ROOT/deploy/release/op-release-launch" \
        "$PROJECT_ROOT/deploy/release/op-release-activate"; do
        [ -f "$input" ] && [ ! -L "$input" ] && [ -r "$input" ] ||
            die "missing or non-regular system payload input: $input"
    done
}

clear_system_payload() {
    # Remove the declaration first so interrupted preparation cannot retain an
    # apparently usable old manifest. These paths are preparation-owned only.
    for old in "$GOLDEN_DIR/system-files.json" "$GOLDEN_DIR/system"; do
        if [ -e "$old" ] || [ -L "$old" ]; then
            remove_owned "$old"
        fi
    done
}

stage_system_payload() (
    validate_system_inputs
    clear_system_payload
    if [ "$DRY_RUN" = 1 ]; then
        printf '%s\n' '  would stage four explicit system files and their digest manifest'
        exit 0
    fi
    scratch=$(mktemp -d "$GOLDEN_DIR/.system-payload.XXXXXX")
    trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM
    entries=$scratch/entries.jsonl
    kit_manifest=$scratch/manifest.sha256
    : > "$entries"
    : > "$kit_manifest"
    stage_entry() {
        mode=$1 input=$2 source=$3 destination=$4 absent=$5
        install_file "$mode" "$input" "$GOLDEN_DIR/$source"
        digest=$(sha256sum "$GOLDEN_DIR/$source")
        digest=${digest%% *}
        jq -n --arg source "$source" --arg path "$destination" \
            --arg sha256 "$digest" --argjson mode "$((mode))" \
            --argjson absent "$absent" \
            '{source:$source,path:$path,sha256:$sha256,mode:$mode} +
             (if $absent then {install_if_absent:true} else {} end)' >> "$entries"
    }
    stage_kit_file() {
        stage_entry 0755 "$1" "system/release/$2" "$3" false
        printf '%s  %s\n' "$digest" "$3" >> "$kit_manifest"
    }
    stage_kit_file "$RELEASE_DIR/op-release" op-release \
        /usr/local/libexec/opdbus-release/op-release
    stage_kit_file "$PROJECT_ROOT/deploy/release/op-release-launch" op-release-launch \
        /usr/local/libexec/opdbus-release/op-release-launch
    stage_kit_file "$PROJECT_ROOT/deploy/release/op-release-activate" op-release-activate \
        /etc/init.d/op-release-activate
    # Publish authorization bytes last in the native transaction's apply order.
    stage_entry 0600 "$kit_manifest" system/release/manifest.sha256 \
        /usr/local/libexec/opdbus-release/manifest.sha256 false
    jq -s '{version:1,files:.}' "$entries" > "$scratch/system-files.json"
    install_file 0644 "$scratch/system-files.json" "$GOLDEN_DIR/system-files.json"
)
