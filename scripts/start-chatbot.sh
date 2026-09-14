#!/bin/bash
set -euo pipefail

# Start MCP compact server and chatbot with gcloud auth

echo "==> Building op-dbus with MCP and chatbot..."
cargo build --release --bin op-mcp-compact --bin op-dbus

echo ""
echo "==> Starting MCP compact server..."
./target/release/op-mcp-compact &
MCP_PID=$!
echo "MCP server started (PID: $MCP_PID)"

# Wait for MCP to be ready
sleep 2

echo ""
echo "==> Verifying gcloud authentication..."
if ! gcloud auth application-default print-access-token &>/dev/null; then
    echo "ERROR: Not authenticated with gcloud"
    echo "Run: gcloud auth application-default login"
    kill $MCP_PID
    exit 1
fi
echo "✓ Authenticated with gcloud ADC"

echo ""
echo "==> Starting chatbot..."
echo "Chatbot will use:"
echo "  - MCP endpoint: http://127.0.0.1:8080/mcp"
echo "  - Auth: gcloud application-default credentials"
echo ""

# Run chatbot (assuming it's integrated into op-dbus)
./target/release/op-dbus chatbot --mcp-url http://127.0.0.1:8080/mcp

# Cleanup on exit
trap "kill $MCP_PID 2>/dev/null" EXIT
