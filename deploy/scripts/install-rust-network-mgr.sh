#!/bin/sh
# Install the socket-owning Rust network manager into an existing Incus
# identity container.  This project is not installable by crate name alone;
# the pinned Cargo Git source is part of the provisioning contract.
set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CONTRACT="$REPO_ROOT/deploy/config/container-socket-contracts.json"
GIT_URL=https://github.com/sparesparrow/rust-network-mgr
GIT_REV=117087cb1bf99cb55ba3e8e40b9e27752cd08f46
TARGET=x86_64-unknown-linux-musl
CACHE_ROOT=${RUST_NETWORK_MGR_INSTALL_ROOT:-/var/cache/operation-dbus/rust-network-mgr-$GIT_REV}
CARGO_BIN=${CARGO_BIN:-cargo}
if [ "$(id -u)" -eq 0 ] && [ -x /root/.cargo/bin/cargo ]; then
    CARGO_BIN=/root/.cargo/bin/cargo
fi

if [ "${1:-}" = "--all" ]; then
    failures=0
    for container in $(jq -r '.containers[].name' "$CONTRACT"); do
        if ! incus info "$container" >/dev/null 2>&1; then
            echo "$container: skipped (container is absent)"
            continue
        fi
        if ! "$0" "$container"; then
            echo "$container: install failed" >&2
            failures=$((failures + 1))
        fi
    done
    [ "$failures" -eq 0 ]
    exit
fi

CONTAINER="${1:?Incus container name or --all required}"

# Identity containers are deliberately NIC-less, so dependency resolution must
# happen on the host.  The resulting artifact still comes from the required
# `cargo install --git` path and is cached by immutable Git revision.
if [ ! -x "$CACHE_ROOT/bin/rust-network-mgr" ]; then
    install -d -m 0755 "$CACHE_ROOT"
    "$CARGO_BIN" install --git "$GIT_URL" --rev "$GIT_REV" \
        --root "$CACHE_ROOT" --locked --target "$TARGET" rust-network-mgr
fi

incus exec "$CONTAINER" -- install -d -m 0755 /etc/rust-network-mgr
incus file push "$CACHE_ROOT/bin/rust-network-mgr" \
    "$CONTAINER/usr/local/bin/.rust-network-mgr.new" --uid 0 --gid 0 --mode 0755
incus exec "$CONTAINER" -- mv -f \
    /usr/local/bin/.rust-network-mgr.new /usr/local/bin/rust-network-mgr
incus file push "$REPO_ROOT/deploy/rust-network-mgr/config-loopback.yaml" \
    "$CONTAINER/etc/rust-network-mgr/config.yaml" --uid 0 --gid 0 --mode 0644
if incus exec "$CONTAINER" -- sh -c 'command -v systemctl' >/dev/null 2>&1; then
    incus file push "$REPO_ROOT/deploy/rust-network-mgr/rust-network-mgr.service" \
        "$CONTAINER/etc/systemd/system/rust-network-mgr.service" --uid 0 --gid 0 --mode 0644
    incus exec "$CONTAINER" -- systemctl daemon-reload
    incus exec "$CONTAINER" -- systemctl enable rust-network-mgr.service
    incus exec "$CONTAINER" -- systemctl restart rust-network-mgr.service
    STATUS_COMMAND='systemctl status rust-network-mgr.service --no-pager -l'
elif incus exec "$CONTAINER" -- sh -c 'command -v rc-service' >/dev/null 2>&1; then
    incus file push "$REPO_ROOT/deploy/rust-network-mgr/rust-network-mgr.openrc" \
        "$CONTAINER/etc/init.d/rust-network-mgr" --uid 0 --gid 0 --mode 0755
    incus exec "$CONTAINER" -- rc-update add rust-network-mgr default
    incus exec "$CONTAINER" -- rc-service rust-network-mgr restart
    STATUS_COMMAND='rc-service rust-network-mgr status'
else
    echo "$CONTAINER: no supported service supervisor (systemd or OpenRC)" >&2
    exit 1
fi

incus exec "$CONTAINER" -- env STATUS_COMMAND="$STATUS_COMMAND" sh -lc '
    set -eu
    for _ in $(seq 1 50); do
        [ -S /run/rust-network-manager/rust-network-manager.sock ] && exit 0
        sleep 0.2
    done
    sh -c "$STATUS_COMMAND" >&2 || true
    exit 1
'

echo "$CONTAINER: rust-network-mgr socket is ready"
