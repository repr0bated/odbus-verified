#!/usr/bin/env bash
# Bounded, offline native XHTTP/VLESS Encryption fixture tests. The script
# touches only loopback listeners, a per-user runtime UDS, and private /var/tmp
# fixtures; it never reads or changes a live Xray config or service.
set -euo pipefail
umask 077
export LC_ALL=C

fail() {
    printf 'FAIL: %s\n' "$1" >&2
    if [[ -n ${fixture_dir:-} && -d ${fixture_dir:-} ]]; then
        printf 'Private fixture evidence retained at %s\n' "$fixture_dir" >&2
        keep_fixture=true
    fi
    exit 1
}

XRAY=${XRAY:-/usr/local/bin/xray}
PREPARE=${PREPARE:-"$(cd -- "$(dirname -- "$0")" && pwd)/prepare.sh"}
for tool in "$XRAY" "$PREPARE" jq curl ss timeout openssl socat grep awk sed tr; do
    if [[ $tool == */* ]]; then [[ -x $tool ]] || { printf 'Missing executable: %s\n' "$tool" >&2; exit 2; }
    else command -v "$tool" >/dev/null 2>&1 || { printf 'Missing executable: %s\n' "$tool" >&2; exit 2; }
    fi
done
[[ $("$XRAY" version 2>&1) == *'Xray 26.3.27 '* ]] || { printf 'Expected native Xray 26.3.27.\n' >&2; exit 2; }
[[ -x $PREPARE ]] || { printf 'prepare.sh is not executable.\n' >&2; exit 2; }

fixture_dir=$(mktemp -d /var/tmp/od106-xhttp-native.XXXXXXXX)
chmod 0700 "$fixture_dir"
keep_fixture=false
declare -a children=()
cleanup() {
    local pid f
    for pid in "${children[@]}"; do kill "$pid" 2>/dev/null || true; done
    for pid in "${children[@]}"; do wait "$pid" 2>/dev/null || true; done
    if [[ ${keep_fixture:-false} != true && -d ${fixture_dir:-} ]]; then
        for f in "$fixture_dir"/*; do [[ -e $f || -L $f ]] && rm -f -- "$f"; done
        rmdir -- "$fixture_dir" 2>/dev/null || true
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

note() { printf '%s\n' "$1"; }
must_test_config() {
    local label=$1 config=$2 logfile=$3
    if ! timeout 10s "$XRAY" run -test -config "$config" >"$logfile" 2>&1; then
        fail "native Xray -test rejected $label (private diagnostics retained)"
    fi
}
expect_test_config_failure() {
    local label=$1 config=$2 logfile=$3
    if timeout 10s "$XRAY" run -test -config "$config" >"$logfile" 2>&1; then
        fail "native Xray -test accepted invalid $label"
    fi
}
start_bg() {
    local name=$1 logfile=$2
    shift 2
    "$@" >"$logfile" 2>&1 &
    children+=("$!")
    printf '%s\n' "$!" >"$fixture_dir/$name.pid"
}
stop_bg() {
    local name=$1 pid
    pid=$(<"$fixture_dir/$name.pid")
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    local -a remaining=() p
    for p in "${children[@]}"; do [[ $p == "$pid" ]] || remaining+=("$p"); done
    children=("${remaining[@]}")
}
wait_listen() {
    local port=$1 pid=$2 i
    for i in {1..80}; do
        ss -H -lnt "( sport = :$port )" 2>/dev/null | grep -q . && return 0
        kill -0 "$pid" 2>/dev/null || return 1
        sleep 0.1
    done
    return 1
}
reserve_port() {
    local port tries
    for tries in {1..100}; do
        port=$((20000 + RANDOM % 40000))
        if ! ss -H -lnt "( sport = :$port )" 2>/dev/null | grep -q .; then
            printf '%s\n' "$port"
            return 0
        fi
    done
    fail 'could not reserve a free loopback test port'
}
extract_pair() {
    local raw=$1 json=$2 block=$3
    awk -v want="$block" '
        /^Authentication:/ { capture = (index($0, want) > 0); next }
        capture && /^"(decryption|encryption)":/ {
            if (count++) printf ",\n"
            print
        }
        END { if (count != 2) exit 1 }
    ' "$raw" >"$json.body" || fail "native Xray did not emit the expected $block pair"
    { printf '{\n'; cat "$json.body"; printf '\n}\n'; } >"$json"
    jq -e 'has("decryption") and has("encryption")' "$json" >/dev/null || fail "cannot parse native $block pair"
    jq -er '.decryption' "$json" | tr -d '\n' >"$json.decryption"
    jq -er '.encryption' "$json" | tr -d '\n' >"$json.encryption"
    chmod 0600 "$json" "$json.decryption" "$json.encryption"
}
run_http_request() {
    local proxy_port=$1 output=$2 codefile=$3 url=$4
    curl --silent --show-error --max-time 8 --noproxy '' --proxy "http://127.0.0.1:$proxy_port" \
        --output "$output" --write-out '%{http_code}' "$url" >"$codefile" 2>"$codefile.err"
}
assert_fixture_body() {
    local label=$1 body=$2 status=$3
    [[ $(<"$status") == 200 ]] || fail "$label did not return HTTP 200"
    grep -qx 'OD106_FIXTURE_OK' "$body" || fail "$label did not reach the isolated HTTP fixture"
}
assert_fixture_rejected() {
    local label=$1 body=$2 status=$3 curl_status=$4
    if [[ $curl_status == 0 && $(<"$status") == 200 ]] && grep -qx 'OD106_FIXTURE_OK' "$body"; then
        fail "$label reached the protected fixture with invalid authentication"
    fi
}

xhttp_server_port=$(reserve_port)
xhttp_proxy_port=$(reserve_port)
origin_port=$(reserve_port)
while [[ $xhttp_proxy_port == "$xhttp_server_port" ]]; do xhttp_proxy_port=$(reserve_port); done
while [[ $origin_port == "$xhttp_server_port" || $origin_port == "$xhttp_proxy_port" ]]; do origin_port=$(reserve_port); done
xhttp_path="/$(openssl rand -hex 20)"
printf '%s' "$xhttp_path" >"$fixture_dir/xhttp.path"

# Generate disposable credentials using the native CLI. The server decryption
# key and client encryption key come from the same native pair. Xray's documented
# third outbound token selects 1-RTT; only that token is changed from the CLI's
# default 0-RTT output, leaving generated key material intact.
"$XRAY" uuid >"$fixture_dir/client-id"
"$XRAY" vlessenc >"$fixture_dir/vlessenc.out"
extract_pair "$fixture_dir/vlessenc.out" "$fixture_dir/vlessenc-pair.json" 'ML-KEM-768'
jq -er '.encryption | select(contains(".native.0rtt.")) | sub("\\.native\\.0rtt\\."; ".native.1rtt.")' \
    "$fixture_dir/vlessenc-pair.json" >"$fixture_dir/vlessenc-pair.json.encryption"
"$XRAY" vlessenc >"$fixture_dir/vlessenc-wrong.out"
extract_pair "$fixture_dir/vlessenc-wrong.out" "$fixture_dir/vlessenc-wrong-pair.json" 'ML-KEM-768'
grep -q '\.1rtt\.' "$fixture_dir/vlessenc-pair.json.encryption" || fail 'generated client encryption value lacks 1-RTT mode'

jq -n --rawfile client_id "$fixture_dir/client-id" --arg path "$xhttp_path" \
    --rawfile decryption "$fixture_dir/vlessenc-pair.json.decryption" \
    --rawfile encryption "$fixture_dir/vlessenc-pair.json.encryption" \
    '{client_id:($client_id|rtrimstr("\n")),xhttp_path:$path,xhttp_decryption:($decryption|rtrimstr("\n")),xhttp_encryption:($encryption|rtrimstr("\n"))}' \
    >"$fixture_dir/private-params.json"
chmod 0600 "$fixture_dir/private-params.json"

# Host inbound starts with a legacy fallback. The transform must replace it
# with the VLESS Encryption decryption pair and packet-up padded XHTTP settings.
jq -n --rawfile client_id "$fixture_dir/client-id" \
    '{log:{loglevel:"error"},inbounds:[{tag:"xhttp-in",listen:"127.0.0.1",port:8444,protocol:"vless",settings:{clients:[{id:($client_id|rtrimstr("\n"))}],decryption:"none",fallbacks:[{dest:"127.0.0.1:18445",xver:0}]},streamSettings:{network:"xhttp",security:"none",xhttpSettings:{path:"/xhttp",mode:"auto"}}}],outbounds:[{tag:"direct",protocol:"freedom"},{tag:"block",protocol:"blackhole"}],routing:{rules:[{type:"field",inboundTag:["xhttp-in"],outboundTag:"direct"}]}}' \
    >"$fixture_dir/host-xhttp-source.json"
# Decoy outbound starts with XMUX on. The transform selects conservative
# packet-up/XMUX split-download parameters; runtime addresses are loopback only.
jq -n --argjson proxy_port "$xhttp_proxy_port" --rawfile client_id "$fixture_dir/client-id" \
    '{log:{loglevel:"error"},inbounds:[{tag:"fixture-proxy",listen:"127.0.0.1",port:$proxy_port,protocol:"http",settings:{}}],outbounds:[{tag:"tunnel",protocol:"vless",settings:{vnext:[{address:"10.0.0.2",port:8444,users:[{id:($client_id|rtrimstr("\n")),encryption:"none"}]}]},streamSettings:{network:"xhttp",security:"none",xhttpSettings:{path:"/xhttp",mode:"auto"}},mux:{enabled:true}},{tag:"direct",protocol:"freedom"},{tag:"block",protocol:"blackhole"}],routing:{rules:[{type:"field",inboundTag:["fixture-proxy"],outboundTag:"tunnel"}]}}' \
    >"$fixture_dir/decoy-xhttp-source.json"

"$PREPARE" host-xhttp "$fixture_dir/host-xhttp-source.json" "$fixture_dir/private-params.json" "$fixture_dir/host-xhttp.candidate.json" \
    >"$fixture_dir/prepare-host-xhttp.log" 2>&1 || fail 'host XHTTP/VLESS Encryption fixture transform failed'
"$PREPARE" decoy-xhttp "$fixture_dir/decoy-xhttp-source.json" "$fixture_dir/private-params.json" "$fixture_dir/decoy-xhttp.candidate.json" \
    >"$fixture_dir/prepare-decoy-xhttp.log" 2>&1 || fail 'decoy XHTTP/VLESS Encryption fixture transform failed'
must_test_config 'host XHTTP/VLESS Encryption candidate' "$fixture_dir/host-xhttp.candidate.json" "$fixture_dir/test-host-xhttp.log"
must_test_config 'decoy XHTTP/VLESS Encryption candidate' "$fixture_dir/decoy-xhttp.candidate.json" "$fixture_dir/test-decoy-xhttp.log"
note 'PASS: native Xray accepts paired host/decoy XHTTP candidates and the 1-RTT encryption string.'

# Runtime copies constrain both client and split-download sockets to loopback.
jq --argjson port "$xhttp_server_port" \
    '(.inbounds[]|select(.tag=="xhttp-in")|.listen)="127.0.0.1" | (.inbounds[]|select(.tag=="xhttp-in")|.port)=$port' \
    "$fixture_dir/host-xhttp.candidate.json" >"$fixture_dir/host-xhttp.runtime.json"
jq --argjson port "$xhttp_server_port" \
    '(.outbounds[]|select(.tag=="tunnel")|.settings.vnext[0].address)="127.0.0.1" |
     (.outbounds[]|select(.tag=="tunnel")|.settings.vnext[0].port)=$port |
     (.outbounds[]|select(.tag=="tunnel")|.streamSettings.xhttpSettings.extra.downloadSettings.address)="127.0.0.1" |
     (.outbounds[]|select(.tag=="tunnel")|.streamSettings.xhttpSettings.extra.downloadSettings.port)=$port' \
    "$fixture_dir/decoy-xhttp.candidate.json" >"$fixture_dir/decoy-xhttp.runtime.json"
must_test_config 'loopback host XHTTP runtime fixture' "$fixture_dir/host-xhttp.runtime.json" "$fixture_dir/test-host-xhttp-runtime.log"
must_test_config 'loopback decoy XHTTP runtime fixture' "$fixture_dir/decoy-xhttp.runtime.json" "$fixture_dir/test-decoy-xhttp-runtime.log"

# Xray must reject the mutually incompatible legacy VLESS fallback and
# decryption combination; production preparation removes the fallback.
jq '(.inbounds[]|select(.tag=="xhttp-in")|.settings.fallbacks)=[{dest:"127.0.0.1:18445",xver:0}]' \
    "$fixture_dir/host-xhttp.runtime.json" >"$fixture_dir/host-xhttp.invalid-fallback.json"
expect_test_config_failure 'XHTTP decryption plus fallback' "$fixture_dir/host-xhttp.invalid-fallback.json" "$fixture_dir/test-host-xhttp-invalid-fallback.log"
grep -q '"fallbacks" can not be used together with "decryption"' "$fixture_dir/test-host-xhttp-invalid-fallback.log" \
    || fail 'native parser rejected fallback configuration for an unexpected reason'
note 'PASS: native Xray rejects XHTTP VLESS decryption combined with fallbacks.'

# Disposable local HTTP target returns a single deterministic response.
cat >"$fixture_dir/http-response.sh" <<'HTTP'
#!/bin/sh
while IFS= read -r line; do
    case $line in ''|"$(printf '\r')") break;; esac
done
printf 'HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 16\r\nConnection: close\r\n\r\nOD106_FIXTURE_OK'
HTTP
chmod 0700 "$fixture_dir/http-response.sh"
start_bg http-origin "$fixture_dir/http-origin.log" \
    socat "TCP-LISTEN:$origin_port,bind=127.0.0.1,reuseaddr,fork" "EXEC:$fixture_dir/http-response.sh"
origin_pid=$(<"$fixture_dir/http-origin.pid")
wait_listen "$origin_port" "$origin_pid" || fail 'loopback HTTP origin did not listen'

# The tunneled URL's destination is 127.0.0.1:$origin_port, so host Xray's
# freedom outbound reaches only the local socat fixture listener.
start_bg xhttp-server "$fixture_dir/xhttp-server.log" "$XRAY" run -config "$fixture_dir/host-xhttp.runtime.json"
xhttp_server_pid=$(<"$fixture_dir/xhttp-server.pid")
wait_listen "$xhttp_server_port" "$xhttp_server_pid" || fail 'loopback XHTTP/VLESS Encryption server did not listen'
start_bg xhttp-client "$fixture_dir/xhttp-client.log" "$XRAY" run -config "$fixture_dir/decoy-xhttp.runtime.json"
xhttp_client_pid=$(<"$fixture_dir/xhttp-client.pid")
wait_listen "$xhttp_proxy_port" "$xhttp_client_pid" || fail 'loopback XHTTP client proxy did not listen'
if ! run_http_request "$xhttp_proxy_port" "$fixture_dir/xhttp-positive.body" "$fixture_dir/xhttp-positive.status" \
    "http://127.0.0.1:$origin_port/xhttp"; then
    fail 'matching VLESS Encryption/XHTTP pair did not complete the loopback HTTP request'
fi
assert_fixture_body 'matching VLESS Encryption/XHTTP client' "$fixture_dir/xhttp-positive.body" "$fixture_dir/xhttp-positive.status"
note 'PASS: matching ML-KEM VLESS Encryption pair completed padded XHTTP with XMUX split-download.'

# A separate native-generated ML-KEM encryption string must not authenticate.
jq -er '.encryption | select(contains(".native.0rtt.")) | sub("\\.native\\.0rtt\\."; ".native.1rtt.")' \
    "$fixture_dir/vlessenc-wrong-pair.json" >"$fixture_dir/vlessenc-wrong-pair.json.encryption.1rtt"
jq --rawfile wrong "$fixture_dir/vlessenc-wrong-pair.json.encryption.1rtt" \
    '(.outbounds[]|select(.tag=="tunnel")|.settings.vnext[0].users[0].encryption)=($wrong|rtrimstr("\n"))' \
    "$fixture_dir/decoy-xhttp.runtime.json" >"$fixture_dir/decoy-xhttp.bad-encryption.json"
must_test_config 'wrong VLESS Encryption client candidate' "$fixture_dir/decoy-xhttp.bad-encryption.json" "$fixture_dir/test-decoy-xhttp-bad-encryption.log"
stop_bg xhttp-client
start_bg xhttp-bad-client "$fixture_dir/xhttp-bad-client.log" "$XRAY" run -config "$fixture_dir/decoy-xhttp.bad-encryption.json"
bad_xhttp_pid=$(<"$fixture_dir/xhttp-bad-client.pid")
wait_listen "$xhttp_proxy_port" "$bad_xhttp_pid" || fail 'bad-key XHTTP client proxy did not listen'
xhttp_curl_status=0
run_http_request "$xhttp_proxy_port" "$fixture_dir/xhttp-negative.body" "$fixture_dir/xhttp-negative.status" \
    "http://127.0.0.1:$origin_port/xhttp" || xhttp_curl_status=$?
assert_fixture_rejected 'wrong VLESS Encryption key' "$fixture_dir/xhttp-negative.body" "$fixture_dir/xhttp-negative.status" "$xhttp_curl_status"
note 'PASS: wrong VLESS Encryption key did not reach the protected HTTP fixture.'

stop_bg xhttp-bad-client
start_bg xhttp-final-control "$fixture_dir/xhttp-final-control.log" "$XRAY" run -config "$fixture_dir/decoy-xhttp.runtime.json"
final_control_pid=$(<"$fixture_dir/xhttp-final-control.pid")
wait_listen "$xhttp_proxy_port" "$final_control_pid" || fail 'known-good XHTTP client final control did not start'
if ! run_http_request "$xhttp_proxy_port" "$fixture_dir/xhttp-final-control.body" "$fixture_dir/xhttp-final-control.status" \
    "http://127.0.0.1:$origin_port/xhttp"; then
    fail 'known-good XHTTP client final control failed after wrong-key rejection'
fi
assert_fixture_body 'known-good XHTTP final control' "$fixture_dir/xhttp-final-control.body" "$fixture_dir/xhttp-final-control.status"
note 'PASS: known-good XHTTP client succeeded after wrong-key rejection (final control).'

stop_bg xhttp-final-control
stop_bg xhttp-server
stop_bg http-origin
note 'PASS: XHTTP fixtures used loopback sockets and private /var/tmp credentials; no live service, config, interface, namespace, route, or external endpoint was touched.'
