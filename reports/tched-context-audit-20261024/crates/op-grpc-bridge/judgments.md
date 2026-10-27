# Declarative JEV receipts — op-grpc-bridge

## J1 — `tched_catalog_binding::select` mixed-family candidate behavior

**State supplied:** exact freshly read source context from `src/tched_catalog_binding.rs:92-178`, including candidate-family guard and matching logic; test context `:204-223`, including `custom.zen` + matching `openai.go` fixture and expected `openai.go`; project authority context that source inventory is not runtime existence proof and candidate inventory may contain profiles from multiple families.

Questions and successful answers:
- Observed semantic intent of family guard: `insufficient`, confidence 0.09. Probabilities: reject entire inventory 0.23, filter family 0.38, insufficient 0.39.
- Internal source/test consistency: `contradictory`, confidence 0.91. Probabilities: consistent 0.05, contradictory 0.94, insufficient 0.01.
- Smallest supported correction: `skip`, confidence 0.48. Probabilities: skip unrelated-family candidate 0.65, change test to expect global rejection 0.12, no supported correction 0.23.

**Human disposition:** changed the guard to skip unrelated-family candidates, relying on the explicit expected behavior in the source's regression test. JEV change recommendation is weak; the outcome is recorded as a narrow test-contract reconciliation, not as a universal runtime or owner policy. No follow-up reframing was performed.
## Continuation judgments (fresh evidence only)

### J2 — Manifest dependency and runtime integration (`Cargo.toml:43-51`)

**Evidence supplied:** exact lines describing the root-owned model registry list, `reqwest` as the HTTP client for the `tched_router` runtime integration, in-process cognitive-tool dispatch, and `tched-fs` path dependency.

**Answers:** tched role = `integration` (confidence 1.00); comment says cognitive-tool dispatch is in-process behind MutationEngine = yes (0.98).

**Disposition:** references identify dependencies/integration, not an independent schema/provider authority.

### J3 — Router projection proto (`proto/operation.proto:916-941`)

**Evidence supplied:** comment says read-only gRPC view of plugin route/provider tree; declarations expose schema JSON, repeated model routes, and providers.

**Answers:** role = `projection` (confidence 1.00); distinct model-route and provider projection fields exist alongside schema JSON (0.96).

**Disposition:** keep as a read-only projection contract; it does not itself select providers or models.

### J4 — Agent run completion path (`src/agent_runs.rs:27,48-70,345-363`, generic “matched” at 329)

**Evidence supplied:** production Completer uses `TchedRouterRuntimeClient::from_env().with_sealed_identity(...).chat_under_guard(input)`; the caller scope supplies identity; request includes router chat message types; line 329 says broker event “matched” the trigger.

**Answers:** router types/client role = `upstream_integration` (confidence 1.00); “matched” = `generic_event` (0.97).

**Disposition:** integration references are current production-path descriptors; generic “matched” is not a project identifier.

### J5 — Historical route comparison (`src/context_bundle.rs:1-17,110-147`)

**Evidence supplied:** documentation explicitly dates `tched_router.Chat` as the pre-2026-09-19 production turn and contrasts it with the chatbot as consumer now; function receives selected/admitted context before memory gathering.

**Answers:** `historical_comparison` (confidence 1.00); this module establishes provider/model authority = no (0.12).

**Disposition:** retain as historical comparison, not proof the old route remains active.

### J6 — Mode route request normalization (`src/chat_mode_routing.rs:1-17,37-90,118-141`)

**Evidence supplied:** ordinary modes preserve request provider/model; ControlPlane and Accountability clear them and choose a dedicated alias; `resolve` rereads the caller's persisted mode, selects alias client or `from_env`, derives and attaches sealed identity. Alias profile loader and upstream contract are absent.

**Answers:** `only_routing` (confidence 0.99); verified caller session and sealed identity are used (0.94).

**Disposition:** request normalization and client constructor are established; the authoritative provider/model source after clearing is not established by this file.

### J7 — Control Plane provider/model precedence (`src/control_plane_routing.rs:1-15,24-88,139-179,182-248`)

**Evidence supplied:** production observes `current_selection`, overwrites request fields with nonempty native provider/model, checks the native admission before prompt construction and execution, requires output provider/model to equal the observation, and records `source="native_agent"`; test fixture exposes native property/list values and asserts request override is ignored.

**Answers:** `native_overrides_request` (confidence 1.00); admission checks plus identity attachment (bool confidence 0.32, uncertain).

**Disposition:** code directly establishes local precedence and reported output matching, not the complete sealed-producer/persisted-role/pinned-backend chain. Low JEV confidence on admission claim is not treated as a contrary code observation.

### J8 — Cron gateway (`src/cron_gateway.rs:1-13,37-45,67-101,121-243`)

**Evidence supplied:** file documents v0.8.4 mapping over the shared runtime client; production constructor uses `TchedRouterRuntimeClient::from_env`; methods map cron endpoints and typed responses; error text labels failures with `tched_router`.

**Answers:** `gateway_integration` (confidence 1.00); this file proves pinned v0.8.4 executable/source (no, 0.06).

**Disposition:** shared gateway adapter; the version string in this file is not pin/provenance evidence.

### J9 — Projector test fixture (`src/chain_vector_projector.rs:302-325`)

**Evidence supplied:** a named test installs `TchedRouterPlugin::new().schema()` into a temporary sealed-catalog fixture, creates a `tched_router` block and asserts schema-filtered embedding text.

**Answers:** role = `fixture` (confidence 1.00); tests described-field filtering into embedding/payload (yes, 0.85).

**Disposition:** fixture occurrence does not establish production routing authority.

### J10 — Bridge executable entry point (`src/bin/op-grpc-bridge.rs:1-8,20-31`)

**Evidence supplied:** current executable uses `run_tched_router_server`; comments say current service exposes the 3tched Router schema API on TCP 8090 and former zeroclaw entrypoint was folded into it.

**Answers:** role = `active_server` (confidence 1.00); old entrypoint is explicitly marked former/folded into current service (0.98).

**Disposition:** current server entrypoint includes the router schema API; former executable name is historical.
