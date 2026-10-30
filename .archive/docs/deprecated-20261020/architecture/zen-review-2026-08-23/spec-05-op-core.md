# Spec 05: `op-core.md`

**Spec Path**: [`docs/specs/op-core.md`](file:///srv/git/odbus/docs/specs/op-core.md)  
**Domain**: Core Primitives, Locking & Concurrency  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Core SHM locking primitives using advisory file locks (`fs4` / `FileExt`). | [`crates/op-core/src/lib.rs`](file:///srv/git/odbus/crates/op-core/src/lib.rs): Implements exclusive and shared lock helpers. | **PASS** |
| **REQ-2** | Atomic file publication standard using `.tmp` write and atomic `std::fs::rename`. | [`crates/op-core/src/projection_shm.rs:45-70`](file:///srv/git/odbus/crates/op-core/src/projection_shm.rs#L45-L70). | **PASS** |
| **REQ-3** | Shared memory segment generation counters for zero-copy change detection. | Segment header includes monotonic generation counter. | **PASS** |
| **REQ-4** | Error handling returns typed `OpError` across storage and IPC boundaries. | Defined in `crates/op-core/src/error.rs`. | **PASS** |
