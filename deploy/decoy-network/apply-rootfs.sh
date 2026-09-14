#!/bin/sh
# Apply the release-owned decoy service and canonical persisted catalog to an
# offline GNU/Linux rootfs. Credentials are explicit external inputs and never
# enter the source tree or artifact.
set -eu

die() {
    printf 'decoy-network rootfs: %s\n' "$*" >&2
    exit 1
}

[ "$#" -eq 7 ] ||
    die "usage: $0 ROOTFS ARTIFACT_ROOTFS ENV_FILE CA_FILE CERT_FILE KEY_FILE BRIDGE_CERT_FILE"
[ "${OP_DECOY_OFFLINE_ROOTFS:-}" = 1 ] ||
    die "set OP_DECOY_OFFLINE_ROOTFS=1 for prepared-image application"

ROOTFS=$(readlink -f "$1")
ARTIFACT_ROOTFS=$(readlink -f "$2")
ENV_FILE=$(readlink -f "$3")
CA_FILE=$(readlink -f "$4")
CERT_FILE=$(readlink -f "$5")
KEY_FILE=$(readlink -f "$6")
BRIDGE_CERT_FILE=$(readlink -f "$7")
[ "$ROOTFS" != / ] || die "refusing the host rootfs"
[ -f "$ROOTFS/etc/os-release" ] || die "target is not a prepared Linux rootfs"
[ -x "$ARTIFACT_ROOTFS/usr/local/bin/op-decoy-network" ] ||
    die "artifact is missing op-decoy-network"
[ -x "$ARTIFACT_ROOTFS/usr/local/bin/opblob" ] || die "artifact is missing opblob"
for source in "$ENV_FILE" "$CA_FILE" "$CERT_FILE" "$KEY_FILE" "$BRIDGE_CERT_FILE"; do
    [ -f "$source" ] || die "external release input is absent: $source"
    [ ! -L "$source" ] || die "external release input must not be a symlink: $source"
done

# Validate the trusted deployment environment before copying it. It is sourced
# by the service at boot, so preparation evaluates the same required contract.
(
    unset OP_NETWORK_ROLE OP_DECOY_NETWORK_INTERFACES OP_DECOY_NATIVE_LISTEN \
        OP_DECOY_NATIVE_CA_FILE OP_DECOY_NATIVE_CERT_FILE OP_DECOY_NATIVE_KEY_FILE \
        OP_DECOY_NATIVE_BRIDGE_CERT_FILE OP_DECOY_NETWORK_COZO_PATH \
        OP_BLOB_CATALOG_DIR
    set -a
    # shellcheck disable=SC1090
    . "$ENV_FILE"
    set +a
    [ "${OP_NETWORK_ROLE:-}" = decoy ]
    [ -n "${OP_DECOY_NETWORK_INTERFACES:-}" ]
    [ -n "${OP_DECOY_NATIVE_LISTEN:-}" ]
    [ "${OP_DECOY_NATIVE_CA_FILE:-}" = /etc/op-dbus/decoy-native/ca.pem ]
    [ "${OP_DECOY_NATIVE_CERT_FILE:-}" = /etc/op-dbus/decoy-native/decoy.pem ]
    [ "${OP_DECOY_NATIVE_KEY_FILE:-}" = /etc/op-dbus/decoy-native/decoy-key.pem ]
    [ "${OP_DECOY_NATIVE_BRIDGE_CERT_FILE:-}" = /etc/op-dbus/decoy-native/bridge.pem ]
    [ "${OP_DECOY_NETWORK_COZO_PATH:-}" = /var/lib/op-dbus/decoy-network-cozo ]
    [ "${OP_BLOB_CATALOG_DIR:-/dev/shm/opdbus/plugin-blobs}" = /dev/shm/opdbus/plugin-blobs ]
) || die "deployment environment does not satisfy the decoy service contract"

for binary in op-decoy-network opblob; do
    install -Dm755 "$ARTIFACT_ROOTFS/usr/local/bin/$binary" "$ROOTFS/usr/local/bin/$binary"
done
runtime_destination="$ROOTFS/usr/local/lib/op-decoy-network"
case "$runtime_destination" in "$ROOTFS"/usr/local/lib/op-decoy-network) ;; *) die "unsafe runtime destination" ;; esac
[ ! -e "$runtime_destination" ] || find "$runtime_destination" -depth -delete
install -d -m 0755 "$runtime_destination/lib"
find "$ARTIFACT_ROOTFS/usr/local/lib/op-decoy-network" -type f -print |
while IFS= read -r source; do
    relative=${source#"$ARTIFACT_ROOTFS/usr/local/lib/op-decoy-network/"}
    case "$relative" in
        lib/*|ld-linux-x86-64.so.2|op-decoy-network.bin|opblob.bin)
            install -Dm755 "$source" "$ROOTFS/usr/local/lib/op-decoy-network/$relative"
            ;;
        *) die "unexpected runtime bundle member: $relative" ;;
    esac
done

install -Dm755 "$ARTIFACT_ROOTFS/etc/runit/sv/op-decoy-network/run" \
    "$ROOTFS/etc/runit/sv/op-decoy-network/run"
install -Dm600 "$ENV_FILE" "$ROOTFS/etc/op-dbus/decoy-network.env"
install -Dm644 "$CA_FILE" "$ROOTFS/etc/op-dbus/decoy-native/ca.pem"
install -Dm644 "$CERT_FILE" "$ROOTFS/etc/op-dbus/decoy-native/decoy.pem"
install -Dm600 "$KEY_FILE" "$ROOTFS/etc/op-dbus/decoy-native/decoy-key.pem"
install -Dm644 "$BRIDGE_CERT_FILE" "$ROOTFS/etc/op-dbus/decoy-native/bridge.pem"
install -d -m 0700 "$ROOTFS/var/lib/op-dbus/decoy-network-cozo"

catalog_source="$ARTIFACT_ROOTFS/var/lib/opdbus/plugin-blobs"
[ -f "$catalog_source/.manifest.json" ] || die "artifact has no persisted catalog manifest"
catalog_destination="$ROOTFS/var/lib/opdbus/plugin-blobs"
catalog_stage="$ROOTFS/var/lib/opdbus/.plugin-blobs.$$"
trap 'find "$catalog_stage" -depth -delete 2>/dev/null || true' EXIT HUP INT TERM
install -d -m 0755 "$catalog_stage"
for blob in "$catalog_source"/*.blob; do
    [ -f "$blob" ] || die "artifact persisted catalog contains no blobs"
    install -m 0644 "$blob" "$catalog_stage/$(basename -- "$blob")"
done
install -m 0644 "$catalog_source/.manifest.json" "$catalog_stage/.manifest.json"
[ ! -e "$catalog_destination" ] || find "$catalog_destination" -depth -delete
mv -- "$catalog_stage" "$catalog_destination"
trap - EXIT HUP INT TERM

install -d -m 0755 "$ROOTFS/etc/runit/runsvdir/default"
ln -sfn /etc/runit/sv/op-decoy-network \
    "$ROOTFS/etc/runit/runsvdir/default/op-decoy-network"
