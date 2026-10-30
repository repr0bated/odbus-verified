# OD-63 · tasks

- [x] T1 persona: `AgentTrigger`/`TriggerDocument` types, validation, `topic_matches`, methods `list_triggers`/`set_trigger`/`delete_trigger` over runtime documents, schema + inventory tests
- [x] T2 chatbot: `RunRecord` on `ReasoningEpisode` (schema v6), validation bounds
- [x] T3 bridge `agent_runs`: bounded queue, worker pool, statuses, timeout, `execute` (sled check → persona → completion → actions → episode)
- [x] T4 exhook `on_message_publish`: match against enabled triggers, bounded payload, enqueue
- [x] T5 tests: matcher, queue drop/timeout, refused-sled drop, allowlist refusal, episode with run record
- [x] T6 docs: identity-boundaries §3.2b, WISHLIST OD-63, SIGNALS

Landed 2026-09-19. Not in v1: UI authoring of triggers, streaming partial results, run-stats read method (stats are logged at debug on enqueue).
