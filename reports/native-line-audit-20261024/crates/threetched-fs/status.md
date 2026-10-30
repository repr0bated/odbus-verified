# threetched-fs native audit — current disposition

Status: **complete (contextual audit; no source changes)**. The latest coordinator correction supersedes the prior exhaustive-read/no-search method and old reader-blocker status below. Case-insensitive `native` expression search covered the 38-file crate inventory; the coordinator's baseline is 8,827 generated-file substring matches and 35 elsewhere. Context was inspected around distinct semantic uses; `alternative(s)` substring matches and `react-native` identifiers were classified lexically, not treated as native claims. Repeated generated validator copies are grouped by exact source line in `occurrences.tsv`.

JEV: prior occurrence-specific batch `jdgb-15b338353943da4c` (22/22 non-generated contexts); current batches `jdgb-15b34681524c0d96` (7/7), `jdgb-15b346aab94c0d97` (22/22), and `jdgb-15b34779f58c0d99` (3/3). Answers, probabilities and human dispositions: `judgments.md`; mapped contexts: `occurrences.tsv`.

No native-specific source correction was established. `src/mounted.rs:405` and `docs/ARCHITECTURE.md:173` remain potentially ambiguous; JEV was insufficient or low-confidence, and implementation evidence establishes no concrete defect. Generated bindings remain derived from sealed schemas; they were not edited as independent authority.

Coverage: `coverage.tsv`. Current findings: `findings.md`. Historical ledgers/reports retained below or in `occurrences-history.tsv`; their partial status and reader-blocker claims are superseded.

No Cargo, tests, runtime, smoke, deployment, or service actions; no source files changed. This audit does not establish build, runtime, or deployment correctness.

## Historical partial status (superseded)

# threetched-fs native audit

Task: `native-line-audit-20261024/threetched-fs`

Status: **partial / blocked on reader limit**. Mandatory repo preload and per-crate instructions were read. Inventory entries 1–22 and 24–39 were read sequentially to EOF. Generated `plugin_types.rs` (inventory entry 23; 121,297 lines) has been read sequentially through line 4164, with the eight clipped prefix lines 136, 143, 150, 258, 265, 293, 300, 307 reread raw.

The source reader reports generated line 3578 as 1.1 MiB. Its raw selector returns only 50 KiB; a widened selector cannot include the complete line. The unread suffix prevents full-text native-occurrence review and complete contextual JEV. Other generated JSON-schema lines clipped at 768 characters remain incomplete at 3795, 3802, 3878, 3885, 3892, 3975 and 3982. The repeated validator lines 3968, 4058, 4065, 4072, 4148, 4155 and 4162 were read raw in full.

The full native occurrence ledger remains unresolved. Generated contexts already JEV-judged include repeated native-auth/credential wording for `antigravity` (lines 911–1117), native Google bridge/configuration wording for `antigravity_chat` (1304–1572), and the native Btrfs description (1608–1652), with low-confidence results and incomplete per-occurrence mapping. This continuation also recorded `dispatch.rs:324` and `mounted.rs:405`; the mounted phrase received two low-confidence JEV answers (0.30, 0.40). Neither source occurrence supports an authorized correction on current evidence.

All non-generated inventory entries are complete. Generated file continuation resumes at line 4165, with previous oversized/truncated content still open. No source changes were made.

Coverage: `coverage.tsv`. Occurrences: `occurrences.tsv`. JEV: `judgments.md`. Findings: `findings.md`.

No Cargo, tests, smoke, runtime, or deployment actions were performed. None were authorized by this audit assignment.

Exact blocker: this reader cannot provide all bytes of source line 3578 (1.1 MiB; raw limit 50 KiB), so the task's required exhaustive occurrence accounting and all contextual JEV decisions cannot be completed without a compliant complete-line read. Do not use searches or extraction workarounds.

## Continuation attempt: lossless generated views

Coordinator follow-up supplied a sequential, byte-partitioned generated-source view with 1,711 parts and SHA-256 receipts matching the 68,429,995-byte source. Part `00000.part` was fully displayed across its returned ranges. Part `00001.part` was read in local line ranges 1–20, 21–40, 41–60, 61–76, and 76–76. The reader output for the final long line stops mid-token at `exp.so`; it provides neither an end-of-part marker nor a byte/column selector to retrieve the omitted suffix. Re-reading that one line returns the same clipped prefix. Therefore part 00001 is not complete, and sequential part coverage cannot advance to part 00002 without omitting source bytes.

The complete prepared manifest was read sequentially through line 1712 (entry `01710.part`). Manifest metadata does not substitute for reading each view part. The generated view remains blocked at part 00001; no new occurrence was claimed from the clipped generated source.

Concrete blocker: the available reader clips a long line inside a 40,000-byte view part and exposes no positional selector within that line. A smaller lossless view or a read interface with byte-range access is required to continue without searches or extraction workarounds. No source edits, Cargo, tests, runtime checks, or delegation were performed.

## Follow-up retry

After rereading the coordinator instruction and exact result receipt, the whole-file raw read of `views/00001.part` was repeated. Its returned content again ends inside generated Rust, without an EOF or displayed-byte-count marker. The reader also returned only a partial response for a whole-file raw read of `views/00002.part`; because part 00001's suffix remains unlocated, part 00002 is diagnostic only and is not counted as covered. The exact next required source bytes remain the unread suffix of part 00001, but the tool provides no offset to identify them.

The view receipts establish partitioning and source identity, not complete display. The retry did not resolve the missing suffix or produce new source occurrences; this status was updated below with separately completed JEV work on previously inventoried non-generated occurrences.

## Contextual JEV for previously inventoried non-generated occurrences

Read exact source contexts for the 22 non-generated native-occurrence rows that previously lacked occurrence-specific JEV linkage. Batch `jdgb-15b338353943da4c` returned 22/22 results, no failures. Each state included that occurrence's own context; results and confidence/probabilities are recorded in `judgments.md` and mapped in `occurrences.tsv`. Outcomes: 15 accurate, 7 insufficient, 0 needs-correction. Low-confidence results remain provisional; no native-specific source/documentation correction was supported by this evidence. Generated occurrences remain unreconciled because sequential part 00001 is not fully displayed.

## Continuation: prepared-view reader truncation

The coordinator receipt was reread; its exact next incomplete part before this continuation was `views/00090.part`. Prepared-view receipt and both source/reassembled SHA-256 files were read; source identity is unchanged. Parts `00087`–`00090` were requested as whole raw reads in order, and part `00087` was retried directly with both `:raw:1-50000` and `:raw:1-1`. The reader rendered only a prefix of each long single-line part; it provides line selectors, not byte offsets, so these parts cannot be counted fully displayed. The next incomplete range is `views/00087.part`; exact visible-output boundaries and missing byte intervals are not supplied by the reader.

The coordinator's concrete receipt correction establishes that `00001.part` and `00002.part` were each fully returned and SHA-256 matched; the earlier status claims that those two parts were incomplete are superseded. Parts `00000`–`00086` are represented as fully displayed in `result.json`; this does not establish generated-occurrence reconciliation for them.

Part `00090` visibly contains seven `cloud-native patterns` occurrences in `agent_kubernetes_expert_{advise,analyze,consult,explain,help,recommend,review}` descriptions on original source line 3592. The repeated descriptive context was evaluated with one direct JEV choice call over all seven visible action records: accurate, confidence 0.89 (probabilities 0.93 accurate / 0 needs-correction / 0.07 insufficient). Each occurrence has a separate operation-key row in `occurrences.tsv`; precise in-line byte positions and all undisplayed content remain unresolved. No correction was supported by the visible context.

Status remains **partial / blocked on complete single-line reads**. A byte-offset or equivalent lossless reader is required to resume at part 00087. No searches, source changes, Cargo, tests, runtime checks, deployment, or delegation were performed.
## Coordinator correction and continuation

The coordinator reread `parts-87-90-output-receipts.jsonl` and recovered the complete successful raw outputs for parts `00087`–`00090`; each output is 40,016 bytes and equals its full view by SHA-256. This supersedes the prior claim above that these four part outputs were truncated. The prepared manifest records part `00091` at byte interval 3,640,000–3,680,000 and part `00092` at 3,680,000–3,720,000, both on original line 3592 with no newline.

Parts `00091`–`00095` were then read sequentially with whole-file raw selectors. No explicit partial-range marker appeared. The newly read content showed continued serialized plugin schemas; no additional native occurrence was identified in the returned content. Prepared views `00000`–`00095` are now recorded in `result.json`, with next sequential view `00096`. Generated occurrence enumeration, contextual JEV decisions for any yet-unseen occurrence, and exact byte mapping remain incomplete; task status is partial/in-progress, not complete.

No searches, Cargo, tests, runtime checks, deployment, or delegation were performed.
## Subsequent sequential reads

Prepared views `00096`–`00110` were read with whole-file raw selectors in order. The returned content remained serialized schemas; no additional native occurrence was identified in the returned content and no explicit partial-range marker appeared. `coverage.tsv` and `result.json` now record parts `00000`–`00110` read, with `views/00111.part` next. Occurrence/JEV reconciliation for the generated file is still incomplete.

Prepared views `00111`–`00115` were read sequentially with whole-file raw selectors. The returned content continued serialized schemas; no additional native occurrence was identified and no explicit partial-range marker appeared. `coverage.tsv` and `result.json` record parts `00000`–`00115` read, with `views/00116.part` next. Generated occurrence/JEV reconciliation remains incomplete.

Prepared views `00116`–`00120` were read in order; view `00118` contains seven repeated `cloud-native patterns` action descriptions for `agent_kubernetes_expert_{advise,analyze,consult,explain,help,recommend,review}`. A direct contextual JEV choice classified these as descriptive-only, confidence 1.0; seven occurrence rows and the judgment were added. Original source line/byte mapping remains pending. `coverage.tsv` and `result.json` record parts `00000`–`00120`, with `views/00121.part` next. Audit remains partial/in-progress.