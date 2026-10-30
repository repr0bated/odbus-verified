# Findings — op-web tched context audit

## F1 — Active op-web callers target obsolete `tched_router` identity; safe replacement is not established

Evidence in assigned crate:
- `src/tched_router_routes.rs:12-14` originally described `tched_router` as the canonical D-Bus/SHM ID; that comment was corrected to say this is the legacy ID referenced by current op-web compatibility paths. The constant and all behavior remain unchanged.
- `src/handlers/tched_router.rs:102-120` passes this ID to `call_plugin_method`; lines 293-315 issue `ListModels` with `cap.software.3tched-router.models.read@v1`; corresponding chat handler dispatches `Chat` and its old capability.
- `src/handlers/tched_router.rs:369-386` reads that plugin's sealed `PluginSchema`; lines 401-467 hard-code Chat/ListModels contracts and old operation IDs.
- `src/routes/llm.rs:74-80,125-140` treats the old projection as the source for the active model-list API.
- `src/routes/mod.rs` wires the schema/model compatibility endpoints to the old handler, while chat paths have separate canonical handlers.

Current authority/context: the assignment's coordinator note reports current registry source retires `tched_router` and `threetched.rs` registers the binary-captured `threetched` schema; it warns against treating source IDs as proof of runtime/deployment state. The shared AGENTS invariant makes the sealed blob the plugin and sole schema authority. `SIGNALS.md:915-917` is explicitly sibling historical/source evidence: the isolated seal found no `tched_router` and 148 threetched methods, while a separate migration checkpoint says chat/UI/router callers remain unfinished and paused. It does not identify the exact op-web replacement contract or establish live state.

JEV `jdgb-15b33b70757c9360` favors fixing active old-ID callers (handler context confidence 0.76; model path 0.75), but the provided evidence does not establish which `threetched` method, capability, input, output, or model-selection fields replace these assumptions. A mechanical ID rename would preserve unverified Chat/ListModels contracts and is unsafe; removing public paths is also unsupported. The only change is the evidence-backed comment correction above; active callers remain pending the canonical contract and compatibility decision.

## F2 — Other references are unresolved, not blanket-renamed

`src/sse.rs` includes `tched_router` in a legacy state hydration allowlist; JEV was insufficient to decide whether to remove it or point it elsewhere. `handlers/agents.rs` calls a helper whose actual state target is the independent `workflows` plugin; JEV did not support calling this a router caller. `handlers/llm.rs` reads selected-provider state through the old router helper and remains a likely stale dependency, but the exact canonical selection fields are absent. Route aliases, comments, negative references, and historical names cannot be changed solely by substring. See partial ledger and JEV receipt.

## Scope limitations

The requested exhaustive per-occurrence audit is incomplete. Ordinary incidental substrings were not enumerated/judged individually; no total count is claimed. This report does not claim source audit completion, test/build success, runtime behavior, or deployment state.
