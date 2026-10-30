# op-core audit status

- Task: `tched-context-audit-20261024/op-core`
- Owner: Luna
- Status: complete static audit
- Coverage: all 22 inventory files / 5,187 supplied lines read sequentially through EOF; see `coverage.tsv`.
- Occurrences: two, both `tched_router` fixture-ID references in the private projection directory test; contextually judged; see `occurrences.tsv` and `judgments.md`.
- Source changes: none. Current code context supports retaining the fixture ID; no availability or runtime claim is made.
- Verification: declarative JEV only. No build/test/Clippy/fmt/smoke/runtime/deployment; expressly prohibited by task scope.
- Limitations: initial hashes not recomputed; reads non-atomic; external sealed/runtime contract not verified. No operation attempted after mounted value listing EIO evidence.
