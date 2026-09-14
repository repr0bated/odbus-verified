#!/usr/bin/env bash
# Read-only public-MCP smoke test. Credentials stay off stdout and curl argv.
# Usage: bash scripts/verify-identity-mcp.sh <human-session-id> [https://host:8090/mcp]
# Optional: OPDBUS_VERIFY_OTHER_SESSION_ID checks the chatbot's isolation too.
set +x
set -euo pipefail
umask 077

identity_session=${1:?usage: verify-identity-mcp.sh HUMAN_SESSION_ID [MCP_URL]}
mcp_url=${2:-https://10.0.0.3:8090/mcp}
case "$mcp_url" in https://*) ;; *) echo 'TLS is required' >&2; exit 1 ;; esac
probe_dir=$(mktemp -d /tmp/odbus-identity-mcp.XXXXXX)
trap 'rm -f -- "$probe_dir/headers" "$probe_dir/body"; rmdir -- "$probe_dir"' EXIT
sealed_id=$(sudo /usr/local/bin/op-identity-headers --session "$identity_session" |
    jq -er '."x-opdbus-sealed-id-bin"')
[[ "$sealed_id" =~ ^[A-Za-z0-9_-]+={0,2}$ ]] || { echo 'Invalid SID1 header encoding' >&2; exit 1; }
mcp_session=''

request() {
    local method=$1 params=$2 credential=${3-$sealed_id}
    local payload
    payload=$(jq -nc --arg method "$method" --argjson params "$params" \
        '{jsonrpc:"2.0",id:1,method:$method,params:$params}')
    http_status=$({
        printf 'header = "Content-Type: application/json"\n'
        printf 'header = "Accept: application/json, text/event-stream"\n'
        if [ -n "$credential" ]; then
            printf 'header = "x-opdbus-sealed-id-bin: %s"\n' "$credential"
        fi
        if [ -n "$mcp_session" ]; then
            printf 'header = "Mcp-Session-Id: %s"\n' "$mcp_session"
            printf 'header = "Mcp-Protocol-Version: 2025-06-18"\n'
        fi
    } | curl --silent --show-error --max-time 120 --config - \
        --dump-header "$probe_dir/headers" --output "$probe_dir/body" \
        --write-out '%{http_code}' --data "$payload" "$mcp_url")
}

expect() {
    if ! jq -e "$1" "$probe_dir/body" >/dev/null; then
        printf 'FAIL: %s (HTTP %s; response withheld)\n' "$2" "$http_status" >&2
        exit 1
    fi
    printf 'PASS: %s\n' "$2"
}

initialize='{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"identity-mcp-verify","version":"1"}}'
request initialize "$initialize" ''
[ "$http_status" = 401 ] || { echo 'FAIL: missing identity accepted'; exit 1; }
echo 'PASS: missing identity rejected'
request initialize "$initialize" invalid
[ "$http_status" = 401 ] || { echo 'FAIL: malformed identity accepted'; exit 1; }
echo 'PASS: malformed identity rejected'
request initialize "$initialize"
expect '.result.protocolVersion == "2025-06-18"' 'SID1-authenticated TLS/MCP initialization'
mcp_session=$(awk 'tolower($1)=="mcp-session-id:" {gsub("\r", "", $2); print $2}' "$probe_dir/headers")
[[ "$mcp_session" =~ ^[A-Za-z0-9_-]+$ ]] || { echo 'FAIL: invalid MCP session header'; exit 1; }

request tools/list '{}' ''
[ "$http_status" = 401 ] || { echo 'FAIL: follow-up without identity accepted'; exit 1; }
echo 'PASS: follow-up still requires identity'
if [ -n "${OPDBUS_VERIFY_OTHER_SESSION_ID:-}" ]; then
    other_id=$(sudo /usr/local/bin/op-identity-headers --session "$OPDBUS_VERIFY_OTHER_SESSION_ID" |
        jq -er '."x-opdbus-sealed-id-bin"')
    [[ "$other_id" =~ ^[A-Za-z0-9_-]+={0,2}$ ]] || { echo 'Invalid second SID1 header encoding'; exit 1; }
    request tools/list '{}' "$other_id"
    [ "$http_status" = 404 ] || { echo 'FAIL: another identity reused the human MCP session'; exit 1; }
    echo 'PASS: another identity cannot reuse the human MCP session'
    human_mcp_session=$mcp_session
    mcp_session=''
    request initialize "$initialize" "$other_id"
    expect '.result.protocolVersion == "2025-06-18"' 'second identity initializes its own session'
    mcp_session=$(awk 'tolower($1)=="mcp-session-id:" {gsub("\r", "", $2); print $2}' "$probe_dir/headers")
    [[ "$mcp_session" =~ ^[A-Za-z0-9_-]+$ ]] || { echo 'FAIL: invalid second MCP session header'; exit 1; }
    request tools/list '{}' "$other_id"
    expect '.result.tools | all(.name | startswith("plugin.identity_sled.") | not)' 'chatbot does not inherit human identity-read authority'
    request tools/call '{"name":"plugin.identity_sled.get_identity","arguments":{}}' "$other_id"
    expect '.error.message | contains("principal lacks required capability identity_sled.read")' 'chatbot identity-read execution is denied'
    mcp_session=$human_mcp_session
    unset other_id
fi
request tools/list '{}'
expect '[.result.tools[] | select(.name == "plugin.identity_sled.get_identity" and .inputSchema.type == "object")] | length == 1' 'typed identity read tool discoverable'
expect '[.result.tools[] | select(.name == "plugin.identity_sled.write_identity")] | length == 0' 'identity write tool not exposed'

unwrap='.result.structuredContent // (.result.content[0].text | fromjson)'
request resources/read '{"uri":"blob://identity_sled"}'
expect '(.result.contents[0].text | fromjson) | type == "object"' 'sealed blob resource resolves to JSON'
request tools/call '{"name":"memory_recall","arguments":{"limit":1}}'
expect '.result != null and .result.isError != true' 'durable memory read'
request tools/call '{"name":"plugin.identity_sled.write_identity","arguments":{}}'
expect '.error.message | contains("not exposed by the configured MCP toolsets")' 'unexposed tools report a truthful denial'
request tools/call '{"name":"plugin.identity_sled.get_identity","arguments":{}}'
expect '.error.message | contains("select toolset identity_read")' 'warm read tool identifies the real toolset'
request tools/call '{"name":"toolsets","arguments":{"operation":"select","toolset_id":"identity_read"}}'
expect "$unwrap | .result.selected_set == \"identity_read\"" 'identity read toolset selected'
selector=$(jq -c "$unwrap | .result.selector" "$probe_dir/body")
for method in get_identity get_session_history; do
    params=$(jq -nc --arg method "plugin.identity_sled.$method" --arg sid "$identity_session" \
        --argjson selector "$selector" \
        '{name:$method,arguments:{session_id:$sid},_meta:{opdbus_toolset:$selector}}')
    request tools/call "$params"
    expect '.result != null and .result.isError != true' "$method executed"
    if [ "$method" = get_identity ]; then
        expected_session=$(jq -nc --arg sid "$identity_session" '$sid')
        expect "$unwrap | .identity.session_id == $expected_session" 'identity read returns the requested authoritative sled'
    else
        expect "$unwrap | .events | type == \"array\"" 'session history returns an event array'
    fi
    expect '[.. | objects | to_entries[] | select(.key | test("sealed_?id|identity.?blob|private.?key|token"; "i"))] | length == 0' "$method structured output contains no credential fields"
    expect "$unwrap | [.. | objects | to_entries[] | select(.key | test(\"sealed_?id|identity.?blob|private.?key|token\"; \"i\"))] | length == 0" "$method decoded output contains no credential fields"
done

request tools/call '{"name":"plugin.notebooklm.get_health","arguments":{}}'
expect '.result != null and .result.isError != true' 'NotebookLM health method executes'
if jq -e "$unwrap | .authenticated == true" "$probe_dir/body" >/dev/null; then
    echo 'PASS: NotebookLM authenticated Google catalog probe'
else
    expect "$unwrap | .authenticated == false and .auth_status == \"unverified\"" 'NotebookLM returns an explicit unverified authentication state'
    echo 'PENDING: NotebookLM needs Google login; health correctly reports unauthenticated'
    exit 2
fi
