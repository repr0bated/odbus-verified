# Sourced by golden.sh. State is published only after every owner succeeds.
# An interrupted journal blocks another transition until explicit recovery.
tx_phase() {
    local phase=$1 tmp
    tmp=$(mktemp "$ROOT/.transaction.XXXXXX")
    jq -n --arg phase "$phase" --arg directory "$TXN" --arg candidate "$CANDIDATE_STATE" \
      '{version:1,phase:$phase,transaction_dir:$directory,candidate_state:$candidate}' >"$tmp"
    chmod 0600 "$tmp"; sync -f "$tmp"
    mv -T "$tmp" "$ROOT/golden-transaction.json"; sync -f "$ROOT"
}
tx_no_pending() {
    [[ ! -f $ROOT/golden-transaction.json ]] ||
      jq -e '.phase=="committed" or .phase=="rolled-back"' "$ROOT/golden-transaction.json" >/dev/null ||
      die 'unfinished transition; use recover --apply before another deploy'
}
# tx_snapshot_owners [all|projection]: projection journals only the boot opblob
# path and admin-down FUSE confs (the live-release projection verb).
tx_snapshot_owners() {
    local mode=${1:-all} svc bin path health decision detail i=0 dir view old
    if [[ $mode == all ]]; then
        while IFS='|' read -r svc bin path health decision detail; do
            [[ $decision == swap ]] || continue
            dir=$TXN/service-$i; mkdir "$dir"
            path=$ACT_PREFIX$path
            printf '%s\n' "$svc" >"$dir/service"; printf '%s\n' "$path" >"$dir/path"
            printf '%s\n' "$health" >"$dir/health"
            if [[ -L $path ]]; then readlink "$path" >"$dir/link"
            elif [[ -f $path ]]; then cp -p "$path" "$dir/binary"
            elif [[ -e $path ]]; then die 'unsupported prior service target'; fi
            i=$((i+1))
        done < <(act_rows)
    fi
    mkdir "$TXN/opblob"
    path=$ACT_PREFIX$ACT_OPBLOB
    printf '%s\n' "$path" >"$TXN/opblob/path"
    if [[ -L $path ]]; then readlink "$path" >"$TXN/opblob/link"
    elif [[ -e $path ]]; then die "boot opblob is not a golden symlink: $path"
    else touch "$TXN/opblob/absent"; fi
    for view in 3tchedFS threetched; do
        case $view in 3tchedFS) svc=3tched-fs;; *) svc=threetched-fs;; esac
        dir=$ACT_ENABLED_DIR/$svc
        [[ -f $dir/conf ]] || continue
        # Administratively down (runit down file) and unmounted: conf pin only,
        # never started; rollback restores the conf bytes and leaves it down.
        if [[ -e $dir/down ]] && ! mountpoint -q "/run/mounts/$view"; then
            mkdir "$TXN/fuse-$view"
            cp -p "$dir/conf" "$TXN/fuse-$view/conf"
            printf '%s\n' "$dir" >"$TXN/fuse-$view/service"
            touch "$TXN/fuse-$view/admin-down"
            continue
        fi
        [[ $mode == all ]] || continue
        mkdir "$TXN/fuse-$view"
        cp -p "$dir/conf" "$TXN/fuse-$view/conf"
        printf '%s\n' "$dir" >"$TXN/fuse-$view/service"
        timeout 10 jq -ce '.mount_identity | select(.kind=="host_session" or .kind=="guest_kernel")' \
            "/run/mounts/$view/.3tchedfs.json" >"$TXN/fuse-$view/identity.json"
        old=$(sed -n "s/^OPDBUS_FUSE_BINARY='\{0,1\}\([^']*\)'\{0,1\}$/\1/p" "$dir/conf")
        [[ -n $old ]] || old=$(readlink -f /usr/local/libexec/op-dbus/bin/3tched-fs)
        [[ -f $old && -x $old && ! -L $old ]] || die 'prior FUSE binary is unavailable'
        printf '%s\n' "$old" >"$TXN/fuse-$view/binary"
    done
}
tx_mount_prepare() {
    local i target expected options stage
    expected=$(jq -er .live.path "$TXN/state.before")
    for i in 0 1; do
        case $i in 0) target=/run/opdbus-os-golden;; *) target=/run/opdbus-os-live;; esac
        mkdir "$TXN/alias-$i"
        printf '%s\n' "$target" >"$TXN/alias-$i/target"
        if mountpoint -q "$target"; then
            options=$(findmnt -rn -M "$target" -o OPTIONS)
            [[ $(findmnt -rn -M "$target" -o FSTYPE) == overlay && ,$options, == *",lowerdir=$expected,"* ]] ||
                die 'existing OS alias differs from the prior selected lower'
            printf '%s\n' "$expected" >"$TXN/alias-$i/prior-lower"
        else
            [[ ! -d $target || -z $(find "$target" -mindepth 1 -maxdepth 1 -print -quit) ]] || die 'occupied OS alias directory'
            touch "$TXN/alias-$i/was-absent"
        fi
    done
    stage=$(mktemp -d /run/opdbus-os-stage.XXXXXX)
    printf '%s\n' "$stage" >"$TXN/stage"
    mount_one "$stage" "$LIVE"
}
tx_mount_switch() {
    local dir=$TXN/alias-$1 target old stage
    target=$(cat "$dir/target"); stage=$(cat "$TXN/stage")
    if [[ ! -f $dir/was-absent ]]; then
        old=$(mktemp -d /run/opdbus-os-stage.XXXXXX)
        printf '%s\n' "$old" >"$dir/old"
        sync -f "$TXN"
        mount --move "$target" "$old"
    fi
    install -d -m 0755 "$target"
    mount --bind "$stage" "$target"
}
tx_mount_restore() {
    local dir target old failed=0 lower
    for dir in "$TXN"/alias-*; do
        [[ -f $dir/target ]] || continue
        target=$(cat "$dir/target"); old=
        [[ ! -f $dir/old ]] || old=$(cat "$dir/old")
        if [[ -n $old ]] && mountpoint -q "$old"; then
            if mountpoint -q "$target"; then
                lower=$(findmnt -rn -M "$target" -o OPTIONS)
                [[ $(findmnt -rn -M "$target" -o FSTYPE) == overlay && ,$lower, == *",lowerdir=$LIVE,"* ]] || { failed=1; continue; }
                umount "$target" || { failed=1; continue; }
            fi
            mount --move "$old" "$target" || failed=1
        elif [[ -f $dir/was-absent ]]; then
            if mountpoint -q "$target"; then umount "$target" || failed=1; fi
        elif [[ -f $dir/prior-lower ]]; then
            lower=$(cat "$dir/prior-lower")
            [[ $(findmnt -rn -M "$target" -o FSTYPE) == overlay && ,$(findmnt -rn -M "$target" -o OPTIONS), == *",lowerdir=$lower,"* ]] || failed=1
        fi
    done
    return "$failed"
}
tx_quiesce_owners() {
    local mode=${1:-all} dir failed=0
    if [[ $mode == all ]]; then
        touch "$TXN/fuse-attempted"; sync -f "$TXN"
        for dir in "$TXN"/fuse-*; do
            [[ -f $dir/service && ! -f $dir/admin-down ]] || continue
            "$ACT_SV" -w "$ACT_RESTART_WAIT" down "$(cat "$dir/service")" </dev/null || failed=1
        done
    fi
    touch "$TXN/services-attempted"; sync -f "$TXN"
    for dir in "$TXN"/service-*; do
        [[ -f $dir/service ]] || continue
        "$ACT_SV" -w "$ACT_RESTART_WAIT" down "$ACT_ENABLED_DIR/$(cat "$dir/service")" </dev/null || failed=1
    done
    return "$failed"
}
tx_restore_owners() {
    local dir svc path health view binary failed=0
    for dir in "$TXN"/service-*; do
        [[ -f $TXN/services-attempted ]] || break
        [[ -f $dir/service ]] || continue
        svc=$(cat "$dir/service"); path=$(cat "$dir/path"); health=$(cat "$dir/health")
        if [[ -f $dir/link ]]; then act_flip "$path" "$(cat "$dir/link")" || { failed=1; continue; }
        elif [[ -f $dir/binary ]]; then cp -p "$dir/binary" "$path.golden-restore" && mv -T "$path.golden-restore" "$path" || { failed=1; continue; }
        else rm -f -- "$path" || { failed=1; continue; }; fi
        if ! "$ACT_SV" -w "$ACT_RESTART_WAIT" restart "$ACT_ENABLED_DIR/$svc" </dev/null || ! act_healthy "$svc" "$health"; then failed=1; fi
    done
    if [[ -f $TXN/services-attempted && -f $TXN/opblob/path ]]; then
        path=$(cat "$TXN/opblob/path")
        if [[ -f $TXN/opblob/link ]]; then act_flip "$path" "$(cat "$TXN/opblob/link")" || failed=1
        elif [[ -f $TXN/opblob/absent ]]; then rm -f -- "$path" || failed=1; fi
    fi
    for dir in "$TXN"/fuse-*; do
        [[ -f $TXN/fuse-attempted ]] || break
        [[ -f $dir/conf ]] || continue
        svc=$(cat "$dir/service")
        if [[ -f $dir/admin-down ]]; then
            # Byte-for-byte conf restore only; the service stays down.
            cp -p "$dir/conf" "$svc/.conf.golden-restore" && mv -T "$svc/.conf.golden-restore" "$svc/conf" || failed=1
            continue
        fi
        view=${dir##*/fuse-}; binary=$(cat "$dir/binary")
        "$SCRIPT_DIR/fuse-swap.sh" "$svc" "$view" "$binary" --restore "$dir/conf" "$dir/identity.json" || failed=1
    done
    return "$failed"
}
tx_restore_file() {
    local backup=$1 target=$2
    if [[ -f $backup ]]; then
        cp -p "$backup" "$target.golden-restore" && sync -f "$target.golden-restore" && mv -T "$target.golden-restore" "$target" && sync -f "$(dirname "$target")"
    elif [[ -f $backup.absent ]]; then rm -f -- "$target"; fi
}
tx_mount_cleanup() {
    local file path
    for file in "$TXN/stage" "$TXN"/alias-*/old; do
        [[ -f $file ]] || continue
        path=$(cat "$file")
        if mountpoint -q "$path"; then umount "$path" || { warn "retained previous mount: $path"; continue; }; fi
        rmdir "$path" 2>/dev/null || true
    done
}
tx_restore() {
    local failed=0
    tx_phase restoring
    if [[ -f $TXN/services-attempted || -f $TXN/catalog-attempted || -f $TXN/fuse-attempted ]]; then
        # Never restart an old executable/FUSE against a new or unknown catalog.
        if ! tx_quiesce_owners all || ! tx_catalog_restore; then
            tx_phase rollback-incomplete
            warn "catalog restore/quiescence incomplete; no old restart attempted, retain $TXN and run recover --apply"
            return 1
        fi
    fi
    tx_mount_restore || failed=1
    tx_restore_owners || failed=1
    tx_restore_file "$TXN/state.before" "$STATE" || failed=1
    tx_restore_file "$TXN/ledger.before" "$ACT_LEDGER" || failed=1
    if ((failed)); then tx_phase rollback-incomplete; warn "restore incomplete; retain $TXN and run recover --apply"; return 1; fi
    tx_phase rolled-back
    tx_mount_cleanup
}
deploy_candidate() (
    set -Eeuo pipefail
    require_lifecycle_lock
    assert_no_cargo
    tx_no_pending
    [[ -f $CANDIDATE_STATE && -f $STATE ]] || die 'candidate and prior boot selection are required'
    ACT_STATE=$CANDIDATE_STATE
    LIVE=$(jq -er .live.path "$CANDIDATE_STATE")
    require_readonly_release "$LIVE"
    verify_release_inputs "$LIVE"
    tx_validate_catalog
    TXN=$(mktemp -d "$ROOT/golden-transition.XXXXXX")
    chmod 0700 "$TXN"
    cp -p "$STATE" "$TXN/state.before"
    if [[ -f $ACT_LEDGER ]]; then cp -p "$ACT_LEDGER" "$TXN/ledger.before"; else touch "$TXN/ledger.before.absent"; fi
    cp -p "$CANDIDATE_STATE" "$TXN/candidate.json"
    cp -p "$ROOT/golden-catalog-before.json" "$TXN/catalog-selection.json"
    ACT_STATE=$TXN/candidate.json
    tx_snapshot_owners
    sync -f "$TXN"
    tx_phase prepared
    committed=0
    finish_transition() {
        local rc=$?
        trap - EXIT HUP INT TERM
        if [[ $committed != 1 ]]; then tx_restore || rc=2; fi
        exit "$rc"
    }
    trap finish_transition EXIT
    trap 'exit 130' HUP INT TERM
    tx_phase overlay-staging
    tx_mount_prepare
    tx_phase quiescing
    tx_quiesce_owners services
    tx_phase catalog-publication
    touch "$TXN/catalog-attempted"; sync -f "$TXN"
    tx_catalog_publish
    tx_phase services
    touch "$TXN/services-attempted"; sync -f "$TXN"
    do_activate
    tx_phase aliases
    tx_mount_switch 0
    tx_mount_switch 1
    tx_phase fuse
    touch "$TXN/fuse-attempted"; sync -f "$TXN"
    do_activate_fuse
    tx_phase publishing
    cat "$TXN/candidate.json" | atomic_state
    tx_phase committed
    committed=1
    tx_mount_cleanup
)
do_recover() (
    jq -e '.phase!="committed" and .phase!="rolled-back"' "$ROOT/golden-transaction.json" >/dev/null ||
        die 'no unfinished transition to recover'
    require_lifecycle_lock
    assert_no_cargo
    TXN=$(jq -er .transaction_dir "$ROOT/golden-transaction.json")
    [[ $TXN == "$ROOT/"golden-transition.* && $(realpath -e "$TXN") == "$TXN" ]] || die 'invalid recovery journal'
    LIVE=$(jq -er .live.path "$TXN/candidate.json")
    ACT_STATE=$TXN/candidate.json
    require_readonly_release "$LIVE"
    verify_release_inputs "$LIVE"
    tx_restore
)
# Apply the boot opblob projection and admin-down FUSE pins to the CURRENT live
# release as its own journaled transition; no catalog, overlay or service swap.
do_project() (
    set -Eeuo pipefail
    if [[ $APPLY != 1 ]]; then
        would "journal and project $ACT_PREFIX$ACT_OPBLOB onto the live release opblob"
        would 'pin administratively down FUSE views to the live 3tched-fs; never start them'
        return
    fi
    require_lifecycle_lock
    assert_no_cargo
    tx_no_pending
    [[ -f $STATE ]] || die 'no live boot selection'
    LIVE=$(jq -er .live.path "$STATE")
    ACT_STATE=$STATE
    require_readonly_release "$LIVE"
    verify_release_inputs "$LIVE"
    local rid
    rid=$(act_release_id) || die 'live state has no commit/id'
    [[ $(jq -r '.active.release // empty' "$ACT_LEDGER") == "$rid" ]] || die "activation ledger active release is not live $rid"
    TXN=$(mktemp -d "$ROOT/golden-transition.XXXXXX")
    chmod 0700 "$TXN"
    cp -p "$STATE" "$TXN/state.before"
    cp -p "$ACT_LEDGER" "$TXN/ledger.before"
    cp -p "$STATE" "$TXN/candidate.json"
    touch "$TXN/projection-only"
    ACT_STATE=$TXN/candidate.json
    tx_snapshot_owners projection
    sync -f "$TXN"
    tx_phase prepared
    committed=0
    finish_transition() {
        local rc=$?
        trap - EXIT HUP INT TERM
        if [[ $committed != 1 ]]; then tx_restore || rc=2; fi
        exit "$rc"
    }
    trap finish_transition EXIT
    trap 'exit 130' HUP INT TERM
    tx_phase services
    touch "$TXN/services-attempted"; sync -f "$TXN"
    act_project_opblob "$rid"
    tx_phase fuse
    touch "$TXN/fuse-attempted"; sync -f "$TXN"
    do_activate_fuse admin-down-only
    tx_phase committed
    committed=1
    log "projection committed for live release $rid; journal $TXN"
)
