# activate.sh: sourced by golden.sh. Implements `activate` (alias `install`):
# stage release binaries from the read-only live slot into a versioned
# install dir, then flip each enabled service's exec path to it one service
# at a time, health-check, and roll everything in this run back on failure.
# Dry-run unless --apply. Never starts/enables a service, never touches the
# hard exclusions, never removes a runit log service, never edits the
# op-release kit ledger (slots.json): its schema is deny_unknown_fields with
# active/previous restricted to kit slots a/b, so golden activation is
# recorded in its own ledger next to it.

ACT_SV_DIR=${GOLDEN_SV_DIR:-/etc/sv}
ACT_ENABLED_DIR=${GOLDEN_ENABLED_DIR:-/etc/runit/runsvdir/default}
ACT_RELEASES=${GOLDEN_RELEASES_DIR:-/usr/local/libexec/op-dbus/releases}
ACT_LEDGER=${GOLDEN_LEDGER:-/var/lib/opdbus-release/golden-activation.json}
ACT_KIT_LEDGER=${GOLDEN_KIT_LEDGER:-/var/lib/opdbus-release/slots.json}
ACT_SV=${GOLDEN_SV:-sv}
ACT_PREFIX=${GOLDEN_INSTALL_PREFIX:-}
ACT_TABLE=${GOLDEN_ACTIVATE_TABLE:-}
ACT_RESTART_WAIT=${GOLDEN_RESTART_WAIT:-30}

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
        op-grpc-bridge|op-grpc-bridge-*) return 0 ;;
        cozo-ublk|cozo-ublk-*|cozo-ublk*) return 0 ;;
        zeroclaw|zeroclaw-*|*zeroclaw*) return 0 ;;
        sddm|sddm-*) return 0 ;;
        *xanmod*) return 0 ;;
    esac
    return 1
}

act_excluded_binary() {
    case "$1" in
        3tched-fs|op-grpc-bridge|cozo-ublk|threetched|op-identity-accept|zeroclaw*) return 0 ;;
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
    [ -r "$STATE" ] || return 0
    if command -v jq >/dev/null 2>&1; then
        jq -r "$1 // empty" "$STATE" 2>/dev/null || true
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

act_print_plan() {
    local rid src svc bin path health decision detail cur resolved cursha new newsha
    rid=$(act_release_id || true)
    src=$(act_live_os)/opt/op-build/target/release
    printf 'plan: activate (stage release binaries from the live slot, flip services one at a time)\n'
    printf '  live slot: %s (commit %s, id %s)\n' "$LIVE" "$(act_state_field '.live.commit')" "$(act_state_field '.live.id')"
    printf '  release binaries from: %s\n' "$src"
    printf '  versioned install dir: %s/%s/bin  (only the binaries below; no target/, source or toolchain)\n' "$ACT_RELEASES" "${rid:-<no live slot>}"
    printf '  golden ledger: %s (active/previous); kit ledger %s is NOT modified (schema: active/previous are kit slots a/b only)\n' "$ACT_LEDGER" "$ACT_KIT_LEDGER"
    printf '  per service: preserve current binary, atomic symlink flip, %s -w %s restart, health; on failure roll back every service flipped in this run\n' "$ACT_SV" "$ACT_RESTART_WAIT"
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
    local rid=$1 result=$2 services=$3 tmp previous
    install -d -m 0755 -- "$(dirname -- "$ACT_LEDGER")"
    previous=null
    if [ -r "$ACT_LEDGER" ]; then
        previous=$(jq -c '.active // null' "$ACT_LEDGER" 2>/dev/null || printf 'null')
    fi
    tmp=$(mktemp "$(dirname -- "$ACT_LEDGER")/.golden-activation.XXXXXX")
    jq -n --arg rid "$rid" --arg result "$result" \
        --arg commit "$(act_state_field '.live.commit')" --arg live_id "$(act_state_field '.live.id')" \
        --arg live "$LIVE" --arg state "$STATE" --arg at "$(date -Is)" \
        --argjson services "$services" --argjson previous "$previous" \
        '{version:1,
          active:(if $result=="active" then {release:$rid,commit:$commit,live_id:$live_id,live:$live,state:$state,activated_at:$at,services:$services} else $previous end),
          previous:(if $result=="active" then $previous else null end),
          last_attempt:{release:$rid,result:$result,at:$at,services:$services}}' > "$tmp"
    chmod 0644 "$tmp"
    mv -T -- "$tmp" "$ACT_LEDGER"
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

do_activate() {
    local rid src stage pre stamp svc bin path health decision detail cur prev new sha
    local -a done_svc=() done_path=() done_prev=()
    local services='[]' i failed=
    if [ "$APPLY" != 1 ]; then
        act_print_plan
        return 0
    fi
    [ "$(id -u)" = 0 ] || [ -n "${GOLDEN_ALLOW_NONROOT_ACTIVATE:-}" ] || die "activate --apply must run as root"
    command -v jq >/dev/null 2>&1 || die "activate --apply needs jq"
    is_subvolume "$LIVE" || [ -n "${GOLDEN_SKIP_SUBVOL_CHECK:-}" ] || die "no live slot: run cut first"
    subvol_readonly "$LIVE" || [ -n "${GOLDEN_SKIP_SUBVOL_CHECK:-}" ] || die "live slot is not read-only"
    rid=$(act_release_id) || die "state.json has no live commit/id"
    src=$(act_live_os)/opt/op-build/target/release
    stage=$ACT_RELEASES/$rid/bin
    stamp=$(date +%Y%m%d-%H%M%S)
    pre=$ACT_RELEASES/pre-golden-$stamp/bin
    act_print_plan
    # 1. stage only the binaries that will be swapped
    while IFS='|' read -r svc bin path health decision detail; do
        [ "$decision" = swap ] || continue
        act_excluded_binary "$bin" && die "refusing excluded binary $bin"
        [ -x "$src/$bin" ] || die "release binary missing: $src/$bin"
        install -d -m 0755 -- "$stage"
        if [ ! -f "$stage/$bin" ] || [ "$(act_sha "$stage/$bin")" != "$(act_sha "$src/$bin")" ]; then
            install -m 0755 -- "$src/$bin" "$stage/.$bin.tmp"
            [ "$(act_sha "$stage/.$bin.tmp")" = "$(act_sha "$src/$bin")" ] || die "staged sha mismatch for $bin"
            mv -T -- "$stage/.$bin.tmp" "$stage/$bin"
        fi
        log "staged $stage/$bin sha256=$(act_sha "$stage/$bin")"
    done < <(act_rows)
    # 2. flip one service at a time
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
        done_svc+=("$svc"); done_path+=("$cur"); done_prev+=("$prev")
        if "$ACT_SV" -w "$ACT_RESTART_WAIT" restart "$ACT_ENABLED_DIR/$svc" </dev/null && act_healthy "$svc" "$health"; then
            sha=$(act_sha "$new")
            services=$(jq -c --arg s "$svc" --arg p "$cur" --arg t "$new" --arg prev "$prev" --arg sha "$sha" \
                '. + [{service:$s,path:$p,target:$t,previous:$prev,sha256:$sha}]' <<<"$services")
            log "healthy: $svc"
        else
            failed=$svc
            warn "health check failed for $svc; rolling back every service flipped in this run"
            break
        fi
    done < <(act_rows)
    if [ -n "$failed" ]; then
        for ((i=${#done_svc[@]}-1; i>=0; i--)); do
            if [ -n "${done_prev[$i]}" ]; then
                act_flip "${done_path[$i]}" "${done_prev[$i]}"
            else
                rm -f -- "${done_path[$i]}"
            fi
            "$ACT_SV" -w "$ACT_RESTART_WAIT" restart "$ACT_ENABLED_DIR/${done_svc[$i]}" </dev/null || warn "restart after rollback failed: ${done_svc[$i]}"
            log "rolled back ${done_svc[$i]} -> ${done_prev[$i]:-removed}"
        done
        act_write_ledger "$rid" "rolled-back:$failed" "$services"
        die "activation rolled back (failed: $failed)"
    fi
    act_write_ledger "$rid" active "$services"
    log "activation complete: release $rid; ledger $ACT_LEDGER"
}
