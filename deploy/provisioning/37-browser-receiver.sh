#!/bin/sh
# Step 3c: give an existing, STOPPED Incus identity container its browser
# receiver (SNI-label path, spec ui-networking-authoritative-2026-11-27).
#   37-browser-receiver.sh <NAME> [UI_ASSETS_DIR]
# - installs the *.perspective.3tched.com certificate for the container UI door
# - points the container UI door at it (env files OP_UI_TLS_CERT_FILE / OP_UI_TLS_KEY_FILE)
# - turns the UI door on at 127.0.0.1:8443 (OP_UI_LISTEN) with CHAT_UI_DIR
# - adds op-browser-receiver.service: op-tunnel-door --lungs-socket <host lungs>
# The existing op-tunnel-door.service (control-plane tunnel) is not touched.
# Every replaced file is kept beside the new one as <file>.pre-<stamp>.
. "$(dirname -- "$0")/lib.sh"; need_root
N=${1:?container}; ASSETS=${2:-}
R=$POOL_DIR/containers/$N/rootfs
CERT_DIR=${PERSPECTIVE_CERT_DIR:-/etc/op-dbus/secrets/perspective/certificates}
CRT=$CERT_DIR/_.perspective.3tched.com.crt; KEY=$CERT_DIR/_.perspective.3tched.com.key
[ "$(incus list -f csv -c s "^$N\$")" = STOPPED ] || fail "$N must be stopped"
[ -d "$R/etc/op-dbus" ] || fail "$R/etc/op-dbus absent"
[ -s "$CRT" ] && [ -s "$KEY" ] || fail "perspective certificate absent in $CERT_DIR"

keep() { [ -e "$1" ] && cp -a "$1" "$1.pre-$STAMP"; return 0; }

install -d -m 0755 "$R/etc/op-dbus/tls"
for pair in "$CRT:perspective.crt:0644" "$KEY:perspective.key:0600"; do
  src=${pair%%:*}; rest=${pair#*:}; dst=$R/etc/op-dbus/tls/${rest%%:*}; mode=${rest#*:}
  cmp -s "$src" "$dst" 2>/dev/null && continue
  keep "$dst"; install -m "$mode" -o 0 -g 0 "$src" "$dst"
done
echo "  certificate installed"

install -d -m 0755 "$R/etc/op-dbus/env"
for setting in "OP_UI_TLS_CERT_FILE:/etc/op-dbus/tls/perspective.crt" "OP_UI_TLS_KEY_FILE:/etc/op-dbus/tls/perspective.key"; do
  file=$R/etc/op-dbus/env/${setting%%:*}
  [ "$(cat "$file" 2>/dev/null)" = "${setting#*:}" ] && continue
  keep "$file"; printf '%s\n' "${setting#*:}" > "$file"; chmod 0600 "$file"
done
echo "  UI door certificate settings written"
if [ "$(cat "$R/etc/op-dbus/env/OP_UI_LISTEN" 2>/dev/null)" != 127.0.0.1:8443 ]; then
  keep "$R/etc/op-dbus/env/OP_UI_LISTEN"
  printf '127.0.0.1:8443\n' > "$R/etc/op-dbus/env/OP_UI_LISTEN"; chmod 0600 "$R/etc/op-dbus/env/OP_UI_LISTEN"
fi
if [ ! -d "$R/usr/local/share/op-browser-ui" ]; then
  [ -n "$ASSETS" ] && [ -f "$ASSETS/index.html" ] || fail "$N has no UI assets; pass UI_ASSETS_DIR"
  install -d -m 0755 "$R/usr/local/share/op-browser-ui"
  cp -a "$ASSETS/." "$R/usr/local/share/op-browser-ui/"
  echo "  UI assets installed"
fi
[ -s "$R/etc/op-dbus/env/CHAT_UI_DIR" ] || { printf '/usr/local/share/op-browser-ui\n' > "$R/etc/op-dbus/env/CHAT_UI_DIR"; chmod 0600 "$R/etc/op-dbus/env/CHAT_UI_DIR"; }
echo "  UI door on 127.0.0.1:8443"

run=$R/usr/local/libexec/op-dbus/browser-receiver-run
keep "$run"
cat > "$run" <<'EOF'
#!/bin/sh
# Serve browser sockets the host pushes over this identity's lungs connection.
set -eu
exec 2>&1
lungs=/run/mounts/opdbus/lungs/lungs.sock
[ -S "$lungs" ] || lungs=/run/opdbus/lungs/lungs.sock
[ -S "$lungs" ] || { echo 'host lungs socket absent'; sleep 5; exit 1; }
exec /usr/local/libexec/op-dbus/bin/op-tunnel-door --lungs-socket "$lungs" --door 127.0.0.1:8443
EOF
chmod 0755 "$run"
unit=$R/etc/systemd/system/op-browser-receiver.service
keep "$unit"
cat > "$unit" <<'EOF'
[Unit]
Description=Receive browser sockets pushed over this identity's lungs and splice them to 127.0.0.1:8443
After=op-grpc-bridge.service
StartLimitIntervalSec=0

[Service]
Type=simple
ExecStart=/usr/local/libexec/op-dbus/browser-receiver-run
Restart=always
RestartSec=2s

[Install]
WantedBy=multi-user.target
EOF
ln -sfn ../op-browser-receiver.service "$R/etc/systemd/system/multi-user.target.wants/op-browser-receiver.service"
echo "  op-browser-receiver.service enabled"
