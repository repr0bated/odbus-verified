# Owner signals — agent orchestration port

2026-10-04 · Codex · Work E. All statuses **open**. Source review and suggestions, not deployed claims. No personal or credential data. Filename follows the task's requested `SIGNALLS.md` spelling; the canonical task board remains WISHLIST.md.

| Type | Signal | Evidence / proposed next action | Status |
|---|---|---|---|
| ⚠️ CONCERN | Removing the initial scan and fallback makes missed MQTT publications load-bearing: turns opened before subscription or during disconnect can remain pending indefinitely. | `op-chat-responder/src/main.rs::run_mqtt`; prove event-driven retained/durable delivery and producer-side acknowledgment in a fixture. Idle subscription cannot identify missing requests. | open |
| 💡 SUGGESTION | Track admitted turn → wake publication → receiver acknowledgment by a synthetic request ID, with explicit producer-side failure evidence. | MQTT-only relay now exits on transport failure/subscription rejection. Neither a healthy idle socket nor QoS alone proves chatbot receipt; design through the existing sealed owner. | open |
| ⚠️ CONCERN | A journal start receipt does not establish queued execution, terminal durability or an episode. | `workstack_runner.rs::enqueue` can refuse; terminal append runs in an unawaited task and episode commit can warn. Inject these failures before claiming full-port completion. | open |
| 💡 SUGGESTION | Accept full WorkstackService activation only after identity per phase, cancellation ownership and definition authority are verified together. | Shared HANDOFF records activation in flight. Reflection/registration and component tests alone are insufficient; use the TESTING.md scoped-negative and runtime gates. | open |
| ⚠️ CONCERN | The owner-wide event-only goal still has known timer/startup candidates outside this removal. | SSE's 15-second keepalive and MQTT protocol keepalive remain; responder run script still retries dependency/mount readiness each second. SSE route is retained pending owner approval, runit changes here are polling-arg removal only. | open |
| 💡 SUGGESTION | Make unsupported schedules explicitly refuse until the sealed scheduling owner and event source exist. | Port scheduling is future work. Test due/cancel/restart/duplicate events with receipts; avoid reintroducing an interval scanner to close the gap. | open |
