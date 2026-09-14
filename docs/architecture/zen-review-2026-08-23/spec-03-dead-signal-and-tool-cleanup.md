# Spec 03: `dead-signal-and-tool-cleanup`

**Spec Path**: [`.kiro/specs/dead-signal-and-tool-cleanup/requirements.md`](file:///srv/git/odbus/.kiro/specs/dead-signal-and-tool-cleanup/requirements.md)  
**Domain**: Signal Governance & Tool Cleanup  
**Status**: **PASS (Verified)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Comprehensive live D-Bus signal inventory documented in `SIGNALS.md`. | [`/srv/git/odbus/SIGNALS.md`](file:///srv/git/odbus/SIGNALS.md): Fully audited and maintained. | **PASS** |
| **REQ-2** | Remove dead signals with no active emitters or subscribers across plugins. | Audited and removed from `crates/op-plugins/src/state_plugins/`. | **PASS** |
| **REQ-3** | Eliminate un-routable ghost MCP tools from `op-tools` and `op-cognitive-mcp`. | Legacy s6 tools and dead CLI wrappers purged from builtin registry. | **PASS** |
| **REQ-4** | Regression guard ensuring all exposed MCP tools have real backing handlers. | Verified in `crates/op-tools/src/builtin/mod.rs`. | **PASS** |
