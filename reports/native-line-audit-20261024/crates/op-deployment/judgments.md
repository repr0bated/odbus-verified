# Declarative JEV receipts — op-deployment

Correction, 2026-10-24: J3/J4 received erroneous source locations for old O10/O11 copied from J2's report excerpt line numbers26/34. Source transfer.rs:26/34 contains no native occurrence. Their old answers are preserved below as historical receipts, not valid evidence for source occurrences. Corrected O10/O11 now identify token2 at source299/389 and use fresh J6 judgments. O16/O19 identify token1 at those lines, also independently refreshed in J6. J3's original completeness claim is superseded; final accounting is28 distinct source occurrences.

## J1 — Native Btrfs/ABI/UUID meaning; fallback behavior

Facility: installed `judge()` through Python eval; successful call `Judging native storage meanings`. No routing/model selection. State key `source` contains source excerpts transcribed from sequential reads. References: `src/bin/op-release.rs:1-3`; `src/bin/release/lifetime.rs:6-19,31-60`; `src/bin/release/model.rs:45-49`.

Evidence: application deployment is described as native Btrfs snapshot/send/receive; lifetime adapter declares a 96-byte repr(C) ioctl input and literal Linux ABI command; `native_uuid.into_bytes()` is written to that input's UUID field, separate from received_uuid; errors from ioctl return as errors. Release records native_uuid separately from source_uuid and received_uuid.

Question `meaning`: classify native as storage/ABI identity, implicit LLM override, or insufficient evidence; preserve legitimate filesystem terminology and infer no runtime success.

Actual answer: `storage_abi_identity`; confidence 1. Probabilities: storage_abi_identity=1, implicit_model_override=0, insufficient=0.

Question `admission`: does destroy replace missing kernel lifetime support with a permissive deletion fallback? Contrary outcomes: no_fallback_shown, permissive_fallback, insufficient.

Actual answer: `no_fallback_shown`; confidence 1. Probabilities: no_fallback_shown=1, permissive_fallback=0, insufficient=0.

Decision: preserve native terminology and UUID fields. No native-specific defect established; this is static source evidence, not proof of ABI availability or deployed behavior.

## J2 — Receiver ABI, fixture label and generic image-lifetime risk

Facility: installed `judge()` through Python eval; successful call `Judging receiver and image contexts`. One combined state contains keys `transfer`, `fixture`, `dedup_delete`; three independent questions see the same state. Source evidence was read completely before the call. The eval transcript retains the supplied source excerpts; these receipts preserve their source locations and decisive statements.

`transfer`: `src/bin/release/transfer.rs:7-15,97-113`. Decisive source:

```rust
    ensure!(info.id == release.id && info.uuid.to_string() == release.native_uuid
        && info.received_uuid.to_string() == release.received_uuid
        && subvolume::subvolume_flags_get(root.as_fd())?.contains(SubvolumeFlags::RDONLY),
        "release root ID/native UUID/received UUID/read-only identity differs");
```

```rust
    ensure!(&elf[..6] == b"\x7fELF\x02\x01" && u16::from_le_bytes(elf[18..20].try_into()?) == 62,
        "receiver must be a native x86_64 little-endian ELF executable");
```

Question `abi`: do native UUID and receiver architecture checks constrain filesystem/CPU ABI, or force LLM provider/model configuration? Choices: storage_cpu_abi, model_override, insufficient.

Actual answer: `storage_cpu_abi`; confidence1. Probabilities: storage_cpu_abi=1, model_override=0, insufficient=0.

`fixture`: `src/image_manager.rs:623-656`, full test context uses a disposable DeploymentFixture, creates input.bin with `image contents`, checks Btrfs inode256, read-only property and file bytes.

```rust
        let image = manager
            .create_image("native-image", vec![input])
            .await
            .unwrap();
```

```rust
        let snapshot = manager
            .get_streamable_snapshot("native-image")
            .await
            .unwrap();
```

Question `fixture`: classify native-image as fixture_label, provider_override, or insufficient; do not imply test execution.

Actual answer: `fixture_label`; confidence0.98. Probabilities: fixture_label=0.99, provider_override=0, insufficient=0.01.

`dedup_delete`: `src/image_manager.rs:195-225,304-328,451-506`. Exact decisive source:

```rust
                let relative_target =
                    self.calculate_relative_path(dest_path.parent().unwrap(), &previous_file)?;

                #[cfg(unix)]
                {
                    async_fs::remove_file(&dest_path).await?;
                    std::os::unix::fs::symlink(&relative_target, &dest_path)
                        .context(format!("Failed to create symlink: {}", dest_path.display()))?;
                }
```

The complete delete_image function was read at351-525; it removes matching snapshots and the named image directory, without scanning or materializing other images' references. The state explicitly describes this complete-function negative evidence, rather than treating an omitted excerpt as proof.

Question `generic_risk`: on Unix with successive equal-content images, can deleting the older image invalidate a newer image's symlink? Choices: generic_dangling_symlink_risk, safe_dependency_handling, native_model_defect, insufficient.

Actual answer: `generic_dangling_symlink_risk`; confidence0.96. Probabilities: generic_dangling_symlink_risk=0.97, safe_dependency_handling=0, native_model_defect=0.03, insufficient=0.

Decision: preserve ABI, native_uuid and fixture vocabulary. Record generic dangling-symlink risk outside native-only edit ownership; no source patch, runtime claim or caller-exposure claim.
## J3 — Complete occurrence-set cross-check

Facility: actual installed `judge()` via Python eval; successful call `JEV occurrence context adjudication`. One state listed each source occurrence O01–O27 individually with exact token, source location and context, including every native_uuid identifier and both fixture-name occurrences. Question asks whether all individually represent accurate contextual meanings without an implicit LLM provider/model/config override, with explicit semantic-defect, model-override, and insufficient-evidence alternatives.

Actual answer: `legitimate_native_context`; confidence0.93; probabilities: legitimate_native_context0.95, native_semantic_defect0.04, implicit_model_override0, insufficient_evidence0.01. This was an aggregate cross-check, not the per-row sole receipt.

## J4 — Independent declarative judgment for each occurrence

Facility: actual installed `judge()` via Python eval; successful call `JEV per-occurrence judgments`; 27 independent typed choice questions O01–O27 over the exact occurrence state described in J3. Each question cites only its one occurrence and asks whether wording/identifier is accurate and whether it avoids implicit provider/model/config selection, with material defect, implicit override and insufficient evidence as alternatives.

Actual individual outcomes (all `legitimate_native_context`):

| Occurrence | Confidence | Occurrence | Confidence | Occurrence | Confidence |
|---|---:|---|---:|---|---:|
| O01 | 0.96 | O02 | 0.96 | O03 | 0.91 |
| O04 | 0.96 | O05 | 0.99 | O06 | 0.97 |
| O07 | 0.97 | O08 | 0.99 | O09 | 0.93 |
| O10 | 1.00 | O11 | 0.97 | O12 | 1.00 |
| O13 | 0.97 | O14 | 0.97 | O15 | 0.99 |
| O16 | 0.79 | O17 | 0.61 | O18 | 0.97 |
| O19 | 0.99 | O20 | 0.99 | O21 | 0.99 |
| O22 | 0.96 | O23 | 0.95 | O24 | 0.97 |
| O25 | 0.91 | O26 | 0.97 | O27 | 0.99 |

Decision: all 27 terms accurately describe Btrfs/Linux ABI, UUID identity, CPU/ELF compatibility, Btrfs send thread, controller recovery, or a fixture label. O17 has lowest confidence; manually checked its source/source_uuid to RootRemoval.native_uuid mapping against the RootRemoval identity model, and found no semantic defect. No scoped native-specific source change warranted. J1–J2 remain the prior actual receipts and are not replaced.

## J5 — Supplemental occurrence O28

Facility: actual installed `judge()` via Python eval; successful call `JEV judgment for omitted UUID occurrence`. Exact source context: `src/bin/release/transfer.rs:251`, construction of `Release` from the inspected receive; it stores `info.uuid` as `native_uuid`, `info.received_uuid` as `received_uuid`, and separately records source UUID/ID.

Question: is `native_uuid` an accurate local Btrfs subvolume identity, distinct from receive/source identity, without implicit LLM provider/model/config selection?

Actual answer: `legitimate_native_context`; confidence0.97; probabilities: legitimate_native_context0.98, native_semantic_defect0.01, implicit_model_override0, insufficient_evidence0.01.

Disposition: add O28 to the occurrence ledger. This context confirms the correctly separated UUID fields; no source change.

## J6 — Corrected double-occurrence UUID lines

Actual installed `judge()` call `Judging corrected UUID token positions`, four independent choice questions. Evidence from sequential reads of transfer.rs:1-175;176-350;351-516. Exact lines:

```rust
// transfer.rs:299
id: old.id, native_uuid: old.native_uuid.clone(), received_uuid: old.received_uuid.clone(),
// transfer.rs:389-390
ensure!(prior.native_uuid == removal.native_uuid && prior.received_uuid == removal.received_uuid
    && prior.parent == removal.parent && prior.name == removal.name, "cleanup identity changed");
```

Additional source evidence supplied: transfer.rs:399-403 compares observed Btrfs info.uuid against removal.native_uuid, separately from received_uuid. Each question asks whether its specific token accurately describes intended local root identity, or requires a scoped semantic fix, or has insufficient evidence.

| Question | Token | Answer | Confidence | Probabilities preserve / fix / insufficient |
|---|---|---|---:|---|
| O16 | 299 token1 field label | preserve | 0.91 | 0.94 / 0.04 / 0.02 |
| O10_corrected | 299 token2 old.native_uuid | preserve | 0.95 | 0.97 / 0.02 / 0.01 |
| O19 | 389 token1 prior.native_uuid | preserve | 0.82 | 0.88 / 0.07 / 0.05 |
| O11_corrected | 389 token2 removal.native_uuid | preserve | 0.94 | 0.96 / 0.03 / 0.01 |

Decision: no source fix warranted. Correct the ledger, replacing two phantom locations with the two omitted second-token positions. Total28 remains numerically unchanged, but these corrections are necessary for accurate occurrence coverage. J4 answers for other source occurrences and J5 remain applicable; J6 supersedes the four ledger mappings listed above.
