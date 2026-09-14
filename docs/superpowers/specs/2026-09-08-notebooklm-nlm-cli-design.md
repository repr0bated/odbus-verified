# NotebookLM provider cutover: jacob-bd `nlm` CLI

Date: 2026-09-08  
Status: draft for review  
Plugin: `notebooklm`  
Upstream binary: `nlm` from PyPI `notebooklm-mcp-cli` (jacob-bd / gemini-notebook-mcp-cli)

## Goal

Make NotebookLM authentication and catalog access reliable on this host.

Success bar: `setup_auth` completes, live `auth_status` is `configured`, `notebook_list` returns the Google account catalog, and that state survives an `op-grpc-bridge` restart with no provider overlay and no hostname patch.

## Non-goals

- nexcore-notebooklm (0.1.0). It is a DOM driver that still hardcodes `https://notebooklm.google.com` and returns from `setup_auth` before login finishes.
- PleasePrompto `notebooklm-mcp` 2.0.0 Node tree, Streamable-HTTP `:3101`, `http.js` overlay, `auth-manager.patch`.
- New Python source in this repository.
- `nlm setup` / `nlm skill` (they configure other agents, not odBus).
- A runit timer for `nlm auth refresh` (optional follow-up, not this cut).
- Promoting every new method into MCP in a second undocumented pass — the allowlist rewrite is part of this cut’s generation bump, listed below.

## Why this provider

PleasePrompto and nexcore both keep a headed Chrome session and a local `library.json`. Login does not populate that file, so `list_notebooks` can be empty while `authenticated` is true.

jacob-bd extracts cookies once (`nlm login`), records whichever host accepted the account (`notebook.google.com` vs `notebooklm.google.com`) in profile `metadata.json`, then calls Gemini Notebook’s internal HTTP APIs. `nlm login --check` live-probes credentials. `nlm notebook list` is the real catalog.

The executable is an external binary. The plugin id stays `notebooklm`. That matches the zeroclaw / `tched_router` rule: `Command::new("nlm")` is the only subprocess; everything around it uses the odBus plugin name.

## Architecture

```
MCP/gRPC/D-Bus  →  MutationEngine.notebooklm  →  /usr/local/bin/nlm <args>
                                              →  parse structured stdout
                                              →  present-state + ready marker
```

- D-Bus remains the control plane. Callers never invoke `nlm` themselves.
- `SupervisedMcpProvider` for notebooklm is removed. There is no loopback MCP sidecar and no `:3101`.
- Identity, capabilities, and the 660s `setup_auth` timeout stay in `op-grpc-bridge`.
- Profile and cookies live under `/home/jeremy/.notebooklm-mcp-cli/` (upstream default). odBus does not invent a second cookie store.
- Never set `NOTEBOOKLM_COOKIES`, `NOTEBOOKLM_CSRF_TOKEN`, or `NOTEBOOKLM_SESSION_ID` in the runit or bridge environment. Those env vars override disk credentials and freeze stale auth.

## Install and pin

- Pin exact PyPI version + sha256 of the wheel in `deploy/config/` (replace the Node tarball declaration; keep a single version file for this provider).
- Golden install puts `nlm` at `/usr/local/bin/nlm` as user `jeremy` can execute. Do not vendor the package source into `crates/`.
- Host needs Python >= 3.11. Fail golden install if it is missing.
- `DISPLAY=:20` and `XAUTHORITY=/home/jeremy/.Xauthority` are required only for interactive `nlm login`. Read-only `nlm` commands run headless.

## Schema

Redo `notebooklm_schema()` to jacob-bd tool names (snake_case). No PleasePrompto aliases. Breaking change. Reseal the plugin blob.

### Canonical methods (jacob-bd)

Notebooks: `notebook_list`, `notebook_create`, `notebook_get`, `notebook_describe`, `notebook_rename`, `notebook_delete`

Sources: `source_add`, `source_list_drive`, `source_sync_drive`, `source_delete`, `source_describe`, `source_get_content`, `source_rename`

Query: `notebook_query`, `notebook_query_start`, `notebook_query_status`, `chat_configure`

Chats: `chat_list`, `chat_get`, `chat_export`

Studio: `studio_create`, `studio_status`, `studio_delete`, `studio_revise`

Downloads/export: `download_artifact`, `download_all_artifacts`, `export_artifact`

Research: `research_start`, `research_status`, `research_import`

Notes/labels: `note`, `label`

Share: `notebook_share_status`, `notebook_share_public`, `notebook_share_invite`, `notebook_share_batch`

Auth/server: `refresh_auth`, `save_auth_tokens`, `server_info`

Meta: `batch`, `cross_notebook_query`, `pipeline`, `tag`

Each method gets a dedicated schemars Input and Output. Do not use `AckOutput`. Unified jacob tools (`source_add`, `studio_create`, `label`, `note`, `batch`, `pipeline`, `tag`) take an action/type field rather than splitting into one plugin method per flag.

Subids stay in the existing taxonomy (`obs.service.plugin.notebooklm.*` / `mut.service.plugin.notebooklm.*`). New methods get new subids registered in the OSCAL registry. Retired PleasePrompto method names are deleted from the schema, not left empty.

### odBus-only methods (jacob has no MCP tool)

| Method | Upstream | Why keep |
|---|---|---|
| `setup_auth` | `nlm login` | Interactive login exists only as CLI |
| `reauth` | `nlm login --force` | Distinct from cookie refresh |
| `select_notebook` | present-state only | Last-used notebook id when callers omit it |
| `get_health` | `nlm login --check` | Same live probe as `server_info`; kept so MCP auth has a health verb |

`get_health` and `server_info` share one CLI invocation. They are not two probes.

### Deleted PleasePrompto methods

`list_notebooks`, `get_notebook`, `query_notebook`, `search_notebooks`, `get_library_stats`, `add_source_url`, `add_source_text`, `add_source_drive`, `add_source_file`, `auto_label_sources`, `create_label`, `rename_label`, `set_label_emoji`, `move_source_label`, `delete_label`, `create_audio`, `create_video`, `create_report`, `create_quiz`, `create_flashcards`, `create_infographic`, `create_mindmap`, `create_slides`, `revise_slides`, `describe_studio`, `get_audio_status`, `list_artifacts`, `start_research`, `import_research`, `share_public`, `share_invite`, `get_share_settings`, `disable_share`, `batch_operation`, `run_pipeline`, `list_pipelines`, `tag_add`, `tag_list`, `tag_smart_select`, `list_sessions`, `close_session`, `reset_session`.

Their replacements are the jacob names above (`list_sources` → `source_list_drive` plus `notebook_get` as needed; `list_artifacts` → `studio_status`; `disable_share` → `notebook_share_public` with disable).

## Dispatch

`MutationEngine` notebooklm arm:

1. Resolve method → argv (fixed map, no shell).
2. If the method needs a notebook id and the args omit it, use `select_notebook` present-state.
3. `Command::new("nlm")` with argv only. `HOME=/home/jeremy`, `USER=jeremy`. For `setup_auth` / `reauth`, also `DISPLAY` and `XAUTHORITY`.
4. Timeout: 660s for `setup_auth`/`reauth`; existing provider-call timeout for others; research/studio may use the longer query timeout jacob documents (default 120s, caller-overridable where the schema has `timeout`).
5. Prefer jacob structured output (`--json` if present, else `--ai`). If neither exists, fail the method rather than scraping Rich tables.
6. Non-zero exit → D-Bus/gRPC error. Redact cookie strings from stderr before returning.
7. Missing `nlm` binary → failed, not the generic echo arm.
8. After `setup_auth`, `reauth`, `refresh_auth`, `save_auth_tokens`, `server_info`, and `get_health`, update `/run/opdbus/runit-ready/notebooklm-mcp-authenticated` from live `auth_status == configured`.

`mcp_frontend` keeps the long timeout for `plugin.notebooklm.setup_auth`. Rename checks if the tool name stays `setup_auth`.

## Capabilities

Keep `notebooklm.read`, `notebooklm.invoke`, `notebooklm.admin`.

- `notebooklm.admin`: `setup_auth`, `reauth`, `save_auth_tokens`
- `notebooklm.read`: list/get/describe/query/status/export/server_info/get_health and other SideEffect::Read methods
- `notebooklm.invoke`: remaining mutations, including `refresh_auth`

Update capability descriptions to the new method names. Grants JSON principal lists stay; they already include these three ids.

## MCP allowlist

Bump `mcp-toolsets.json` generation.

`notebooklm_auth` (hot, requires provider health): `plugin.notebooklm.get_health`, `plugin.notebooklm.setup_auth`, `plugin.notebooklm.server_info`, `plugin.notebooklm.refresh_auth`

`notebooklm_research` (hot, requires authenticated marker): `plugin.notebooklm.notebook_list`, `plugin.notebooklm.notebook_get`, `plugin.notebooklm.notebook_query`, `plugin.notebooklm.select_notebook`, `plugin.notebooklm.chat_list`, `plugin.notebooklm.tag`

Further jacob methods are wired on D-Bus/gRPC in this cut. Adding them to MCP is a later generation bump, not an accidental expansion of the research set.

Provider health for the research set: the authenticated ready marker, not a `:3101` HTTP probe. `requires_provider_health` for auth set: `nlm` exists and `get_health` is callable (binary present). If `nlm` is missing, neither set is admitted.

## Runit / golden cutover

- Remove service `notebooklm-mcp` (run, check, log, finish) from golden and from `build-golden.sh` helper lists.
- Remove `deploy/runit/provider-overlays/notebooklm-mcp/` and the Node tarball pin.
- Remove `OP_NOTEBOOKLM_MCP_URL` default `http://127.0.0.1:3101/mcp`.
- Plugin `current_state()`: ready when `nlm` is on PATH and profile dir exists; `authentication_required` when `auth_status` is not `configured`; `unavailable` when `nlm` is missing.
- `notebook-sources-sync` is unchanged (local source tree, not this provider).

## Error handling

- Live check `stale` → `get_health`/`server_info` report unauthenticated and tell the caller to run `setup_auth`. Do not treat file presence as success.
- Live check `unverified` → do not flip the ready marker to false if it was configured; return the jacob status so callers can retry without re-login.
- Internal Google API changes: surface jacob’s error text (redacted). No overlay patches on jacob source.

## Testing

Unit: argv map for every schema method; selected-notebook defaulting; cookie-redaction; ready-marker transitions for `configured` / `stale` / `unverified`.

Live (this host):

1. `nlm login --check` as jeremy.
2. MCP `plugin.notebooklm.setup_auth` if not `configured` (headed `:20`).
3. MCP `plugin.notebooklm.get_health` / `server_info` → `configured`.
4. MCP `plugin.notebooklm.notebook_list` → non-empty Google catalog (or an explicit empty-account result, not a local `library.json` stub).
5. `sudo sv restart op-grpc-bridge` then repeat 3–4 with no overlay files on disk.
6. Direct `127.0.0.1:3101` must be closed.

## Risks

- jacob-bd uses undocumented Google APIs; they can break without notice.
- Interactive login still needs the existing Chrome/display path.
- Breaking MCP tool names: clients using `plugin.notebooklm.list_notebooks` must switch to `notebook_list`.
- Python is an upstream binary only. Recurring “no new Python in-repo” still holds.
