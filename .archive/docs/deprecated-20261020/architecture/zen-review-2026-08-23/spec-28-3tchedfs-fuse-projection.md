# Spec 28: `3tchedFS` FUSE Projection

**Spec Path**: [`/srv/3tchedFS/README.md`](file:///srv/3tchedFS/README.md)  
**Domain**: Virtual FUSE Filesystem & Dual-SHM Authority  
**Status**: **PASS (Verified & 9/9 Tests Passing)**

---

## Requirements Verification Matrix

| Requirement | Requirement Statement | Code Implementation | Status |
|---|---|---|:---:|
| **REQ-1** | Dual SHM: Schema authority read from sealed `OPBLOB01` blobs; value authority read from live present-state SHM. | [`/srv/3tchedFS/src/source.rs:16-125`](file:///srv/3tchedFS/src/source.rs#L16-L125): Reads `/dev/shm/opdbus/plugin-blobs` & `/state`. | **PASS** |
| **REQ-2** | Pinned view mounts serve leaf scalar files under `data/` live from SHM snapshot on `open()`. | [`/srv/3tchedFS/src/fuse_fs.rs:65-85`](file:///srv/3tchedFS/src/fuse_fs.rs#L65-L85): `NodeKind::LiveFile` snapshot on open. | **PASS** |
| **REQ-3** | Sparse copy-on-write (COW) workspaces validate staged writes against JSON Schema before committing. | [`/srv/3tchedFS/src/store.rs`](file:///srv/3tchedFS/src/store.rs) & `src/model.rs`: Full schema validation on write. | **PASS** |
| **REQ-4** | Controlled D-Bus dispatch (`threetched-fs call`) requires `--confirm-side-effects` for mutating methods. | [`/srv/3tchedFS/src/dispatch.rs:52-57`](file:///srv/3tchedFS/src/dispatch.rs#L52-L57): Enforces side-effect confirmation. | **PASS** |
| **REQ-5** | Service supervised under runit at `/run/mount/3tchedFS` with `--auto-unmount` and `--allow-other`. | [`/etc/runit/sv/threetched-fs/run:48-52`](file:///etc/runit/sv/threetched-fs/run#L48-L52): Active production run script. | **PASS** |
| **REQ-6** | Automated test suite verifies capture, inspection, schema validation, and dispatch. | `cargo test` in `/srv/3tchedFS` passes **9/9 tests**. | **PASS** |
