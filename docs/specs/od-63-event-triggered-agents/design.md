# OD-63 · design

```
EMQX ──exhook message.publish (loopback mTLS gRPC)──▶ emqx_hook_provider
         │ record digest observation (unchanged)
         └ agent_triggers::match(topic) ──enqueue──▶ agent_runs (bounded mpsc)
                                                        │ worker pool (M)
                                                        ▼
                                             agent_runs::execute(run)
                                             1. verified_session(agent sled)   → else dropped
                                             2. assemble persona (pieces)      R7
                                             3. router completion (guarded)    prompt = persona + instructions + event(untrusted)
                                             4. parse actions (JSON), ≤ max_actions
                                             5. each action: allowlist ∧ dispatch_method_call_with_identity(agent)
                                             6. commit episode with RunRecord  R6 (agent EpisodeScope)
```

## Pieces
| Piece | Where | Notes |
|---|---|---|
| `AgentTrigger`, `TriggerDocument{revision, triggers}` | `op-plugins::persona` | schema types; `list_triggers`/`set_trigger`/`delete_trigger` methods over `RuntimeDocuments("persona")`; validation incl. filter grammar and namespace exclusion |
| `topic_matches(filter, topic)` | `op-plugins::persona` | MQTT wildcard match, pure, tested |
| `RunRecord`, `ReasoningEpisode.run: Option<RunRecord>` | `op-plugins::ctl_plane_chatbot` | `#[serde(default)]`, hashed with the episode; schema v6 |
| `agent_runs.rs` | `op-grpc-bridge` | `RunQueue::spawn(engine, cap, concurrency)`; `enqueue(TriggerEvent)`; `execute`; statuses; per-run timeout |
| exhook `on_message_publish` | `op-grpc-bridge` | reads payload only on match; enqueue; still `Ignore` |
| run identity | `op-grpc-bridge` | `project_dispatch::verified_session`; `session_context_for_actor` stamps `sealed_id_digest` |
| actions | `op-grpc-bridge` | model answers `{"summary": str, "actions": [{"plugin","method","args"}]}`; each `plugin.method` ∈ allowed; dispatched with the method's declared capability from the sealed schema |

## Trust
- Trigger table: sealed schema + durable revision — the only policy, and it is
  plugin state, not broker state.
- Broker: delivers bytes; its clients' identities are never consulted.
- Agent: an identity sled with `principal_kind = "agent-principal"`; grants
  materialized like any principal's. Revocation = tombstone (existing).
- Model output: untrusted; only the allow-listed, contract-admitted actions
  execute.
