# Findings — op-cognitive

Task: `tched-context-audit-20261024/op-cognitive`; completion reconciled after coordinator follow-up.

- Seven actual `tched` substring occurrences across the crate were contextually judged and retained. No evidence-backed tched-specific source corrections were warranted. `batched` and `fetched` are ordinary prose; product-looking strings are limited to a generic UUID test fixture and E2E retrieval fixture, with no evidence of an inaccurate product claim. Three nearby `touched` strings were excluded because they do not contain contiguous `tched`.
- Initial report claimed all 35 inventory files read, but coordinator follow-up found no actual read metadata for `src/blob_vectors_tool.rs`. Supplemental sequential read of the complete 188-line source closed this gap and found zero occurrences. Initial claim is not retroactively treated as having full metadata; the coverage report notes the supplemental read.
- All 28 mandatory preload paths in `mandatory-preload.txt` were read fully in this supplemental turn. Coordinator follow-up had stated them complete; supplemental reads are recorded now, not backdated as initial reads.
- No effect on prior occurrence contexts or JEV decisions. Newly read `blob_vectors_tool.rs` contains no `tched` occurrence; no per-occurrence judgment was applicable.
- Coordinator follow-up says current `rag_pipeline.rs` and `voyage.rs` hash differences from initial inventory are the separate native worker's comment-only changes; preserved. Current `rag_pipeline.rs` EOF is line 1312 versus the inventory's initial 1316-line count.
- Source/inventory evidence does not prove live deployment, mounted catalog membership, or runtime use. No deployment or runtime state was checked or inferred.
- No source files changed. No Cargo, tests, build, formatting, smoke, runtime, deployment, seal, credential, grant, or commit actions were performed.

See `judgments.md`, `occurrences.tsv`, and `coverage.tsv` for exact contexts and evidence.
