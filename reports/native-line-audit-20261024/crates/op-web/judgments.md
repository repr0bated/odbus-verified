# JEV judgments — op-web

Declarative judge instructions were read from `xd://eval/judge`.

## Successful call J1

State supplied all actual contexts and this authoritative project context: production UI is external SvelteKit/Vite/json-render Svelte; op-web serves static deployed assets; `crates/op-web/ui` is retired. Questions:
- Cargo.toml:71 `tokio1-native-tls`: TLS backend versus provider/model routing, accuracy.
- build.rs:2-4 `current UI is native Rust/egui`: accurate, stale, or insufficient.

Answers: `tls_backend` confidence 0.99 (p=.99), `stale` confidence 1.0 (p=1).

## Successful call J2

State supplied the full pre-edit build.rs comment and Cargo watcher plus the current UI contract, and `door_proxy.rs:68-79` TLS configuration. Questions:
- Cargo feature term `native`: `accurate_tls`, confidence 1.0.
- Build comment and proposed correction: `fix_supported`, confidence 0.89 (probabilities: fix_supported .93, no_change .05, unresolved .02). The comment is stale; correction removes only the inaccurate UI description while retaining the watcher claim.
- `with_native_roots()`: `accurate_trust_roots`, confidence 1.0.

The first attempted call used an invalid question shape and failed; it produced no judgment and is not counted as evidence. J1 and J2 are the successful calls.

## Successful call J3 — first contextual review of remaining source occurrences

Source contexts supplied: `execution.rs:234-253` and `orchestrator/tools.rs:132-171` (two `native protocols` descriptions); `parsing.rs:1-14` and `:99-137` (all three parser uses at lines 8, 99 and 107); `routes/admin.rs:222-228` (`native tools` warning), plus the relevant current tool/UI context.

Answers:
- The two protocol claims together: `insufficient`, confidence 0.51 (probabilities: accurate .04, overstated .28, insufficient .67).
- The parser terminology (all three occurrences): `insufficient`, confidence 0.89 (accurate .04, inaccurate .03, insufficient .93).
- Admin warning: `inaccurate`, confidence 0.22 (accurate .44, inaccurate .48, insufficient .08); low confidence, treated as uncertain rather than dispositive.

## Failed call between J3 and J4

An attempted `judge()` call passed the question as a positional argument instead of the required `questions` parameter and failed with `TypeError: judge() missing 1 required positional argument: 'questions'`. It yielded no evidence and is not counted.

## Successful call J4 — admin warning patch

Exact source context: `routes/admin.rs:222-228`; local dispatch evidence: `handlers/tools.rs:122-158` resolves through `state.tool_registry.get(tool_name)` and `tool.execute(arguments)`. Asked whether replacing `native tools` with `registered tools` is supported versus retaining or insufficient. Result: `supported`, confidence 0.92 (probabilities supported .95, retain .02, insufficient .03). Decision: narrow wording change to `registered tools`.

## Successful call J5 — protocol wording patch

Exact contexts: `execution.rs:234-253`, `orchestrator/tools.rs:132-141,171`, `state.rs:608-640`. The supplied excerpts advertise `shell_exec` alongside tool categories and show built-in tool registration plus D-Bus discovery, but do not establish native backends for every listed capability. Asked whether to replace native-protocol claims with registered-tool/use-not-CLI language. Result: `replace`, confidence 0.98 (probabilities replace .99, retain 0, insufficient .01). Decision: replace the unsupported protocol characterization while preserving the instruction to use registered tools instead of suggesting CLI commands.

## Successful call J6 — parser terminology after implementation context

Exact contexts: `parsing.rs:8,99-137`; implementation checks provider-supplied `tool_calls` first, then invokes text parsing fallback. Asked whether `native` accurately names the structured provider-tool-call path. Result: `retain`, confidence 0.91 (probabilities retain .94, correct .03, insufficient .03). Decision: retain all three parser uses.

## Successful call J7 — coordinator follow-up on specific tool availability

Exact current source supplied: `orchestrator/tools.rs:171` still stated access to `D-Bus (NetworkManager)` as a registered tool. Local evidence included `handlers/tools.rs:14-32` (list endpoint enumerates the dynamic registry), `:62-84` (missing names return not found), `state.rs:612-640` (built-in registration plus System/Session D-Bus discovery attempts with possible errors), and the earlier prompt category list `orchestrator/tools.rs:132-140` (generic D-Bus calls, not a guaranteed NetworkManager-specific operation). The coordinator also supplied its current op-tools review finding that NetworkManager-specific recommendations were removed.

Asked whether current wording overclaims NetworkManager availability and whether replacement `Use list_tools and get_tool_schema to identify available operations and their arguments; do not suggest CLI commands.` states only the supported registry contract. Result: `replace`, confidence 0.94 (probabilities replace .96, retain .02, insufficient .02). Final decision: replace the specific tool availability list with discovery against the current registry; retain the no-CLI suggestion instruction.

## Coordinator source-evidence reconciliation

After the coordinator's v3 receipt, directly reread the current `handlers/chat.rs` through EOF (line 925), `orchestrator/execution.rs` through its current EOF (line 324), and `orchestrator/parsing.rs` through EOF (line 185). Chat has no native occurrence; the execution native-protocol occurrence had already been corrected and its pre-edit context/JEV is preserved above; all three parser occurrences remain accurate per J6. The v3 receipt listed missing execution lines 325-339, but those lines are beyond the current file EOF; current complete read confirms there is no unread current source interval. No other native occurrence or JEV disposition changed.
