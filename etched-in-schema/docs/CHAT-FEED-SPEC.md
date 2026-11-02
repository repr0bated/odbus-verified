# Chat feed loop spec (draft — platform seals it)

Defines `chat.control_plane`: the capability authorizing the chatbot's own
feed loop — each turn persists an episode, each episode is explicitly
vectorized (PII-aware), vectorized episodes are cleared. Covers
`Chat → record to identity_sled → vectorize → clear`, nothing else.

Status: proposal from the etched-in-schema side. Method schemas, capability
strings and subids below are PROPOSED; the platform seals them into a future
`ctl_plane_chatbot` blob. Nothing here grants anything until sealed.

## 1. Capability (proposed)

- `cap.identity.chatbot.feed.loop@v1` — authorizes the full loop for the
  designated root identity (control-plane-chatbot) only.
- Follows the sealed naming pattern (`cap.<domain>.<subject>.<verb>@v1`,
  cf. `cap.identity.sled.self.read@v1`).

## 2. Wiring the feed to identity_sled (uses sealed methods only)

- After every `ctl_plane_chatbot/Chat` turn, the flow calls the sealed
  `identity_sled/record_session_event` with the episode id, trigger,
  the platform calls it (verified 2026-10-28: sled state holds identities
  classifies `goal_text`, `reasoning_summary`, `decision_output` as PII;
  `catalog-routing::catalogs::projection` enforces the same classes).
- No new sealed method needed for the write path. Gap today: nothing in
  the platform calls it (verified 2026-0-28: sled state holds identities
  only, zero session events).

## 3. New method: purge_episode (proposed)

Clearing has no sealed method today — episodes would accumulate with no
observed retention. Proposed sealed method on `ctl_plane_chatbot`:

- Input: `{ episode_id }` (must already carry a completed `vectorize`
  output; unvectorized episodes refuse — vectorize-then-clear, in that
  order).
- Effect: removes the episode payload from the plugin sled; retains the
  episode id + content hash + vector reference in the index.
- Capability: `cap.identity.chatbot.feed.purge@v1`
  (subid `mut.service.ctl-plane-chatbot.episode.purge@v1`).
- Refusals: unknown id → `episode-not-found`; unvectorized →
  `episode-not-vectorized`; both first-class, like all refusals.

## 4. Loop invariant (acceptance)

1. A `Chat` turn with no following `record_session_event` is incomplete —
   visible as an episode id absent from identity_sled history.
2. A `vectorize` call on a PII-flagged episode emits no PII text
   (platform test `vectorization_plan_uses_typed_episode_and_redacts_pii_text`
   proves both directions; the projection crate mirrors it).
3. A `purge_episode` on an unvectorized episode refuses.
4. `chat.control_plane` authorizes exactly this loop; any use outside it
   (e.g. direct state writes) is out of scope and refuses.

## 5. Relation to catalog-routing

- `projection::project(episode, Vectorize)` is the pre-check mirror of
  `plan_vectorization`: same classes, same gate, pure and testable without
  the platform. The sealed code stays authoritative.
- Until the cap and method seal, `chat.control_plane` MUST NOT appear in
  the compliance table (fail-closed on unsealed — crate rule). Add it only
  with the blob hash recorded.
