#!/bin/sh
exec 2>&1
set -eu

# Keep only public service settings across the first exec. The shared
# environment file can contain unrelated provider credentials; none of those
# become ambient input to the router daemon.
if [ "${TCHED_ROUTER_SANITIZED:-}" != 1 ]; then
    set -a
    [ -r /etc/op-dbus/environment ] && . /etc/op-dbus/environment
    set +a
    exec env -i TCHED_ROUTER_SANITIZED=1 PATH=/usr/local/bin:/usr/bin:/bin \
        TCHED_ROUTER_CONFIG_DIR="${TCHED_ROUTER_CONFIG_DIR:-/var/lib/tched-router}" \
        TCHED_ROUTER_BOOTSTRAP_CONFIG="${TCHED_ROUTER_BOOTSTRAP_CONFIG:-/etc/op-dbus/zeroclaw-runtime.toml}" \
        TCHED_ROUTER_BOOTSTRAP_SKILLS="${TCHED_ROUTER_BOOTSTRAP_SKILLS:-/etc/op-dbus/zeroclaw-skills/default}" \
        TCHED_ROUTER_OPENCODE_KEY_FILE="${TCHED_ROUTER_OPENCODE_KEY_FILE:-/etc/op-dbus/secrets/opencode-api-key}" \
        TCHED_ROUTER_CHEAPER_INFERENCE_KEY_FILE="${TCHED_ROUTER_CHEAPER_INFERENCE_KEY_FILE:-/etc/op-dbus/secrets/cheaperinverence_api_key}" \
        TCHED_ROUTER_NATIVE_BIN="${TCHED_ROUTER_NATIVE_BIN:-/usr/bin/zeroclaw}" \
        TCHED_ROUTER_NATIVE_MANIFEST="${TCHED_ROUTER_NATIVE_MANIFEST:-/usr/share/op-dbus/native-manifests/tched-router.json}" \
        TCHED_ROUTER_HOST="${TCHED_ROUTER_HOST:-127.0.0.1}" \
        TCHED_ROUTER_PORT="${TCHED_ROUTER_PORT:-8084}" \
        TCHED_ROUTER_CONTROL_PLANE_MODEL_PROVIDER="${TCHED_ROUTER_CONTROL_PLANE_MODEL_PROVIDER:-custom.opencode_control_plane}" \
        OPDBUS_SLED_AUTH_URL="${OPDBUS_SLED_AUTH_URL:-https://10.0.0.2:8090/mcp}" \
        OPDBUS_SLED_CA_FILE="${OPDBUS_SLED_CA_FILE:-/etc/op-dbus/tls/tonic-svc0-ca.crt}" \
        /bin/sh "$0" --sanitized
fi

CONFIG_DIR=${TCHED_ROUTER_CONFIG_DIR:-/var/lib/tched-router}
BOOTSTRAP_CONFIG=${TCHED_ROUTER_BOOTSTRAP_CONFIG:-/etc/op-dbus/zeroclaw-runtime.toml}
BOOTSTRAP_SKILLS=${TCHED_ROUTER_BOOTSTRAP_SKILLS:-/etc/op-dbus/zeroclaw-skills/default}
OPENCODE_API_KEY_FILE=${TCHED_ROUTER_OPENCODE_KEY_FILE:-/etc/op-dbus/secrets/opencode-api-key}
CHEAPER_INFERENCE_KEY_FILE=${TCHED_ROUTER_CHEAPER_INFERENCE_KEY_FILE:-/etc/op-dbus/secrets/cheaperinverence_api_key}
NATIVE_BIN=${TCHED_ROUTER_NATIVE_BIN:-/usr/bin/zeroclaw}
NATIVE_MANIFEST=${TCHED_ROUTER_NATIVE_MANIFEST:-/usr/share/op-dbus/native-manifests/tched-router.json}
HOST=${TCHED_ROUTER_HOST:-127.0.0.1}
PORT=${TCHED_ROUTER_PORT:-8084}
CONTROL_PLANE_MODEL_PROVIDER=${TCHED_ROUTER_CONTROL_PLANE_MODEL_PROVIDER:-custom.opencode_control_plane}
printf '%s' "$CONTROL_PLANE_MODEL_PROVIDER" |
    jq -Re 'test("^[A-Za-z0-9_-]+[.][A-Za-z0-9_-]+$")' >/dev/null || {
    echo "tched-router: invalid Control Plane model provider reference" >&2
    exit 78
}

[ -x "$NATIVE_BIN" ] || { echo "pinned tched_router native binary is missing" >&2; exit 78; }
[ -f "$NATIVE_MANIFEST" ] || { echo "tched_router native provenance manifest is missing" >&2; exit 78; }
manifest_format=$(jq -er '.format' "$NATIVE_MANIFEST")
manifest_commit=$(jq -er '.upstream.commit' "$NATIVE_MANIFEST")
manifest_binary_sha256=$(jq -er '.artifact.sha256' "$NATIVE_MANIFEST")
[ "$manifest_format" = opdbus.tched-router-native.v1 ] || { echo "unsupported tched_router provenance" >&2; exit 78; }
[ "$manifest_commit" = a56c345d51dd8ab562e9351e0d4ab83f6a741db9 ] || { echo "wrong tched_router upstream commit" >&2; exit 78; }
[ "$(sha256sum "$NATIVE_BIN" | awk '{print $1}')" = "$manifest_binary_sha256" ] || { echo "tched_router binary/provenance mismatch" >&2; exit 78; }
[ "$($NATIVE_BIN --version)" = "zeroclaw 0.8.4" ] || { echo "unexpected native version" >&2; exit 78; }

install -d -o jeremy -g jeremy -m 0700 "$CONFIG_DIR" "$CONFIG_DIR/home"
if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    install -o jeremy -g jeremy -m 0600 "$BOOTSTRAP_CONFIG" "$CONFIG_DIR/config.toml"
fi
if [ -d "$BOOTSTRAP_SKILLS" ]; then
    install -d -o jeremy -g jeremy -m 0755 "$CONFIG_DIR/shared/skills/default"
    cp -a "$BOOTSTRAP_SKILLS"/. "$CONFIG_DIR/shared/skills/default/"
    chown -R jeremy:jeremy "$CONFIG_DIR/shared/skills/default"
fi

# Use the native configuration owner for both fresh installs and migration.
# Observe model presence before init: init itself supplies a native default.
native_config() {
    chpst -u jeremy:jeremy env HOME="$CONFIG_DIR/home" USER=jeremy \
        PATH=/usr/local/bin:/usr/bin:/bin ZEROCLAW_CONFIG_DIR="$CONFIG_DIR" \
        "$NATIVE_BIN" config "$@" --config-dir "$CONFIG_DIR"
}
# An external selection must already exist. Check before any startup migration
# so a misspelled variable cannot replace the role's current provider.
if [ "$CONTROL_PLANE_MODEL_PROVIDER" != custom.opencode_control_plane ]; then
    native_config get "providers.models.$CONTROL_PLANE_MODEL_PROVIDER.model" --json |
        jq -e '.value | strings | test("\\S")' >/dev/null || {
        echo "tched-router: Control Plane model provider is not configured" >&2
        exit 78
    }
fi
for risk in control_plane accountability; do
    native_config init "risk_profiles.$risk" >/dev/null
    jq -n --arg p "/risk_profiles/$risk" '[
      {op:"add",path:($p+"/level"),value:"readonly"},
      {op:"add",path:($p+"/workspace_only"),value:true},
      {op:"add",path:($p+"/require_approval_for_medium_risk"),value:true},
      {op:"add",path:($p+"/block_high_risk_commands"),value:true}
    ]' | native_config patch - >/dev/null
done
while IFS='|' read -r agent model risk wire; do
    profile="opencode_$agent"
    provider="custom.$profile"
    if [ "$agent" = control_plane ]; then
        provider=$CONTROL_PLANE_MODEL_PROVIDER
    fi
    existing=$(native_config get "providers.models.custom.$profile.model" --json 2>/dev/null || true)
    created=true
    if printf '%s' "$existing" | jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1; then
        created=false
    fi
    native_config init "providers.models.custom.$profile" >/dev/null
    # Dashboard is an existing Personal agent. Never rewrite its model,
    # skills, memory, risk profile, or A2A settings during this migration.
    # Fresh bootstrap already supplies its full dashboard configuration.
    if [ "$agent" = dashboard ]; then
        selected=$(native_config get agents.dashboard.model_provider --json | jq -r '.value // ""')
        case "$selected" in
            *.*) ;;
            *) echo "tched-router: dashboard model provider is malformed" >&2; exit 78 ;;
        esac
        native_config get "providers.models.$selected.model" --json |
            jq -e '.value | type == "string" and length > 0' >/dev/null || {
            echo "tched-router: dashboard model provider is not configured" >&2
            exit 78
        }
    else
        for section in "agents.$agent" "agents.$agent.a2a" "agents.$agent.memory"; do
            native_config init "$section" >/dev/null
        done
    fi
    jq -n --arg p "/providers/models/custom/$profile" --arg a "/agents/$agent" \
        --arg model "$model" --arg provider "$provider" --arg risk "$risk" \
        --arg wire "$wire" \
        --argjson dashboard "$([ "$agent" = dashboard ] && echo true || echo false)" \
        --argjson created "$created" '
      (if $created then [{op:"add",path:($p+"/model"),value:$model}] else [] end) + [
       {op:"replace",path:($p+"/uri"),value:"https://opencode.ai/zen/v1"},
       {op:"replace",path:($p+"/wire_api"),value:$wire},
       {op:"replace",path:($p+"/native_tools"),value:false},
       {op:"remove",path:($p+"/provider_extra")}
      ] + (if $dashboard then [] else [
       {op:"add",path:($a+"/enabled"),value:true},
       {op:"add",path:($a+"/model_provider"),value:$provider},
       {op:"add",path:($a+"/risk_profile"),value:$risk},
       {op:"add",path:($a+"/skill_bundles"),value:["default"]},
       {op:"add",path:($a+"/a2a/published"),value:true},
       {op:"add",path:($a+"/a2a/exposed_skills"),value:["plugin-schema"]},
       {op:"add",path:($a+"/memory/backend"),value:"none"}
      ] end)' | native_config patch - >/dev/null
done <<'ROUTES'
dashboard|nemotron-3-ultra-free|dashboard|chat_completions
accountability|glm-5.3-flash|accountability|chat_completions
control_plane|glm-5.3-flash|control_plane|chat_completions
cp_route_grok_47|grok-4.7|control_plane|responses
cp_route_nemotron_3_ultra|nemotron-3-ultra-free|control_plane|chat_completions
cp_route_deepseek_v41_flash|deepseek-v4.1-flash|control_plane|chat_completions
cp_route_glm_53_flash|glm-5.3-flash|control_plane|chat_completions
cp_route_gpt6_astra|gpt-6-astra|control_plane|responses
ROUTES

# Messages-API models. The Anthropic client appends /v1/messages and sends
# x-api-key, which is what OpenCode Zen accepts. uri is the Zen origin only.
while IFS='|' read -r agent alias model; do
    native_config init "providers.models.anthropic.$alias" >/dev/null
    native_config init "agents.$agent" >/dev/null
    provider="anthropic.$alias"
    model_op=add
    uri_op=add
    provider_op=add
    native_config get "providers.models.anthropic.$alias.model" --json 2>/dev/null |
        jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1 && model_op=replace
    native_config get "providers.models.anthropic.$alias.uri" --json 2>/dev/null |
        jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1 && uri_op=replace
    native_config get "agents.$agent.model_provider" --json 2>/dev/null |
        jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1 && provider_op=replace
    jq -n --arg model_op "$model_op" --arg uri_op "$uri_op" --arg provider_op "$provider_op" \
        --arg p "/providers/models/anthropic/$alias" --arg a "/agents/$agent" \
        --arg model "$model" --arg provider "$provider" '[
       {op:$model_op,path:($p+"/model"),value:$model},
       {op:$uri_op,path:($p+"/uri"),value:"https://opencode.ai/zen"},
       {op:$provider_op,path:($a+"/model_provider"),value:$provider}
      ]' | native_config patch - >/dev/null
done <<'MESSAGES'
cp_route_claude_fable_51|zen_claude_fable_51|claude-fable-5-1
cp_route_qwen3_coder_flash|zen_qwen38_flash|qwen3.8-flash
MESSAGES

# Named agents use a runtime profile; the historical [agent] limit is inert.
# The native CLI owns provisioning dynamic profile keys. Its HTTP init endpoint
# only initializes schema sections and cannot create this map entry.
native_config init runtime_profiles.chatbot_500 >/dev/null
iteration_patch='[{"op":"replace","path":"/runtime_profiles/chatbot_500/max_tool_iterations","value":500}]'
for agent in dashboard accountability control_plane cp_route_grok_47 cp_route_nemotron_3_ultra cp_route_deepseek_v41_flash cp_route_glm_53_flash cp_route_qwen3_coder_flash cp_route_gpt6_astra cp_route_claude_fable_51; do
    current_profile=$(native_config get "agents.$agent.runtime_profile" --json | jq -er '.value | strings')
    case "$current_profile" in
        ''|chatbot_500) ;;
        *) echo "tched-router: unexpected named runtime profile; reconcile before migration" >&2; exit 78 ;;
    esac
    iteration_patch=$(printf '%s' "$iteration_patch" | jq --arg p "/agents/$agent/runtime_profile" --arg old "$current_profile" '. + [
      {op:"test",path:$p,value:$old},
      {op:"replace",path:$p,value:"chatbot_500"}
    ]')
done
printf '%s' "$iteration_patch" | native_config patch - >/dev/null
unset iteration_patch current_profile

# Provision the separate, initially empty CP bundle through the native owner.
# The admitted config API can then assign servers without sharing Personal's
# control_plane bundle. Existing bundle contents are preserved by init.
native_config init mcp_bundles.opdbus_control_plane >/dev/null

# The OpenCode API key is raw-file input to native config patch and never an argument.
[ -f "$OPENCODE_API_KEY_FILE" ] || { echo "tched-router: missing OpenCode API key" >&2; exit 78; }
set -- $(stat -Lc '%u %a' "$OPENCODE_API_KEY_FILE")
[ "$1" -eq 0 ] && { [ "$2" = 400 ] || [ "$2" = 600 ]; } || {
    echo "tched-router: OpenCode API key must be root-owned with mode 0400 or 0600" >&2
    exit 78
}
anthropic_key_ops=$(
    for alias in zen_claude_fable_51 zen_qwen38_flash; do
        op=add
        native_config get "providers.models.anthropic.$alias.api_key" --json 2>/dev/null |
            jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1 && op=replace
        printf '%s\t%s\n' "$op" "$alias"
    done
)
jq -n --rawfile key "$OPENCODE_API_KEY_FILE" --arg ops "$anthropic_key_ops" '
  ($key | gsub("[\\r\\n]+$"; "")) as $key |
  (["dashboard","accountability","control_plane","cp_route_grok_47","cp_route_nemotron_3_ultra","cp_route_deepseek_v41_flash","cp_route_glm_53_flash","cp_route_gpt6_astra"] |
   map({op:"replace",path:("/providers/models/custom/opencode_" + . + "/api_key"),value:$key}))
  + ($ops | split("\n") | map(select(length > 0) | split("\t")) |
     map({op:.[0], path:("/providers/models/anthropic/" + .[1] + "/api_key"), value:$key}))' |
    chpst -u jeremy:jeremy env HOME="$CONFIG_DIR/home" USER=jeremy \
        PATH=/usr/local/bin:/usr/bin:/bin ZEROCLAW_CONFIG_DIR="$CONFIG_DIR" \
        "$NATIVE_BIN" config patch --config-dir "$CONFIG_DIR" - >/dev/null

if [ -f "$CHEAPER_INFERENCE_KEY_FILE" ]; then
    set -- $(stat -Lc '%u %a' "$CHEAPER_INFERENCE_KEY_FILE")
    [ "$1" -eq 0 ] && { [ "$2" = 400 ] || [ "$2" = 600 ]; } || {
        echo "tched-router: Cheaper Inference key must be root-owned with mode 0400 or 0600" >&2
        exit 78
    }
    native_config init "providers.models.custom.cheaper_inference" >/dev/null
    cheaper_model_op=add
    native_config get "providers.models.custom.cheaper_inference.uri" --json 2>/dev/null |
        jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1 && cheaper_model_op=replace
    cheaper_key_op=add
    native_config get "providers.models.custom.cheaper_inference.api_key" --json 2>/dev/null |
        jq -e '.value | type == "string" and length > 0' >/dev/null 2>&1 && cheaper_key_op=replace
    jq -n --rawfile key "$CHEAPER_INFERENCE_KEY_FILE" \
        --arg uri_op "$cheaper_model_op" --arg key_op "$cheaper_key_op" '
      ($key | gsub("[\\r\\n]+$"; "")) as $key |
      [
        {op:$uri_op, path:"/providers/models/custom/cheaper_inference/uri", value:"https://api.cheaperinference.com/v1"},
        {op:$uri_op, path:"/providers/models/custom/cheaper_inference/wire_api", value:"chat_completions"},
        {op:$uri_op, path:"/providers/models/custom/cheaper_inference/native_tools", value:false},
        {op:$key_op, path:"/providers/models/custom/cheaper_inference/api_key", value:$key}
      ]' | chpst -u jeremy:jeremy env HOME="$CONFIG_DIR/home" USER=jeremy \
            PATH=/usr/local/bin:/usr/bin:/bin ZEROCLAW_CONFIG_DIR="$CONFIG_DIR" \
            "$NATIVE_BIN" config patch --config-dir "$CONFIG_DIR" - >/dev/null
else
    echo "tched-router: Cheaper Inference key file is absent; provider not configured" >&2
fi

export OPENCODE_API_KEY=$(cat "$OPENCODE_API_KEY_FILE")
[ -n "$OPENCODE_API_KEY" ] || { echo "tched-router: empty legacy provider key" >&2; exit 78; }
export HOME="$CONFIG_DIR/home" USER=jeremy PATH=/usr/local/bin:/usr/bin:/bin
export ZEROCLAW_CONFIG_DIR="$CONFIG_DIR"
# The Sled-authenticated MCP door remains the identity authority. These are
# public endpoint/CA settings, never reconstructed credentials.
export OPDBUS_SLED_AUTH_URL OPDBUS_SLED_CA_FILE
unset TCHED_ROUTER_SANITIZED
exec chpst -u jeremy:jeremy \
    "$NATIVE_BIN" daemon --config-dir "$CONFIG_DIR" --host "$HOST" --port "$PORT" --log-level warn
