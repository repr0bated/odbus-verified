#!/bin/sh
# Run only after deployment and explicit owner authorization. No live host writes.
set -eu
repo=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
fixture=$(mktemp -d)
trap 'rm -rf -- "$fixture"' EXIT HUP INT TERM
PROJECT_ROOT=$repo
RELEASE_DIR=$fixture/release
GOLDEN_DIR=$fixture/golden
DRY_RUN=0
mkdir "$RELEASE_DIR" "$GOLDEN_DIR"
# A regular fixture binary exercises packaging without executing a controller.
printf 'prepared-controller-fixture\n' > "$RELEASE_DIR/op-release"
die() { printf '%s\n' "$*" >&2; exit 1; }
remove_owned() { rm -rf -- "$1"; }
install_file() { mkdir -p -- "$(dirname -- "$3")"; install -m "$1" -- "$2" "$3"; }
. "$repo/deploy/lib/golden/system-payload.sh"
validate_system_inputs
stage_system_payload
jq -e '.version == 1 and
    (.files[-1].path == "/usr/local/libexec/opdbus-release/manifest.sha256") and
    ([.files[] | select(.install_if_absent == true) | .path] ==
      ["/etc/sv/op-3tchedfs-recovery/run", "/etc/sv/op-3tchedfs-recovery/down"]) and
    all(.files[]; (.source | startswith("system/")) and
      (.sha256 | test("^[0-9a-f]{64}$")))' "$GOLDEN_DIR/system-files.json" >/dev/null
jq -r '.files[] | [.source, .sha256, (.mode | tostring)] | @tsv' "$GOLDEN_DIR/system-files.json" |
while read -r source expected mode; do
    [ -f "$GOLDEN_DIR/$source" ] && [ ! -L "$GOLDEN_DIR/$source" ]
    actual=$(sha256sum "$GOLDEN_DIR/$source")
    [ "${actual%% *}" = "$expected" ]
    [ "$((0$(stat -c %a "$GOLDEN_DIR/$source")))" -eq "$mode" ]
done
[ "$(wc -l < "$GOLDEN_DIR/system/release/manifest.sha256")" -eq 3 ]
for item in 'op-release /usr/local/libexec/opdbus-release/op-release' \
            'op-release-launch /usr/local/libexec/opdbus-release/op-release-launch' \
            'op-release-activate /etc/init.d/op-release-activate'; do
    set -- $item
    digest=$(sha256sum "$GOLDEN_DIR/system/release/$1")
    grep -Fxq "${digest%% *}  $2" "$GOLDEN_DIR/system/release/manifest.sha256"
done
[ ! -s "$GOLDEN_DIR/system/services/op-3tchedfs-recovery/down" ]
clear_system_payload
[ ! -e "$GOLDEN_DIR/system-files.json" ] && [ ! -e "$GOLDEN_DIR/system" ]
mv "$RELEASE_DIR/op-release" "$RELEASE_DIR/controller"
ln -s controller "$RELEASE_DIR/op-release"
if (validate_system_inputs); then die 'symlink controller accepted'; fi
# Invalid verbs/flags must fail before installed-kit or host-state access.
for verb in release status provision-slots rollback abort boot; do
    if sh "$repo/deploy/release/op-release-launch" "$verb" --system; then
        die "unsupported --system accepted for $verb"
    else
        [ "$?" -eq 2 ]
    fi
done
if sh "$repo/deploy/release/op-release-launch" release; then
    die 'retired release alias accepted'
else
    [ "$?" -eq 2 ]
fi
printf '%s\n' 'system payload packaging regression checks passed'
