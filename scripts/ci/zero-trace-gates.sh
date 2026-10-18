#!/bin/sh
# Fail when a retired MCP listener, client endpoint, or op-web execution bypass
# returns to an active source/deployment surface. Historical audits and the
# explicit retirement inventories are evidence, not runtime inputs, and are
# intentionally outside this gate.
set -eu

cd "$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)"

fail_match() {
    description=$1
    pattern=$2
    shift 2
    if rg -n "$pattern" "$@"; then
        echo "zero-trace gate failed: $description" >&2
        exit 1
    else
        status=$?
        if [ "$status" -ne 1 ]; then
            echo "zero-trace gate could not scan required inputs: $description" >&2
            exit "$status"
        fi
    fi
}

# These are local client settings, not required fresh-source inputs. Scan them
# when present; missing required versioned directories still fail the gate.
set --
for local_settings in .mcp.json .junie/mcp .kiro/settings; do
    if [ -e "$local_settings" ]; then
        set -- "$@" "$local_settings"
    fi
done

fail_match "retired MCP bind or endpoint" \
    '(:3003|10\.200\.0\.2:50052|100\.90\.37\.254:50052|127\.0\.0\.1:1143[5-8]|localhost:1143[5-8])' \
    crates/op-cognitive/src crates/op-mcp/src \
    deploy/config deploy/mcp deploy/runit deploy/scripts scripts/uv-tools \
    "$@" \
    --glob '!**/*.md' --glob '!**/*.patch' \
    --glob '!retired-services' --glob '!retired-binaries'

fail_match "retired standalone MCP executable in client/runtime config" \
    'op-mcp-server|op-mcp-(agents|compact|blob-schema|cognitive)' \
    crates/op-mcp/Cargo.toml deploy/config deploy/mcp deploy/runit \
    "$@" \
    --glob '!**/*.md' --glob '!retired-services' --glob '!retired-binaries'

fail_match "retired standalone op-chat MCP listener" \
    'OP_CHAT_LISTEN|run_chat_mcp_server|ChatMcpServer|op-chat-mcp' \
    crates/op-chat/src deploy/config deploy/mcp deploy/runit \
    --glob '!**/*.md' --glob '!retired-services' --glob '!retired-binaries'

fail_match "op-web MCP or gRPC execution bypass" \
    'grpc_proxy|mcp_agents|mcp_compact|mcp_discovery|/jsonrpc|/rpc|well-known/mcp|nest\("/mcp"' \
    crates/op-web/src

fail_match "retired Waypipe workspace dependency or runtime configuration" \
    'op[-_]waypipe[-_]grpc|op\.waypipe\.v1' \
    Cargo.toml Cargo.lock crates/*/Cargo.toml deploy/config deploy/mcp deploy/runit \
    --glob '!**/*.md' --glob '!retired-services' --glob '!retired-binaries'

fail_match "retired standalone adapter dependency, client or runtime input" \
    'op[-_]grpc[-_]adapters|op\.adapters\.v1|NetmakerService|NetmakerAdapter|netmaker_client|ExecuteNetclientCommand' \
    Cargo.toml Cargo.lock crates/*/Cargo.toml crates/op-grpc-bridge/src \
    deploy/config deploy/mcp deploy/runit \
    --glob '!**/*.md' --glob '!retired-services' --glob '!retired-binaries'

for retired in \
    crates/op-waypipe-grpc/Cargo.toml \
    crates/op-grpc-adapters/Cargo.toml \
    crates/op-chat/src/main.rs \
    crates/op-chat/src/mcp_server.rs \
    deploy/op-chat.service \
    deploy/runit/fwd-8090 \
    deploy/runit/fwd-nm-mesh-8090 \
    deploy/runit/op-cognitive-mcp \
    deploy/runit/op-mcp-agents \
    deploy/runit/op-mcp-blob-schema \
    deploy/runit/op-mcp-cognitive \
    deploy/runit/op-mcp-compact \
    deploy/runit/op-waypipe-grpc
do
    if [ -f "$retired" ] || find "$retired" -type f -print -quit 2>/dev/null | grep -q .; then
        echo "zero-trace gate failed: retired runtime path remains at $retired" >&2
        exit 1
    fi
done

scripts/ci-gate-deprecated-plugin-schema-dat.sh
echo "unified MCP zero-trace gates: clean"
