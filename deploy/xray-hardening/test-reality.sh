#!/bin/bash
# Bounded source fixture. All listeners are loopback or private temporary UDS.
set -euo pipefail
umask 077
xray=${XRAY_BIN:-/usr/local/bin/xray}
relay=${UDS_RELAY_BIN:-/usr/local/bin/op-uds-relay}
work=$(mktemp -d /tmp/od106-reality.XXXXXX)
pids=()
log_index=0
cleanup() {
  local p
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  sleep .2
  # Unreaped children retain their PID, so escalation cannot target a reused PID.
  for p in "${pids[@]}"; do kill -KILL "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; done
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
start() { "$@" > "$work/process-$log_index.log" 2>&1 & pids+=("$!"); log_index=$((log_index + 1)); }
fail() { printf 'REALITY fixture failed: %s; private evidence %s\n' "$1" "$work" >&2; exit 1; }
[[ $($xray version | head -1) == *'26.3.27'* ]] || fail 'native version mismatch'
# No generated key material is printed or passed as command arguments.
timeout 15 "$xray" x25519 > "$work/x25519.txt"
timeout 15 "$xray" mldsa65 > "$work/mldsa.txt"
timeout 15 "$xray" mldsa65 > "$work/wrong-mldsa.txt"
timeout 15 "$xray" uuid > "$work/uuid.txt"
jq -n --rawfile x "$work/x25519.txt" --rawfile m "$work/mldsa.txt" --rawfile u "$work/uuid.txt" --arg socket "$work/http.sock" '
 def value($s;$label): $s|split("\n")|map(select(startswith($label))|ltrimstr($label))|.[0];
 {client_id:($u|rtrimstr("\n")),reality_password:value($x;"Password (PublicKey): "),reality_short_id:"1234abcd",mldsa65_seed:value($m;"Seed: "),mldsa65_verify:value($m;"Verify: "),http_uds_path:$socket,private_key:value($x;"PrivateKey: ")}' > "$work/params.json"
# The production preparation contract permits only private shared-mount socket paths;
# use that contract, then relocate its new socket solely for the disposable fixture.
jq '.http_uds_path="/run/od106-fixture/http.sock"' "$work/params.json" > "$work/prepare-params.json"
jq -n '{inbounds:[],outbounds:[{tag:"baseline",protocol:"blackhole"}],routing:{rules:[]}}' > "$work/host-before.json"
jq -n --slurpfile p "$work/params.json" '{inbounds:[{tag:"egress-reality",listen:"0.0.0.0",port:9443,protocol:"vless",settings:{clients:[{id:$p[0].client_id,flow:"xtls-rprx-vision"}],decryption:"none"},streamSettings:{network:"tcp",security:"reality",realitySettings:{dest:"127.0.0.1:443",serverNames:["ghostbridge.tech"],privateKey:$p[0].private_key,shortIds:[$p[0].reality_short_id]}}}],outbounds:[{tag:"direct",protocol:"freedom"}],routing:{rules:[]}}' > "$work/server-before.json"
dir=$(cd -- "$(dirname -- "$0")" && pwd)
"$dir/prepare.sh" host-reality "$work/host-before.json" "$work/prepare-params.json" "$work/client-candidate.json" > "$work/prepare-client.log"
"$dir/prepare.sh" decoy-reality "$work/server-before.json" "$work/prepare-params.json" "$work/server-candidate.json" > "$work/prepare-server.log"
# Choose free loopback ports by letting a bounded Perl fixture reserve/release them.
# Collision after reservation fails safely at listener startup.
command -v perl >/dev/null || fail 'Perl required for free-port allocation'
ports=$(perl -MIO::Socket::INET -e 'my @s;for(1..5){push @s,IO::Socket::INET->new(LocalAddr=>"127.0.0.1",LocalPort=>0,Listen=>1,Proto=>"tcp") or die $!;}print join(" ",map {$_->sockport} @s);')
read -r server_port carrier_port origin_port proxy_port cover_port <<< "$ports"
target=${REALITY_COVER_TARGET:-129.80.64.127:9443}
case $target in local) target="127.0.0.1:$cover_port" ;; 129.80.64.127:9443) ;; *) fail 'unsupported cover target' ;; esac
jq --argjson port "$server_port" --arg dest "$target" '.log={loglevel:"info"}|.inbounds[0].listen="127.0.0.1"|.inbounds[0].port=$port|.inbounds[0].streamSettings.realitySettings.dest=$dest' "$work/server-candidate.json" > "$work/server.json"
jq --arg socket "$work/http.sock" --argjson port "$carrier_port" '.log={loglevel:"info"}|(.inbounds[]|select(.tag=="od106-http-egress")|.listen)=$socket|(.outbounds[]|select(.tag=="od106-reality-egress")|.settings.vnext[0].port)=$port' "$work/client-candidate.json" > "$work/client.json"
jq --rawfile wrong "$work/wrong-mldsa.txt" '(.outbounds[]|select(.tag=="od106-reality-egress")|.streamSettings.realitySettings.mldsa65Verify)=($wrong|split("\n")|map(select(startswith("Verify: "))|ltrimstr("Verify: "))|.[0])' "$work/client.json" > "$work/wrong-client.json"
for config in server client wrong-client; do timeout 15 "$xray" run -test -config "$work/$config.json" > "$work/test-$config.log" 2>&1 || fail "native $config validation"; done
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$work/origin.key" -out "$work/origin.crt" -days 1 -subj /CN=localhost > "$work/cert.log" 2>&1
# A real three-certificate local cover chain exceeds the MLDSA signature budget.
for name in root intermediate cover; do
  openssl req -new -newkey rsa:4096 -nodes -keyout "$work/$name.key" -out "$work/$name.csr" -subj "/CN=$name" > "$work/cert-$name.log" 2>&1
done
printf "basicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\n" > "$work/ca.ext"
printf "basicConstraints=critical,CA:FALSE\nsubjectAltName=DNS:assistant.ghostbridge.tech\n" > "$work/cover.ext"
openssl x509 -req -in "$work/root.csr" -signkey "$work/root.key" -days 1 -extfile "$work/ca.ext" -out "$work/root.crt" > "$work/sign-root.log" 2>&1
openssl x509 -req -in "$work/intermediate.csr" -CA "$work/root.crt" -CAkey "$work/root.key" -CAcreateserial -days 1 -extfile "$work/ca.ext" -out "$work/intermediate.crt" > "$work/sign-intermediate.log" 2>&1
openssl x509 -req -in "$work/cover.csr" -CA "$work/intermediate.crt" -CAkey "$work/intermediate.key" -CAcreateserial -days 1 -extfile "$work/cover.ext" -out "$work/cover.crt" > "$work/sign-cover.log" 2>&1
cat "$work/intermediate.crt" "$work/root.crt" > "$work/chain.crt"
start openssl s_server -accept "127.0.0.1:$origin_port" -cert "$work/origin.crt" -key "$work/origin.key" -www -quiet
start openssl s_server -accept "127.0.0.1:$cover_port" -cert "$work/cover.crt" -key "$work/cover.key" -cert_chain "$work/chain.crt" -tls1_3 -groups X25519MLKEM768 -www -quiet
start "$xray" run -config "$work/server.json"
start "$relay" --unix-to-tcp "$work/carrier.sock=127.0.0.1:$server_port" --tcp-to-unix "127.0.0.1:$carrier_port=$work/carrier.sock" --tcp-to-unix "127.0.0.1:$proxy_port=$work/http.sock"
start "$xray" run -config "$work/client.json"
client_pid=${pids[4]}
for attempt in {1..30}; do [[ -S $work/http.sock && -S $work/carrier.sock ]] && break; sleep .1; done
[[ -S $work/http.sock && -S $work/carrier.sock ]] || fail 'fixture sockets unavailable'
request() { timeout 20 curl --silent --show-error --fail --max-time 15 --noproxy '' --proxy "http://127.0.0.1:$proxy_port" --insecure "https://127.0.0.1:$origin_port/"; }
request > "$work/good-response.txt" 2> "$work/good-curl.log" || fail 'correct MLDSA-pinned end-to-end request'
rg -q 's_server|SSL_SESSION' "$work/good-response.txt" || fail 'unexpected origin response'
kill "$client_pid"; wait "$client_pid" || true
unset 'pids[4]'
rm -f "$work/http.sock"
wrong_log="$work/process-$log_index.log"
start "$xray" run -config "$work/wrong-client.json"
for attempt in {1..30}; do [[ -S $work/http.sock ]] && break; sleep .1; done
[[ -S $work/http.sock ]] || fail 'wrong-verifier client socket unavailable'
if request > "$work/wrong-response.txt" 2> "$work/wrong-curl.log"; then fail 'wrong MLDSA verifier accepted'; fi
# Failed MLDSA verification falls back to ordinary x509 validation of the
# REALITY synthetic certificate, which must fail. show remains false.
rg -qi 'x509:|REALITY.*verification|signature' "$wrong_log" || fail 'missing certificate rejection evidence'
wrong_pid=${pids[4]}
kill "$wrong_pid"; wait "$wrong_pid" || true
unset 'pids[4]'
rm -f "$work/http.sock"
start "$xray" run -config "$work/client.json"
for attempt in {1..30}; do [[ -S $work/http.sock ]] && break; sleep .1; done
request > "$work/restored-response.txt" 2> "$work/restored-curl.log" || fail 'restored correct verifier failed'
rg -q 's_server|SSL_SESSION' "$work/restored-response.txt" || fail 'unexpected restored response'
printf 'REALITY paired fixture PASS: UDS HTTP CONNECT, TCP/UDS/TCP carrier, Vision + MLDSA pin; wrong verifier rejected and correct verifier restored. Private evidence %s\n' "$work"
