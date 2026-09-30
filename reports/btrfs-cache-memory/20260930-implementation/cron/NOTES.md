# OD-95 live evidence and remaining identity-runtime integration

No configuration, job, grant, service, seal, or host scheduler was changed in this investigation. Existing job `936792bf-cebd-407b-94bc-b241324b6c54` remains enabled and retains its history.

## Verified live state

- Admitted chatbot lungs `cron.list_jobs` (event 203244) returns `provider-model-refresh`, agent `control_plane`, hourly `17 * * * *`, UTC, isolated, memory-free. Its last error is `blocked by security policy: autonomy is read-only`.
- Admitted human configuration reads confirm `agents.control_plane.risk_profile = control_plane`, `risk_profiles.control_plane.level = readonly`, runtime profile `chatbot_500`, MCP bundle `opdbus_control_plane` containing server `opdbus_compact`.
- Redacted `GetConfig` (event 203269) shows the risk profile is shared by eight control-plane aliases; runtime profile `chatbot_500` is shared by ten aliases. Changing these profiles affects those aliases, not just the job.
- The runtime profile has `agentic = false`. This is not established as a cron blocker: pinned cron calls `agent::run` directly, whose complete source has no `agentic` condition. Do not change this field merely from its label.
- HTTP MCP URL is `https://dashboard.ghostbridge.tech:8090/mcp?opdbus_mode=compact`. Normal certificate verification fails hostname matching. The certificate includes `svc0`, `localhost`, `10.0.0.2`, and other private IPs; normal verification succeeds at `https://10.0.0.2:8090/mcp?opdbus_mode=compact`. Unauthenticated initialize correctly denies access. The configured server already contains an `x-opdbus-sealed-id-bin` header; the unauthenticated probe does not establish that configured identity is missing. No header value was printed.
- `cron.update_job` currently cannot change `allowed_tools`; `agent` is used for shell validation, not an owning-agent reassignment. Existing per-run list `["model_registry.refresh"]` does not match the native MCP `<server>__<tool>` naming convention. Compact `execute_tool` is generic, so allowing it does not enforce a single backend method.
- Host namespace has no observed 3tchedFS FUSE mount or mount process. This does not establish what is mounted inside another container namespace. Native daemon PID 1504 runs as `jeremy`; the host lungs socket is root-client-only. Do not replace the identity filesystem with a privileged external launcher.

## Completed provider refresh and durable readback

The existing admitted chatbot lungs identity successfully called `model_registry.refresh` without a provider filter (event 203253), using provider model-list GETs, with no inference call. Cozo revision advanced from 18 to 28, refreshed at `2026-09-30T09:52:14Z`.

Seven providers refreshed successfully, with 858 currently available models in total: OpenCode Go 30, OpenRouter 464, Cheaper Inference 72, OpenAI 136, Salad 4, Hugging Face 135, Ollama 17. xAI returned HTTP 403; Anthropic's configured key file was absent; VoyageAI returned HTTP 404. Their errors remain visible. Admitted `list_providers` and `list_models` read revision 28 back, including model `last_seen` at the refresh timestamp. See `initial-refresh.json`, `list-providers-after.json`, and `list-models-after.json`.

## Source-checked identity-filesystem route

`crates/threetched-fs/src/namespace.rs:174-190,410-436` creates identity lungs files at `plugins/model_registry/operations/refresh/{call,result,error}` with a convenience symlink at `operations/model_registry/refresh`. A complete JSON write to `call` is dispatched as the mount identity; the receipt is read from `result` and a stage-specific failure from `error` (`fuse_fs.rs:149-170`). State leaves are live SHM projections on every open.

Pinned native `file_write` uses a direct `tokio::fs::write`, so it can operate on a lungs call file, subject to policy/path guards and its persistent-workspace gate. `uses_memory=false` removes memory tools but does not intrinsically remove filesystem tools. A scheduled identity-local job must see its own mount within its allowed workspace, write `{}` to the actual non-symlink `plugins/model_registry/operations/refresh/call`, read both outcome files, then read fresh provider/model lookup state. No direct Cozo edit or external HTTP/stdio substitution is needed.

## Concrete remaining integration decision

Before mutation, locate the existing control-plane container and its identity-local 3tchedFS mount, verify this exact call/result/error path through that container's filesystem tools, and verify the existing identity grants there. This mount path and scheduled execution context are not supplied by the current native job record or admitted config reads; registering a filesystem job before resolving them would save an unexecutable prompt.

Once the existing runtime path is known, the least scoped native job repair is: permit the named owning agent's actual filesystem call operation under a reviewed supervised policy; use strict per-run `file_write`/`file_read` tools; retain isolated/memory-free execution; disable the old UUID preserving history and create the corrected hourly replacement because the retained update schema cannot replace its tool list. Preserve the old job and exact configuration values for rollback. The shared eight-agent risk-profile scope requires an explicit configuration decision; do not silently weaken it. An existing agent-specific profile is preferable if the identity runtime already provides one.

The HTTP hostname repair is a separate existing configuration defect. It is unnecessary to introduce a new transport to meet the identity-filesystem architecture.

## Bounded follow-up: existing identity runtime located

Read-only Incus inventory (default is the only project) locates the control-plane chatbot identity container `b0c777e3-0955-cd8b-3d01-ca12998cae28`. It is **Stopped**, PID 0. The human identity and both archived identity instances are also stopped; only mail-3tched, qdrant and xray run. See `container-inventory.json`.

The chatbot's existing Incus `3tched` disk device is writable and maps host `/run/opdbus-lungs/b0c777e3-0955-cd8b-3d01-ca12998cae28` to identity-local `/opt/run-mounts/3tched`. Thus its exact intended operation path is `/opt/run-mounts/3tched/plugins/model_registry/operations/refresh/call`, with sibling `result` and `error`. See `chatbot-container-devices.txt`. The host lungs source directory currently does not exist; no FUSE mount is active there. The shared bridge socket exists.

Read-only installed `op-identity-root status` reports both identities' `device=down root=down lungs=down ct=STOPPED`. No start, mount, repair or configuration action was performed. Release-owned `deploy/runit/libexec-3tched/op-identity-root:131-155,160-180` already defines the correct identity lungs mounting and container startup; `op-lungs-check:20,40-57` explicitly names this chatbot session and its in-container path. Its call-bearing check was read but **not executed** during this read-only follow-up.

Scheduled native execution is separate from that stopped identity context: admitted redacted configuration says `runtime.kind = native`; native daemon PID 1504 has cgroup `/` and mount namespace `4026531832`, identical to this host session. Pinned `cron/scheduler.rs:527,730-746,809-828` resolves the configured agent and calls `agent::run`; `agent/loop_.rs:1215-1216` creates the adapter from `config.runtime`. No identity-container entry or mount bind occurs at these inspected call sites. Other running Incus init mount namespaces were checked and show no identity lungs mount. `lxc-ls` is empty; Docker/Podman tools were absent.

The missing decision is therefore concrete: restore the **existing** chatbot device/root/lungs/container lifecycle through its existing admitted runtime authority, and bind scheduled `control_plane` execution to that identity's filesystem/tool context. Merely changing read-only autonomy in the host daemon or creating another native cron record would not establish that binding. Existing admitted chatbot calls have already proven `model_registry.refresh` and `model_registry.read`; configured source grants also include `cron.read`/`cron.invoke`. No new identity or grant is required for those methods. Activation and scheduler execution-context changes were outside this bounded read-only task and remain unapplied.
