# Code Assist Escalation Notes (2026-02-11)

## Scope

Validate whether moving to a brand new enterprise project fixes `cloudcode-pa.googleapis.com` errors.

## Projects

- Old project: `operation-dbus` (`419902526714`)
- New project: `op-dbus-ent-20260211-1621` (`175169996230`)

## New Project Setup Completed

- Billing linked: `billingAccounts/016C53-111148-0513AB`
- Enabled APIs:
  - `cloudaicompanion.googleapis.com`
  - `geminicloudassist.googleapis.com`
  - `geminicodeassistmanagement.googleapis.com`
  - `serviceusage.googleapis.com`
  - `aiplatform.googleapis.com`
- IAM on new project for `jeremy@3tched.com`:
  - `roles/owner`
  - `roles/cloudaicompanion.settingsAdmin`
- ADC quota project set and verified:
  - `gcloud auth application-default set-quota-project op-dbus-ent-20260211-1621`
  - `~/.config/gcloud/application_default_credentials.json` contains:
    - `"quota_project_id": "op-dbus-ent-20260211-1621"`

## Runtime Env Correction

`op-web` had stale environment values until service definition reload.

Required sequence:

```bash
doas dinitctl reload op-web
doas dinitctl restart op-web
```

After reload/restart, runtime env shows:

- `GOOGLE_CLOUD_PROJECT=op-dbus-ent-20260211-1621`
- `GOOGLE_CLOUD_QUOTA_PROJECT=op-dbus-ent-20260211-1621`
- `MCP_PROXY_GCLOUD_PROJECT=op-dbus-ent-20260211-1621`
- `MCP_PROXY_QUOTA_PROJECT=op-dbus-ent-20260211-1621`
- `MCP_PROXY_SEND_X_GOOG_USER_PROJECT=true`
- `MCP_PROXY_DISABLE_GEMINI_OAUTH=true`

## Direct Repro (Proxy Path)

Command path used:

- `/usr/local/bin/op-mcp-proxy-select3`
- `DIRECT_MODE=1`
- `MCP_PROXY_DISABLE_GEMINI_OAUTH=true`
- Explicit project+quota vars set to target project

Result for old and new projects is the same:

- HTTP `403`
- `PERMISSION_DENIED`
- `reason: SERVICE_DISABLED`
- `service: cloudcode-pa.googleapis.com`
- Consumer matches requested project

### New project proof

- `consumer: "projects/op-dbus-ent-20260211-1621"`
- Message: Cloud Code Private API not used in project or disabled

### Old project proof

- `consumer: "projects/operation-dbus"`
- Same `SERVICE_DISABLED` message

## Manual API Repro (No App Layer)

Direct `curl` to:

- `https://cloudcode-pa.googleapis.com/v1internal:generateContent`

Headers included:

- `Authorization: Bearer $(gcloud auth application-default print-access-token)`
- `x-goog-user-project: <project-id>`
- VS Code Cloud Code style headers (`User-Agent`, `x-goog-api-client`, `Origin`, `Referer`, `x-client-data`)

Result for both projects:

- HTTP `403`
- `SERVICE_DISABLED`
- `consumer` equals selected project

This confirms the issue is not in local app routing when using direct calls.

## Current App Limitation (Separate)

Current `op-web` runtime advertises only:

- `providers=["antigravity"]`

`mcp-proxy` provider is not available in this deployed binary, so `/api/chat` currently cannot route through the MCP proxy path directly.

## Escalation Ask

Support request should include:

1. Both project IDs and numbers.
2. Exact 403 payload showing `SERVICE_DISABLED` and `consumer` for each project.
3. Confirmation that required companion/gemini APIs are enabled and billing is attached.
4. Confirmation that ADC quota project is set and `x-goog-user-project` header is sent.
5. Clarification request on entitlement gating for `cloudcode-pa.googleapis.com` in enterprise org projects.
