# Sourced by golden.sh. Build compiles $GOLDEN_SOURCE_REPO bind-mounted at
# /opt/op-build.src; source_commit/source_dirty/source_repo come from that repo
# for provenance only and the digest of its build inputs is the source authority. Release
# snapshots carry no source: they verify receipts, artifacts and the staged
# catalog only. Only seal may record an isolated, removal-reviewed catalog;
# build records execution only. Cut and activation never seal.
NATIVE_ARTIFACT_DIR=/opt/op-build/target/x86_64-unknown-linux-gnu/release
SOURCE_MOUNT=/opt/op-build.src
# Build inputs only: workspace manifests, crates, vendored crates, optional
# toolchain/config, plus the exact out-of-crates files crates include_str!/include_bytes!.
# Any directory named target or target-* (any depth) and .git are excluded.
SOURCE_REQUIRED_INPUTS=(Cargo.toml Cargo.lock crates)
SOURCE_OPTIONAL_INPUTS=(vendor .cargo rust-toolchain rust-toolchain.toml
    certs/op-dbus-ca.crt
    deploy/config/mcp-audience-policy.json
    deploy/config/mcp-toolsets.json
    deploy/config/model-providers.toml
    deploy/config/op-mcp-door/mcp-toolsets.json.example
    deploy/config/threetched-skills/default/ui-authoring/SKILL.md
    deploy/config/threetched-skills/default/ui-authoring/sources.json
    deploy/config/toolsets/cognitive.toolsets.json
    deploy/runit/libexec-3tched/emqx-prepare
    deploy/runit/libexec-3tched/opdbus-rundirs-up
    deploy/runit/opdbus-rundirs/run
    deploy/security/capability-grants.json)
source_digest() {
    local root=$1 p inputs=()
    for p in "${SOURCE_REQUIRED_INPUTS[@]}"; do
        [[ -e $root/$p ]] || die "source input absent: $root/$p"
        inputs+=("./$p")
    done
    for p in "${SOURCE_OPTIONAL_INPUTS[@]}"; do
        [[ ! -e $root/$p && ! -L $root/$p ]] || inputs+=("./$p")
    done
    tar --sort=name --format=gnu --mtime=@0 --owner=0 --group=0 --numeric-owner \
        --exclude='.git' --exclude='target' --exclude='target-*' \
        -C "$root" -cf - -- "${inputs[@]}" | sha256sum | cut -d' ' -f1
}
source_git() { git -c safe.directory="$SOURCE_REPO" -C "$SOURCE_REPO" "$@"; }
source_commit() { source_git rev-parse --verify HEAD; }
source_dirty() {
    local status
    status=$(source_git status --porcelain) || die "git status failed: $SOURCE_REPO"
    if [[ -n $status ]]; then echo true; else echo false; fi
}
# Host path of a build-OS path; the source mount resolves to the repository.
build_host_path() {
    case $1 in
        "$SOURCE_MOUNT"/*) printf '%s%s' "$SOURCE_REPO" "${1#"$SOURCE_MOUNT"}" ;;
        *) printf '%s%s' "$WORK" "$1" ;;
    esac
}
assert_no_cargo() {
    ! ps -eo comm= | grep -Eq '^(cargo|rustc|clippy-driver)$' || die 'another Cargo/rustc/Clippy owner is active'
}
require_lifecycle_lock() {
    [[ $(readlink /proc/$$/fd/9) == "$ROOT/.golden.lock" ]] && flock -n 9 || die 'existing Golden lifecycle lock is required'
}
capture_pair() {
    [[ ( -n ${THREETCHED_CAPTURE_BIN:-} && -n ${THREETCHED_CAPTURE_MANIFEST:-} ) ||
       ( -z ${THREETCHED_CAPTURE_BIN:-} && -z ${THREETCHED_CAPTURE_MANIFEST:-} ) ]] || die 'capture binary and manifest must be supplied together'
    CAPTURE_BIN=${THREETCHED_CAPTURE_BIN:-/opt/op-build/owner-capture/threetched}
    CAPTURE_MANIFEST=${THREETCHED_CAPTURE_MANIFEST:-$SOURCE_MOUNT/crates/op-grpc-bridge/threetched-manifests/threetched.json}
    local bin manifest
    bin=$(build_host_path "$CAPTURE_BIN"); manifest=$(build_host_path "$CAPTURE_MANIFEST")
    [[ $CAPTURE_BIN == /* && $CAPTURE_MANIFEST == /* && -x $bin && -f $manifest ]] || die 'retained capture pair is absent'
    [[ $(sha256sum "$bin" | cut -d' ' -f1) == "$(jq -er .artifact.sha256 "$manifest")" ]] || die 'capture pair digest differs'
    CAPTURE_CONFIG=${THREETCHED_CONFIG_DIR:-/opt/op-build/owner-capture/config}
    [[ $CAPTURE_CONFIG == /opt/op-build/* && $CAPTURE_CONFIG != *..* ]] || die 'capture config must remain isolated inside the build OS'
}
record_build_receipt() {
    local digest=$1 commit=$2 dirty=$3 artifacts='{}' path name sha tmp
    [[ $(source_digest "$SOURCE_REPO") == "$digest" ]] || die 'source changed during the build; receipt refused'
    while IFS= read -r -d '' path; do
        name=${path##*/}; sha=$(sha256sum "$path" | cut -d' ' -f1)
        artifacts=$(jq -c --arg name "$name" --arg sha "$sha" '. + {($name):$sha}' <<<"$artifacts")
    done < <(find "$WORK$NATIVE_ARTIFACT_DIR" -maxdepth 1 -type f -executable -print0)
    jq -e 'has("op-grpc-bridge") and has("3tched-fs") and has("opblob")' <<<"$artifacts" >/dev/null || die 'required build artifacts absent'
    tmp=$(mktemp "$WORK/opt/op-build/.execution-receipt.XXXXXX")
    jq -n --arg commit "$commit" --argjson dirty "$dirty" --arg repo "$SOURCE_REPO" --arg digest "$digest" \
        --arg artifact_dir "$NATIVE_ARTIFACT_DIR" --arg at "$(date -u +%FT%TZ)" --argjson artifacts "$artifacts" \
        '{version:1,exit:0,source_commit:$commit,source_dirty:$dirty,source_repo:$repo,source_digest:$digest,artifact_dir:$artifact_dir,artifacts:$artifacts,finished_at:$at}' >"$tmp"
    sync -f "$tmp"; mv -T "$tmp" "$WORK/opt/op-build/execution-receipt.json"; sync -f "$WORK/opt/op-build"
}
# verify_build_receipt BASE [SOURCE_ROOT]: the source digest is checked only when
# SOURCE_ROOT is given (build OS); release snapshots have an empty source mountpoint.
verify_build_receipt() {
    local base=$1 source_root=${2:-} receipt=$1/opt/op-build/execution-receipt.json name sha
    [[ -f $receipt && ! -L $receipt ]] || die 'successful execution receipt required'
    jq -e --arg dir "$NATIVE_ARTIFACT_DIR" '.exit==0 and (.source_commit|test("^[0-9a-f]{40}$")) and (.source_digest|test("^[0-9a-f]{64}$")) and .artifact_dir==$dir and (.artifacts|type=="object") and (.artifacts|has("op-grpc-bridge") and has("3tched-fs") and has("opblob"))' "$receipt" >/dev/null || die 'invalid execution provenance'
    [[ -z $source_root || $(source_digest "$source_root") == "$(jq -er .source_digest "$receipt")" ]] ||
        die "source $source_root differs from execution receipt"
    while IFS=$'\t' read -r name sha; do
        [[ $name =~ ^[A-Za-z0-9_-]+$ && $sha =~ ^[0-9a-f]{64}$ ]] || die 'invalid artifact pin'
        [[ $(sha256sum "$base$NATIVE_ARTIFACT_DIR/$name" | cut -d' ' -f1) == "$sha" ]] || die "artifact changed: $name"
    done < <(jq -r '.artifacts|to_entries[]|[.key,.value]|@tsv' "$receipt")
}
verify_release_inputs() {
    local base=$1 source_root=${2:-} receipt=$1/opt/op-build/execution-receipt.json seal=$1/opt/op-build/catalog-seal-receipt.json
    [[ -f $seal && ! -L $seal ]] || die 'successful catalog seal receipt required'
    verify_build_receipt "$base" "$source_root"
    jq -e --slurpfile build "$receipt" '.exit==0 and .source_commit==$build[0].source_commit and .source_digest==$build[0].source_digest and (.catalog_manifest_sha256|test("^[0-9a-f]{64}$"))' "$seal" >/dev/null || die 'seal receipt does not match the built source'
    [[ $(sha256sum "$base/usr/share/op-dbus/plugin-blobs/.manifest.json" | cut -d' ' -f1) == "$(jq -er .catalog_manifest_sha256 "$seal")" ]] || die 'same-source catalog is not staged in the OS snapshot'
}
# Bind $SOURCE_REPO at $WORK/opt/op-build.src. A retained copy found there is
# renamed once (kept, never deleted); any other mount there is refused.
source_bind_enter() {
    local mp=$WORK$SOURCE_MOUNT retained
    if mountpoint -q -- "$mp"; then
        [[ $(stat -c %d:%i -- "$mp") == "$(stat -c %d:%i -- "$SOURCE_REPO")" ]] ||
            die "$mp is busy with a mount other than $SOURCE_REPO"
        return
    fi
    [[ ! -L $mp ]] || die "$mp is a symlink"
    if [[ -e $mp ]] && [[ ! -d $mp || -n $(find "$mp" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
        retained=$mp.retained-$(date -u +%Y%m%dT%H%M%SZ)
        [[ ! -e $retained && ! -L $retained ]] || die "retained source path exists: $retained"
        mv -T -- "$mp" "$retained"
        sync -f "$WORK/opt"
        log "retained previous build source at $retained"
    fi
    install -d -m 0755 -- "$mp"
    mount --bind -- "$SOURCE_REPO" "$mp"
    BUILD_BINDS+=("$mp")
    mount --make-rslave -- "$mp"
}
build_mounts_leave() { local i; for ((i=${#BUILD_BINDS[@]}-1;i>=0;i--)); do umount -R -- "${BUILD_BINDS[$i]}" || true; done; }
# Call only inside a subshell: binds the source and proc/sys/dev; its EXIT trap
# unmounts only what this run created and leaves pre-existing mounts untouched.
build_mounts_enter() {
    BUILD_BINDS=()
    trap build_mounts_leave EXIT
    source_bind_enter
    for b in proc sys dev; do
        [[ -d $WORK/$b ]] || die "missing chroot directory $b"
        if ! mountpoint -q "$WORK/$b"; then
            mount --rbind "/$b" "$WORK/$b"
            BUILD_BINDS+=("$WORK/$b")
            mount --make-rslave "$WORK/$b"
        fi
    done
}
# Run one build-OS command with the source and proc/sys/dev binds of the build.
in_build_chroot() ( build_mounts_enter; chroot "$WORK" "$@" )
catalog_plugin_ids() { jq -er '.plugins | keys | .[]' "$1/.manifest.json" | sort -u; }
record_catalog_seal() {
    if [[ $APPLY != 1 ]]; then
        would "verify $SOURCE_REPO digest equals the execution receipt"
        would "seal with build OS $NATIVE_ARTIFACT_DIR/opblob (source bind-mounted) into NEW EMPTY /opt/op-build/sealed-catalog-<execution-source-digest> (inventory candidate)"
        would 'retain any staged build-OS catalog, compare IDs with it and read-only host persisted catalog; stop on removals'
        would 'only then stage into build-OS /usr/share/op-dbus/plugin-blobs and record catalog-seal-receipt.json'
        return
    fi
    require_lifecycle_lock
    assert_no_cargo
    local receipt=$WORK/opt/op-build/execution-receipt.json seal=$WORK/opt/op-build/catalog-seal-receipt.json
    local host_persisted=/var/lib/opdbus/plugin-blobs
    local digest commit opblob_sha catalog publication staged=/usr/share/op-dbus/plugin-blobs manifest_sha tmp
    local stamp before='' candidate_ids staged_ids='' host_ids='' removed_staged removed_host report
    if [[ -f $seal && ! -L $seal ]] && (verify_release_inputs "$WORK" "$SOURCE_REPO") >/dev/null 2>&1; then
        log 'existing catalog seal receipt and staged catalog verified'
        return
    fi
    verify_build_receipt "$WORK" "$SOURCE_REPO"
    digest=$(jq -er .source_digest "$receipt")
    commit=$(jq -er .source_commit "$receipt")
    opblob_sha=$(jq -er .artifacts.opblob "$receipt")
    catalog=/opt/op-build/sealed-catalog-$digest
    publication=/usr/share/op-dbus/.catalog-publication-$digest
    [[ ! -e $WORK$catalog && ! -L $WORK$catalog ]] || die "candidate catalog already exists: $WORK$catalog; refusing to reseal"
    [[ ! -e $WORK$publication && ! -L $WORK$publication ]] || die 'catalog publication directory already exists; reconcile before sealing'
    [[ ! -L $WORK$staged ]] || die 'staged build-OS catalog is a symlink'
    capture_pair
    # 1. The candidate is sealed into a NEW EMPTY directory: its sweep can only
    # touch that directory. Nothing else is written before review.
    in_build_chroot env THREETCHED_CAPTURE_BIN="$CAPTURE_BIN" THREETCHED_CAPTURE_MANIFEST="$CAPTURE_MANIFEST" \
        THREETCHED_CONFIG_DIR="$CAPTURE_CONFIG" HOME=/root PATH=/usr/local/bin:/usr/bin:/bin \
        "$NATIVE_ARTIFACT_DIR/opblob" seal-plugins "$catalog"
    manifest_sha=$(sha256sum "$WORK$catalog/.manifest.json" | cut -d' ' -f1)
    candidate_ids=$(catalog_plugin_ids "$WORK$catalog") || die 'candidate manifest has no plugin IDs'
    # 2. Retain the existing staged build-OS catalog before any comparison/staging.
    stamp=$(date -u +%Y%m%dT%H%M%SZ)
    if [[ -d $WORK$staged ]]; then
        before=/opt/op-build/catalog-before-seal-$stamp
        [[ ! -e $WORK$before ]] || die 'retained catalog path collision'
        cp -a -- "$WORK$staged" "$WORK$before"
        sync -f "$WORK$before"
        staged_ids=$(catalog_plugin_ids "$WORK$before") || die 'existing staged catalog manifest is unreadable'
    fi
    # Host persisted catalog is read only, never written.
    if [[ -f $host_persisted/.manifest.json && ! -L $host_persisted/.manifest.json ]]; then
        host_ids=$(catalog_plugin_ids "$host_persisted") || die 'host persisted manifest is unreadable'
    fi
    removed_staged=$(comm -23 <(printf '%s\n' "$staged_ids" | sed '/^$/d') <(printf '%s\n' "$candidate_ids"))
    removed_host=$(comm -23 <(printf '%s\n' "$host_ids" | sed '/^$/d') <(printf '%s\n' "$candidate_ids"))
    report=$WORK/opt/op-build/catalog-seal-comparison-$digest.json
    jq -n --arg candidate "$catalog" --arg before "$before" --arg manifest "$manifest_sha" \
        --arg candidate_ids "$candidate_ids" --arg staged_ids "$staged_ids" --arg host_ids "$host_ids" \
        --arg removed_staged "$removed_staged" --arg removed_host "$removed_host" --arg at "$(date -u +%FT%TZ)" '
        def ids: split("\n") | map(select(length>0));
        {version:1,candidate_catalog_dir:$candidate,candidate_manifest_sha256:$manifest,
         retained_staged_copy:(if $before=="" then null else $before end),
         candidate_ids:($candidate_ids|ids),staged_ids:($staged_ids|ids),host_persisted_ids:($host_ids|ids),
         removed_from_staged:($removed_staged|ids),removed_from_host_persisted:($removed_host|ids),
         added_vs_host_persisted:(($candidate_ids|ids)-($host_ids|ids)),compared_at:$at}' >"$report"
    sync -f "$report"
    if [[ -n $removed_staged || -n $removed_host ]]; then
        printf 'golden: removals detected; seal stopped before staging\n' >&2
        printf '  removed from staged build-OS catalog: %s\n' "$(printf '%s' "$removed_staged" | tr '\n' ' ')" >&2
        printf '  removed from host persisted catalog: %s\n' "$(printf '%s' "$removed_host" | tr '\n' ' ')" >&2
        printf '  candidate: %s\n  retained copy: %s\n  report: %s\n' "$WORK$catalog" "${before:+$WORK$before}" "$report" >&2
        exit 3
    fi
    # 3. No removals: stage into the build OS only, then record the receipt.
    chroot "$WORK" mkdir -p /usr/share/op-dbus
    chroot "$WORK" mkdir -m 0700 "$publication"
    # persist opens directories via /proc/self/fd, so it needs the build binds.
    in_build_chroot env OPDBUS_RELEASE_CATALOG_STAGE="$publication" \
        "$NATIVE_ARTIFACT_DIR/opblob" persist "$catalog" "$staged"
    [[ $(sha256sum "$WORK$staged/.manifest.json" | cut -d' ' -f1) == "$manifest_sha" ]] ||
        die 'staged catalog manifest differs from the isolated seal'
    tmp=$(mktemp "$WORK/opt/op-build/.catalog-seal-receipt.XXXXXX")
    jq -n --arg commit "$commit" --arg digest "$digest" --arg manifest "$manifest_sha" \
        --arg opblob "$opblob_sha" --arg catalog "$catalog" --arg staged "$staged" \
        --arg before "$before" --arg report "${report#"$WORK"}" --arg at "$(date -u +%FT%TZ)" \
        '{version:1,exit:0,source_commit:$commit,source_digest:$digest,catalog_manifest_sha256:$manifest,opblob_sha256:$opblob,catalog_source_dir:$catalog,staged_catalog_dir:$staged,retained_previous_staged_catalog:(if $before=="" then null else $before end),id_comparison:$report,finished_at:$at}' >"$tmp"
    sync -f "$tmp"; mv -T "$tmp" "$seal"; sync -f "$WORK/opt/op-build"
    verify_release_inputs "$WORK" "$SOURCE_REPO"
    log "catalog seal recorded: $seal"
}
