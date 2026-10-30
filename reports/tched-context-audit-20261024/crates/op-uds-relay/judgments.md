# Fresh declarative JEV judgments

Tool: `eval` prelude `judge(state, questions)` per `xd://eval/judge` instructions. One state and three independent typed-choice questions. The state supplied the exact inventory paths and counts, read coverage, zero-occurrence result, task constraints, project naming context, and static-vs-runtime scope. This is fresh evidence; no prior audit coverage or judgments were reused.

## Questions and exact returned answers

**coverage** — “Given the complete listed-file read receipts and reported zero case-insensitive 'tched' occurrences, which static audit disposition best fits? Consider that this conclusion is limited to reviewed files and not runtime/deployment.” Criteria: `complete_zero_occurrences` = all listed files and lines covered and no tched substring reported; `partial` = incomplete coverage or unreviewed occurrence; `insufficient` = evidence does not establish coverage or occurrence status.

Returned: `{"type":"choice","choice":"complete_zero_occurrences","confidence":0.98,"probabilities":{"complete_zero_occurrences":0.99,"partial":0.01,"insufficient":0}}`.

**change** — “Does the evidence support any tched-specific source correction in this crate? Choose no change if source has no relevant occurrence; do not infer unrelated defects or runtime state.” Criteria: `no_tched_specific_change` = no tched-specific source correction warranted; `correct_source` = a concrete tched-specific source correction supported; `insufficient` = evidence does not permit reliable disposition.

Returned: `{"type":"choice","choice":"no_tched_specific_change","confidence":1,"probabilities":{"no_tched_specific_change":1,"correct_source":0,"insufficient":0}}`.

**scope** — “What does this evidence establish about runtime/catalog/deployment?” Criteria: `unverified` = only static source coverage established; runtime/catalog/deployment remain unverified; `verified` = runtime/catalog/deployment state established; `contradictory` = evidence conflicts about that state.

Returned: `{"type":"choice","choice":"unverified","confidence":1,"probabilities":{"unverified":1,"verified":0,"contradictory":0}}`.

These judgments are evidence, not permission to mutate source or proof of runtime/deployment state.
