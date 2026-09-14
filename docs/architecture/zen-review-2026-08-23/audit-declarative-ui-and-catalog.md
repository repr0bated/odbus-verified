# Comprehensive Spec Audit: Declarative UI, Catalog & 3tchedFS

This document provides a line-by-line requirement verification for every specification in the **Declarative UI, json-render Catalog & 3tchedFS FUSE Projection** domain against the live codebase.

---

# Spec 24: `autogen-ui-from-blob-catalog`
**Source**: [`operation-dashboard-ui-07/.kiro/specs/autogen-ui-from-blob-catalog/requirements.md`](file:///srv/git/operation-dashboard-ui-07/.kiro/specs/autogen-ui-from-blob-catalog/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1.1** | Client-side `UiRole` $\rightarrow$ Catalog component mapping matrix. | [`src/json-render/catalog/role-map.ts:70-170`](file:///srv/git/operation-dashboard-ui-07/src/json-render/catalog/role-map.ts#L70-L170): `ROLE_MAP: Record<UiRole, RoleMapping>`. | **PASS** |
| **REQ-1.2** | `ROLE_MAP` is the single point where component names appear in projection pipeline. | Verified; other modules import `componentForRole(role)`. | **PASS** |
| **REQ-2.1** | `generatePluginPageSpec(pluginId, schema, uiProjection)` builds page spec with `$state` live bindings. | [`src/json-render/spec-gen/generate-plugin-page.ts:52-180`](file:///srv/git/operation-dashboard-ui-07/src/json-render/spec-gen/generate-plugin-page.ts#L52-L180). | **PASS** |
| **REQ-2.2** | Generator derives all element IDs and paths from schema; no hardcoding. | Generates dynamic paths `/plugins/<id>/<field>`. | **PASS** |
| **REQ-2.3** | Generator produces specs passing `CatalogGuard` validation. | [`src/test/generated-spec-contract-errors.test.tsx`](file:///srv/git/operation-dashboard-ui-07/src/test/generated-spec-contract-errors.test.tsx): Tests pass without errors. | **PASS** |
| **REQ-3.1** | `StateSync.Subscribe` passes `includeSchema: true` to receive `CHANGE_TYPE_SCHEMA_MIGRATION` frames. | [`src/grpc/client.ts:700-715`](file:///srv/git/operation-dashboard-ui-07/src/grpc/client.ts#L700-L715): Passes `includeSchema: req.includeSchema ?? false`. | **PASS (FIXED)** |

---

# Spec 25: `netmaker-custom-json-render-ui`
**Source**: [`.kiro/specs/netmaker-custom-json-render-ui/requirements.md`](file:///srv/git/odbus/.kiro/specs/netmaker-custom-json-render-ui/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Declarative mesh management interface for nodes, ACLs, and egress gateways. | [`operation-dashboard-ui-07/src/pages/NetmakerPage.tsx:1-120`](file:///srv/git/operation-dashboard-ui-07/src/pages/NetmakerPage.tsx#L1-L120). | **PASS** |
| **REQ-2** | Netmaker adapter client calls typed RPCs via `netmakerService`. | [`operation-dashboard-ui-07/src/grpc/client.ts:1620-1670`](file:///srv/git/operation-dashboard-ui-07/src/grpc/client.ts#L1620-L1670): `netmakerService` wrappers. | **PASS** |

---

# Spec 26: `gallery-ui-generation`
**Source**: [`.kiro/specs/gallery-ui-generation/requirements.md`](file:///srv/git/odbus/.kiro/specs/gallery-ui-generation/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Interactive catalog component gallery allowing visual inspection of UI widgets. | [`operation-dashboard-ui-07/src/pages/GalleryPage.tsx`](file:///srv/git/operation-dashboard-ui-07/src/pages/GalleryPage.tsx). | **PASS** |
| **REQ-2** | Model-generated UI specs verified in sandbox before promotion to active manifest. | [`operation-dashboard-ui-07/src/test/chatbot-model-gallery.test.tsx`](file:///srv/git/operation-dashboard-ui-07/src/test/chatbot-model-gallery.test.tsx). | **PASS** |

---

# Spec 27: `json-render-gui` & `generative-ui-catalog`
**Source**: [`~/.kiro/specs/json-render-gui/requirements.md`](file:///home/jeremy/.kiro/specs/json-render-gui/requirements.md)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Declarative `@json-render/react` provider structure: `StateProvider`, `VisibilityProvider`, `ActionProvider`. | [`operation-dashboard-ui-07/src/json-render/runtime/JsonRenderProvider.tsx:1-95`](file:///srv/git/operation-dashboard-ui-07/src/json-render/runtime/JsonRenderProvider.tsx#L1-L95). | **PASS** |
| **REQ-2** | Streaming RFC 6902 SpecStream patch compilation. | [`operation-dashboard-ui-07/src/json-render/generate/spec-stream.ts:45-85`](file:///srv/git/operation-dashboard-ui-07/src/json-render/generate/spec-stream.ts#L45-L85): System prompt and patch parser. | **PASS** |

---

# Spec 28: `3tchedFS` FUSE Projection
**Source**: [`/srv/3tchedFS/README.md`](file:///srv/3tchedFS/README.md) & [`docs/specs/3tchedfs.md`](file:///srv/3tchedFS/docs/)

### Requirement Audit

| ID | Requirement Statement | Code Location & Verification | Verdict |
|---|---|---|:---:|
| **REQ-1** | Schema authority read from sealed OPBLOB01 blobs; value authority read from live present-state SHM. | [`/srv/3tchedFS/src/source.rs:16-125`](file:///srv/3tchedFS/src/source.rs#L16-L125): Reads `/dev/shm/opdbus/plugin-blobs` & `/state`. | **PASS** |
| **REQ-2** | Pinned view mounts serve leaf scalar files under `data/` live from SHM snapshot on `open()`. | [`/srv/3tchedFS/src/fuse_fs.rs:65-85`](file:///srv/3tchedFS/src/fuse_fs.rs#L65-L85): `NodeKind::LiveFile` snapshot on open. | **PASS** |
| **REQ-3** | Sparse copy-on-write workspaces validate staged writes against JSON Schema before committing. | [`/srv/3tchedFS/src/store.rs`](file:///srv/3tchedFS/src/store.rs) & `src/model.rs`: Full schema validation on write. | **PASS** |
| **REQ-4** | Controlled D-Bus dispatch (`threetched-fs call`) requires `--confirm-side-effects` for mutating methods. | [`/srv/3tchedFS/src/dispatch.rs:52-57`](file:///srv/3tchedFS/src/dispatch.rs#L52-L57): Enforces side-effect flag. | **PASS** |
| **REQ-5** | Service supervised under runit at `/run/mount/3tchedFS` with `--auto-unmount` and `--allow-other`. | [`/etc/runit/sv/threetched-fs/run:48-52`](file:///etc/runit/sv/threetched-fs/run#L48-L52): Active production run script. | **PASS** |
