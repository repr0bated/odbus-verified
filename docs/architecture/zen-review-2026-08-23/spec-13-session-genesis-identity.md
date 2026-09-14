# Spec 13: `session-genesis-identity`

**Spec Path**: [`.kiro/specs/session-genesis-identity/requirements.md`](file:///srv/git/odbus/.kiro/specs/session-genesis-identity/requirements.md)  
**Domain**: Identity Sleds, Capability Grants & Genesis  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Initial operator connection creates immutable session genesis record in ledger. | [`crates/op-identity/src/anna_scribe.rs:1-90`](file:///srv/git/odbus/crates/op-identity/src/anna_scribe.rs#L1-L90): `write_session_genesis()`. | **PASS** |
| **REQ-2** | Permissions mapped from declarative capability grants file. | [`deploy/security/capability-grants.json`](file:///srv/git/odbus/deploy/security/capability-grants.json): Loaded at startup. | **PASS** |
| **REQ-3** | Identity sled persisted to `/dev/shm/opdbus/identity_sled.dat` with atomic 152-byte mmap. | [`crates/op-identity/src/lib.rs:1-70`](file:///srv/git/odbus/crates/op-identity/src/lib.rs#L1-L70). | **PASS** |
| **REQ-4** | Sled includes actor pubkey, capabilities bitmask, and creation timestamp. | Memory layout verified in `crates/op-identity/src/lib.rs`. | **PASS** |
