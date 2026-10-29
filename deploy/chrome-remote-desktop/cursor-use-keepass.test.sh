#!/bin/sh
# Exercises cursor-use-keepass against a fake home. No database is opened.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
HELPER=$ROOT/cursor-use-keepass
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

BIN=$WORKDIR/bin
mkdir -p "$BIN"
cat > "$BIN/keepassxc" <<'EOF'
#!/bin/sh
exit 0
EOF
chmod +x "$BIN/keepassxc"

USER_NAME=$(id -un)
GROUP_NAME=$(id -gn)
HOME_DIR=$WORKDIR/home
mkdir -p "$HOME_DIR"

# Existing files must survive, with only the KeePassXC keys changed.
mkdir -p "$HOME_DIR/.cursor" "$HOME_DIR/.config/keepassxc"
cat > "$HOME_DIR/.cursor/argv.json" <<'EOF'
// comment stays
{
	"enable-crash-reporter": true,
	"password-store": "kwallet6"
}
EOF
cat > "$HOME_DIR/.config/kwalletrc" <<'EOF'
[Wallet]
Enabled=true

[KSecretD]
Enabled=true
Foo=bar
EOF
cat > "$HOME_DIR/.config/keepassxc/keepassxc.ini" <<'EOF'
[General]
ConfigVersion=2

[FdoSecrets]
Enabled=false
ShowNotification=true
EOF

PATH="$BIN:/usr/bin:/bin" "$HELPER" "$HOME_DIR" "$USER_NAME" "$GROUP_NAME"

grep -q '"password-store": "gnome-libsecret"' "$HOME_DIR/.cursor/argv.json"
grep -q '"enable-crash-reporter": true' "$HOME_DIR/.cursor/argv.json"
grep -q 'comment stays' "$HOME_DIR/.cursor/argv.json"
grep -q '^Enabled=true$' "$HOME_DIR/.config/kwalletrc"
awk '
    /^\[KSecretD\]/ { s=1; next }
    /^\[/ { s=0 }
    s && $0 == "Enabled=false" { found=1 }
    s && $0 == "Foo=bar" { foo=1 }
    END { exit !(found && foo) }
' "$HOME_DIR/.config/kwalletrc"
awk '
    /^\[FdoSecrets\]/ { s=1; next }
    /^\[/ { s=0 }
    s && $0 == "Enabled=true" { found=1 }
    s && $0 == "ShowNotification=true" { note=1 }
    END { exit !(found && note) }
' "$HOME_DIR/.config/keepassxc/keepassxc.ini"
grep -q '^LaunchAtStartup=true$' "$HOME_DIR/.config/keepassxc/keepassxc.ini"
grep -q "^Exec=$BIN/keepassxc$" \
    "$HOME_DIR/.local/share/dbus-1/services/org.freedesktop.secrets.service"
grep -q "^Exec=$BIN/keepassxc$" \
    "$HOME_DIR/.config/autostart/org.keepassxc.KeePassXC.desktop"
test "$(stat -c '%U:%G' "$HOME_DIR/.cursor/argv.json")" = "$USER_NAME:$GROUP_NAME"

# Second run does not duplicate keys.
PATH="$BIN:/usr/bin:/bin" "$HELPER" "$HOME_DIR" "$USER_NAME" "$GROUP_NAME"
test "$(grep -c '"password-store"' "$HOME_DIR/.cursor/argv.json")" -eq 1
test "$(awk '/^\[KSecretD\]/{s=1;next} /^\[/{s=0} s && /^Enabled=/{c++} END{print c+0}' \
    "$HOME_DIR/.config/kwalletrc")" -eq 1

# Missing keepassxc leaves the home untouched.
EMPTY=$WORKDIR/empty-home
MISSING_BIN=$WORKDIR/missing
mkdir -p "$EMPTY" "$MISSING_BIN"
ln -s "$(command -v id)" "$MISSING_BIN/id"
PATH="$MISSING_BIN" "$HELPER" "$EMPTY" "$USER_NAME" "$GROUP_NAME"
test ! -e "$EMPTY/.cursor"
test ! -e "$EMPTY/.config"

# Commented keys and braces are not live settings. Nested settings stay intact.
JSONC=$WORKDIR/jsonc
mkdir -p "$JSONC/.cursor"
cat > "$JSONC/.cursor/argv.json" <<'EOF'
// Example: { "password-store": "kwallet6" }
/* another { brace */
{
    "nested": {"password-store": "keep-nested"},
    "description": "braces { } and // are text",
    "enable-crash-reporter": true,
}
EOF
PATH="$BIN:/usr/bin:/bin" "$HELPER" "$JSONC" "$USER_NAME" "$GROUP_NAME"
grep -q '^// Example: { "password-store": "kwallet6" }$' "$JSONC/.cursor/argv.json"
grep -q '"nested": {"password-store": "keep-nested"}' "$JSONC/.cursor/argv.json"
test "$(grep -c '"password-store": "gnome-libsecret"' "$JSONC/.cursor/argv.json")" -eq 1

# Failed parsing must preserve the original and skip the remaining setup.
INVALID=$WORKDIR/invalid
mkdir -p "$INVALID/.cursor"
printf '%s\n' '{"password-store": false}' > "$INVALID/.cursor/argv.json"
cp "$INVALID/.cursor/argv.json" "$WORKDIR/invalid-original"
if PATH="$BIN:/usr/bin:/bin" "$HELPER" "$INVALID" "$USER_NAME" "$GROUP_NAME"; then
    echo 'invalid password-store unexpectedly accepted' >&2
    exit 1
fi
cmp "$INVALID/.cursor/argv.json" "$WORKDIR/invalid-original"
test ! -e "$INVALID/.config"

# Fresh home gets a valid argv file and does not turn the wallet subsystem off.
FRESH=$WORKDIR/fresh
mkdir -p "$FRESH"
PATH="$BIN:/usr/bin:/bin" "$HELPER" "$FRESH" "$USER_NAME" "$GROUP_NAME"
grep -q '"password-store": "gnome-libsecret"' "$FRESH/.cursor/argv.json"
awk '
    /^\[Wallet\]/ { s=1; next }
    /^\[/ { s=0 }
    s && $0 == "Enabled=false" { bad=1 }
    END { exit bad }
' "$FRESH/.config/kwalletrc"
grep -q '^\[KSecretD\]$' "$FRESH/.config/kwalletrc"
grep -q '^Enabled=false$' "$FRESH/.config/kwalletrc"

echo "cursor-use-keepass.test.sh: ok"
