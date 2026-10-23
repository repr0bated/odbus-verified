# Nous / Hermes Support Request — Unintended Recursive Panel Loop ($14 Spend)

**Subject:** First-time JEV + LiteLLM Fusion setup triggered unintended 246-request / 32-panel recursive workpool loop (2026-10-19 04:34–04:45 UTC)

---

Hi Nous / Hermes Support Team,

I am writing regarding a **$14 usage spike on 2026-10-19 (primarily between `04:34:44 UTC` and `04:45:12 UTC`)** caused by a first-time configuration bug when testing JEV (`typesafe/jev-latest`) with a local LiteLLM `fusion_router` script (`run_fusion.py`).

### What Happened
1. In `run_fusion.py:160` (`code/run_fusion.py`), my script selected `routers[0]` (`invocation: required`, which fires all 5 panel models + 1 analyst model + 1 outer model = **7 completions per request**) whenever a chat request contained no prior `assistant` message:
   ```python
   router = routers[int(any(m["role"] == "assistant" for m in body["messages"]))]
   ```
2. During the run (`reports/provisioning-fusion-20261019/run/receipt.json`), the agent spawned **17 concurrent `workpool("CorpusReview")` subagents** (`CorpusReview-1.jsonl` .. `CorpusReview-17.jsonl`).
3. Because each subagent started with a fresh message array (no `assistant` message), `run_fusion.py:160` treated **every single subagent start** as a brand-new root request and fired a mandatory 5-model + analyst Fusion panel (`panel-14.json` .. `panel-33.json`), plus 12 additional auto-invoked panels (`panel-97.json` .. `panel-210.json`) — resulting in **246 `/v1/chat/completions` requests and 32 full multi-model panel rounds in just 10 minutes** before hitting upstream HTTP 502/429 and being terminated.

### Attached Evidence
- `transcripts-and-audit/fusion-246-requests-ledger.csv`: Every one of the 224 recorded completion/panel/error artifacts (`completion-1.json` through `error-246.json`) with exact UTC timestamps and response IDs (`chatcmpl-095b5a66706c5a22016ad59e4a0dbc87d0b51d50e43a9bc5da` .. `chatcmpl-0dd32da5fbd9ffdf016ad5a0543a6087d09656aa5d2363839b`).
- `transcripts-and-audit/CorpusReview-1-workpool-panel-explosion-excerpt.jsonl`: Transcript header proving `CorpusReview-1..17` inherited `consolidation-fusion/litellm/fusion-1`.
- `code/run_fusion.py` & `code/fusion_config.yaml`: The exact script and router config.
- `screenshots/`: Visual breakdown of the 10-minute 246-request loop and immediate cancellation.

I have since fixed the script so `invocation: required` can only fire once on `sequence == 1` and subagent workpools are blocked. If possible, I would appreciate a credit adjustment for the unintended $14 runaway loop.

Thank you!
