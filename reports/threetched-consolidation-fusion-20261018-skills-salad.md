# Fusion rerun: skills and Salad repair

Task `threetched-consolidation-fusion-20261018`, 2026-10-18. This records runner preparation, not application implementation or deployment.

## Selected skills

Always preload all nine mandated files under `.agents/skills/`: `grpc-expert`, `grpc-protocol-expert`, `json-render`, `ovs-db-analysis`, `kernel-fuse-diagnosis`, `fuse`, `btrfs`, `rust-memory-optimization`, `memory-safety-patterns`.

Implementation additions: `rust` for native contracts/dispatch/async state, `application-security` for exact capabilities/private state/identity admission, and explicitly requested `jev-router` at `/home/jeremy/.omp/local-router/mixed-trial/skills/jev-router/SKILL.md`.

Runner diagnosis: official `salad`, `salad-ai-gateway-troubleshoot`, `salad-ai-gateway-request` from that same trial skill root, plus `systematic-debugging`.

Available on demand: `svelte` for json-render Svelte catalogs, `svelte-code-writer` for Svelte source, `sveltekit` for routing/load/adapters, `vite` for affected asset release inputs, `typescript-patterns` for typed callers, `openapi` only when importing a real native gateway export. No blanket React, UI design or testing skill preload. The run overlay also retains relevant harness/delegation and evidence-verification skills.

Repo/owner instructions override stale generic React/JWT/SQL examples, automatic tests, formatters, security scans or Svelte autofixers. UI remains Svelte/SvelteKit/Vite/json-render; Cozo remains database authority; tests/Clippy/format are on-request. Mandatory preloads are explicit instructions, not merely discovered metadata.

## JEV integration

`consolidation-omp.yml` is a run-only overlay, not a global settings change. It adds the installed JEV skill directory and sets native `modelRoles.judge: typesafe/jev-latest`. Fusion remains the primary execution model. JEV is a structured judge, not a fictitious sixth text-generating panel member. The execution prompt requests an actual JEV task/evidence classification receipt. No Cargo model mapping or separate model dispatcher was added. Merely loading the skill/configuration does not establish that a judgment ran; inspect the execution session receipt.

## Salad corrected-call receipt

The prior log error was `System message must be at the beginning`. The pinned Fusion helper emits `[system(panel guardrails), user(query)]`; the original wrapper prepended a second system message. Corrected sequence `[system, user]` merges the two leading system contents in their original order using `\n\n`, preserving all guardrails and the user bytes.

A worker made exactly one corrected completion with automatic SDK retries disabled:

- UTC 2026-10-18T21:19:28.648419Z–21:20:17.125117Z; 48.473 seconds including model discovery.
- Exact endpoint `https://ai.salad.cloud/v1`; model `qwen3.6-35b-a3b`.
- Public model catalog HTTP 200 and authenticated completion HTTP 200. Catalog success alone was not treated as credential evidence.
- Response `chatcmpl-811a8b5f451567c0`, `finish_reason=stop`, 2,629 characters, no tool calls.
- 39,428 input + 1,384 output tokens. No subscription/quota/actual-charge claim.
- Complete original user request retained: SHA256 `263c1f218174232e185fbf39ab11c3cab4226cc11ee8ee3070f58a9084c1e5ba`.
- Merged system SHA256 `941756a2ae7216f1fca22bfa756c667f21d558bcbb393c7f5fff5aa53e46e215`; response content SHA256 `460e95df9dd415d1393deab8b32ff1c35146e679d2704b5f5bd359f56e31ecb0`.
- Process exit 0, empty stderr. Key resolved only in process and never printed or persisted. The existing trial launcher documents this user's legacy `SALAD_API_KEY` as the gateway key; it was sent as Bearer authentication, not `Salad-Api-Key`.

The tool-enabled runner now prepends current system guidance into the existing panel system message rather than adding a second one. Panel query context is separately labeled and does not replace the original prompt/evidence. The isolated Salad success proves acceptance of the corrected message shape, not full Fusion completion, application implementation or deployment.

Official references: [Salad request](https://docs.salad.com/agents/ai-gateway/select-model-and-send-request), [troubleshooting](https://docs.salad.com/agents/ai-gateway/troubleshoot-request), [retry safety](https://docs.salad.com/agents/reference/safety-retries-and-freshness), [LiteLLM Fusion](https://docs.litellm.ai/docs/fusion).
