# VPS ZeroClaw Runtime Integration Handoff — 2026-08-09

## Goal

Make the dashboard on the VPS (`vps`, `100.69.0.1`, port `8080`) use the real
ZeroClaw runtime as the provider/router authority, with OpenCode
`deepseek-v4-flash-free` as its default. Do not implement another OpenCode
provider inside `op-web`.

## Verified topology

- Target is the **VPS host**, not an Incus container and not the local OpenWrt
  router at `192.168.1.1`.
- The OpenWrt router was briefly changed by mistake and then restored from its
  backup. Its `repr0bate` agent is again `openrouter.openrouter`.
- VPS `op-web-server` listens on `0.0.0.0:8080` under runit.
- VPS `op-grpc-bridge` listens on `127.0.0.1:8090` under runit.
- The VPS has `/usr/local/bin/zeroclaw-gui`, and the GUI process is running.
- No ZeroClaw core executable or daemon was found on the VPS host under the
  names `zeroclaw`, `zeroclawlabs`, `zeroclawctl`, or `zero-claw`.
- No VPS package/cache/history entry for ZeroClaw core was found. AUR currently
  advertises `zeroclaw`, `zeroclaw-git`, `zeroclaw-bin`, and `zeroclawlabs`.
- `/dev/shm/opdbus/state/zeroclaw.json` is a declared projection, not proof of a
  running ZeroClaw daemon.
- `/etc/op-dbus/environment` currently forces:
  `LLM_PROVIDER=salad`, `LLM_MODEL=qwen3.6-27b`.
- The live API confirms the same: `GET /api/llm/status` reports Salad and
  `qwen3.6-27b`; available providers are only Antigravity and Salad.
- The VPS OpenCode CLI is installed and authenticated. `opencode models`
  includes `opencode/deepseek-v4-flash-free`; a direct smoke request succeeded.

## Plugin contract versus runtime behavior

The expanded plugin contract is in
`crates/op-plugins/src/state_plugins/zeroclaw.rs`.

- Typed methods include `ListProviders`, `ListModels`, `SetProvider`,
  `SetModel`, `ListUiSurfaces`, and `Chat`.
- `dispatch_zeroclaw_method` exists for the projected/control methods.
- `SetProvider` and `SetModel` currently only validate against projected state
  and return a `DispatchOutcome` plus a signal. They do not persist a live
  ZeroClaw configuration or reload a daemon.
- `Chat` is deliberately excluded from plugin-owned dispatch and is marked as
  bridge-runtime-owned in tests.
- Therefore, the contract is broad enough for the UI, but it is not yet an
  adapter to a ZeroClaw core process.

Do not solve this by adding an OpenCode CLI provider to `op-llm`/`op-web`. That
was started, recognized as the wrong layer, fully reverted, and the relevant
`op-llm` files are clean.

## Recommended troubleshooting/integration sequence

1. Resolve the intended VPS ZeroClaw core artifact first. Prefer the original
   build/package if it exists outside the searched paths; otherwise install a
   version matching the introspected plugin contract. Verify with
   `zeroclaw --version` before configuring it.
2. Run the core on the VPS host under **runit** (this host does not use
   systemd). Use `zeroclaw daemon`, not a GUI-only process.
3. Configure the core's `opencode` provider using the VPS's existing OpenCode
   credential without printing or copying secrets into Git. The provider-local
   model value is expected to be `deepseek-v4-flash-free`; the OpenCode CLI
   catalog's fully qualified ID is `opencode/deepseek-v4-flash-free`.
4. Verify the runtime directly: health, model listing, then one short chat.
5. Add a narrow ZeroClaw runtime adapter in the bridge/plugin dispatch layer:
   reads proxy runtime state; `SetProvider`/`SetModel` update the runtime and
   reload it; `Chat` proxies the core's typed/HTTP chat surface. Keep capability
   checks in the bridge.
6. Change `op-web` from Salad only after the ZeroClaw runtime path passes live
   verification. Avoid a fallback that silently returns to Salad.
7. Verify through the dashboard and its gRPC-Web calls, not only by editing
   `/dev/shm` projection JSON.

## Capability failures observed in Gallery

The dashboard correctly sends declared `x-opdbus-capability` headers, but its
authenticated sled footprint is not granted the required capabilities in
`/dev/shm/opdbus/capability-grants.json`.

Observed denials:

- `zeroclaw.ListProviders` requires
  `cap.software.zeroclaw.providers.read@v1`.
- `gemma_brain.GetUiSpec` requires `gemma.read`.
- The Gallery was incorrectly calling `zeroclaw.ListUiSurfaces` for its
  Antigravity source. Antigravity's sealed schema instead exposes authoritative
  `fields.ui_surfaces.default` route descriptors.

Do not weaken capability enforcement. Grant the dashboard sled only the needed
read capabilities through the existing grants provisioning path.

## Gallery rendering issue

`GET /api/ui-model/gallery` returns three catalog specs. They are nested tree
specs shaped as `type/props/children`, while the installed json-render React
renderer expects normalized `root/elements` references. This is why cards show
Promote/Delete controls but a blank rendered body.

The intended UI fix is a compatibility normalizer in `SpecRenderer`, plus an
Antigravity source based on Antigravity's own sealed projection. An attempted
remote patch did not apply because `apply_patch` is unavailable on the VPS;
there are no partial Gallery edits from that attempt.

## Useful verification commands

```sh
ssh vps 'ps -eo pid,user,comm,args | rg -i "claw|zero"'
ssh vps 'curl -fsS http://127.0.0.1:8080/api/llm/status | jq .'
ssh vps 'curl -fsS http://127.0.0.1:8080/api/llm/providers | jq .'
ssh vps 'opencode models | rg "deepseek-v4-flash-free"'
ssh vps 'jq "{status,selected_provider,selected_model}" /dev/shm/opdbus/state/zeroclaw.json'
```

Never print `/etc/op-dbus/environment`, ZeroClaw encrypted secrets, OpenCode
credentials, pairing tokens, or the full capability-grants document in logs.
