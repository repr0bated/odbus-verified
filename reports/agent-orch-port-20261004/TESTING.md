# Agent orchestration port — verification checklist

2026-10-04 · Codex · Work E · source-only, uncommitted. Authority: [AGENTS.md](../../AGENTS.md), [shared handoff](../../HANDOFF.md), and the current source, not historical deployment claims. Work E owns responder polling removal and documentation; other workers own door adapters, runner tests, WorkstackService registration and executor identity/episodes. This checklist is a plan unless an evidence row explicitly says passed.

## Boundaries and evidence

- [ ] Record current HEAD, branch, owned dirty files, fixture identifiers, exact command, exit status and sanitized output for every gate. The tree is shared and changes while workers run.
- [ ] Use disposable identities, definitions, journals and episode stores through the declared identity-bound test/maintenance surface. Start from that identity's mounted 3tchedFS namespace, discover operations and inspect receipts plus resulting authoritative state. A local write or accepted start receipt alone does not prove execution.
- [ ] Keep workflows (sealed declarative definitions and receipts) distinct from executable workstacks. Do not infer WorkstackService activation from its source or generated proto alone.
- [ ] No commits/pushes, golden preparation, sealing, snapshot activation or service commands in this verification lane. Runtime checks below require an isolated fixture or separately authorized operator session.
- [ ] Coordinator runs final `/verify` after all workers land. Work E's focused gates do not substitute for whole-workspace formatting, Clippy or tests.

## Door adapters and admission

- [ ] Exercise every changed REST adapter with a valid fixture credential and exact sealed grant. Compare the door method/arguments, unchanged identity metadata, receipt and scoped authoritative result. Confirm op-web performs no local execution or identity minting.
- [ ] Missing credential, malformed/expired credential, wrong principal/session, and valid identity without the method's capability must fail closed. Record status and denial; prove no queued execution, journal start or episode from rejected requests.
- [ ] Supply another principal's execution/conversation ID to status, cancel and update surfaces. Confirm scoped refusal without revealing private data or broadening grants.
- [ ] Malformed step binding/JSON, unknown agent/method, nonexistent definition and sealed schema mismatch must produce explicit errors, with no fallback transport or fabricated success.
- [ ] Preserve downstream refusal/status semantics rather than reporting admission as execution success. Test door unavailable, deadline, client disconnect and duplicate submission with the actual method's declared idempotency rules.

## Reactive runner and full WorkstackService (activation in flight)

- [ ] An admitted `workflows.start_workflow` creates one execution ID/start receipt and directly enqueues work. Observe execution without any subsequent status call, journal tail or broker scan. Idle fixture causes zero recurring state reads/calls.
- [ ] Exercise missing verified session, changed/revoked scope between admission and execution, queue full and runner unavailable. An accepted receipt whose enqueue fails must have observable failure evidence; do not call it a completed run.
- [ ] Verify ordered step results, per-step deadlines, tool refusal and malformed output. Confirm subsequent steps stop where required and unrelated queued work remains intact.
- [ ] Test concurrent submissions, duplicate wake delivery, cancellation during a step, rollback and shutdown with queued work. Record execution ownership and each terminal outcome; unresolved pause/resume/cancel semantics remain gaps.
- [ ] Prove bridge registration of the full WorkstackService with an actual isolated call, not reflection alone. Exercise the former `spawn_local` path on its supported executor and show no runtime panic.
- [ ] Verify full executor identity per phase and delegated tool call; no `default` or borrowed principal may replace the admitted scope. List/GetStatus must use the declared definition and execution authority, not a second registry.
- [ ] Crash/restart fixture with accepted-but-unexecuted work. Document loss/replay behavior and duplicate-side-effect protection; an in-memory mpsc queue is not durable recovery. Do not add timer-based journal draining.

## MQTT-only responder

- [ ] Post-removal CLI accepts the existing principal/lungs/MQTT/D-Bus arguments; `--poll-interval` and `--polling-mode` fail as unknown arguments. Legacy polling environment variables no longer configure the binary or run script.
- [ ] Valid topic publication forwards the request once through the existing chatbot notification path. Wrong topic/malformed payload must not dispatch; exercise owner/session validation at the receiving admission boundary.
- [ ] MQTT connection failure and rejected SubAck produce explicit relay failure. Check notification failure logs and existing method-error handling; no periodic scan or sleep-based retry may recover the turn.
- [ ] Suppress a publication while recording a producer-side admitted turn. Expect no notification and no startup/backlog scan. Report missing wake as delivery failure using correlated producer/receiver evidence. An idle subscriber has no expected request ID and cannot distinguish lost wake from no work; this removal does not implement that detector.
- [ ] Test disconnected/startup-gap publications and QoS duplicates in a disposable broker. Current clean-session setup does not establish durable backlog replay; broker retention/acknowledgment must be proven before claiming eventual delivery.
- [ ] Read `deploy/runit/op-chat-responder/run` and run `sh -n` only. Its bounded dependency/mount waits still use `sleep 1`; these are known startup polling candidates, retained under Work E's argument-only runit scope. Do not execute it against services.

## No-poll source assertions

Run from repository root and retain outputs. `rg` exit 1 means no matches, not an execution error; exit 2 means an invalid assertion. Inspect matches in context rather than treating keywords as proof.

```sh
rg -n 'poll_interval|polling_mode|list_pending_turns|tokio::time::(interval|sleep)|std::fs|Instant::now' crates/op-chat-responder/src
rg -n 'POLL_INTERVAL|POLLING_MODE|--poll-interval|--polling-mode' deploy/runit/op-chat-responder/run
rg -n 'tokio::time::interval|sleep|Instant::now|std::fs|poll_interval|polling_mode' crates/op-grpc-bridge/src/workstack_runner.rs crates/op-web/src/handlers/agents.rs crates/op-web/src/sse.rs
rg -n '\.poll\(|\.interval\(|sleep|while|loop' crates/op-chat-responder/src/main.rs crates/op-web/src/sse.rs deploy/runit/op-chat-responder/run
```

Expected responder operational scan: no fallback identifiers, filesystem reads, interval or sleep. Retired CLI spelling appears only in its rejection test. `eventloop.poll().await` drives rumqttc's asynchronous network events, not a pending-state scan. MQTT's 30-second protocol keepalive remains timer-driven. Runner `Instant::now` is duration measurement and journal file access is append-only; inspect for read/sleep loops. SSE waits on broadcast events and refreshes only initially/on resync/lag; its 15-second HTTP keepalive is timer-driven legacy compatibility, explicitly deprecated. There is no claim that all timers or startup polling across the system were eliminated.

## Observability and schedules to come

- [ ] Correlate admitted execution ID, workflow ID, starter scope, step actions, terminal journal record and episode. Verify journal append is durable and failures are visible; the current runner spawns a terminal append task without awaiting its result, so a terminal log alone does not prove durability.
- [ ] Inspect isolated journal contents once after a bounded execution signal. `status: terminal` alone does not distinguish success/failure: compare reason and the episode's run/action outcomes.
- [ ] Verify episode commit point after authoritative result/state commit, correct principal/session/genesis, refusal and cancellation records, and no unrelated child/parent overwrite. Inject episode-store/journal failures independently and preserve truthful outcomes.
- [ ] Do not copy identity credentials, private previews, payloads or account data into reports. Use synthetic fixture identifiers; existing responder preview logging deserves separate owner review.
- [ ] Scheduling remains future work until a declared sealed owner and event-driven activation exist. Before acceptance test one-shot due delivery, scope revalidation, cancellation, missed trigger, duplicate trigger and restart replay. Require an observable refusal for unsupported schedule operations; no interval scanner or second control plane.

## Work E execution evidence

Focused command results are recorded here after verification; runtime checklist items remain unexecuted unless explicitly updated. Coordinator owns final full-port `/verify`.

Work E source assertions (2026-10-04):

| Exact command | Result / output |
|---|---|
| `rg -n 'poll_interval\|polling_mode\|list_pending_turns\|tokio::time::(interval\|sleep)\|std::fs\|Instant::now' crates/op-chat-responder/src` | exit 1, no output (expected no matches) |
| `rg -n 'POLL_INTERVAL\|POLLING_MODE\|--poll-interval\|--polling-mode' deploy/runit/op-chat-responder/run` | exit 1, no output (expected no matches) |
| `rustfmt --edition 2024 --check crates/op-chat-responder/src/main.rs` | exit 0, no output |
| `sh -n deploy/runit/op-chat-responder/run` | exit 0, no output; script not executed |
| `git diff --check -- crates/op-chat-responder/src/main.rs crates/op-web/src/sse.rs deploy/runit/op-chat-responder/run` | exit 0, no output |

The broad loop/timer review found responder `eventloop.poll().await`, SSE broadcast receive loop/15-second keepalive, and runit `sleep 1` at lines 14 and 50 with mount wait at line 44. These are classified above; none is presented as evidence of a removed route or changed startup behavior.

Focused Cargo verification, execution worker `gpt-6-luna`, native/glibc default target, repository root (both exit 0, no warnings):

```text
$ env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo check -p op-chat-responder
    Blocking waiting for file lock on build directory
    Checking op-chat-responder v1.0.0 (/srv/git/odbus-verified/crates/op-chat-responder)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 30s
```

Test command: `env CARGO_CALLER_FAMILY=gpt CARGO_DISPATCHED=1 cargo test -p op-chat-responder`. Finished test build in 5m25s including shared lock wait; **2 passed, 0 failed, 0 ignored** (`service_arguments_use_mqtt_only`, `retired_polling_arguments_are_rejected`). Exact local logs: `/tmp/work-e-responder-check.log`, `/tmp/work-e-responder-test.log`. These temporary logs are not durable release receipts. The recorded commands/results above are the repository evidence. MQTT integration, end-to-end port checks and coordinator's final `/verify` were not run by Work E.
