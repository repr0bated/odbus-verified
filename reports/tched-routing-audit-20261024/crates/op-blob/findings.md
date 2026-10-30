# op-blob — tched routing audit

## Outcome

All 19 inventory files read sequentially through EOF, 5,146 lines. Root, src and tests directory listings reconcile with the inventory; no nested agent instruction files encountered. Four literal `tched` substrings found, each inside `threetched`, all in one local descriptor test. No ordinary-word occurrences found. No occurrence-specific dead ends, model reroutes, FUSE reroutes, deletions or renames recommended. Source unchanged.

## Complete occurrence ledger

Paths below are relative to `crates/op-blob/`. Each occurrence received three separate typed declarative JEV questions; full source context, criteria, questions and receipts are in `judgments.md`.

| ID | Path:line | Exact containing token | Role | Integrity | Required action | Role / integrity / action confidence |
| --- | --- | --- | --- | --- | --- | --- |
| O1 | src/descriptor.rs:877 | `threetched.read` | contract_identifier | not_applicable | leave_as_is | 1 / 1 / 0.96 |
| O2 | src/descriptor.rs:878 | `obs.threetched.configure@v1` | contract_identifier | not_applicable | leave_as_is | 1 / 1 / 0.98 |
| O3 | src/descriptor.rs:886 | `threetched` | contract_identifier | not_applicable | leave_as_is | 1 / 1 / 0.98 |
| O4 | src/descriptor.rs:888 | `threetched` | contract_identifier | not_applicable | leave_as_is | 1 / 1 / 0.98 |

`cli_flag_fields_round_trip_with_exact_json_names_and_existing_numbers` constructs a local schema/descriptor pool and round-trips JSON flags. O1 is fixture capability metadata; O2 is fixture subid metadata; O3 is the fixture plugin ID; O4 is the descriptor namespace input. No plugin operation, external executable, FUSE CLI, HTTP route or model provider is invoked by this test. Its synthetic `configure` declaration is not proof of a published method. Correct external-surface spelling does not establish a deployed receiver, so integrity is not_applicable rather than alive. Repointing descriptor synthesis to op-llm or a FUSE operation would change the purpose of the fixture rather than repair routing.

## Execution context and boundaries

The crate serializes schema/manifest/descriptor bytes (`src/blob.rs:179-250`, `src/descriptor.rs:59-165`), reads/writes a blob catalog (`src/catalog.rs`) and prepares a Btrfs image (`src/btrfs.rs`). It contains no tched-router/model dispatch occurrence. Generic schema-dependent names are not evidence of a hardcoded retired destination. The single-plugin contract reader pins the selected artifact to the sealer-owned catalog and checks schema identity (`src/catalog.rs:361-403`); this is not an operation-routing client.

Assignment authority says `tched_router` is retired and external methods publish under `threetched`; older root/skill descriptions retain historical tched_router integration language. This audit uses current assignment terminology while recording only what this crate shows. Neither the retired registry entry nor live catalog/mounts was inspected outside the assigned crate. No availability or deployment claim follows from this static audit.

## Uncertainty / out-of-scope observations

No ambiguous occurrence or JEV confidence below 0.6. External method availability and runtime admission deliberately unverified, not blockers to the fixture classifications. README.md:74-78 still describes an isolated port with unavailable workspace crates, while Cargo.toml:10 and src/lib.rs:72-100 use the actual workspace schema. This generic documentation drift is not a tched occurrence or routing defect; no change made. Unrelated identity/storage/test-quality concerns are not expanded into this focused audit.

## Evidence and limits

Coverage hashes are the supplied initial SHA-256 receipts from `/var/tmp/tched-routing-audit-20261024/crates/op-blob/inventory.tsv`, not recomputed current hashes. HEAD unqueried. Reads were explicit sequential ranges (the reader sometimes adds surrounding lines). No search/extraction command or programmatic source scan. One installed `judge()` request returned all 12 answers, minimum confidence 0.96. No Cargo/build/tests/lint/format/smoke/runtime/deploy/seal/grant/credential operations. No built, tested, installed or deployed claim.

## Self-evaluation

Accuracy 4/5: exact source lines and actual JEV receipts; initial hashes not independently recomputed. Completeness 4/5: all assigned inventory lines and occurrences covered; no live receiver verification, intentionally outside scope. Clarity 4/5: per-occurrence ledger and explicit static/runtime distinction; shared rubric vocabulary is terse. Actionability 4/5: exact leave decisions and evidence; no source migration needed in this crate. Conciseness 4/5: receipt repetition retained for traceability. Overall 4.0/5. Improvement: coordinator should retain static-only qualification and reconcile supplied hashes with the parent audit snapshot if cross-agent changes matter. User-agreement self-check: this meets the requested report-only scope without treating tests as runtime calls.
