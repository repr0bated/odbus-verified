## What the evidence shows

**The incident is real; the exact amount billed to this one job is not established.** The records show unusually heavy automated activity during the five-hour window, including a looping Fusion/workpool run and native OpenAI model calls. They do **not** show that every call—or all of the reported credit loss—came from that run.

### Run and failure timeline

- **Usage window audited:** October 19, **00:49:55–05:49:55 UTC**.
- **Original provisioning run:** started **04:34:44 UTC**, ended **04:45:12 UTC** with exit code **1**. It used PID `4737`, session `01a15270-c57b-741c-9016-c6725374bd6a`, and an authenticated loopback/SSE connection. The receipt records the repo, config, context, and extension.
- **Failure:** the run’s final error artifact says `BadGatewayError: Fusion stream failed`; the handoff records an upstream HTTP 502 and no fallback. The failure is a provider/stream failure, **not evidence of a provisioning or storage failure**.
- **Runaway workload evidence:** the handoff records **17 CorpusReview workers** and **248 numbered Fusion requests**. The incident summary groups the activity as **246 Fusion requests across 32 panel rounds in about 10 minutes**. Those are reported run/request counts, not 246 successful model answers.
- **It continued beyond that first run:** the existing session was resumed at **04:48:31 UTC**. A later correction attempt hit repeated HTTP 429s; the handoff records 12 failures between 05:05:01 and 05:05:38 UTC before cancellation. A further retry started at **05:36:04 UTC**. These continuations are part of the wider incident history, not the original run’s 04:34–04:45 receipt.

The launch was recorded as:

```bash
/home/jeremy/litellm-fusion/.venv/bin/python \
  /home/jeremy/litellm-fusion/run_fusion.py --agent
```

The runner then launched OMP with the Fusion model, the task config, the external extension, and the `/srv/git/odbus-verified` repo as its working directory. The captured runner source shows OMP using `--model consolidation-fusion/litellm/fusion-1` and the task’s `fusion-omp.yml`. The run receipt and runner source are in `reports/provisioning-fusion-20261019/run/` and `reports/credit-refund-evidence-20261020/code/run_fusion.py`.

### What the local usage audit counted

The audit deduplicated **534 native OpenAI/Astra responses** across 14 files, plus **5 separately listed completion probes**—**539 total** in the window:

| Recorded activity | Responses | Uncached input tokens | Cached input tokens | Output tokens |
|---|---:|---:|---:|---:|
| Incus inspection/repair | 213 | 980,725 | 25,024,512 | 91,745 |
| Parent conversation/evidence recovery | 163 | 938,041 | 19,795,456 | 97,856 |
| Seed/FUSE preparation | 73 | 238,750 | 10,249,088 | 30,334 |
| Fusion source-research workers | 72 | 1,221,448 | 6,192,256 | 45,533 |
| Other recent session | 13 | 69,137 | 599,168 | 2,030 |
| Completion probes | 5 | 2,396 | 722,944 | 30 |
| **Total** | **539** | **3,450,497** | **62,583,424** | **267,528** |

The four Fusion source-research workers account for the 72-response Fusion row. Their transcripts show native `openai-codex/gpt-6-astra` calls. So Fusion activity **did** reach OpenAI directly in at least some worker paths; the outer Fusion calls and the native OpenAI worker calls are different parts of the workflow.

One concrete record near the original run’s failure is request 244: **04:44:53–04:45:11 UTC**, model `openai/gpt-6-astra-pro`, with **98,125 prompt tokens**, **90,809 cached**. Its stored cost field is `0.0`; that field is not an account invoice or proof of zero billing.

### What likely amplified the activity—and what remains unproven

The saved Fusion config sets `modelRoles.judge: typesafe/jev-latest` but does not define `default`, `smol`, or `scout` roles. The support packet and child-session records identify direct OpenAI Astra use in spawned research workers. That supports the conclusion that some child work bypassed the intended “all work stays on Fusion” assumption. The workpool/panel records also document substantial fan-out and repeated panel requests.

There are important limits:

- The audit is a **local session-log reconstruction**, not account-wide billing data. `omp usage clients` had no client records; the initial file enumeration was capped, though the focused follow-up covered 36 files.
- The 539 OpenAI responses span several categories, not just Fusion. The 72 Fusion-source-worker responses are the portion specifically attributed to Fusion in this audit.
- Saved packet-review outputs often lack a resolved model, so their exact provider and debited usage cannot be inferred.
- The report records the owner’s account observation—about **20,000 of 40,000 credits used in roughly five hours**, and a separate **$14 Hermes spend**—but does not independently reconcile either to provider billing. The Hermes amount and OpenAI credits are different meters; they cannot be converted or added from this evidence.
- Consequently, **“this automated setup coincided with heavy usage and had an unintended routing/fan-out path” is supported**. **“This one run caused exactly 20,000 credits of OpenAI billing” is not established**.

The main records are `reports/provisioning-fusion-20261019/credit-usage-audit.json`, `HANDOFF.md` entries for October 19, and the evidence bundle under `reports/credit-refund-evidence-20261020/`.
