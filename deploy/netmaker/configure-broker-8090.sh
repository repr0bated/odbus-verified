#!/bin/sh
# Converge the NIC-less NetMaker container on the single MQTT/WebSocket port.
# The container and host have separate network namespaces, so EMQX and the
# host op-grpc-bridge can both own :8090. broker.sock joins the two surfaces.
set -eu

NAME=${NETMAKER_INCUS_NAME:-NetMaker}
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
[ -r /etc/op-dbus/netmaker-broker.env ] && . /etc/op-dbus/netmaker-broker.env
BROKER_PORT=${NETMAKER_BROKER_PORT:-8090}
BROKER_PATH=${NETMAKER_BROKER_PATH:-/mqtt}
EXHOOK_PORT=${NETMAKER_EXHOOK_PORT:-9091}
[ "$BROKER_PORT" = 8090 ] || {
    echo "this deployment requires NETMAKER_BROKER_PORT=8090" >&2
    exit 1
}
[ "$EXHOOK_PORT" = 9091 ] || {
    echo "this deployment requires NETMAKER_EXHOOK_PORT=9091" >&2
    exit 1
}
if [ -f "$SCRIPT_DIR/emqx-ws-8090.hocon" ]; then
    ASSET_DIR=$SCRIPT_DIR
else
    ASSET_DIR=${NETMAKER_ASSET_DIR:-/etc/op-dbus/netmaker}
fi

[ "$(id -u)" -eq 0 ] || {
    echo "configure-broker-8090.sh must run as root" >&2
    exit 1
}

incus list -f csv -c ns | grep -q "^${NAME},RUNNING$" || {
    echo "NetMaker container $NAME is not running" >&2
    exit 1
}

# Install the persistent relay override before changing the live EMQX listener.
incus exec "$NAME" -- mkdir -p /etc/systemd/system/op-uds-relay.service.d
incus file push "$ASSET_DIR/op-uds-relay-8090.conf" \
    "$NAME/etc/systemd/system/op-uds-relay.service.d/8090.conf"
incus file push "$ASSET_DIR/emqx-ws-8090.hocon" \
    "$NAME/run/emqx-ws-8090.hocon"

# These files are the container's existing NetMaker configuration layers. Keep
# their other (including secret) values intact and change only the MQTT/WS port.
incus exec "$NAME" -- sed -i "s#:8083/mqtt#:${BROKER_PORT}${BROKER_PATH}#g" \
    /etc/netmaker/.env /etc/netmaker/netmaker.env /etc/netmaker/config.yaml

# Materialize the host plugin's API credential as runtime-only state. This file
# is deliberately absent from git and the golden image; the container remains
# the secret authority. Validate the values before writing a shell-sourceable
# root-only environment file, then replace it atomically.
API_MASTER_KEY=$(incus exec "$NAME" -- sh -lc \
    'sed -n "s/^MASTER_KEY=//p" /etc/netmaker/netmaker.env | tail -1 | tr -d "\"\047[:space:]"')
API_TENANT_ID=$(incus exec "$NAME" -- sh -lc \
    'sed -n "s/^NETMAKER_TENANT_ID=//p" /etc/netmaker/netmaker.env | tail -1 | tr -d "\"\047[:space:]"')
[ -n "$API_MASTER_KEY" ] || { echo "NetMaker MASTER_KEY is empty" >&2; exit 1; }
[ -n "$API_TENANT_ID" ] || { echo "NetMaker tenant ID is empty" >&2; exit 1; }
case "$API_MASTER_KEY$API_TENANT_ID" in
    *[!A-Za-z0-9._-]*)
        echo "NetMaker API credential contains unsupported environment-file characters" >&2
        exit 1
        ;;
esac
install -d -m 0755 /etc/op-dbus
umask 077
API_ENV_TMP=$(mktemp /etc/op-dbus/netmaker-api.env.XXXXXX)
trap 'rm -f "$API_ENV_TMP"' EXIT HUP INT TERM
{
    printf 'NETMAKER_API_BASE=http://127.0.0.1:8081\n'
    printf 'NETMAKER_MASTER_KEY=%s\n' "$API_MASTER_KEY"
    printf 'NETMAKER_TENANT_ID=%s\n' "$API_TENANT_ID"
} > "$API_ENV_TMP"
chmod 0600 "$API_ENV_TMP"
mv -f "$API_ENV_TMP" /etc/op-dbus/netmaker-api.env
trap - EXIT HUP INT TERM
unset API_MASTER_KEY API_TENANT_ID

# EMQX persists this cluster configuration update in its data directory.
incus exec "$NAME" -- emqx ctl conf load --merge /run/emqx-ws-8090.hocon

# Container/application lifecycle goes through D-Bus, never systemctl.
incus exec "$NAME" -- busctl call \
    org.freedesktop.systemd1 /org/freedesktop/systemd1 \
    org.freedesktop.systemd1.Manager Reload
incus exec "$NAME" -- busctl call \
    org.freedesktop.systemd1 /org/freedesktop/systemd1 \
    org.freedesktop.systemd1.Manager RestartUnit ss op-uds-relay.service replace
incus exec "$NAME" -- busctl call \
    org.freedesktop.systemd1 /org/freedesktop/systemd1 \
    org.freedesktop.systemd1.Manager RestartUnit ss netmaker.service replace

i=0
until [ -S "${NETMAKER_BROKER_SOCKET:-/run/ghostbridge/NetMaker/broker.sock}" ] && \
    incus exec "$NAME" -- sh -c \
        "ss -lnt | grep -q ':${BROKER_PORT} ' && ss -lnt | grep -q '127.0.0.1:${EXHOOK_PORT} '"; do
    i=$((i + 1))
    [ "$i" -ge 30 ] && {
        echo "NetMaker broker did not converge on :8090" >&2
        exit 1
    }
    sleep 1
done

EXHOOK_CONF=$(incus exec "$NAME" -- emqx ctl conf show exhook)
printf '%s\n' "$EXHOOK_CONF" | grep -q "127.0.0.1:${EXHOOK_PORT}" || {
    echo "EMQX ExHook did not retain the Ghostbridge adapter URL" >&2
    exit 1
}
printf '%s\n' "$EXHOOK_CONF" | grep -q 'failed_action = ignore' || {
    echo "EMQX ExHook did not retain failed_action=ignore" >&2
    exit 1
}

echo "NetMaker EMQX converged on :${BROKER_PORT}${BROKER_PATH}; Ghostbridge ExHook adapter is 127.0.0.1:${EXHOOK_PORT}"
