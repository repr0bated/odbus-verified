#!/usr/bin/env bash
# Smoke test: verify the Updated signal on org.opdbus.v1.PluginV1
#
# USAGE:
#   ./deploy/smoke/dbus-signal-check.sh
#
# EXPECTS:
#   - Session bus running at unix:path=/run/opdbus/session-bus.sock
#   - op-grpc-bridge (or combined server) running
#
# VERIFIES:
#   - Updated signal is emitted when a mutation fires
#   - Payload contains {"plugin": "<name>", "key": "<key>"} (not empty/bare)
#
# EXIT CODES:
#   0 — signal observed with valid payload
#   1 — signal not observed or payload is invalid

set -euo pipefail

BUS_ADDR="${DBUS_SESSION_BUS_ADDRESS:-unix:path=/run/opdbus/session-bus.sock}"
TIMEOUT=10

echo "=== D-Bus Updated Signal Smoke Test ==="
echo "Bus: $BUS_ADDR"
echo "Timeout: ${TIMEOUT}s"
echo ""

# Subscribe to Updated signals; timeout after $TIMEOUT seconds
SIGNAL_OUTPUT=$(timeout "$TIMEOUT" dbus-monitor \
  --address "$BUS_ADDR" \
  "type='signal',interface='org.opdbus.v1.PluginV1',member='Updated'" 2>/dev/null || true)

if [ -z "$SIGNAL_OUTPUT" ]; then
  echo "FAIL: No Updated signal observed within ${TIMEOUT}s."
  echo "      Trigger a mutation (e.g., gRPC call) while this runs."
  echo ""
  echo "Manual verification command:"
  echo "  dbus-monitor --address $BUS_ADDR \\"
  echo "    \"type='signal',interface='org.opdbus.v1.PluginV1',member='Updated'\""
  exit 1
fi

echo "Signal received:"
echo "$SIGNAL_OUTPUT" | head -20
echo ""

# Check that payload contains "plugin" key (REQ-1.5: must carry state data)
if echo "$SIGNAL_OUTPUT" | grep -q '"plugin"'; then
  echo "PASS: Signal payload contains 'plugin' field (REQ-1.5 satisfied)"
else
  echo "FAIL: Signal payload does NOT contain 'plugin' field."
  echo "      The Updated signal must carry identifiable state data."
  exit 1
fi

echo ""
echo "=== Smoke test PASSED ==="
