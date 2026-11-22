# Sourced by golden.sh. Build preflight: the same rules whichever shell, agent
# or desktop launches the build. Fails before any mount or compile.
#   - source: GOLDEN_SOURCE_REPO must be the truth clone (/srv/git/odbus-verified)
#     unless GOLDEN_ALLOW_SOURCE_OVERRIDE=1.
#   - caches: ccache (CC/CXX) and the RocksDB archive link are reported; sccache
#     is used as RUSTC_WRAPPER when installed in the build OS.
#   - static match: every plugin version in the source (StatePlugin name()/version()
#     literals) must equal the sealed catalog's (`opblob inspect`), unless
#     --allow-catalog-change (a deliberate bump that will be sealed after the build).
#   - no rebuild: a commit+digest that already has a verified successful execution
#     receipt in the build OS is refused unless --force.
GOLDEN_DEFAULT_SOURCE=/srv/git/odbus-verified
FORCE=${FORCE:-0}
ALLOW_CATALOG_CHANGE=${ALLOW_CATALOG_CHANGE:-0}
preflight_source() {
    [[ $SOURCE_REPO == "$GOLDEN_DEFAULT_SOURCE" || $(setting GOLDEN_ALLOW_SOURCE_OVERRIDE 0) == 1 ]] ||
        die "build source must be $GOLDEN_DEFAULT_SOURCE (GOLDEN_ALLOW_SOURCE_OVERRIDE=1 to build $SOURCE_REPO)"
}
# "<plugin> <version>" for each state plugin file with exactly one literal
# name() and one literal version().
source_plugin_versions() {
    local f
    while IFS= read -r -d '' f; do
        awk '
            /fn name\(&self\) -> &str/ { want = "n" }
            /fn version\(&self\) -> &str/ { want = "v" }
            want != "" && match($0, /"[^"]+"/) {
                v = substr($0, RSTART + 1, RLENGTH - 2)
                if (want == "n") { names++; name = v } else { vers++; ver = v }
                want = ""
            }
            END { if (names == 1 && vers == 1) print name, ver }' "$f"
    done < <(find "$SOURCE_REPO/crates/op-plugins/src" -name '*.rs' -print0 | sort -z)
}
# "<plugin> <version>" for each blob in the sealed catalog.
catalog_plugin_versions() {
    local dir=$1 opblob=$2 blob
    for blob in "$dir"/*.blob; do
        [[ -f $blob ]] || continue
        "$opblob" inspect "$blob" 2>/dev/null | awk 'NR == 1 && $1 == "plugin" { sub(/^v/, "", $3); print $2, $3; exit }'
    done
}
preflight_catalog() {
    local dir opblob mismatches
    dir=$(setting GOLDEN_SEALED_CATALOG /var/lib/opdbus/plugin-blobs)
    opblob=$(setting GOLDEN_OPBLOB /usr/local/libexec/op-dbus/bin/opblob)
    [[ -f $dir/.manifest.json && -x $opblob ]] || die "sealed catalog or opblob absent: $dir $opblob"
    mismatches=$(LC_ALL=C join <(source_plugin_versions | LC_ALL=C sort -u) <(catalog_plugin_versions "$dir" "$opblob" | LC_ALL=C sort -u) |
        awk '$2 != $3 { printf "%s source=%s catalog=%s\n", $1, $2, $3 }')
    if [[ -n $mismatches ]]; then
        printf 'golden: source/catalog plugin version mismatch:\n%s\n' "$mismatches" >&2
        [[ $ALLOW_CATALOG_CHANGE == 1 ]] || die 'refusing to build against a different sealed catalog (--allow-catalog-change for a deliberate bump)'
        warn 'building a deliberate catalog change; seal it after the build'
    fi
}
preflight_receipt() {
    local commit=$1 digest=$2 receipt=$WORK/opt/op-build/execution-receipt.json
    [[ -f $receipt ]] || return 0
    jq -e --arg c "$commit" --arg d "$digest" '.exit==0 and .source_commit==$c and .source_digest==$d' "$receipt" >/dev/null || return 0
    ( verify_build_receipt "$WORK" ) >/dev/null 2>&1 || return 0
    [[ $FORCE == 1 ]] || die "commit $commit (digest $digest) already has a successful build receipt; --force to rebuild"
    warn "rebuilding $commit despite a successful receipt (--force)"
}
preflight_caches() {
    local rocks_dir=$1
    if [[ -f $WORK$rocks_dir/librocksdb.a ]]; then log "cache: RocksDB archive $rocks_dir"; else log "cache: no RocksDB archive yet ($rocks_dir); harvested after this build"; fi
    log "cache: ccache CC/CXX in $BUILD_CACHE/ccache"
    if [[ -x $WORK/usr/bin/sccache || -x $WORK/root/.cargo/bin/sccache ]]; then log "cache: sccache RUSTC_WRAPPER in $BUILD_CACHE/sccache"; else log 'cache: sccache not installed in the build OS (rustc uncached; incremental target dir retained)'; fi
}
sccache_env() {
    local bin
    for bin in /usr/bin/sccache /root/.cargo/bin/sccache; do
        if [[ -x $WORK$bin ]]; then
            install -d -m 0700 "$WORK$BUILD_CACHE/sccache"
            printf '%s\n' "RUSTC_WRAPPER=$bin" "SCCACHE_DIR=$BUILD_CACHE/sccache"
            return
        fi
    done
}
plan_preflight() {
    would "preflight: source $SOURCE_REPO (must be $GOLDEN_DEFAULT_SOURCE unless GOLDEN_ALLOW_SOURCE_OVERRIDE=1); plugin versions vs $(setting GOLDEN_SEALED_CATALOG /var/lib/opdbus/plugin-blobs) (--allow-catalog-change); refuse a commit with a successful receipt (--force)"
}
