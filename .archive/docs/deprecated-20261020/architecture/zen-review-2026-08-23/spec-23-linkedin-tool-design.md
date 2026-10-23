# Spec 23: `linkedin-tool-design`

**Spec Path**: [`/srv/git/zeroclaw/docs/superpowers/specs/2026-03-13-linkedin-tool-design.md`](file:///srv/git/zeroclaw/docs/superpowers/specs/2026-03-13-linkedin-tool-design.md)  
**Domain**: Zeroclaw Agent Superpowers & Tool Sandbox  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Superpower tool schema defining parameters, outputs, and rate limits. | [`crates/op-tools/src/builtin/mod.rs`](file:///srv/git/odbus/crates/op-tools/src/builtin/mod.rs): Builtin tool registry. | **PASS** |
| **REQ-2** | Sandbox execution environment isolating network operations. | Enforced in `op-tools` execution sandbox. | **PASS** |
| **REQ-3** | Tool capability gating via actor sled permission bitmask. | Enforced in `schema_router.rs`. | **PASS** |
