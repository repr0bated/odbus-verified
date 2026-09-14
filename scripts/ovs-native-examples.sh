#!/bin/bash
# OVS Native OVSDB JSON-RPC Examples
# Collection of useful native OVS operations without CLI tools

set -euo pipefail

OVSDB_SOCKET="/var/run/openvswitch/db.sock"

# Function to send JSON-RPC request
ovsdb_rpc() {
    # OVSDB uses newline-delimited JSON-RPC — entire request must be on one line.
    local payload
    payload=$(echo "$1" | tr -d '\n')
    printf '%s\n' "$payload" | socat - "UNIX-CONNECT:$OVSDB_SOCKET" 2>/dev/null
}

# Pretty print JSON if python3 is available
pretty_json() {
    if command -v python3 &> /dev/null; then
        python3 -m json.tool
    else
        cat
    fi
}

echo "=== OVS Native OVSDB JSON-RPC Examples ==="
echo ""

# 1. List all databases
echo "1. List Databases:"
ovsdb_rpc '{"method":"list_dbs","params":[],"id":0}' | pretty_json
echo ""

# 2. Get schema for Open_vSwitch database
echo "2. Get Database Schema (truncated):"
ovsdb_rpc '{"method":"get_schema","params":["Open_vSwitch"],"id":0}' | head -20
echo "... (truncated)"
echo ""

# 3. List all bridges
echo "3. List All Bridges:"
ovsdb_rpc '{"method":"transact","params":["Open_vSwitch",[{"op":"select","table":"Bridge","where":[],"columns":["name"]}]],"id":0}' | pretty_json
echo ""

# 4. List all ports
echo "4. List All Ports:"
ovsdb_rpc '{"method":"transact","params":["Open_vSwitch",[{"op":"select","table":"Port","where":[],"columns":["name"]}]],"id":0}' | pretty_json
echo ""

# 5. List all interfaces
echo "5. List All Interfaces:"
ovsdb_rpc '{"method":"transact","params":["Open_vSwitch",[{"op":"select","table":"Interface","where":[],"columns":["name","type","ofport"]}]],"id":0}' | pretty_json
echo ""

# 6. Get Open_vSwitch table (global config)
echo "6. Get Global OVS Configuration:"
ovsdb_rpc '{"method":"transact","params":["Open_vSwitch",[{"op":"select","table":"Open_vSwitch","where":[],"columns":["bridges","cur_cfg","db_version"]}]],"id":0}' | pretty_json
echo ""

# 7. Example: Create a test bridge (commented out - uncomment to run)
# echo "7. Create Test Bridge 'test-br':"
# ovsdb_rpc '{"method":"transact","params":["Open_vSwitch",[
#   {"op":"insert","table":"Bridge","row":{"name":"test-br","ports":["set",[["named-uuid","port-test-br"]]]},"uuid-name":"bridge-test-br"},
#   {"op":"insert","table":"Port","row":{"name":"test-br","interfaces":["set",[["named-uuid","iface-test-br"]]]},"uuid-name":"port-test-br"},
#   {"op":"insert","table":"Interface","row":{"name":"test-br","type":"internal"},"uuid-name":"iface-test-br"},
#   {"op":"mutate","table":"Open_vSwitch","where":[],"mutations":[["bridges","insert",["set",[["named-uuid","bridge-test-br"]]]]]}
# ]],"id":0}' | pretty_json
# echo ""

# 8. Example: Delete a bridge (commented out - uncomment to run)
# echo "8. Delete Bridge 'test-br':"
# First get the bridge UUID, then delete it
# BRIDGE_UUID=$(ovsdb_rpc '{"method":"transact","params":["Open_vSwitch",[{"op":"select","table":"Bridge","where":[["name","==","test-br"]],"columns":["_uuid"]}]],"id":0}' | grep -oP '(?<="uuid",")[^"]+')
# if [ -n "$BRIDGE_UUID" ]; then
#   ovsdb_rpc "{\"method\":\"transact\",\"params\":[\"Open_vSwitch\",[
#     {\"op\":\"mutate\",\"table\":\"Open_vSwitch\",\"where\":[],\"mutations\":[[\"bridges\",\"delete\",[\"uuid\",\"$BRIDGE_UUID\"]]]},
#     {\"op\":\"delete\",\"table\":\"Bridge\",\"where\":[[\"_uuid\",\"==\",[\"uuid\",\"$BRIDGE_UUID\"]]]}
#   ]],\"id\":0}" | pretty_json
# fi
# echo ""

echo "=== Examples Complete ==="
echo ""
echo "To create a bridge, run: ./setup-ovs-bridge-native.sh <bridge-name> [port-name]"
echo "Example: ./setup-ovs-bridge-native.sh br0 eth1"
