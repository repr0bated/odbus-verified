# Spec 04: `remove-projection-static-tree`

**Spec Path**: [`.kiro/specs/remove-projection-static-tree/requirements.md`](file:///srv/git/odbus/.kiro/specs/remove-projection-static-tree/requirements.md)  
**Domain**: State Persistence & Dynamic SHM  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Eliminate hardcoded disk-backed `/var/lib/opdbus/projection` tree. | Replaced by memory-mapped `/dev/shm/opdbus/` filesystem layout. | **PASS** |
| **REQ-2** | Present-state value authority resides at `/dev/shm/opdbus/state/<plugin>.json`. | [`crates/op-core/src/projection_shm.rs`](file:///srv/git/odbus/crates/op-core/src/projection_shm.rs): Atomic state file updates. | **PASS** |
| **REQ-3** | FUSE projection daemon (`3tchedFS`) reads directly from dynamic SHM instead of disk. | [`/srv/3tchedFS/src/source.rs:16-18`](file:///srv/3tchedFS/src/source.rs#L16-L18): SourceCatalog reads live SHM. | **PASS** |
| **REQ-4** | State updates trigger atomic rename on `/dev/shm/opdbus/state/<plugin>.json.tmp`. | [`crates/op-core/src/projection_shm.rs:45-70`](file:///srv/git/odbus/crates/op-core/src/projection_shm.rs#L45-L70). | **PASS** |
