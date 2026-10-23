#!/bin/sh
# Regression tests for the nlm wrapper/venv helpers in build-golden.sh.
# These deliberately extract only the helpers; the deployment entry point is
# never sourced or executed.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRIPT="$SCRIPT_DIR/build-golden.sh"
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/build-golden-nlm-test.XXXXXX")
trap 'rm -rf "$TEST_ROOT"' EXIT HUP INT TERM

fail() { printf 'not ok: %s\n' "$*" >&2; exit 1; }
assert_file() { [ -f "$1" ] || fail "missing file: $1"; }
assert_eq() {
    [ "$1" = "$2" ] || fail "expected <$1>, got <$2>"
}
run() {
    if [ "$DRY_RUN" = 1 ]; then
        printf 'would run: %s\n' "$*"
    else
        "$@"
    fi
}
die() { printf '%s\n' "$*" >&2; exit 1; }

# Keep this extraction bounded to the two helpers under test. In particular,
# do not source build-golden.sh: its top-level code validates artifacts and
# prepares the golden subvolume.
HELPERS=$(awk '
    /^write_nlm_wrapper\(\)/ { capture = 1 }
    capture && /^\[ -r "\$EMQX_VERSION_FILE" \]/ { exit }
    capture { print }
' "$SCRIPT")

export NLM_CLI_ROOT=/opt/opdbus
NOTEBOOKLM_MCP_INSTALL_NAME=nlm-cli-1.2.3-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
NOTEBOOKLM_MCP_SHA256=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
NOTEBOOKLM_MCP_ARTIFACT_PATH="$TEST_ROOT/provider.whl"
DRY_RUN=0

eval "$(printf '%s\n' "$HELPERS")"

# A wrapper invokes the target runtime path and preserves every shell argument.
runtime="$TEST_ROOT/runtime with spaces"
mkdir -p "$runtime/bin"
logfile="$TEST_ROOT/python-args"
cat > "$runtime/bin/python" <<'EOF'
#!/bin/sh
for arg do printf '<%s>\n' "$arg"; done
EOF
chmod 0755 "$runtime/bin/python"
wrapper="$TEST_ROOT/nlm"
write_nlm_wrapper "$wrapper" "$runtime"
assert_file "$wrapper"
assert_eq "$(sed -n '2p' "$wrapper")" "exec \"$runtime/bin/python\" -I -m notebooklm_tools.cli.main \"\$@\""
"$wrapper" 'one two' 'quote"$and spaces' > "$logfile"
assert_eq "$(sed -n '1p' "$logfile")" '<-I>'
assert_eq "$(sed -n '2p' "$logfile")" '<-m>'
assert_eq "$(sed -n '3p' "$logfile")" '<notebooklm_tools.cli.main>'
assert_eq "$(sed -n '4p' "$logfile")" '<one two>'
assert_eq "$(sed -n '5p' "$logfile")" '<quote"$and spaces>'

# Dry-run must not create either a venv or wrapper.
dry_root="$TEST_ROOT/dry-root"
dry_wrapper="$TEST_ROOT/dry-nlm"
DRY_RUN=1
install_nlm_cli "$dry_root" "$dry_wrapper" >/dev/null
DRY_RUN=0
[ ! -e "$dry_root" ] || fail "dry-run created destination tree"
[ ! -e "$dry_wrapper" ] || fail "dry-run created wrapper"

# Golden staging uses the target runtime path in its wrapper, not its staging
# prefix (which would be unusable after the received release is activated).
golden_root="$TEST_ROOT/golden"
golden_venv="$golden_root$NLM_CLI_ROOT/$NOTEBOOKLM_MCP_INSTALL_NAME"
mkdir -p "$golden_venv/bin" "$golden_root/bin"
printf '%s\n' "$NOTEBOOKLM_MCP_SHA256" > "$golden_venv/.opdbus-artifact-sha256"
cp "$runtime/bin/python" "$golden_venv/bin/python"
chmod 0755 "$golden_venv/bin/python"
golden_wrapper="$golden_root/bin/nlm"
install_nlm_cli "$golden_root" "$golden_wrapper"
assert_eq "$(sed -n '2p' "$golden_wrapper")" \
    "exec \"$NLM_CLI_ROOT/$NOTEBOOKLM_MCP_INSTALL_NAME/bin/python\" -I -m notebooklm_tools.cli.main \"\$@\""

# Existing invalid content-addressed trees are rejected and preserved.
invalid_root="$TEST_ROOT/invalid-root"
invalid_venv="$invalid_root$NLM_CLI_ROOT/$NOTEBOOKLM_MCP_INSTALL_NAME"
mkdir -p "$invalid_venv"
printf 'preserve\n' > "$invalid_venv/sentinel"
if (install_nlm_cli "$invalid_root" "$TEST_ROOT/invalid-nlm") 2>/dev/null; then
    fail "accepted invalid existing venv tree"
fi
assert_file "$invalid_venv/sentinel"
assert_eq "$(sed -n '1p' "$invalid_venv/sentinel")" preserve

# Existing wrapper symlinks are never followed or replaced.
symlink_target="$TEST_ROOT/symlink-target"
printf 'target\n' > "$symlink_target"
symlink_wrapper="$TEST_ROOT/symlink-wrapper"
ln -s "$symlink_target" "$symlink_wrapper"
if (write_nlm_wrapper "$symlink_wrapper" "$runtime") 2>/dev/null; then
    fail "accepted wrapper symlink"
fi
assert_eq "$(sed -n '1p' "$symlink_target")" target

# A failed venv validation leaves the old wrapper byte-for-byte unchanged.
fake_bin="$TEST_ROOT/fake-bin"
mkdir -p "$fake_bin"
cat > "$fake_bin/python3" <<'EOF'
#!/bin/sh
if [ "$1" = -m ] && [ "$2" = venv ]; then
    mkdir -p "$3/bin"
    cat > "$3/bin/python" <<'PYEOF'
#!/bin/sh
case "$*" in
    *notebooklm_tools.cli.main*) exit 1 ;;
    *) exit 0 ;;
esac
PYEOF
    chmod 0755 "$3/bin/python"
    exit 0
fi
exit 1
EOF
chmod 0755 "$fake_bin/python3"
failure_root="$TEST_ROOT/failure-root"
failure_wrapper="$TEST_ROOT/failure-nlm"
mkdir -p "$failure_root"
printf 'old wrapper\n' > "$failure_wrapper"
before=$(cksum "$failure_wrapper")
if (PATH="$fake_bin:$PATH" install_nlm_cli "$failure_root" "$failure_wrapper") 2>/dev/null; then
    fail "accepted failed nlm validation"
fi
assert_eq "$(cksum "$failure_wrapper")" "$before"
assert_eq "$(sed -n '1p' "$failure_wrapper")" 'old wrapper'

printf 'ok: build-golden nlm helper regressions\n'
