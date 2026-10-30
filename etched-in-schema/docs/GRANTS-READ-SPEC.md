# Grants read spec (draft — platform seals it)

The console needs live data: effective grants per principal with provenance
(cap → role → position → chart event). The UI reads only through sealed
methods or committed state; today effective grants live in a root-only JSON
file with no read method in front. This spec proposes the read path. Nothing
here grants anything; reads are `obs.*` class.

## Proposed method: get_effective_grants

- Owner plugin: the capability-grants keeper (platform to place — today the
  file is written outside any visible plugin; placement is part of this spec).
- Input: `{ principal }` (key or alias).
- Output: `{ principal, groups[], effective: [{ capability, via_role,
  via_position, chart_event }] }` — provenance included so the console can
  render "why do I hold this cap?" with no second call.
- Capability: `cap.identity.grants.read@v1`
  (subid `obs.service.identity.grants.get@v1`).
- Refusals: unknown principal → `principal-not-found`. No other failure
  modes; reads never mutate.

## Console wiring (after seal)

1. `specgen::console_spec` rows come from `get_effective_grants` instead of
   the trim file — same table, live data, same validation gate.
2. `org_bridge::grants_for` replays any row selection through `decide()` —
   the "simulate trim" button calls the sealed replay, keeps its receipt.
3. Provenance column renders from `via_role`/`via_position` + chart event;
   no separate lookup path.

## Acceptance

1. Console renders from a live call: same spec shape as
   `local/grants-console.spec.json`, rows from the method, validation clean.
2. Simulated removal replays `decide()` and shows the changed verdicts with
   the replay receipt id.
3. The method exposes no write path; a write attempt through it refuses.
4. Until sealed, the console stays on file-backed rows (current state) —
   fail-closed, no live reads through unsealed paths.
