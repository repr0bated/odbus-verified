# MCP Bridge: VS Code Extension Emulation

This setup replaces the old Antigravity path. `op-mcp-proxy` direct mode calls
`cloudcode-pa.googleapis.com` and sends VS Code Cloud Code-style request headers.

## Runtime Model Policy

For `LLM_MODEL=auto`, the runtime selector is constrained to Gemini 3 family only:

- `gemini-3-flash` for default traffic.
- `gemini-3-pro` for larger/complex prompts.
- If preview mode is enabled, selector rewrites to:
  - `gemini-3-flash-preview`
  - `gemini-3-pro-preview`

This is implemented by `deploy/dinit/op-mcp-proxy-select3`.

## Required Environment

```bash
export ENABLE_MCP_PROXY_PROVIDER=true
export LLM_PROVIDER=mcp-proxy
export OP_MCP_PROXY_BIN=/usr/local/bin/op-mcp-proxy-select3
export OP_MCP_PROXY_REAL_BIN=/usr/local/bin/op-mcp-proxy
export LLM_MODEL=auto

# Optional project override
export MCP_PROXY_GCLOUD_PROJECT=operation-dbus

# Gemini 3 family auto-selection bounds
export MCP_PROXY_AUTO_FLASH_MODEL=gemini-3-flash
export MCP_PROXY_AUTO_PRO_MODEL=gemini-3-pro
export MCP_PROXY_AUTO_PRO_THRESHOLD_CHARS=6000

# Optional: force preview model IDs regardless of Gemini CLI settings
export MCP_PROXY_EXPERIMENTAL=true
```

If `MCP_PROXY_EXPERIMENTAL` is unset, preview mode follows
`~/.gemini/settings.json` -> `general.previewFeatures`.

## VS Code Emulation Headers

Defaults are applied by `op-mcp-proxy`. Override only if needed:

```bash
export MCP_PROXY_USER_AGENT="google-cloud-code-vscode/1.22.0 (GPN:Cloud Code for VS Code) vscode/1.85.0 (linux; x64)"
export MCP_PROXY_X_GOOG_API_CLIENT="gl-rust/1.76.0 gax/2.12.0 gapic/1.0.0"
export MCP_PROXY_ORIGIN="vscode://googlecloudtools.cloudcode"
export MCP_PROXY_REFERER="vscode://googlecloudtools.cloudcode"
export MCP_PROXY_X_CLIENT_DATA="eyJpc0lkZSI6dHJ1ZSwiaWRlVHlwZSI6InZzY29kZSIsImlkZVZlcnNpb24iOiIxLjg1LjAiLCJwbHVnaW5WZXJzaW9uIjoiMS4yMi4wIn0="
```

`x-goog-user-project` is sent from `MCP_PROXY_GCLOUD_PROJECT` (or discovered
project) unless disabled:

```bash
export MCP_PROXY_SEND_X_GOOG_USER_PROJECT=false
```

## Dinit Service Install

The tracked setup files for this runtime live in `deploy/dinit/`:

- `deploy/dinit/op-dbus`
- `deploy/dinit/op-dbus-dinit.sh`
- `deploy/dinit/op-mcp-proxy-select3`
- `deploy/dinit/environment.op-dbus.template`
- `deploy/dinit/install-op-dbus-dinit.sh`

Install:

```bash
doas ./deploy/dinit/install-op-dbus-dinit.sh
```

## Quick Verify

1. Ensure Gemini CLI creds exist: `ls ~/.gemini/oauth_creds.json`
2. Confirm service env has `LLM_MODEL=auto`.
3. Confirm logs show bridge emulation:
   `MCP bridge IDE emulation enabled`.
