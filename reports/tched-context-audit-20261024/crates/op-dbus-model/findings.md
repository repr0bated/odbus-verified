# Findings — op-dbus-model tched audit

1. **No tched-specific issue identified.** All five inventory files (390 lines) were directly read through EOF. No case-insensitive `tched` occurrence was found. No source change is warranted by this assignment.
2. **Separate, out-of-scope documentation inconsistency.** `SPEC.md:11,21-24,79-90,92-110,114-125` documents SQLite/sqlx schema persistence and introspection discovery, while `Cargo.toml:6-15` has no sqlx dependency and `src/lib.rs:16-66` implements a process-local `Arc<RwLock<HashMap<...>>>` catalog. This is not tched-specific and was not changed. `compare-op-dbus-model.md` also records implementation/dependency inventory differences; no generic cleanup was authorized.
3. **Authority limits.** `src/models.rs:23-47` describes `PluginCatalogDocument` as a canonical persisted plugin document and identifies `PluginSchema` as plugin-owned schema. These source contracts do not prove sealed-catalog membership, runtime availability, or deployment state. No such claim is made.

No unresolved tched-specific blocker or cross-crate proposal.
