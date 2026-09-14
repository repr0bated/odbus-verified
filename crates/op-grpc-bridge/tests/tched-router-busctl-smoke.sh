#!/usr/bin/env sh
# 3tched Router D-Bus smoke test (mission T-18).
#
# Requires a running op-grpc-bridge that owns org.opdbus.v1.plugins — the only
# legal tree. This is an integration check, not a unit test; it is not run by
# `cargo test`. Run it after starting the bridge.
#
# The bus is reached by address; override with BUSCTL_ARGS.
#
# Exit non-zero on the first failed assertion.
set -eu

BUS="${BUSCTL_ARGS:---address=unix:path=/run/opdbus/session-bus.sock}"
DEST="org.opdbus.v1.plugins"
PLUGIN_PATH="/org/opdbus/v1/plugins/tched_router"
IFACE="org.opdbus.v1.PluginV1"

say() { printf '[tched-router-smoke] %s\n' "$1"; }

# 1. GetModelRoutes on the top-level plugin object returns a JSON array.
say "calling GetModelRoutes"
routes_json=$(busctl $BUS call "$DEST" "$PLUGIN_PATH" "$IFACE" Call ss "GetModelRoutes" "{}")
echo "$routes_json" | grep -q "model_routes\|hint\|result" || {
  say "FAIL: GetModelRoutes returned no recognizable payload"; exit 1; }

# 2. The route sub-objects are registered under .../routes.
say "introspecting $PLUGIN_PATH/routes"
busctl $BUS introspect "$DEST" "$PLUGIN_PATH/routes" >/dev/null 2>&1 || {
  say "FAIL: routes sub-tree not introspectable (no sub-objects registered)"; exit 1; }

# 3. SelectModel with a minimal payload returns JSON carrying selected_model.
say "calling SelectModel"
payload='{"task_class":"chat","requested_effort":"medium","context_tokens":1000,"privacy_tier":"public"}'
sel_json=$(busctl $BUS call "$DEST" "$PLUGIN_PATH" "$IFACE" Call ss "SelectModel" "$payload")
echo "$sel_json" | grep -q "selected_model" || {
  say "FAIL: SelectModel response missing selected_model"; exit 1; }

say "PASS: GetModelRoutes, routes sub-tree, and SelectModel all responded"
