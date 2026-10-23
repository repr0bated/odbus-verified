# OD-63 · Event-triggered agent runs — requirements

Decided with the owner 2026-09-19 after evaluating EMQX Cloud "Agents"
(trigger → run → tools → run record; hosted on EMQX Cloud/GCP — rejected as
off-host). Same loop, hosted here, under the identity sled.

Constraints inherited (non-negotiable): one source of truth (SHM/Cozo), no
broker ACL tables, no MCP over MQTT, D-Bus/plugin methods are the only
control plane, identity blob on every mutation, no live-host mutation by
workers.

## R1 · Triggers are sealed plugin state
- WHEN an operator declares a trigger THEN it is stored as a durable
  runtime document of the `persona` plugin (`triggers` child), validated by
  the sealed schema, with a monotonic revision.
- A trigger is `{trigger_id, topic_filter, agent_session_id, instructions,
  allowed_methods: [plugin.method], max_payload_bytes, max_actions,
  timeout_ms, enabled}`.
- `topic_filter` is an MQTT filter (`+`/`#`) and MUST NOT match the
  `opdbus/` doorbell namespace or `$` topics.

## R2 · The broker only delivers the event
- WHEN EMQX calls the exhook's `message.publish` THEN the bridge matches the
  topic against enabled triggers and, on a match, enqueues a run with the
  topic and a payload bounded by `max_payload_bytes`; otherwise it records
  the digest-only observation exactly as today.
- The exhook handler MUST NOT block on the run (match + enqueue only) and
  MUST NOT change the publish verdict.

## R3 · Runs are bounded and observable
- The queue is bounded; a full queue drops the run with status `dropped`.
- Statuses: `queued | running | succeeded | dropped | failed | timeout`.
- Concurrency is bounded; a run exceeding `timeout_ms` ends as `timeout`.

## R4 · A run acts under an agent identity sled
- The trigger's `agent_session_id` MUST name an anchored, unexpired identity
  sled whose principal is registered and unrevoked (`verified_session`);
  otherwise the run is `dropped` before any effect. No fallback actor.
- Every action the run performs is a plugin method dispatched through
  `dispatch_method_call_with_identity` under the agent's
  `(principal, session, genesis)`, so the mutation carries the agent's
  `sealed_id_digest` and passes the agent's own capability grants.
- The publisher of the triggering message is never an identity; its payload
  is untrusted data in the prompt.

## R5 · Actions are allow-listed twice
- An action MUST appear in the trigger's `allowed_methods` AND be admitted by
  the agent's grants + method contract; either failure ends the run `failed`
  with the refusal recorded. At most `max_actions` per run.

## R6 · The run record is durable and private to the agent
- WHEN a run ends (any terminal status) THEN a `ReasoningEpisode` is
  committed in the agent's own `EpisodeScope` with `run: RunRecord{status,
  trigger_id, topic_ref (digest), queued_ms, duration_ms, tokens_in,
  tokens_out, actions: [{method, ok, ms}]}` — the same durable + private SHM
  path as chat episodes.

## R7 · The persona is assembled, not static
- The run's system prompt is composed by `op_agents::dynamic::assemble` from
  the shared piece pool for the trigger's `instructions`, with the allowed
  methods offered as the only capabilities.

## Out of scope (v1)
- Cron/time triggers (exists: `cron_dispatch`), UI authoring of triggers
  (operator seeds the document), streaming partial results.
