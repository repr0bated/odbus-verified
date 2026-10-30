# JEV judgment receipts

## J1 — Fresh declarative contextual assessment

**Evidence supplied:** Exact four `threetched` contexts at `snowball.rs:2510,2513,3107,3132`; the fixture's Chat action, sample payload and metadata; assertions parse and flatten the record and compare rendered text; crate design ownership separating Snowball's payload transport from plugin identity/catalog; owner naming guidance; coordinator's distinction between retired `tched_router` source ID, `threetched` schema ID and mounted catalog names.

**Questions/results:**
- Meaning: choice A, a fixture/test identifier checked for parsing/rendering and not proof of runtime — confidence 0.99.
- Intent accuracy: retain literal as test data without claiming availability — confidence 0.99.
- Source change needed: `no_change` most probable, 0.68 probability / 0.57 reported confidence; alternative neutralize 0.22, insufficient 0.09. Human disposition retained provisionally pending stronger accuracy evidence; no specific rename supported.

## J2 — Completion and human disposition

**Evidence supplied:** All 13 inventory entries read sequentially through EOF; all collapsed outputs were resolved through exact raw reads; four occurrences individually-positioned in `occurrences.tsv`; no source change; assignment forbids runtime/build/test/deployment verification.

- Completion: `static_complete`, confidence 0.80 (probability 0.87); runtime-verified explicitly excluded.
- Occurrence disposition: retain exact fixture literals without runtime claims, confidence 0.97.

JEV is semantic evidence only; it does not prove runtime, catalog freshness, or deployment. Calls used `judge(state, questions)` through eval per installed instructions. The first J1 invocation attempt failed due to a missing positional argument; the successful call is J1.
