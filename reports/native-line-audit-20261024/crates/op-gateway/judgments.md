# Declarative JEV receipts — op-gateway

Task `native-line-audit-20261024/op-gateway`, Codex crate worker, 2026-10-24.
Installed instructions read: `xd://eval/judge`. Actual successful call: `functions.eval`, Python, title **Judging native encryption contexts**, `await judge(native_state, {N1,N2,N3,N4})`. No routing extension, model selection, filesystem execution or source extraction. Full returned answers read via `artifact://0:raw:1-1` after normal output truncation. Evidence is static source, not runtime proof.

## Shared evidence supplied

`crates/op-gateway/src/encrypted_storage.rs`, fully read in 1-240,241-480,481-674 through EOF, snapshot EB66. Judge received actual contexts of all 20 native occurrences, not a scope-only statement:

- 4: `//! using encrypted Btrfs subvolumes with native encryption (experimental) or LUKS.`
- 24-26: `/// Use native Btrfs encryption (experimental)` / `pub use_native_encryption: bool,` / `/// LUKS device name (if not using native encryption)`.
- 75-81: Default fixes base path `/var/lib/op-dbus/encrypted`, subvolume `wireguard-keys`, `use_native_encryption: true, // Use experimental Btrfs encryption`, mapper `Some("opdbus_wg_keys".to_string())`.
- 123-139: initialize creates base directory, then `if self.config.use_native_encryption { self.setup_native_btrfs_encryption().await?; } else { self.setup_luks_encryption().await?; }`, loads/generates master key, sets initialized.
- 144-159: `/// Setup native Btrfs encryption (experimental)`; `async fn setup_native_btrfs_encryption(&self) -> Result<()>`; log `Setting up native Btrfs encryption for subvolume: {}`. Comment 151-153: `// A directory or unencrypted subvolume cannot be treated as encrypted` / `// merely because it exists. There is no reliable native-encryption` / `// attestation for this legacy backend on reopen.` Existing path bails with `Cannot verify native encryption of existing key storage {}; use a verified LUKS mapper`.
- 161-184: comment says kernel support required, calls `Command::new("btrfs").args(["subvolume", "create", "-e", path_str]).output()`, annotates `-e` as enabling experimental encryption. Requires `output.status.success()` with error `Native Btrfs encryption is unavailable; refusing plaintext key storage: {}`, logs created encrypted subvolume, sets permissions, returns success. No encryption attestation of the resulting path.
- 195-281: LUKS path validates mapper name, requires mapper present, mounts if path absent, always verifies. Verification requires nonsymlink directory, block-device mapper, successful cryptsetup status whose type is exactly LUKS1/LUKS2, and exact findmnt mount source device identity matching mapper rdev.
- 539-564: `let fs_info = self.get_filesystem_info().await?;` precedes StorageStats construction. Fields include `is_encrypted: self.config.use_native_encryption || self.config.luks_device_name.is_some()` and `encryption_type: if self.config.use_native_encryption { "btrfs-native".to_string() } else { "luks".to_string() }`. get_filesystem_info unconditionally bails: `Storage capacity is unavailable through this legacy gateway; request filesystem_usage from the sealed btrfs plugin through the canonical door`.
- 637: absent-mapper fixture selects `use_native_encryption: false`.
- 649,651: `async fn existing_plain_directory_cannot_claim_native_encryption()` uses temporary namespace `opdbus-native-unverified-{}-{}`. Lines658-664 create ordinary directory and default config with custom base. Lines665-672 obtain new(config) error, assert `.contains("Cannot verify native encryption")` at669, assert no master.key and remove fixture directory.
- `src/wireguard_auth.rs:129-145`: public new() uses storage default through pub(crate) new_with_storage_config, then constructs EncryptedKeyStorage::new(storage_config).await?.
- `src/lib.rs:8-11`: publicly reexports storage config, manager and stats.

## N1

Question: Evaluate native in lines4,24,25,26,80,130,131,144,145,147,152,156,176. Is this model/provider routing, harmless verified storage terminology, or storage behavior/claims lacking demonstrated encryption attestation? Judge actual evidence, not whether all native words should be removed. Do not claim installed btrfs behavior was run.

Choices:
- storage_attestation_defect: Native denotes filesystem encryption, not LLM routing. Default explicitly selects it. Existing storage is rejected, but new storage accepts command success as encrypted without checking encryption state; evidence does not establish native encryption support. Claims/default require repair or verified backend authority.
- accurate_verified_storage: Source establishes supported native encryption and verifies it before storing keys; wording/default justified.
- model_override: Source implicitly chooses an LLM/provider/model from native names.
- insufficient: Evidence cannot distinguish outcomes.

Observed result: **storage_attestation_defect**, confidence **1**. Probabilities: selected1, all other choices0.
Decision: F1 source finding; do not mechanically rename native or claim a plaintext write/runtime exploit occurred.

## N2

Question: Evaluate native in get_stats lines545,547,548 using actual control flow539-564. Distinguish inaccurate intended encryption classification from currently observable runtime output.

Choices:
- unreachable_unverified_label: config-derived encryption labels, not verification; get_filesystem_info always errors before StorageStats returns. Record misleading/dead native classification with storage repair, not an observed successful false stats response.
- verified_runtime_stats: Function currently returns verified btrfs-native statistics.
- routing_override: Labels change model routing.
- insufficient: Insufficient evidence about ordering/meaning.

Observed result: **unreachable_unverified_label**, confidence **1**. Probabilities: selected1, all other choices0.
Decision: include native classification in F1 cutover; do not expand repair into implementing storage capacity.

## N3

Question: Evaluate fixture occurrences637,649,651,669. Do their actual contexts independently establish defective terminology or accurate names for the rejected native-encryption claim? Account for exact-error-wording assertion.

Choices:
- fixture_describes_rejection:637 selects LUKS;649 and651 describe unverified encryption fixture;669 checks existing-directory rejection. Not model routing or encryption proof. Removing native backend requires supported LUKS fixture migration rather than mechanical rename; exact wording assertion should not be re-pinned.
- fixture_proves_native_support: Tests prove successful native encryption.
- independent_routing_defect: Fixture names cause provider override.
- insufficient: Cannot infer fixture intent.

Observed result: **fixture_describes_rejection**, confidence **1**. Probabilities: selected1, all other choices0.
Decision: accurate current rejection vocabulary; remove obsolete fixture and migrate behavior assertion with supported-backend cutover. Tests were read, not executed.

## N4

Question: Evaluate proposed clean cutover, not permission: remove unverified native backend, use_native_encryption field/default/branch/native stats labels; retain verified LUKS path/checks, migrate public config callers/serialized config/fixtures, update docs. Public reexport/default caller shown; external consumers uninspected and outside single-crate ownership. Atomic proven crate-only change, cross-owner dependency, or unnecessary? Do not invent external callers.

Choices:
- coordinator_reconciliation: Cutover addresses defect without weakening LUKS checks, but deleting public serialized field requires coordinator-owned external consumer/config reconciliation before claiming all-caller migration; actual external caller existence unknown.
- proven_crate_only: Evidence proves no external consumer/config can depend on public field, fully demonstrated crate-only.
- keep_unverified_backend: Unverified native mode should remain advertised/default enabled.
- insufficient: Proposed change cannot be evaluated.

Observed result: **coordinator_reconciliation**, confidence **0.99**. Probabilities: coordinator_reconciliation0.99, proven_crate_only0, keep_unverified_backend0, insufficient0.01.
Historical decision: field-deleting proposal required public-contract reconciliation; no external consumer existence was asserted. **Superseded by coordinator-followup.md and R1–R5 below:** the smaller repair retains the public serialized field, rejects true before storage side effects, and needs no external caller migration.

## Reconciliation — source repair and all final contexts

The coordinator's complete `coordinator-followup.md` was read. Same session resumed; all prior full-file reads remain evidence. Before mutation the entire storage source was reread in exact raw ranges1-240,241-480,481-674 and saved under the assignment directory as `encrypted_storage.rs.BEFORE`; write receipt22647bytes/snapshotEB66 matches the pre-edit read. `SECURITY-MODEL.md.BEFORE` receipt1213bytes/D499 likewise matches its complete pre-edit read. No filesystem extraction or arbitrary execution used.

Two additional successful installed declarative judge calls:
- `functions.eval`, **Judging bounded fail-closed repair**, `await judge(repair_evidence, {R1,R2})`.
- `functions.eval`, **Judging every final native context**, `await judge(final_native_evidence, {R3,R4,R5})`.

No tests or runtime effects were claimed by either call. JEV is evidence, not mutation permission or runtime verification.

### R1

Question: Assess whether the smaller fail-closed delta resolves the native encryption attestation defect inside this crate while preserving explicit caller intent and the verified LUKS path. Include contrary and insufficient outcomes. Judge is not permission.

Evidence supplied: current public serde field/defaulttrue; initialize formerly created base then dispatched true to command-success-only native setup and false to exact verified LUKS. Public new returns only after initialize. Proposed first-statement `anyhow::ensure!(!self.config.use_native_encryption, "Native Btrfs encryption is unsupported; explicitly configure verified LUKS storage")`; preserve field/default and false path; delete native setup/sole-use permissions helper; keep statistics capacity error, replace native labels with verified-LUKS invariant; migrate existing wording fixture to absent/existing path rejection; update existing docs.

Choices: bounded_repair (reject true before side effects, preserve false/attestation, remove fake backend without field deletion); weakens_storage (admits unverified encryption or silently converts true); external_contract_blocker (retention still requires identified cross-crate edit); insufficient (gate ordering/invariant unclear).

Observed: **bounded_repair**, confidence **1**; probabilities bounded_repair1, weakens_storage0, external_contract_blocker0, insufficient0.
Decision: implement this smaller same-crate correction; prior field-deletion blocker withdrawn. Default remains deliberately non-operational, not silently changed to LUKS.

### R2

Question: Evaluate wording/default/statistics/fixture semantics, not just routing. Is retaining true/default and native names truthful after unconditional rejection, and do proposed docs/stats/fixture reflect the contract?

Same evidence as R1. Proposed fixture covers absent base and existing plain storage, asserts error plus unchanged base existence and absent master.key, not diagnostic text. Stats remain unavailable through existing capacity error.

Choices: truthful_explicit_rejection (native accurately names rejected legacy request, non-operational default requires explicit false, stats invariant LUKS-only, fixture checks side effects); misleading_capability (still advertises native support); unsupported_fallback (false/stats fabricate success); insufficient.

Observed: **truthful_explicit_rejection**, confidence **0.93**; probabilities truthful_explicit_rejection0.95, misleading_capability0.05, unsupported_fallback0, insufficient0.
Decision: retain accurate rejection/config/fixture vocabulary rather than blanket renaming; explicitly document default failure tradeoff.

### Actual applied evidence supplied to R3–R5

Source patch receipt `encrypted_storage.rs#1A92`,616lines; doc receipt `SECURITY-MODEL.md#99A0`,56lines. Complete prior source plus applied edit receipts and contextual rereads establish all current native contexts. Middle unchanged source is retained prior-read evidence, not newly claimed coverage.

All eight source occurrences, exact current text:

```rust
// encrypted_storage.rs:24-28
    /// Legacy native Btrfs encryption request; true is rejected before storage setup.
    /// Set false explicitly to select verified LUKS storage.
    pub use_native_encryption: bool,
    /// Required LUKS mapper name when the unsupported backend is not requested.
    pub luks_device_name: Option<String>,
// :81
            use_native_encryption: true, // Require explicit selection of verified LUKS storage
// :124-143
    /// Initialize key storage only on a verified LUKS mount.
    async fn initialize(&mut self) -> Result<()> {
        anyhow::ensure!(
            !self.config.use_native_encryption,
            "Native Btrfs encryption is unsupported; explicitly configure verified LUKS storage"
        );
        info!("Setting up verified LUKS storage for WireGuard keys");

        // Ensure base directory exists
        async_fs::create_dir_all(&self.config.base_path).await?;

        self.setup_luks_encryption().await?;

        // Load or generate master key
        self.load_or_generate_master_key().await?;

        self.is_initialized = true;
        info!("Encrypted key storage initialized successfully");
        Ok(())
    }
// :577 — absent-LUKS-mapper fixture
            use_native_encryption: false,
// :588-615 — existing updated regression fixture
    #[tokio::test]
    async fn unsupported_native_encryption_never_initializes_storage() {
        let unique = format!(
            "opdbus-native-unverified-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        for existing_storage in [false, true] {
            let base = std::env::temp_dir().join(format!("{unique}-{existing_storage}"));
            let storage = base.join("wireguard-keys");
            if existing_storage {
                std::fs::create_dir_all(&storage).unwrap();
            }
            let config = EncryptedStorageConfig {
                base_path: base.clone(),
                ..EncryptedStorageConfig::default()
            };
            assert!(EncryptedKeyStorage::new(config).await.is_err());
            assert_eq!(base.exists(), existing_storage);
            assert!(!storage.join("master.key").exists());
            if existing_storage {
                std::fs::remove_dir_all(base).unwrap();
            }
        }
    }
```

Additional exact-call evidence supplied:
- new103-121 logs base, joins path, constructs uninitialized Self without master key, awaits initialize, returns only on success.
- LUKS setup145-179 unchanged except truthful doc; safe mapper-name validation, required present mapper, conditional mount, unconditional verify.
- Verification181-239 unchanged: real nonsymlink directory, block-device mapper, cryptsetup success/type exactlyLUKS1 orLUKS2, findmnt exact mount source block-device rdev match.
- Applied edit deletes native command/function and sole-use set_secure_permissions helper.
- Stats476-504 retain initialized check and existing unconditional capacity error before any response; surviving labels `is_encrypted: true`, `encryption_type: "luks".to_string()` follow private successfully initialized manager invariant.
- wireguard_auth129-145 unchanged: public new defaults config; custom config constructor remains pub(crate); no new public setter or permission bypass introduced.

All three added documentation occurrences, with actual complete section:

```text
42: ## Key storage backend
44: `EncryptedKeyStorage` supports only an explicitly configured, verified LUKS mapper.
45: The public `use_native_encryption` field is a backend request, not encryption proof.
46: Its default remains `true` to avoid silently selecting a different backend.
47: Native Btrfs encryption is unsupported: `true` is rejected before directory
48: creation, subprocess execution, or master-key loading/generation.
49: To opt into supported storage, set `use_native_encryption = false` and specify
50: `luks_device_name`. Provision and open the mapper through the authorized storage
51: owner first; this crate does not provision an absent mapper.
53: The LUKS path requires an active LUKS1/LUKS2 mapper and verifies that the exact
54: storage mount is backed by that block device before loading or generating keys.
55: The default `WireGuardAuthManager::new()` therefore fails closed; callers must
56: explicitly configure supported storage rather than rely on its default.
```

### R3

Question: Evaluate actual context and intent of every current source native occurrence24,26,81,127,128,577,589,591 and disposition of original removed capability/label occurrences. Do not judge just model routing or assume tests ran.

Choices: accurate_rejection_vocabulary (all eight accurately identify rejected legacy request/default, explicit LUKS selection or behavioral fixture; gate before side effects, native implementation removed, false attestation intact, no remaining native correction); native_defect_remains (promises/accepts unverified native support or fallback); external_migration_required (identified required outside patch despite preserved field); insufficient.

Observed: **accurate_rejection_vocabulary**, confidence **0.92**. Probabilities accurate_rejection_vocabulary0.94, native_defect_remains0.04, external_migration_required0.01, insufficient0.01.
Decision: all eight current source occurrences retain accurate meanings; original removed occurrences resolved by deletion/correction. Not proof of executed fixture behavior.

### R4

Question: Evaluate all actual documentation native occurrences45,47,49 together with source, default and caller access. Is wording truthful or does it overclaim encryption? Distinguish unsupported here from universal kernel capability assertion.

Choices: accurate_crate_contract (crate rejection/default/explicit-false/preprovisioned mapper described, no universal kernel proof or new public auth config setter claimed); misleading_docs (promises native/default success/fallback/nonexistent setter); insufficient.

Observed: **accurate_crate_contract**, confidence **1**. Probabilities accurate_crate_contract1, misleading_docs0, insufficient0.
Decision: all three documentation occurrences are accurate contract/rejection vocabulary.

### R5

Question: Is completion properly bounded: every original20 native occurrence dispositioned, all current11 source/doc occurrences examined, F1 repaired with retained serialized request and no needed native change unresolved, tests/build/runtime/deployment explicitly unverified? Evaluate source-review completeness versus runtime success.

Choices: static_complete_only (complete static audit/implemented repair only; earlier blocker superseded); native_work_unfinished (concrete needed correction unresolved); runtime_success_proven (executed checks/deployment establish behavior); insufficient.

Observed: **static_complete_only**, confidence **0.68**. Probabilities static_complete_only0.76, native_work_unfinished0.14, runtime_success_proven0, insufficient0.10.
Decision: scope claim to complete static audit and implemented source correction, never built/tested/deployed success. Completion evidence is the occurrence ledger and actual patch/context receipts; the modest confidence is retained, not converted into runtime certainty. Build/tests remain unperformed under the owner restriction.
