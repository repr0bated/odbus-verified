# Sourced by the existing Golden command. Only opblob writes catalog bytes.
CATALOG_LIVE=/dev/shm/opdbus/plugin-blobs
CATALOG_PERSISTED=/var/lib/opdbus/plugin-blobs
catalog_identity() {
    if [[ ! -e $1 && ! -L $1 ]]; then printf 'absent\n'; return; fi
    [[ -d $1 && ! -L $1 ]] || return 1
    stat -c '%d:%i' "$1"
}
catalog_files() {
    local directory=$1 path name sha files='{}'
    [[ -d $directory && ! -L $directory && -f $directory/.manifest.json && ! -L $directory/.manifest.json ]] || return 1
    while IFS= read -r -d '' path; do
        [[ -f $path && ! -L $path ]] || return 1
        name=${path##*/}; sha=$(sha256sum "$path" | cut -d' ' -f1) || return 1
        files=$(jq -c --arg name "$name" --arg sha "$sha" '.+{($name):$sha}' <<<"$files") || return 1
    done < <(find "$directory" -maxdepth 1 \( -name '*.blob' -o -name .manifest.json \) -print0 | sort -z)
    jq -cS . <<<"$files"
}
catalog_uuid() {
    local uuid
    uuid=$(btrfs subvolume show "$1" | awk '$1=="UUID:" {print $2; exit}') || return 1
    [[ $uuid =~ ^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$ ]] || return 1
    printf '%s\n' "$uuid"
}
# The coordinated publisher preserves exchanged directories. Reconcile actual
# device/inode identities AND complete bytes even if it reports an error.
catalog_copy() {
    local source=$1 destination=$2 label=$3 expected stage before stage_id op rc actual effect
    expected=$(catalog_files "$source") || return 1
    before=$(catalog_identity "$destination") || return 1
    mkdir -p -m 0700 "$CATALOG_OPERATIONS"
    op=$(mktemp -d "$CATALOG_OPERATIONS/$label.XXXXXX") || return 1
    stage=$(mktemp -d "${destination%/*}/.opblob-golden.XXXXXX") || return 1
    stage_id=$(catalog_identity "$stage") || return 1
    jq -n --arg source "$source" --arg destination "$destination" --arg stage "$stage" \
      --arg before "$before" --arg stage_id "$stage_id" --argjson files "$expected" \
      '{source:$source,destination:$destination,stage:$stage,before_identity:$before,stage_identity:$stage_id,expected_files:$files}' >"$op/intent.json" || return 1
    sync -f "$op/intent.json" && sync -f "$op" && sync -f "${destination%/*}" || return 1
    if env OPDBUS_RELEASE_CATALOG_STAGE="$stage" "$CATALOG_OPBLOB" persist "$source" "$destination" >"$op/publisher.log" 2>&1; then rc=0; else rc=$?; fi
    effect=unreconciled
    if [[ $(catalog_identity "$destination") == "$stage_id" && $(catalog_identity "$stage") == "$before" ]] &&
       actual=$(catalog_files "$destination") && [[ $actual == "$expected" ]] && sync -f "${destination%/*}"; then effect=published;
    elif [[ $(catalog_identity "$destination") == "$before" && $(catalog_identity "$stage") == "$stage_id" ]]; then effect=not-published; fi
    jq -n --arg effect "$effect" --argjson exit "$rc" '{effect:$effect,publisher_exit:$exit}' >"$op/result.json" || return 1
    sync -f "$op/result.json" && sync -f "$op" || return 1
    [[ $effect == published ]]
}
catalog_select_before() {
    local selection=$1 state=$2 data receipt expected uuid
    [[ -f $selection && ! -L $selection ]] || return 1
    data=$(jq -er .directory "$selection") || return 1
    [[ $data == "$ROOT/"catalog-before.*/data && $(realpath -e "$data") == "$data" ]] || return 1
    [[ $(btrfs property get -ts "$data" ro) == ro=true ]] || return 1
    uuid=$(catalog_uuid "$data") || return 1
    [[ $uuid == "$(jq -er .uuid "$selection")" ]] || return 1
    receipt=$data/receipt.json
    [[ -f $receipt && ! -L $receipt && $(act_sha "$receipt") == "$(jq -er .receipt_sha256 "$selection")" ]] || return 1
    CATALOG_OPBLOB=$LIVE$NATIVE_ARTIFACT_DIR/opblob
    [[ -f $CATALOG_OPBLOB && ! -L $CATALOG_OPBLOB && -x $CATALOG_OPBLOB ]] || return 1
    jq -e --arg state "$(act_sha "$state")" --arg publisher "$(act_sha "$CATALOG_OPBLOB")" \
      --arg manifest "$(jq -er .catalog_manifest_sha256 "$LIVE/opt/op-build/catalog-seal-receipt.json")" \
      --arg source "$(jq -er .source_digest "$LIVE/opt/op-build/execution-receipt.json")" \
      '.version==1 and .state_sha256==$state and .publisher_sha256==$publisher and .candidate_catalog_manifest_sha256==$manifest and .candidate_source_digest==$source' "$receipt" >/dev/null || return 1
    for expected in live persisted; do
        [[ $(catalog_files "$data/$expected") == "$(jq -cS --arg k "$expected" '.before[$k].files' "$receipt")" ]] || return 1
    done
    CATALOG_BEFORE_DATA=$data
}
catalog_current_known() {
    local kind=$1 destination=$2 actual before candidate
    actual=$(catalog_files "$destination") || return 1
    before=$(jq -cS --arg k "$kind" '.before[$k].files' "$CATALOG_BEFORE_DATA/receipt.json") || return 1
    candidate=$(catalog_files "$LIVE/usr/share/op-dbus/plugin-blobs") || return 1
    [[ $actual == "$before" || $actual == "$candidate" ]]
}
do_catalog_prepare() {
    if [[ $APPLY != 1 ]]; then would 'opblob-capture complete old live/persisted catalogs into a new immutable catalog-before receipt before publication'; return; fi
    require_lifecycle_lock; assert_no_cargo; tx_no_pending
    [[ -f $CANDIDATE_STATE && -f $STATE ]] || die 'cut a candidate before catalog-prepare'
    ACT_STATE=$CANDIDATE_STATE; LIVE=$(jq -er .live.path "$CANDIDATE_STATE")
    require_readonly_release "$LIVE"; verify_release_inputs "$LIVE"
    if catalog_select_before "$ROOT/golden-catalog-before.json" "$STATE"; then
        catalog_current_known live "$CATALOG_LIVE" && catalog_current_known persisted "$CATALOG_PERSISTED" || die 'catalog changed outside retained old/new inputs'
        return
    fi
    local dir data live_files persisted_files tmp
    live_files=$(catalog_files "$CATALOG_LIVE") || die 'old live catalog is unavailable'
    persisted_files=$(catalog_files "$CATALOG_PERSISTED") || die 'old persisted catalog is unavailable'
    dir=$(mktemp -d "$ROOT/catalog-before.XXXXXX"); chmod 0700 "$dir"
    data=$dir/data
    btrfs subvolume create "$data" >/dev/null; chmod 0700 "$data"
    CATALOG_OPBLOB=$LIVE$NATIVE_ARTIFACT_DIR/opblob; CATALOG_OPERATIONS=$dir/operations
    catalog_copy "$CATALOG_LIVE" "$data/live" capture-live || die 'old live catalog capture failed; preserve capture journal'
    catalog_copy "$CATALOG_PERSISTED" "$data/persisted" capture-persisted || die 'old persisted catalog capture failed; preserve capture journal'
    [[ $(catalog_files "$CATALOG_LIVE") == "$live_files" && $(catalog_files "$data/live") == "$live_files" &&
       $(catalog_files "$CATALOG_PERSISTED") == "$persisted_files" && $(catalog_files "$data/persisted") == "$persisted_files" ]] || die 'catalog changed while capturing old inputs'
    jq -n --arg state "$(act_sha "$STATE")" --arg publisher "$(act_sha "$CATALOG_OPBLOB")" \
      --arg manifest "$(jq -er .catalog_manifest_sha256 "$LIVE/opt/op-build/catalog-seal-receipt.json")" \
      --arg source "$(jq -er .source_digest "$LIVE/opt/op-build/execution-receipt.json")" \
      --argjson live "$live_files" --argjson persisted "$persisted_files" \
      '{version:1,state_sha256:$state,publisher_sha256:$publisher,candidate_catalog_manifest_sha256:$manifest,candidate_source_digest:$source,before:{live:{files:$live},persisted:{files:$persisted}}}' >"$data/receipt.json"
    sync -f "$data/receipt.json"; sync -f "$data"
    btrfs property set -ts "$data" ro true
    [[ $(btrfs property get -ts "$data" ro) == ro=true ]] || die 'catalog-before did not seal'
    tmp=$(mktemp "$ROOT/.catalog-before.XXXXXX"); chmod 0600 "$tmp"
    jq -n --arg directory "$data" --arg uuid "$(catalog_uuid "$data")" --arg sha "$(act_sha "$data/receipt.json")" \
      '{directory:$directory,uuid:$uuid,receipt_sha256:$sha}' >"$tmp"
    sync -f "$tmp"; mv -T "$tmp" "$ROOT/golden-catalog-before.json"; sync -f "$ROOT"
    log "immutable catalog-before: $data/receipt.json"
}
tx_validate_catalog() {
    catalog_select_before "$ROOT/golden-catalog-before.json" "$STATE" || die 'immutable catalog-before receipt required; run catalog-prepare BEFORE any live publication'
    catalog_current_known live "$CATALOG_LIVE" && catalog_current_known persisted "$CATALOG_PERSISTED" || die 'live/persisted catalog differs from both retained release inputs'
}
tx_catalog_publish() {
    CATALOG_OPERATIONS=$TXN/catalog-operations
    catalog_copy "$LIVE/usr/share/op-dbus/plugin-blobs" "$CATALOG_PERSISTED" publish-persisted || return 1
    catalog_copy "$CATALOG_PERSISTED" "$CATALOG_LIVE" publish-live
}
tx_catalog_restore() {
    [[ -f $TXN/catalog-attempted ]] || return 0
    catalog_select_before "$TXN/catalog-selection.json" "$TXN/state.before" || return 1
    catalog_current_known live "$CATALOG_LIVE" && catalog_current_known persisted "$CATALOG_PERSISTED" || return 1
    local kind destination expected
    CATALOG_OPERATIONS=$TXN/catalog-operations
    for kind in persisted live; do
        case $kind in persisted) destination=$CATALOG_PERSISTED;; live) destination=$CATALOG_LIVE;; esac
        expected=$(jq -cS --arg k "$kind" '.before[$k].files' "$CATALOG_BEFORE_DATA/receipt.json") || return 1
        [[ $(catalog_files "$destination") != "$expected" ]] || continue
        catalog_copy "$CATALOG_BEFORE_DATA/$kind" "$destination" "restore-$kind" || return 1
    done
}
