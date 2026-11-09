# Tasks — 3tchedFS Mount Consolidation

**Status:** SPECIFICATION ONLY (revision 0). Dependency-ordered implementation tasks.
Nothing implemented/built/deployed/tested. Repo policy: implement → build → deploy;
tests, Clippy, fmt, `/verify`, parity and behavioral verification run only after
deployment and on explicit owner request. Deployment is `./activate-golden.sh`
(btrfs send/receive); reseal (`opblob seal-shm` + `persist`) is owner-run.
Spec id: `3tched-fs-mount-consolidation`.

**Legend:** `[B]` backend `/srv/git/odbus-verified`, `[U]` UI `/srv/git/3tched.com`.
Each task: files · contract · acceptance evidence. Maintain per task when work begins:
`implemented: no | built: no | deployed: no | tested: no`.

---

## Phase 0 — Investigation / prep (no code changes)

### T0. Reviewer gate (satisfied for Phase 0)
**Requirements:** R1–R9, design §0.
- Read requirements.md, design.md, and this tasks.md.
- Verify the design's four-affected-file scope: op-plugins (schema), _3tched-fs (binary), deploy (helper), namespace.rs (inodes).
- Confirm no scope expansion beyond filesystem projection layer.
- This review is not deployment, test, or identity-change authorization.

### T0.1 Verify source baselines
**Requirements:** R1, R4, R6.
- Confirm `fuse.rs` (8,046 lines) contains duplicate contract types at lines 7692–7808.
- Confirm `_3tched_fs.rs` (561 lines) contains identical types at lines 26–80.
- Confirm `_3tched_fs_contract.rs` (16,102 lines) is the stable contract document.
- Confirm `mount-3tchedfs` line 86 checks `_3tched_fs`.
- Confirm `main.rs` Mount subcommand has no `--surface` argument.
- Confirm `namespace.rs:1234` has `inode_for_path(path)` without surface prefix.
- Evidence: `wc -l` and `grep` outputs on each file.

---

## Phase 1 — Shared contract module (R4, R2, R6)

### T1. Create `crates/op-plugins/src/state_plugins/filesystem_contract.rs`
**Requirements:** R4.1, R4.4, R6.1
**Files:** new file at `crates/op-plugins/src/state_plugins/filesystem_contract.rs`
**Contract:** Extract contract types from both `fuse.rs` and `_3tched_fs.rs` into one shared module.

**Implementation details:**
1. Extract these types from both files (identical implementations):
   - `ContractDescriptor` (fuse.rs:7692–7696, _3tched_fs.rs:26–30)
   - `FilesystemContract` (fuse.rs:7699–7725, _3tched_fs.rs:35–71)
   - `literal_contract_schema` / `literal_schema` helper (fuse.rs:7728–7754, _3tched_fs.rs:74–100)
   - `Invocation` (fuse.rs:7765–7773, _3tched_fs.rs:102–115)
   - `FilesystemProtocol` (fuse.rs:7776–7808, _3tched_fs.rs:117–159)
   - `GetContractOutput`, `GetProtocolInput` (fuse.rs:7756–7760, 7810–7812)
   - `FilesystemProtocol::default()` impl (fuse.rs:7787–7808)
2. Add `pub use _3tched_fs_contract::{document, schema_version, source_commit};` re-exports.
3. Keep the existing `_3tched_fs_contract` module as-is (the contract document itself).

**Acceptance evidence:**
- `cargo check -p op-plugins` passes.
- The shared file compiles with no unused imports or dead code.
- Both `fuse.rs` and `_3tched_fs.rs` can reference these types via `use crate::state_plugins::filesystem_contract::*;`.

---

### T2. Rename `_3tched_fs.rs` → `threetched_fs.rs` and update references
**Requirements:** R2.1, R2.5, R6.1, R6.2
**Files:** `crates/op-plugins/src/state_plugins/threetched_fs.rs` (renamed from `_3tched_fs.rs`), `crates/op-plugins/src/state_plugins/mod.rs`

**Implementation details:**
1. Rename `crates/op-plugins/src/state_plugins/_3tched_fs.rs` → `threetched_fs.rs`.
2. Update `mod.rs` line 104: `pub mod _3tched_fs;` → `pub mod threetched_fs;`.
3. In `threetched_fs.rs`:
   - Line 17: `pub const PLUGIN_NAME: &str = "_3tched_fs";` → `pub const PLUGIN_NAME: &str = "threetched_fs";`
   - Line 173: `impl StatePlugin for ThreeTchedFsPlugin` → `ThreetchedFsPlugin`
   - Line 181: `_3tched_fs_schema()` → `threetched_fs_schema()`
   - All test references: `3tched_fs_schema` → `threetched_fs_schema`
   - All test references: `_3tched_fs` → `threetched_fs`
   - Add `use crate::state_plugins::filesystem_contract::*;` to import shared types.
   - Remove inline contract type definitions (they now come from shared module).
   - Keep `_3tched_fs_contract` import (the contract document is stable).
4. Update any downstream references: `default_registry.rs`, any code that calls `_3tched_fs_schema()`.

**Acceptance evidence:**
- `cargo check -p op-plugins` passes.
- `grep -r "_3tched_fs"` in `state_plugins/` shows only `mod _3tched_fs_contract` (the contract doc, preserved).
- `grep -r "threetched_fs"` shows the renamed module and all references use the new name.

---

### T3. Remove duplicate contract types from `fuse.rs`
**Requirements:** R2.5, R4.2, R4.3
**Files:** `crates/op-plugins/src/state_plugins/fuse.rs`

**Implementation details:**
1. Lines 7692–7808: Remove the duplicate contract type definitions (`ContractDescriptor`, `FilesystemContract`, `Invocation`, `FilesystemProtocol`, `GetContractOutput`, `GetProtocolInput`, `FilesystemProtocol::default()`).
2. Replace with: `use crate::state_plugins::filesystem_contract::*;`
3. Lines 8027–8037: Remove `dispatch_fs_projection()` — it duplicates the `threetched_fs` plugin's `get_contract`/`get_protocol` methods.
4. The `fuse.rs` plugin schema (`fuse_schema()`) will no longer include filesystem contract methods. Update the method list in the schema generation to exclude them.

**Acceptance evidence:**
- `cargo check -p op-plugins` passes.
- `fuse.rs` no longer contains `ContractDescriptor`, `FilesystemContract`, `Invocation`, or `FilesystemProtocol` definitions (only `use` import).
- `grep -c "ContractDescriptor\|FilesystemContract" fuse.rs` returns the count of references (should be 1 per type for the `use` import, not the definitions).

---

### T4. Update `fuse.rs` schema to remove filesystem contract methods
**Requirements:** R4.4
**Files:** `crates/op-plugins/src/state_plugins/fuse.rs`

**Implementation details:**
1. Review `fuse_schema()` (line 7903) and `fuse_schema_json()` (line 7940).
2. Remove `get_contract` and `get_protocol` method declarations from the fuse plugin schema.
3. These methods are now only on the `threetched_fs` plugin.
4. Ensure `add_method::<GetContractInput, GetContractOutput>(...)` and
   `add_method::<GetProtocolInput, FilesystemProtocol>(...)` calls are removed.

**Acceptance evidence:**
- The `fuse` plugin schema no longer contains `get_contract` or `get_protocol`.
- The `threetched_fs` plugin schema does contain both methods.
- `cargo check -p op-plugins` passes.

---

## Phase 2 — Binary `--surface` flag (R1, R5)

### T5. Add `--surface` CLI flag to `_3tched-fs` binary
**Requirements:** R1.1–R1.4, R5.1–R5.4
**Files:** `crates/_3tched-fs/src/main.rs`

**Implementation details:**
1. Add to global CLI struct (near `store` arg):
   ```rust
   #[arg(long)]
   surface: Option<String>,
   ```
2. In the `Mount` subcommand handler, validate `--surface`:
   - If `None`: error with usage message listing valid surfaces (`3tched_fs`, `threetched_fs`).
   - If `Some(val)`:
     - If `val == "3tched_fs"`: set projection to exclude threetched, FUSE subtype to `3tched_fs`.
     - If `val == "threetched_fs"`: set projection to include only threetched, FUSE subtype to `threetched_fs`.
     - Else: error with "unrecognized surface: {val}".
3. Add a `--surface` argument to the `Capture` subcommand as well (needed by mount helper).
4. Update namespace building to use the surface name for the FUSE subtype.

**Acceptance evidence:**
- `cargo build --release -p _3tched-fs` produces a binary that accepts `--surface 3tched_fs` and `--surface threetched_fs`.
- `--surface foo` produces an error: "unrecognized surface: foo".
- Missing `--surface` produces an error listing valid surfaces.

---

### T6. Update inode computation to include surface name (R8)
**Requirements:** R8.1–R8.5
**Files:** `crates/_3tched-fs/src/namespace.rs`

**Implementation details:**
1. Modify `inode_for_path()` (line 1234):
   ```rust
   // Before:
   fn inode_for_path(path: &str) -> u64 {
       let bytes = blake3::hash(path.as_bytes()).as_bytes()[..8]
           .try_into().expect("eight-byte slice");
       let mut ino = u64::from_le_bytes(bytes) | (1 << 63);
       if ino == ROOT_INO { ino += 1; }
       ino
   }
   
   // After:
   fn inode_for_path(surface: &str, path: &str) -> u64 {
       let input = format!("{surface}{path}");
       let bytes = blake3::hash(input.as_bytes()).as_bytes()[..8]
           .try_into().expect("eight-byte slice");
       let mut ino = u64::from_le_bytes(bytes) | (1 << 63);
       if ino == ROOT_INO { ino += 1; }
       ino
   }
   ```
2. Update all callers of `inode_for_path()` to pass the surface name as first argument.
   - Surface name is available from the Namespace's mount configuration or from the caller.
3. The ROOT_INO collision check and high-bit setting are preserved.
4. Inodes are still computed on-the-fly (no table).

**Acceptance evidence:**
- `cargo test -p _3tched-fs --lib` passes.
- Paths on `3tched_fs` and `threetched_fs` with the same name produce different inodes.
- Same path on same surface produces the same inode across restarts.

---

## Phase 3 — Mount helper updates (R3, R6)

### T7. Update `mount-3tchedfs` for `--surface` and renamed plugin
**Requirements:** R3.1–R3.5, R6.1, R6.2
**Files:** `deploy/runit/libexec-3tched/mount-3tchedfs`

**Implementation details:**
1. Update the mount source detection block (lines 31–33):
   ```sh
   case "$fs_name:$mount_path" in
       3tchedFS:/run/mounts/3tchedFS) surface=3tched_fs; selection=--exclude-plugin ;;
       threetched:/run/mounts/threetched) surface=threetched_fs; selection=--include-plugin ;;
       *) fail 'unexpected mount source or target' ;;
   esac
   ```
   Previously: `selection=--exclude-plugin` / `selection=--include-plugin` only.

2. Update binary invocation (lines 109–110):
   ```sh
   "$binary" --store "$store" capture --schema-only --catalog "$catalog" \
       --state /dev/shm/opdbus/state --surface "$surface" \
       "$selection" "$upstream_plugins" >"$runtime/capture.json"
   ```
   Previously: no `--surface` flag.

3. Update mount invocation (lines 111–116):
   ```sh
   nohup setsid "$binary" --store "$store" mount \
       --surface "$surface" \
       --catalog /dev/shm/opdbus/plugin-blobs --state /dev/shm/opdbus/state \
       --allow-other --auto-unmount --call-owner-uid "$owner_uid" \
       --lungs-socket /run/opdbus/lungs/lungs.sock --lungs-session "$session" \
       --fs-name "$fs_name" \
       "$mount_path" >"$runtime/daemon.log" 2>&1 </dev/null 9>&- &
   ```
   Previously: no `--surface` flag, no `--fs-name` flag.

4. Update readiness check (lines 79–89):
   ```sh
   timeout 3 jq -e --arg name "$surface" --arg upstream "$upstream_plugins" \
       --slurpfile catalog "$catalog/.manifest.json" '
       ($upstream | split(",") | sort) as $upstream |
       (.objects | keys | sort) as $actual |
       (.source_catalog_hash == $catalog[0].catalog_hash and
        .source_catalog_generation == $catalog[0].generation) and
       (if $name == "threetched_fs" then $actual == $upstream
       else (.objects | has("threetched_fs")) and
           ($actual == (($catalog[0].plugins | keys | sort) - $upstream))
       end)
   ' "$mount_path/view.json" >/dev/null
   ```
   Previously: checked `_3tched_fs` (line 86), used `threetched` (line 85).

5. Update line 72: `[ "$(mounted_kind)" = fuse.3tchedFS ]` — verify this fstype check
   is still valid for both surfaces.

**Acceptance evidence:**
- The helper exits 0 for both valid fstab entries.
- The helper exits non-zero for unrecognized mount source/target.
- The readiness check looks for `threetched_fs` in view.json, not `_3tched_fs`.

---

### T8. Update fstab and runit staging files
**Requirements:** R7.3, R6.3
**Files:** `deploy/workspace-image/3tchedfs.fstab`, `deploy/host-fuse/3tchedfs.fstab`,
`/usr/local/libexec/op-dbus/helpers/3tchedfs.fstab`, `deploy/runit/3tched-fs/run`

**Implementation details:**
1. Update fstab entries if they reference `_3tched_fs` (unlikely — fstab uses mount
   source names, not plugin IDs).
2. Update runit run script `deploy/runit/3tched-fs/run` to pass `--surface` if it
   invokes the binary directly.
3. Ensure all staging files use consistent surface naming (`3tched_fs`, `threetched_fs`).

**Acceptance evidence:**
- All staging files pass `grep -q "3tched_fs\|threetched_fs"` or equivalent.
- No staging file references `_3tched_fs` as a plugin ID.

---

## Phase 4 — Verification / integration

### T9. Cross-repo reference audit
**Requirements:** R6.1, R6.2, R6.3
**Files:** All `.rs`, `.sh`, `.json`, `.md` files in `/srv/git/odbus-verified`

**Implementation details:**
1. `grep -rn "_3tched_fs" --include="*.rs" --include="*.sh" --include="*.json" --include="*.md"` 
   across the repo.
2. Replace each reference with `threetched_fs` where appropriate (plugin name references).
3. Preserve `_3tched_fs_contract` references (the contract document module is stable).
4. Document any references that must be updated outside this repo (as per R6.4).

**Acceptance evidence:**
- `grep -c "_3tched_fs"` returns only references to `_3tched_fs_contract` and historical docs.
- No other references to `_3tched_fs` as a plugin name remain.

---

### T10. Build and cargo check
**Requirements:** R7.1, R7.2
**Files:** All affected crates

**Implementation details:**
1. `CXXFLAGS="-include cstdint" cargo build --release -p op-plugins`
2. `CXXFLAGS="-include cstdint" cargo build --release -p _3tched-fs`
3. `cargo check --workspace` to verify no transitive breakage.
4. `cargo test -p op-plugins` — library tests only (integration tests deferred per owner policy).
5. `cargo test -p _3tched-fs` — library tests only.

**Acceptance evidence:**
- All builds pass.
- Library tests pass.
- No new Clippy warnings in affected crates.
- `cargo fmt --all -- --check` passes.

---

### T11. Smoke test both surfaces
**Requirements:** R5, R8, R9
**Files:** New or modified scripts in `hackathon/tools/` or `deploy/`

**Implementation details:**
1. Write a smoke test script that:
   a. Builds the binary with a test catalog.
   b. Mounts `3tched_fs` surface and verifies:
      - view.json contains `threetched_fs` key
      - FUSE readdir returns expected plugin files
      - `get_contract` and `get_protocol` methods are accessible
   c. Mounts `threetched_fs` surface and verifies:
      - view.json contains only `["threetched"]` in objects
      - Only threetched plugin methods are accessible
      - Inodes for same path differ between surfaces
   d. Verifies inode stability across mount restarts.
2. Test error paths:
   a. Missing `--surface` → error
   b. Invalid `--surface` → error
   c. Surface without catalog → error

**Acceptance evidence:**
- Both surfaces mount and respond to FUSE operations.
- Inodes are stable and unique across surfaces.
- Error paths produce clear error messages.

---

### T12. SHM representation gap analysis (R9.3)
**Requirements:** R9.3, R9.6
**Files:** `crates/_3tched-fs/src/namespace.rs`, `crates/_3tched-fs/src/store.rs`

**Implementation details:**
1. Audit all `FileSource` variants in `namespace.rs` to verify coverage for both surfaces.
2. Document explicitly excluded representations (frozen workspace values on DirectShm,
   catalog blobs, generation metadata).
3. Ensure DirectShm returns ENODATA/ENOENT for unavailable representations rather than
   silently omitting paths.
4. Verify that `namespace.rs:1155-1157` ("direct SHM does not expose frozen values")
   still applies post-consolidation.

**Acceptance evidence:**
- All known SHM gaps documented in requirements.md (R9.2, § "Explicitly known SHM gaps").
- DirectShm error paths verified via test or code review.

---

## Phase 5 — Finalization

### T13. Update spec files
**Requirements:** All
**Files:** `requirements.md`, `design.md`, `tasks.md`

**Implementation details:**
1. Update requirement traceability: mark each R# as satisfied by specific tasks.
2. Update design.md to reflect any implementation decisions discovered during tasks.
3. Record any new gaps or exclusions discovered during implementation.

**Acceptance evidence:**
- All three spec files are internally consistent.
- No contradictions between requirements and design.

### T14. Final audit
**Requirements:** R1–R9, R7.3
**Files:** All files modified during this spec

**Implementation details:**
1. Verify no duplicate contract definitions remain in compiled binary.
2. Verify all plugin references use `threetched_fs` (not `_3tched_fs`, `tched_fs`, etc.).
3. Verify mount helper readiness check uses `threetched_fs`.
4. Verify inode computation includes surface prefix.
5. Verify both surfaces pass the same FUSE operation tests.

**Acceptance evidence:**
- `grep -rn "_3tched_fs" --include="*.rs" --include="*.sh"` shows only `_3tched_fs_contract` and historical docs.
- `cargo build --release` succeeds for both affected crates.
- Mount helper script runs clean for both fstab entries.

---

## Dependency Graph

```
T0 → T0.1 → T1 → T2 → T3 → T4 → T5 → T6 → T7 → T8 → T9 → T10 → T11 → T12 → T13 → T14
```

**Parallelism opportunities:**
- T1 (shared contract module) can be done independently.
- T3 (remove from fuse.rs) depends on T1.
- T2 (rename module) can proceed in parallel with T1, T3, T4.
- T5 (CLI flag) and T6 (inodes) are independent of each other and of the op-plugins changes.
- T7 (mount helper) depends on T5 (binary accepts --surface).
- T8 (staging files) depends on T7.
- T10 (build/check) depends on T1–T7.
- T11 (smoke test) depends on T10.

**Critical path:** T0.1 → T1 → T3 → T4 → T5 → T6 → T7 → T10 → T11

---

## Requirement Traceability

| Requirement | Tasks | Design Section |
|------------|-------|----------------|
| R1 | T5 | §1.2 |
| R2 | T2, T3, T4, T9 | §1.1, §1.3, §1.4, §1.7 |
| R3 | T7, T8 | §1.3 |
| R4 | T1, T3, T4 | §1.1 |
| R5 | T5, T7 | §1.2, §1.3 |
| R6 | T2, T3, T4, T7, T8, T9 | §1.1–§1.4, §1.8 |
| R7 | T10, T11, T14 | §7, §10–§12 |
| R8 | T6, T11 | §1.4, §3.3 |
| R9 | T11, T12 | §3, §1.2 |
