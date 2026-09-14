#!/bin/bash
# Setup OVS Bridge using Native OVSDB JSON-RPC
# NO CLI COMMANDS - Pure JSON-RPC over Unix socket
#
# This script demonstrates native OVSDB operations without ovs-vsctl

set -euo pipefail

OVSDB_SOCKET="/var/run/openvswitch/db.sock"
BRIDGE_NAME="${1:-br0}"
PORT_NAME="${2:-eth1}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if socat is available
if ! command -v socat &> /dev/null; then
    log_error "socat is required but not installed. Install with: sudo apt install socat"
    exit 1
fi

# Check if OVSDB socket exists
if [ ! -S "$OVSDB_SOCKET" ]; then
    log_error "OVSDB socket not found at $OVSDB_SOCKET"
    log_info "Is Open vSwitch running? Check with: systemctl status openvswitch-switch"
    exit 1
fi

log_info "OVSDB socket found at $OVSDB_SOCKET"

# Function to send JSON-RPC request
ovsdb_rpc() {
    # OVSDB uses newline-delimited JSON-RPC — entire request must be on one line.
    local payload
    payload=$(echo "$1" | tr -d '\n')
    printf '%s\n' "$payload" | socat - "UNIX-CONNECT:$OVSDB_SOCKET" 2>/dev/null
}

# List available databases
log_info "Listing OVSDB databases..."
RESPONSE=$(ovsdb_rpc '{"method":"list_dbs","params":[],"id":0}')
echo "Databases: $RESPONSE"

# Check if bridge already exists
log_info "Checking if bridge '$BRIDGE_NAME' already exists..."
CHECK_BRIDGE=$(cat <<EOF
{"method":"transact","params":["Open_vSwitch",[{
  "op":"select",
  "table":"Bridge",
  "where":[["name","==","$BRIDGE_NAME"]],
  "columns":["name"]
}]],"id":0}
EOF
)

BRIDGE_CHECK=$(ovsdb_rpc "$CHECK_BRIDGE")
if echo "$BRIDGE_CHECK" | grep -q "\"$BRIDGE_NAME\""; then
    log_warn "Bridge '$BRIDGE_NAME' already exists"
    
    # List existing ports
    log_info "Listing ports on bridge '$BRIDGE_NAME'..."
    LIST_PORTS=$(cat <<EOF
{"method":"transact","params":["Open_vSwitch",[{
  "op":"select",
  "table":"Bridge",
  "where":[["name","==","$BRIDGE_NAME"]],
  "columns":["ports"]
}]],"id":0}
EOF
)
    PORTS_RESPONSE=$(ovsdb_rpc "$LIST_PORTS")
    echo "Ports response: $PORTS_RESPONSE"
    
    exit 0
fi

# Create the bridge using native JSON-RPC transaction
log_info "Creating bridge '$BRIDGE_NAME' via OVSDB JSON-RPC..."

CREATE_BRIDGE=$(cat <<EOF
{"method":"transact","params":["Open_vSwitch",[
  {
    "op":"insert",
    "table":"Bridge",
    "row":{"name":"$BRIDGE_NAME","ports":["set",[["named-uuid","port-$BRIDGE_NAME"]]]},
    "uuid-name":"bridge-$BRIDGE_NAME"
  },
  {
    "op":"insert",
    "table":"Port",
    "row":{"name":"$BRIDGE_NAME","interfaces":["set",[["named-uuid","iface-$BRIDGE_NAME"]]]},
    "uuid-name":"port-$BRIDGE_NAME"
  },
  {
    "op":"insert",
    "table":"Interface",
    "row":{"name":"$BRIDGE_NAME","type":"internal"},
    "uuid-name":"iface-$BRIDGE_NAME"
  },
  {
    "op":"mutate",
    "table":"Open_vSwitch",
    "where":[],
    "mutations":[["bridges","insert",["set",[["named-uuid","bridge-$BRIDGE_NAME"]]]]]
  }
]],"id":0}
EOF
)

CREATE_RESPONSE=$(ovsdb_rpc "$CREATE_BRIDGE")

if echo "$CREATE_RESPONSE" | grep -q '"error"'; then
    log_error "Failed to create bridge: $CREATE_RESPONSE"
    exit 1
fi

log_info "Bridge '$BRIDGE_NAME' created successfully"

# Verify bridge creation
log_info "Verifying bridge creation..."
VERIFY_RESPONSE=$(ovsdb_rpc "$CHECK_BRIDGE")
if echo "$VERIFY_RESPONSE" | grep -q "\"$BRIDGE_NAME\""; then
    log_info "✓ Bridge '$BRIDGE_NAME' verified in OVSDB"
else
    log_error "Bridge creation claimed success but not found in OVSDB"
    exit 1
fi

# List all bridges
log_info "Listing all bridges..."
LIST_BRIDGES=$(cat <<EOF
{"method":"transact","params":["Open_vSwitch",[{
  "op":"select",
  "table":"Bridge",
  "where":[],
  "columns":["name"]
}]],"id":0}
EOF
)

BRIDGES_RESPONSE=$(ovsdb_rpc "$LIST_BRIDGES")
echo "All bridges: $BRIDGES_RESPONSE"

# Optionally add a port if specified
if [ -n "${PORT_NAME:-}" ] && [ "$PORT_NAME" != "none" ]; then
    log_info "Adding port '$PORT_NAME' to bridge '$BRIDGE_NAME'..."
    
    # First check if the interface exists
    if ! ip link show "$PORT_NAME" &> /dev/null; then
        log_warn "Interface '$PORT_NAME' does not exist, skipping port addition"
    else
        ADD_PORT=$(cat <<EOF
{"method":"transact","params":["Open_vSwitch",[
  {
    "op":"insert",
    "table":"Port",
    "row":{"name":"$PORT_NAME","interfaces":["set",[["named-uuid","iface-$PORT_NAME"]]]},
    "uuid-name":"port-$PORT_NAME"
  },
  {
    "op":"insert",
    "table":"Interface",
    "row":{"name":"$PORT_NAME"},
    "uuid-name":"iface-$PORT_NAME"
  },
  {
    "op":"mutate",
    "table":"Bridge",
    "where":[["name","==","$BRIDGE_NAME"]],
    "mutations":[["ports","insert",["set",[["named-uuid","port-$PORT_NAME"]]]]]
  }
]],"id":0}
EOF
)
        
        PORT_RESPONSE=$(ovsdb_rpc "$ADD_PORT")
        
        if echo "$PORT_RESPONSE" | grep -q '"error"'; then
            log_error "Failed to add port: $PORT_RESPONSE"
        else
            log_info "✓ Port '$PORT_NAME' added to bridge '$BRIDGE_NAME'"
        fi
    fi
fi

# Show final bridge configuration
log_info "Final bridge configuration:"
GET_BRIDGE_INFO=$(cat <<EOF
{"method":"transact","params":["Open_vSwitch",[{
  "op":"select",
  "table":"Bridge",
  "where":[["name","==","$BRIDGE_NAME"]],
  "columns":[]
}]],"id":0}
EOF
)

BRIDGE_INFO=$(ovsdb_rpc "$GET_BRIDGE_INFO")
echo "$BRIDGE_INFO" | python3 -m json.tool 2>/dev/null || echo "$BRIDGE_INFO"

log_info "Bridge setup complete!"
log_info ""
log_info "Summary:"
log_info "  Bridge: $BRIDGE_NAME"
log_info "  Method: Native OVSDB JSON-RPC"
log_info "  Socket: $OVSDB_SOCKET"
log_info ""
log_info "To verify with kernel, run: ip link show $BRIDGE_NAME"
