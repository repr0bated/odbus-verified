# Current findings — contextual native audit

Status: **no actionable native-specific correction established; no source files changed**. The latest coordinator correction supersedes the partial/blocker statements below. Case-insensitive expression search and context inspection surfaced 8,827 generated-file substring matches and 35 elsewhere in the coordinator baseline; these are not 8,862 semantic claims. The generated output is derived from sealed schemas and is not edited as an independent authority.

## Dispositions

- `alternative(s)` contains the letters `native` but is a lexical false positive. `react-native` and literal `native` identifiers/arguments are likewise non-prose identifiers. Preserve exact API/property/renderer/CLI names.
- `src/mounted.rs:405` (“native clients”): discovery returns declarations from the verified mounted schema; implementation does not identify a distinct native-client class. JEV `jdgb-15b34681524c0d96`: insufficient, 0.38. No factual defect established; no correction.
- `docs/ARCHITECTURE.md:173` (“native zbus connection”): implementation uses `zbus::Connection` over the shared socket and preserves D-Bus identity/authorization. JEV `jdgb-15b34681524c0d96`: needs_correction, 0.34 (0.56 correction probability). “Native” is avoidable ambiguity, but the evidence does not establish a material false claim; no edit.
- Remaining source/docs wording and grouped generated contexts have current decisions in `occurrences.tsv` and `judgments.md`. JEV marked many generated-only contexts insufficient; these are retained without elevating them into source defects. Explicit qualifiers, disclaimed behavior, and identity/capability boundaries remain intact.

No source edits, build, tests, runtime checks or deployment. This is a contextual wording audit, not proof of implementation/runtime correctness.

## Historical partial findings (superseded)

# Findings — partial native audit

Status: no native-specific source change made. This is not a clean/no-findings result: source review is incomplete and native occurrence accounting is unresolved.

## Candidate contexts requiring disposition

- Documentation wording in `README.md:482` (“Native threetched's 147 methods”) is ambiguous in isolation; JEV returned insufficient evidence (0.27 confidence). Preserve pending context and contract-authority review.
- `docs/ARCHITECTURE.md:173` calls the zbus connection “native”; JEV returned insufficient evidence (0.43). Read connection construction and surrounding contract before deciding.
- `docs/typed-filesystem-binding-input.json:2297` and `docs/typed-filesystem-policy.json:105` qualify backend allocator/native buffers; JEV returned insufficient evidence (0.56). Corroborate source allocation ownership before changing this boundary statement.
- Build-target wording (“native GNU release build”) appears in gaps, policy, verification, and schema documentation. Repo context declares `x86_64-unknown-linux-gnu`; JEV's answers were low-confidence. Current/historical build distinction is stated in the evidence, but no full validation of references occurred.
- `native_list_providers` is a generated method key at `docs/typed-specialization-report.json:3297`. Preserve provisionally; compare with the manifest-pinned sealed method key before proposing any name change.
- `native container` is described as GuestKernel binding using kernel peer evidence (`docs/typed-filesystem.schema.json:5369`); JEV insufficient (0.37). Confirm against source identity path before disposition.
- macOS-only FUSE callback descriptions say “not native Linux surface” in the schema and node export. JEV was insufficient for the schema context (0.45); the nodes-export instances were not separately evaluated.


## Generated bindings and blocker

- `generated/plugin_types.rs` contains repeated native-auth and credential wording for `antigravity` (observed at lines 911–1117), native Google bridge/configuration wording for `antigravity_chat` (1304–1572), and a `Native Btrfs filesystem operations` description (1608–1652). JEV batch `jdgb-15b33462d4577323` returned accurate / 0.22, insufficient / 0.33, and insufficient / 0.80 for these three supplied context groups, respectively. The first two require human follow-up; none establishes implementation behavior or a source correction.
- Full occurrence accounting is blocked: `read` reports generated line 3578 is 1.1 MiB; raw reading exposes only 50 KiB and a widened selector still cannot include the suffix. The unread suffix may contain relevant wording. This continuation read generated lines sequentially through 4164; previously clipped lines and the suffix through line 121297 remain incomplete. All non-generated inventory files are read. This is not a no-findings conclusion.

No source changes made. Full audit remains partial-blocked pending a complete-line read path that does not violate the no-search restriction.
No patch is recommended from this partial evidence. No source files changed. Generic filesystem defects outside the native-specific scope were not pursued.

- `src/dispatch.rs:324` describes the container-bootstrap identity derivation as a “native Incus binding.” JEV judged this accurate (0.43) from lines 321–325; confidence remains low and source code does not independently establish the Incus/native mechanism. No wording change is supported yet.
- `src/mounted.rs:405` calls the discovery API a surface “for native clients.” The API reads declarations from the pinned mount; the source context does not identify a distinct native-client category. Two contextual JEV judgments favored correction at confidence 0.30 and 0.40; both are below the required 0.6 review threshold. No wording change made; leave unresolved for stronger contract context.
- Full inventory source coverage now includes every non-generated entry. Generated bindings were read sequentially through line 4164 (line 3578 remains unread beyond its 50 KiB cap; lines 3795, 3802, 3878, 3885, 3892, 3975 and 3982 remain clipped). This still cannot support exhaustive native occurrence accounting or complete JEV coverage.

## Updated disposition of previously inventoried non-generated occurrences

Batch `jdgb-15b338353943da4c` judged 22 occurrence-specific contexts after the source excerpts were reread. Results: 15 accurate, 7 insufficient, 0 needs-correction; all results, confidence and probabilities are mapped in `judgments.md` and `occurrences.tsv`. The earlier candidate notes above describe the pre-batch state and are superseded for these known occurrence contexts. Low-confidence and insufficient answers remain provisional; no source/documentation change is supported. This does not complete exhaustive occurrence accounting because the generated-file read remains blocked at the suffix of `views/00001.part`.