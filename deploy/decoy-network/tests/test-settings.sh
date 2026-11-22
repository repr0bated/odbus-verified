#!/bin/bash
# Exact loader/selector with disposable root-owned fixtures; no real service/catalog.
set -euo pipefail
if [[ $(id -u) != 0 ]]; then exec sudo -n bash "$0"; fi
here=$(cd -- "$(dirname -- "$0")/.." && pwd)
h=$(mktemp -d); trap 'rm -rf "$h"' EXIT
mkdir -m 0700 "$h/env" "$h/bin" "$h/a" "$h/b"
for r in a b; do
  mkdir -p "$h/$r/usr/local/bin"
  cat >"$h/$r/usr/local/bin/opblob" <<'STAGE'
#!/bin/sh
[ "$1" = stage-shm ] || exit 1
printf 'stage\n' >>"$GOLDEN_TEST_LOG"
ln -sfn "$GOLDEN_TEST_ROOT_B" "$GOLDEN_TEST_SELECTOR"
STAGE
  printf '#!/bin/sh\n[ -z "${KIROWEEN_WIREGUARD_PRIVATE_KEY+x}" ] || exit 1\nprintf "owner-%s\\n" >>"$GOLDEN_TEST_LOG"\n' "$r" >"$h/$r/usr/local/bin/op-decoy-network"
  chmod 0755 "$h/$r/usr/local/bin/"*
done
printf '#!/bin/sh\nexit 0\n' >"$h/bin/install"; chmod +x "$h/bin/install"
ln -s "$h/a" "$h/current"
for pair in 'op-network-role=decoy' 'op-decoy-network-interfaces=wg-opdbus' 'op-decoy-native-listen=10.0.0.1:51889' 'op-decoy-native-ca-file=/fixture/ca' 'op-decoy-native-cert-file=/fixture/cert' 'op-decoy-native-key-file=/fixture/key' 'op-decoy-native-bridge-cert-file=/fixture/bridge' 'op-decoy-network-cozo-path=/fixture/cozo'; do
  printf '%s\n' "${pair#*=}" >"$h/env/${pair%%=*}"
done
printf '%s\n' "$h/current" >"$h/env/op-decoy-native-release-root"
printf 'fixture-secret-never-imported\n' >"$h/env/kiroween-wireguard-private-key"
chmod 0600 "$h/env/"*
run() {
  env -i PATH="$h/bin:/usr/bin:/bin" OP_DECOY_NETWORK_SETTINGS_DIR="$h/env" OP_DECOY_NETWORK_ENV_FILE="$h/legacy" \
    GOLDEN_TEST_LOG="$h/log" GOLDEN_TEST_ROOT_B="$h/b" GOLDEN_TEST_SELECTOR="$h/current" sh "$here/run"
}
run
[[ $(cat "$h/log") == $'stage\nowner-a' && $(readlink "$h/current") == "$h/b" ]]
echo 'ok protected per-setting loader ignores unrelated secret and pins one release across selector flip'
rm "$h/log"
chmod 0644 "$h/env/op-decoy-native-key-file"
if run >"$h/rejected.log" 2>&1; then exit 1; fi
[[ ! -e $h/log ]]
echo 'ok unprotected setting rejected before catalog or native owner'
chmod 0600 "$h/env/op-decoy-native-key-file"
mv "$h/env/op-decoy-native-ca-file" "$h/ca.saved"
if run >"$h/rejected.log" 2>&1; then exit 1; fi
[[ ! -e $h/log ]]
echo 'ok incomplete selected per-setting config fails closed'
mv "$h/ca.saved" "$h/env/op-decoy-native-ca-file"
rm "$h/env/op-network-role"
cat >"$h/legacy" <<LEGACY
OP_NETWORK_ROLE=decoy
OP_DECOY_NETWORK_INTERFACES=wg-opdbus
OP_DECOY_NATIVE_LISTEN=10.0.0.1:51889
OP_DECOY_NATIVE_CA_FILE=/fixture/ca
OP_DECOY_NATIVE_CERT_FILE=/fixture/cert
OP_DECOY_NATIVE_KEY_FILE=/fixture/key
OP_DECOY_NATIVE_BRIDGE_CERT_FILE=/fixture/bridge
OP_DECOY_NETWORK_COZO_PATH=/fixture/cozo
OP_DECOY_NATIVE_RELEASE_ROOT='$h/a'
LEGACY
chmod 0600 "$h/legacy"
run
[[ $(cat "$h/log") == $'stage\nowner-a' ]]
echo 'ok existing protected legacy config remains supported; no aggregate created for new selection'
echo 'native settings regressions passed'
