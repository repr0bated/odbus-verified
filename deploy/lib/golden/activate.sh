# Sourced by golden.sh. Service participant in the complete catalog/overlay/
# FUSE transaction; only its outer owner may restore previous executables.
# Deployment validates existing grants; it never expands $ROOT or another
# principal. Explicit policy maintenance updates the durable document separately.
# The native release kit ledger remains independent.

ACT_SV_DIR=${GOLDEN_SV_DIR:-/etc/sv}
ACT_ENABLED_DIR=${GOLDEN_ENABLED_DIR:-/etc/runit/runsvdir/default}
ACT_RELEASES=${GOLDEN_RELEASES_DIR:-/usr/local/libexec/op-dbus/releases}
ACT_LEDGER=${GOLDEN_LEDGER:-/var/lib/opdbus-release/golden-activation.json}
ACT_KIT_LEDGER=${GOLDEN_KIT_LEDGER:-/var/lib/opdbus-release/slots.json}
ACT_SV=${GOLDEN_SV:-sv}
ACT_PREFIX=${GOLDEN_INSTALL_PREFIX:-}
ACT_TABLE=${GOLDEN_ACTIVATE_TABLE:-}
ACT_RESTART_WAIT=${GOLDEN_RESTART_WAIT:-90}
ACT_GRANTS_SOURCE=${GOLDEN_GRANTS_SOURCE:-/etc/opdbus/capability-grants.json}
ACT_GRANTS_PATH=${GOLDEN_GRANTS_PATH:-/dev/shm/opdbus/capability-grants.json}
ACT_GRANTS_FRESH_WAIT=${GOLDEN_GRANTS_FRESH_WAIT:-30}
# Boot stage-shm (helpers/opdbus-rundirs-up) executes this path; activation
# projects it onto the live release so a reboot republishes the live catalog.
ACT_OPBLOB=${GOLDEN_OPBLOB_PATH:-/usr/local/libexec/op-dbus/bin/opblob}

act_table() {
    if [ -n "$ACT_TABLE" ]; then
        printf '%s' "$ACT_TABLE"
    else
        printf '%s' "$SCRIPT_DIR/activate-services.tsv"
    fi
}

# Hard exclusions: never swapped, regardless of the table.
act_excluded_service() {
    case "$1" in
        3tched-fs|3tched-fs-*|threetched*|*3tchedfs*|*3tched-fs*) return 0 ;;
        op-grpc-bridge-*) return 0 ;;
        cozo-ublk|cozo-ublk-*|cozo-ublk*) return 0 ;;
        zeroclaw|zeroclaw-*|*zeroclaw*) return 0 ;;
        sddm|sddm-*) return 0 ;;
        *xanmod*) return 0 ;;
    esac
    return 1
}

act_excluded_binary() {
    case "$1" in
        3tched-fs|cozo-ublk|threetched|op-identity-accept|zeroclaw*) return 0 ;;
    esac
    return 1
}

act_live_os() {
    local base
    base=$(os_dir_of "$LIVE")
    if [ -d "$base/usr" ]; then
        printf '%s' "$base"
    else
        printf '%s' "$LIVE"
    fi
}

act_state_field() {
    # act_state_field <jq filter>; empty when absent.
    local state=${ACT_STATE:-$STATE}
    [ -r "$state" ] || return 0
    if command -v jq >/dev/null 2>&1; then
        jq -r "$1 // empty" "$state" 2>/dev/null || true
    fi
}

act_release_id() {
    local commit id
    commit=$(act_state_field '.live.commit')
    id=$(act_state_field '.live.id')
    [ -n "$commit" ] && [ -n "$id" ] || return 1
    printf '%s-%s' "$(printf '%s' "$commit" | cut -c1-12)" "$id"
}

act_sha() {
    if [ -f "$1" ]; then
        sha256sum -- "$1" | cut -d' ' -f1
    else
        printf 'absent'
    fi
}

act_run_script() {
    local svc=$1 dir
    dir=$(readlink -f -- "$ACT_ENABLED_DIR/$svc" 2>/dev/null || true)
    [ -n "$dir" ] || dir=$ACT_SV_DIR/$svc
    printf '%s/run' "$dir"
}

act_pid() {
    "$ACT_SV" status "$ACT_ENABLED_DIR/$1" 2>/dev/null | sed -n 's/^run: [^(]*(pid \([0-9][0-9]*\)).*/\1/p' | head -n1
}

# True when the run script references <path> as a whole token (not as a
# prefix of a longer path such as <path>-other).
act_run_execs() {
    local run=$1 path=$2 re
    re=$(printf '%s' "$path" | sed 's/[][\\.^$*+?(){}|]/\\&/g')
    grep -qE "(^|[[:space:]\"'=])${re}([[:space:]\"';]|\$)" -- "$run"
}

# act_rows: emit service|binary|path|health|decision|detail for each table row
# plus each enabled host service that matches a hard exclusion.
act_rows() {
    local table svc bin path health decision detail run seen=' '
    table=$(act_table)
    [ -r "$table" ] || die "activation table missing: $table"
    while IFS=$'\t' read -r svc bin path health; do
        case "$svc" in ''|'#'*) continue ;; esac
        seen="$seen$svc "
        run=$(act_run_script "$svc")
        if [ "$health" = excluded ] || act_excluded_service "$svc" || act_excluded_binary "$bin"; then
            decision=excluded; detail="hard exclusion"
        elif [ ! -e "$ACT_ENABLED_DIR/$svc" ] && [ ! -L "$ACT_ENABLED_DIR/$svc" ]; then
            if [ -d "$ACT_SV_DIR/$svc" ]; then
                decision=skip; detail="installed in $ACT_SV_DIR but not enabled (activation never starts services)"
            else
                decision=skip; detail="not installed on this host"
            fi
        elif [ ! -r "$run" ]; then
            decision=skip; detail="enabled but run script unreadable: $run"
        elif ! act_run_execs "$run" "$path"; then
            decision=skip; detail="run script $run does not exec $path"
        else
            decision=swap; detail="enabled; run script $run execs $path"
        fi
        printf '%s|%s|%s|%s|%s|%s\n' "$svc" "$bin" "$path" "$health" "$decision" "$detail"
    done < "$table"
    if [ -d "$ACT_ENABLED_DIR" ]; then
        for run in "$ACT_ENABLED_DIR"/*; do
            [ -e "$run" ] || [ -L "$run" ] || continue
            svc=${run##*/}
            case "$seen" in *" $svc "*) continue ;; esac
            if act_excluded_service "$svc"; then
                printf '%s|-|-|-|excluded|hard exclusion (enabled on host, not in table)\n' "$svc"
            fi
        done
    fi
}

act_artifact_dir() {
    printf '%s%s' "$(act_live_os)" "$NATIVE_ARTIFACT_DIR"
}

act_live_materializer() {
    printf '%s/op-grants-materializer' "$(act_artifact_dir)"
}

act_print_grants_plan() {
    printf '  grants: validate the existing durable document and require Fresh; no authority expansion\n'
}

act_validate_grants() {
    local bin i
    bin=$(act_live_materializer)
    [[ -x $bin && -r $ACT_GRANTS_SOURCE ]] || return 1
    OP_GRANTS_SOURCE=$ACT_GRANTS_SOURCE OP_GRANTS_PATH=$ACT_GRANTS_PATH \
        "$bin" validate "$ACT_GRANTS_SOURCE" || return 1
    for ((i=0;i<ACT_GRANTS_FRESH_WAIT;i++)); do
        if OP_GRANTS_SOURCE=$ACT_GRANTS_SOURCE OP_GRANTS_PATH=$ACT_GRANTS_PATH "$bin" check; then
            return 0
        fi
        sleep 1
    done
    warn 'existing grants are not Fresh; deployment does not modify policy'
    return 1
}

act_print_plan() {
    local rid src svc bin path health decision detail cur resolved cursha new newsha
    rid=$(act_release_id || true)
    src=$(act_artifact_dir)
    printf 'plan: activate (stage release binaries from the live slot, flip services one at a time)\n'
    printf '  live slot: %s (commit %s, id %s)\n' "$LIVE" "$(act_state_field '.live.commit')" "$(act_state_field '.live.id')"
    printf '  release binaries from: %s\n' "$src"
    printf '  versioned install dir: %s/%s/bin  (only the binaries below; no target/, source or toolchain)\n' "$ACT_RELEASES" "${rid:-<no live slot>}"
    printf '  golden ledger: %s (active/previous); kit ledger %s is NOT modified (schema: active/previous are kit slots a/b only)\n' "$ACT_LEDGER" "$ACT_KIT_LEDGER"
    printf '  per service: preserve current binary, atomic symlink flip, %s -w %s restart, health; on failure roll back every service flipped in this run\n' "$ACT_SV" "$ACT_RESTART_WAIT"
    printf '  boot projection: %s%s -> %s/%s/bin/opblob (stage-shm on boot)\n' "$ACT_PREFIX" "$ACT_OPBLOB" "$ACT_RELEASES" "${rid:-?}"
    printf '  admin-down FUSE views: stage live 3tched-fs and rewrite the conf pin only; never started, down file kept\n'
    act_print_grants_plan
    while IFS='|' read -r svc bin path health decision detail; do
        printf '\n  service %s: %s\n' "$svc" "$decision"
        printf '    reason: %s\n' "$detail"
        [ "$bin" = - ] && continue
        cur=$ACT_PREFIX$path
        resolved=$(readlink -f -- "$cur" 2>/dev/null || printf '%s' "$cur")
        cursha=$(act_sha "$resolved")
        new=$src/$bin
        newsha=$(act_sha "$new")
        printf '    current: %s -> %s  sha256=%s\n' "$cur" "$resolved" "$cursha"
        if [ "$decision" = swap ]; then
            printf '    new:     %s/%s/bin/%s  (from %s) sha256=%s\n' "$ACT_RELEASES" "${rid:-?}" "$bin" "$new" "$newsha"
            printf '    health:  %s\n' "$health"
            [ "$cursha" = "$newsha" ] && printf '    note:    binary identical; flip still recorded\n'
        else
            printf '    release: %s sha256=%s (not installed)\n' "$new" "$newsha"
        fi
    done < <(act_rows)
}

act_write_ledger() {
    # act_write_ledger <rid> <result> <services-json-array>
    local rid=$1 result=$2 services=$3 tmp previous rollback
    install -d -m 0755 -- "$(dirname -- "$ACT_LEDGER")"
    previous=null
    rollback=null
    if [ -r "$ACT_LEDGER" ]; then
        previous=$(jq -c '.active // null' "$ACT_LEDGER" 2>/dev/null || printf 'null')
        rollback=$(jq -c --arg rid "$rid" '
            if .active.release == $rid then .previous else .active end // null
        ' "$ACT_LEDGER" 2>/dev/null || printf 'null')
    fi
    tmp=$(mktemp "$(dirname -- "$ACT_LEDGER")/.golden-activation.XXXXXX")
    jq -n --arg rid "$rid" --arg result "$result" \
        --arg commit "$(act_state_field '.live.commit')" --arg live_id "$(act_state_field '.live.id')" \
        --arg live "$LIVE" --arg state "$STATE" --arg at "$(date -Is)" \
        --argjson services "$services" --argjson previous "$previous" --argjson rollback "$rollback" \
        '{version:1,
          active:(if $result=="active" then {release:$rid,commit:$commit,live_id:$live_id,live:$live,state:$state,activated_at:$at,services:$services} else $previous end),
          previous:$rollback,
          last_attempt:{release:$rid,result:$result,at:$at,services:$services}}' > "$tmp"
    chmod 0644 "$tmp"
    mv -T -- "$tmp" "$ACT_LEDGER"
}

act_ledger_put() {
    # act_ledger_put <jq-path-under-active> <json>: record a projection on the active release.
    local key=$1 value=$2 tmp
    [ -r "$ACT_LEDGER" ] || die "activation ledger missing: $ACT_LEDGER"
    tmp=$(mktemp "$(dirname -- "$ACT_LEDGER")/.golden-activation.XXXXXX")
    jq --argjson v "$value" ".active$key = \$v" "$ACT_LEDGER" > "$tmp"
    chmod 0644 "$tmp"; sync -f "$tmp"
    mv -T -- "$tmp" "$ACT_LEDGER"
}

act_project_opblob() {
    # act_project_opblob <rid>: stage the live opblob and flip the boot path to it.
    local rid=$1 src stage path prev new sha
    src=$(act_artifact_dir); stage=$ACT_RELEASES/$rid/bin; path=$ACT_PREFIX$ACT_OPBLOB
    act_stage_binary "$src" "$stage" opblob
    new=$stage/opblob
    if [ -L "$path" ]; then prev=$(readlink -- "$path"); elif [ -e "$path" ]; then die "boot opblob is not a symlink: $path"; else prev=; fi
    act_flip "$path" "$new"
    sha=$(act_sha "$new")
    [ "$(act_sha "$(readlink -f -- "$path")")" = "$sha" ] || die "boot opblob projection does not resolve to $new"
    act_ledger_put .projections.opblob "$(jq -nc --arg p "$path" --arg t "$new" --arg prev "$prev" --arg sha "$sha" \
        '{path:$p,target:$t,previous:$prev,sha256:$sha}')"
    log "projected boot opblob: $path -> $new (previous ${prev:-absent}) sha256=$sha"
}

act_flip() {
    # act_flip <path> <target>: atomic replace of <path> with a symlink.
    local path=$1 target=$2
    install -d -m 0755 -- "$(dirname -- "$path")"
    ln -sfn -- "$target" "$path.golden-new"
    mv -T -- "$path.golden-new" "$path"
}

act_healthy() {
    local svc=$1 health=$2 secs pid1 pid2
    case "$health" in
        stable:*) secs=${health#stable:} ;;
        *) secs=5 ;;
    esac
    pid1=$(act_pid "$svc")
    [ -n "$pid1" ] || return 1
    sleep "$secs"
    pid2=$(act_pid "$svc")
    [ -n "$pid2" ] && [ "$pid1" = "$pid2" ]
}

act_stage_binary() {
    local src=$1 stage=$2 bin=$3
    [ -x "$src/$bin" ] || die "release binary missing: $src/$bin"
    install -d -m 0755 -- "$stage"
    if [ ! -f "$stage/$bin" ] || [ "$(act_sha "$stage/$bin")" != "$(act_sha "$src/$bin")" ]; then
        install -m 0755 -- "$src/$bin" "$stage/.$bin.tmp"
        [ "$(act_sha "$stage/.$bin.tmp")" = "$(act_sha "$src/$bin")" ] || die "staged sha mismatch for $bin"
        mv -T -- "$stage/.$bin.tmp" "$stage/$bin"
    fi
    log "staged $stage/$bin sha256=$(act_sha "$stage/$bin")"
}

# do_activate_fuse [admin-down-only]: running views are swapped and health
# checked; views the transaction recorded admin-down get the live 3tched-fs
# pinned in their conf exactly as a swap writes it, but are never started and
# keep their down file. admin-down-only leaves running views to activate.
do_activate_fuse() {
    local mode=${1:-all} rid src stage view service dir sha
    local staged=0
    rid=$(act_release_id) || die "state.json has no live commit/id"
    subvol_readonly "$LIVE" || die "live slot is not read-only"
    src=$(act_artifact_dir)
    stage=$ACT_RELEASES/$rid/bin
    for view in 3tchedFS threetched; do
        case "$view" in
            3tchedFS) service=3tched-fs ;;
            threetched) service=threetched-fs ;;
        esac
        dir=$ACT_ENABLED_DIR/$service
        [ -f "$dir/conf" ] || continue
        if [ "$APPLY" != 1 ]; then
            would "stage the live 3tched-fs binary and update the $view mount pin (pin only if administratively down)"
            continue
        fi
        [ "$(id -u)" = 0 ] || die "activate-fuse --apply must run as root"
        if [ -n "${TXN:-}" ] && [ -f "$TXN/fuse-$view/admin-down" ]; then
            if [ "$staged" = 0 ]; then act_stage_binary "$src" "$stage" 3tched-fs; staged=1; fi
            "$SCRIPT_DIR/fuse-swap.sh" "$dir" "$view" "$stage/3tched-fs" --pin-down
            sha=$(act_sha "$stage/3tched-fs")
            act_ledger_put ".fuse_pins[\"$view\"]" "$(jq -nc --arg s "$service" --arg b "$stage/3tched-fs" --arg sha "$sha" \
                '{service:$s,binary:$b,sha256:$sha,state:"admin-down"}')"
            continue
        fi
        [ "$mode" = all ] || { log "$service is running; its swap belongs to activate"; continue; }
        if [ "$staged" = 0 ]; then act_stage_binary "$src" "$stage" 3tched-fs; staged=1; fi
        "$SCRIPT_DIR/fuse-swap.sh" "$dir" "$view" "$stage/3tched-fs" --transaction
    done
}

do_activate() {
    local rid src stage pre stamp svc bin path health decision detail cur prev new sha
    local services='[]' failed=
    if [ "$APPLY" != 1 ]; then
        act_print_plan
        return 0
    fi
    [ "$(id -u)" = 0 ] || [ -n "${GOLDEN_ALLOW_NONROOT_ACTIVATE:-}" ] || die "activate --apply must run as root"
    command -v jq >/dev/null 2>&1 || die "activate --apply needs jq"
    is_subvolume "$LIVE" || [ -n "${GOLDEN_SKIP_SUBVOL_CHECK:-}" ] || die "no live slot: run cut first"
    subvol_readonly "$LIVE" || [ -n "${GOLDEN_SKIP_SUBVOL_CHECK:-}" ] || die "live slot is not read-only"
    rid=$(act_release_id) || die "state.json has no live commit/id"
    src=$(act_artifact_dir)
    stage=$ACT_RELEASES/$rid/bin
    stamp=$(date +%Y%m%d-%H%M%S)
    pre=$ACT_RELEASES/pre-golden-$stamp/bin
    act_print_plan
    # 1. stage only the binaries that will be swapped
    while IFS='|' read -r svc bin path health decision detail; do
        [ "$decision" = swap ] || continue
        act_excluded_binary "$bin" && die "refusing excluded binary $bin"
        act_stage_binary "$src" "$stage" "$bin"
    done < <(act_rows)
    # Validate existing authority before any service flip; never generate grants.
    if ! act_validate_grants; then
        act_write_ledger "$rid" "rolled-back:grants-validation" "$services"
        die "activation aborted: existing grants validation/Fresh failed; no services flipped"
    fi
    # 3. flip one service at a time
    while IFS='|' read -r svc bin path health decision detail; do
        [ "$decision" = swap ] || continue
        cur=$ACT_PREFIX$path
        if [ -L "$cur" ]; then
            prev=$(readlink -- "$cur")
        elif [ -f "$cur" ]; then
            install -d -m 0755 -- "$pre"
            cp -a -- "$cur" "$pre/$bin"
            prev=$pre/$bin
        else
            prev=
        fi
        new=$stage/$bin
        log "flip $svc: $cur -> $new (previous ${prev:-none})"
        act_flip "$cur" "$new"
        if "$ACT_SV" -w "$ACT_RESTART_WAIT" restart "$ACT_ENABLED_DIR/$svc" </dev/null && act_healthy "$svc" "$health"; then
            sha=$(act_sha "$new")
            services=$(jq -c --arg s "$svc" --arg p "$cur" --arg t "$new" --arg prev "$prev" --arg sha "$sha" \
                '. + [{service:$s,path:$p,target:$t,previous:$prev,sha256:$sha}]' <<<"$services")
            log "healthy: $svc"
        else
            failed=$svc
            warn "health check failed for $svc; complete transaction owner will restore catalog before old executable restart"
            break
        fi
    done < <(act_rows)
    if [ -n "$failed" ]; then
        act_write_ledger "$rid" "rollback-required:$failed" "$services"
        die "activation failed (service: $failed); transaction owner must verify the complete restore"
    fi
    act_write_ledger "$rid" active "$services"
    act_project_opblob "$rid"
    log "activation complete: release $rid; ledger $ACT_LEDGER"
}
