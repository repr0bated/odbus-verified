To: support@openai.com
From: jeremy.alan.hobson@gmail.com
Subject: Goodwill credit & weekly allowance review — First-time JEV judge setup + "Allow other apps to use credits" triggered unintended 539-call subagent loop (2026-10-19 00:49–05:49 UTC)
Attachment: openai-credit-refund-evidence-20261020.zip

---

Hi OpenAI Support Team,

I am writing from my subscription email (jeremy.alan.hobson@gmail.com) to request a goodwill review and restoration of my weekly Codex allowance and the ~20,000 of 40,000 credits that were unintentionally consumed in a single 5-hour automated window on 2026-10-19 between 00:49:55 UTC and 05:49:55 UTC.

1. WHAT HAPPENED (FIRST-TIME JEV JUDGE SETUP & APP CREDITS TOGGLE)
This was my very first time configuring and testing JEV (`typesafe/jev-latest`) as a lightweight observing judge alongside a local LiteLLM Fusion loopback runner (`run_fusion.py`).

- As shown in the attached screenshot `screenshots/00a-openai-webpage-credits-and-app-limits-before-burn.png` (captured just prior to this setup), my account had **43,463 credits remaining**, with **Hermes Portal (100%)** and **"Allow other apps to use credits after reaching your usage limit"** toggled ON in my OpenAI Usage settings.
- In `code/run_fusion.py` (lines 231–239), I explicitly passed `--model consolidation-fusion/litellm/fusion-1` and `--config fusion-omp.yml` (`modelRoles.judge: typesafe/jev-latest`), intending all agent completions to stay on my local loopback bridge and JEV to run only as a tiny judge.
- Because it was my first time configuring `modelRoles` for JEV and I only set `judge: typesafe/jev-latest` without overriding the `default` / `smol` / `scout` subagent roles, every time the harness spawned background `task` / `scout` workers (`SourceChain.jsonl`, `BackendProof.jsonl`, `HistoricalScripts.jsonl`, `NativeFeasibility.jsonl`, `ProvisionShellConsumers.jsonl`, `ProvisionOwnerPaths.jsonl`, etc.) or `task-completion-probe` checks, those child workers silently bypassed the `--model` CLI flag and routed directly to `openai-codex/gpt-6-astra` (`api: "openai-codex-responses"`, credential pin hash `80f303b3492005162d579a5ebf4a99033e1b9e996a7b94347d064d9fb39f8b54`).
- At the same time, `run_fusion.py:160` (`router = routers[int(any(m["role"] == "assistant" for m in body["messages"]))]`) caused 17 spawned `workpool("CorpusReview")` subagents to fire 32 full 5-model panels (246 requests in 10 minutes between 04:34:44Z and 04:45:12Z).
- Because "Allow other apps to use credits after reaching your usage limit" was enabled on my OpenAI account, the automated loop burned through my weekly allowance and ~20,000 of my 43,463 credits in ~5 hours before I noticed the credit drop and immediately cancelled the background job (`bg_4`).

2. AUDITED LOCAL LEDGER (2026-10-19 00:49:55.382Z – 05:49:55.382Z)
From the local session JSONL records (full 534-row + 5-probe ledger attached in `transcripts-and-audit/openai-codex-534-deduplicated-ledger.csv` and `credit-usage-audit.json`):

- Incus disk inspection & repair workers: 213 responses | 980,725 uncached input | 25,024,512 cached input | 91,745 output
- Evidence-recovery & parent workers: 163 responses | 938,041 uncached input | 19,795,456 cached input | 97,856 output
- Seed / FUSE preparation session: 73 responses | 238,750 uncached input | 10,249,088 cached input | 30,334 output
- Fusion source-research subagents (4 scouts): 72 responses | 1,221,448 uncached input | 6,192,256 cached input | 45,533 output
- Other recent session in window: 13 responses | 69,137 uncached input | 599,168 cached input | 2,030 output
- Automated task-completion probes: 5 responses | 2,396 uncached input | 722,944 cached input | 30 output
- TOTAL (5-Hour Automated Window): 539 responses | 3,450,497 uncached input | 62,583,424 cached input | 267,528 output tokens

Sample OpenAI Response / Call IDs from the attached ledger:
- `call_fabc4c10ffd0497c8055ba567741aaff|fc_0282c3dbf6527681016ad53f117710819195eb511ddd249e53` (2026-10-18T21:50:08.905Z)
- `call_3af208eb55514e0f9ee94b9a84cf2322|fc_0c47f16dc5e025a3016ad59e90ac1881929194e4ab6a3cb900` (2026-10-19T04:36:56.819Z)
- `call_8481959c03bb4e2f89598d73bc28c456|fc_093163b4fd4ae0a6016ad59e9094a081938a7254e6fa8ab8e3` (2026-10-19T04:36:57.175Z)
- `call_47f1b4351cf14a60b67d1cd928265c8c|fc_0318543d07ef9706016ad59e90531881a0b0917226d185ff9a` (2026-10-19T04:36:57.529Z)
- `call_f75031e193b844e69ca473f5544d387a|fc_0c34eb320cb694cb016ad59e919eb08190b7d9a72434c0160a` (2026-10-19T04:37:01.077Z)

3. ATTACHED EVIDENCE IN `openai-credit-refund-evidence-20261020.zip`
- `screenshots/00a-openai-webpage-credits-and-app-limits-before-burn.png` — My OpenAI Usage page showing 43,463 credits remaining, Hermes Portal (100%), and "Allow other apps to use credits after reaching your usage limit" enabled.
- `screenshots/00b-hermes-nous-linked-plan-and-topup-settings.png` — Linked ChatGPT plan & Hermes Portal billing page.
- `screenshots/01-credit-burn-summary-dashboard.png` — Visual summary of the 539 OpenAI Codex responses and 246 Fusion requests.
- `screenshots/02-subagent-model-escape-proof.png` — Side-by-side code and JSONL proof of how child subagents bypassed `--model` and fell back to `openai-codex/gpt-6-astra`.
- `screenshots/03-call-timeline-and-containment.png` — 15-minute call histogram across 00:49Z–05:49Z and the containment receipt (`credit-containment-status.json`).
- `code/` — `run_fusion.py`, `consolidation-omp.yml`, `fusion_config.yaml`, `omp_fusion.ts`, `hermes_bridge.py`, `omp_bridge.py` (all secrets redacted).
- `transcripts-and-audit/` — Full 534-row CSV/JSONL call ledger (`openai-codex-534-deduplicated-ledger.csv`), 246-request Fusion ledger (`fusion-246-requests-ledger.csv`), JSONL subagent headers, and `credit-usage-audit.json`.

I have already terminated the runaway background job and updated the runner so subagents and workpools cannot fall back to `openai-codex` or trigger recursive panels. Because this was an unintended first-time JEV configuration loop in a single 5-hour window, could you please review my account usage for 2026-10-19 00:49–05:49 UTC and grant a one-time goodwill restoration of the ~20,000 credits / weekly allowance?

Thank you very much,
Jeremy Hobson
jeremy.alan.hobson@gmail.com
