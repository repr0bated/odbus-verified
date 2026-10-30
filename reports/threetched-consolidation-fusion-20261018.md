# LiteLLM Fusion submission — threetched consolidation

Recorded 2026-10-18, task `threetched-consolidation-fusion-20261018`. Owner repeated the request: use the top implementation prompt unchanged and the grounded brief below it as evidence. The earlier attempt incorrectly substituted a critical-review task; that substitution is withdrawn.

## Tool-enabled streaming rerun

Owner subsequently requested repository/tools, JEV skill, relevant skills, Salad troubleshooting, and streaming into OMP. The run-only integration in `/home/jeremy/litellm-fusion/` now connects Fusion to OMP's tool loop with cwd `/srv/git/odbus-verified`, sibling UI `/srv/git/3tched.com`, and both mounted namespaces. No global model settings changed. Mandatory skill contents are preloaded; selected conditional skills and `jev-router` are available through the run overlay.

Native asynchronous upstream SSE carries text and tool-call deltas through LiteLLM to OMP. OMP's text deltas and tool progress are forwarded to the runner's output while full events and timestamped model chunks are saved under `threetched-consolidation-fusion-20261018-stream-run/`. The private Fusion panel/analyst phase is still deliberation; no simulated token stream is claimed. The first request requires a panel, later tool steps use supported automatic invocation.

**Verified first streamed execution:** request started21:24:55Z, first chunk21:26:36.530846Z, completion21:26:45.811104Z, **22 native stream chunks**, final model `openai/gpt-6-astra-pro`, `finish_reason=tool_calls`. Fusion metadata: invoked=true, protocol=fusion-tool-v1, **5 accepted panel responses / 0 failures**, analysis_available=true. The final model received twelve public execution tools and issued ten real skill reads; OMP's `tool_execution_end` receipts show all ten completed with `isError=false`. Evidence: [completion metadata](threetched-consolidation-fusion-20261018-stream-run/completion-1.json), [timestamped chunks](threetched-consolidation-fusion-20261018-stream-run/stream-1.jsonl), [OMP execution events](threetched-consolidation-fusion-20261018-stream-run/agent.jsonl).

The streaming run ended at21:44:51Z, exit0, with a source slice implemented and release preparation blocked by another compiler process. Its final report records preservation of18 wrapper removals, retirement of five router namespace facades, mount/symlink/doc reconciliation and pre-spawn refusal of unsupported editor-dependent calls. No consolidation release build, isolated seal or deployment completed.

**Owner clarification: JEV is a judge, not a model router.** The parent's interpretation of “I expected that” as a routing request was wrong. The briefly launched routing continuation was cancelled and its child confirmed exited; all added router hooks/overlay/credential wiring were removed. The owner reports the trial router was broken and always assigned Haiku; it was not retested for that claim. Original native judging remains. Judge-only continuation uses the preserved execution session and records output under `threetched-consolidation-fusion-20261018-judge-resume/`.

The preliminary buffered invocation under `threetched-consolidation-fusion-20261018-tools-run/` was deliberately interrupted, exit 130, before being replaced with full mandatory preloads and native streaming. It is not a completed implementation run.

See [skills and Salad evidence](threetched-consolidation-fusion-20261018-skills-salad.md). A corrected real Salad call returned HTTP 200/stop; the prior error was duplicate leading system messages. A native JEV judgment at 21:25:12Z selected implementation/build/isolated-catalog scope and native/sealed/SHM authority. Its actual session usage receipt identifies `provider=typesafe`, `model=jev-latest`, `purpose=judge`; [judgment answers](threetched-consolidation-fusion-20261018-judgment.json) are supporting classification, not source or deployment proof.

## Prior completed advisory result

**Fusion ran and returned a final response.** The request completed at 20:40:09 UTC in 379.715 seconds, exit 0, `finish_reason=stop`. Metadata confirms `invoked=true`, protocol `fusion-tool-v1`, **3 accepted panel responses / 2 failures**, and `analysis_available=true`. Final model: `openai/gpt-6-astra-pro`.

The [unaltered final response](threetched-consolidation-fusion-20261018-run/response.md) says implementation is blocked in this completion-only invocation because repository, mounted-filesystem, shell, JEV and deployment tools are not attached. This is a successful Fusion submission, **not completed consolidation**. It reports no source edits, build, sealing, deployment or tests.

| Panel alias | Recorded outcome |
|---|---|
| `astra-pro` | Accepted response |
| `opus-5-5` | Accepted response |
| `glm-5-3` | Accepted response |
| `kimi-k3` | `TimeoutError` |
| `qwen-3-6-35b-a3b` | `BadRequestError` |

The original failure metadata alone did not establish Qwen's cause. Later inspection of its execution log identified `System message must be at the beginning`; the corrected-call receipt above supersedes that uncertainty. Provider callbacks were not substituted for accepted panel results.

## Artifacts and provenance

- [Completion receipt](threetched-consolidation-fusion-20261018-run/receipt.json): timestamps, runtime, source/input hashes, final model, finish reason and Fusion counts.
- [Exact prompt](threetched-consolidation-fusion-20261018-run/prompt.txt), [evidence](threetched-consolidation-fusion-20261018-run/evidence.txt), [assembled request](threetched-consolidation-fusion-20261018-run/request.txt), and [repository rules/preloads](threetched-consolidation-fusion-20261018-run/system.txt).
- [Panel results and analyst comparison](threetched-consolidation-fusion-20261018-run/panel.json), [native response](threetched-consolidation-fusion-20261018-run/response.json), and [execution output](threetched-consolidation-fusion-20261018-run/execution.log). Panel material is advisory, not verified implementation evidence; candidate text uses Fusion's configured 12,000-character bound.
- Runtime: `/home/jeremy/litellm-fusion/.venv/bin/python`, LiteLLM 1.103.0 pinned to `acc788352a65fce6649d773a7dec3ed0f793af17` on the Fusion branch. No installation or configuration repair was needed.
- Source checkout HEAD: `051a20d056afba074e21d797aa34f8947c543c5a`; unrelated shared work preserved.
- Source handoff SHA-256: `282b6befe8b7bfa312407f5563bedbcb9b6c968248d3c0c86fd39bfa5916a935`.
- Final response SHA-256: `71f9cf05152a4017e5fe7fbfe444e2ee75194de6325fd5894a02ee2028960059`.

After completion, exact reconstruction `prompt + "\n---\n" + evidence` matched the unchanged source handoff; its hash matched the submission receipt. No review-task suffix was added. The [official Fusion documentation](https://docs.litellm.ai/docs/fusion) confirms that released LiteLLM does not contain this feature, explains the panel/judge pipeline, and distinguishes public tool availability from private advisory panel calls.

## Historical failed attempts — superseded runtime diagnosis

1. Ran repository launcher `./scripts/or-fusion` with the handoff file and review instructions, `--mode quality`, `--max-tokens 8192`, and `OR_FUSION_NO_EXPORT=1`. It stopped before HTTP submission with exit 1: `FUSION_API_KEY is not set (or OPENROUTER_API_KEY)`. This OpenRouter launcher is distinct from the requested local LiteLLM Fusion app.
2. Ran `/home/jeremy/litellm-fusion/run_fusion.py` through the installed LiteLLM uv-tool interpreter (`/home/jeremy/.local/share/uv/tools/litellm/bin/python`), substituting the handoff prompt/evidence for its hard-coded sample question in memory only. It stopped during `Router(model_list=...)`, before completion dispatch, with `litellm.BadRequestError`: `LLM Provider NOT provided. You passed model=fusion_router`. The configured model alias is `litellm/fusion-1`; the app's `fusion_config.yaml` maps it to `model: fusion_router`, but the installed LiteLLM package contains no `fusion_router` implementation. The local app source imports `hermes_bridge` but does not register a Fusion router implementation.

The installed LiteLLM tool environment was present; system Python and the Hermes Python environment did not have `litellm`. No `or-fusion` API key was present in the command environment. The local runner read its configured secret file internally before Router construction; no secret was printed or copied into this report. No model request, external call, source/config edit, or NotebookLM export occurred.

## Submission scope and remaining implementation prerequisite

The original handoff is unchanged: text before its first horizontal rule is the prompt; text after it is the evidence. Both were forwarded in full to every panel member and the analyst through request-local wrappers; no package or runner source was edited. The existing configured five-model panel, outer model and analyst remained unchanged. Repository rules and all nine mandatory skill files were supplied separately.

The local Fusion runner is a completion/panel interface, with no repository execution tools attached. Its response cannot itself establish source edits, builds, catalog preparation or deployment; those statuses require actual tool receipts. No alternate OpenRouter service or credential is needed for this request.

Further application implementation requires a tool-enabled execution session, including a client tool-execution loop for the public Fusion model if Fusion itself is to perform the changes. This run did not add that integration or treat model-generated recommendations as an applied patch. Earlier blocked-runtime notes in HANDOFF and SIGNALS are superseded by this completed submission.
