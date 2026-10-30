# op-deployment — native contextual audit

## Outcome

Complete static review of all 19 inventory files, sequentially through EOF: 4,832 supplied newline-counted lines; 4,833 observed source lines because Cargo.toml ends at line29, not28. Root, src, src/bin and src/bin/release listings reconcile with the inventory. No nested guidance, additional source, symlink, missing, binary or dependency entries encountered. No content searches or source sampling. Earlier complete source reads and coverage.tsv are retained; this reconciliation reread the native-bearing spans and kept the original coverage.

Occurrence reconciliation:28 distinct native/native-compound/identifier occurrences have individual rows in occurrences.tsv. Final evidence mapping uses J4 except O10/O11/O16/O19, J5 for O28, and fresh J6 for both tokens on each of transfer.rs:299/389. Earlier O10/O11 used report excerpt line numbers26/34 as source locations; those phantom locations are withdrawn, and their old JEV answers are preserved as historical receipts only. The two previously omitted second-token positions replace them; total28 is unchanged. All28 are accurate Btrfs/Linux ABI, UUID, CPU/ELF, sender-thread, controller-recovery or fixture vocabulary. No scoped native-specific source defect or source changes. Six actual declarative JEV calls total (38 questions, including superseded answers); see judgments.md.

## Per-occurrence dispositions

`occurrences.tsv` maps each occurrence, exact location/context, intended meaning, JEV receipt, outcome/confidence and disposition. Cases include `native` in deployment/ABI/CPU/sender/recovery phrases, every `native_uuid` identity read/write/comparison (including Release construction at transfer.rs:251), and both `native-image` fixture-name uses. No occurrence makes a provider/model/config selection or inaccurate claim. O17's lower JEV confidence (0.61) was resolved by direct contextual comparison of `source_uuid` and `RootRemoval.native_uuid`; it remains accurate.

Original findings remain: no independent native-specific fix; one generic ImageManager dependent-symlink deletion risk and historical inventory doc issue outside native-only edit ownership.

Accounting correction: current transfer.rs was reread sequentially1-175;176-350;351-516. Line299 contains both the `native_uuid` field label and `old.native_uuid`; line389 contains both `prior.native_uuid` and `removal.native_uuid`. Each now has a separate ledger position and individual J6 judgment. Neither source line26 nor34 contains native. Prior source coverage remains valid; this corrects the reporting error, not source behavior. No coordinator-followup.md was present when directly read.

## Native contexts — preserve

- `src/bin/op-release.rs:1-3`: native Btrfs application deployment, not a provider/model selector.
- `src/bin/release/lifetime.rs:6-19,31-60`: native Linux ABI, cleaner vocabulary and native_uuid. The fixed ioctl carries exact FSID/subvolume/original UUID/received UUID; errors do not fall back to ordinary deletion.
- `src/bin/release/model.rs:45-49,70-74` and `transfer.rs:7-15,251-253,268-307,345-350,374-440,461-496`: native_uuid distinguishes original subvolume incarnation from received/source UUIDs. These values enforce filesystem identity; they are not configuration defaults.
- `src/bin/release/transfer.rs:97-113`: native x86_64 little-endian ELF receiver gate is CPU/ABI compatibility. `:151-173` uses the Btrfs send library and labels its thread a native sender. No LLM route is selected.
- `src/bin/release/system.rs:1-9`: native recovery describes the controller/launcher maintenance implementation; it does not bypass launcher digest admission.
- `src/image_manager.rs:623-657`: native-image is an isolated Btrfs snapshot fixture label, used by create and lookup calls. Test source was read, not executed.

No implicit provider/model/config override path was found anywhere in the complete crate. `model.rs` models release state, not LLM models. Hard-coded deployment paths/FSID/OPT_ID are host maintenance operands verified against prepared inventories; this audit establishes no competing plugin schema source and does not authorize changing host layout.

## Other observations — outside native-only edit ownership

### G1 — deleting referenced images can invalidate later images

Source: `src/image_manager.rs:195-225,304-328,451-506`.

On Unix, equal-content files in a new image can become relative symlinks to canonicalized files under a previous image. `delete_image` deletes the named image and its snapshots without rejecting references from other images or materializing their contents. Therefore deleting the old image can leave a later image/snapshot with a dangling symlink. JEV classifies this as a generic image-lifecycle risk, not a native-provider defect (confidence0.96). Source-derived risk, not reproduced here; no claim of deployed impact or current caller exposure.

Coordinator option under separate scope: define reference lifetime semantics, then refuse deletion while referenced or materialize dependent content before deletion. Do not change the root-owner checked-delete ABI as a workaround. No repair applied because this task owns independent native-specific fixes only.

### G2 — historical documentation is not current controller authority

`compare-op-deployment.md:11-21,43-44` describes two Rust files, zero binaries and no internal dependencies; the current complete inventory contains the release binary/modules and Cargo.toml:26 depends on op-cozo-store. SPEC.md describes the ImageManager design, not the complete release controller. These documents must not establish live deployment or current binary inventory. No general documentation cleanup applied under native-only scope.

## Verification and limitations

Evidence: complete line reads recorded in coverage.tsv; directory reconciliation; successful declarative JEV source judgments. No behavioral change, build, test, lint, formatting, smoke fixture, runtime operation, deploy, seal, identity/grant/credential action, Git command or recursive delegation. No runtime success is inferred from source or JEV confidence. Inventory hashes are pre-audit supplied receipts; no current rehash or HEAD query performed.

The higher-priority session handoff contract required a minimal shared HANDOFF claim/completion entry despite the assignment's report-only preference; no unrelated shared claims changed. Exclusive reports and result receipt contain the coordinator handoff.

## Self-evaluation

Accuracy4/5: source and JEV evidence grounded; no runtime verification authorized. Completeness4/5: every inventory line covered; cross-crate caller exposure intentionally not established. Clarity4/5: native dispositions separated from generic risks; numerous UUID call sites summarized by ranges. Actionability4/5: exact generic risk and safe coordinator options supplied, but implementation requires separate scope. Conciseness4/5: per-file ledger is necessarily repetitive. Overall4.0/5. Improvement: coordinator can decide a separate image-lifetime repair and authorize its build/runtime verification. Assessment matches the static-only assignment, not a system-quality approval.
