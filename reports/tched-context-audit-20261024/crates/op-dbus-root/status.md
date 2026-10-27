# Root/workspace tched audit — status

Task: `tched-context-audit-20261024` · scope: only the five files in the supplied root inventory; no member-crate sources inspected.

Status: **complete**. All five files were read sequentially from line 1 through EOF before disposition. Five contextual `tched` matches were identified and judged. One stale manifest-comment block was removed; four Cargo.lock package/dependency identifiers were retained as the documented `tched-fs` Cargo package for 3tchedFS. No runtime behavior change.

Files reviewed: `Cargo.lock` (9166 lines), `Cargo.toml` (284), `crates/bun.lock` (356), `crates/package-lock.json` (2094), `crates/package.json` (5). Total: 11,905 lines. See `coverage.tsv` for inventory receipts and disposition.

Judgments: JEV-1 rated the Cargo.toml ZeroClaw/tched_router claims stale/misleading (confidence 0.91; probabilities: stale 0.94, accurate 0.01, insufficient 0.05). JEV-2 rated all four Cargo.lock `tched-fs` package/dependency references expected filesystem Cargo identifiers (confidence 0.86; probabilities: expected 0.91, stale 0.06, insufficient 0.03). Human disposition follows these results and the current root owner naming/schema-capture guidance; lock entries do not establish runtime use.

Change: removed Cargo.toml lines 54–57 as a stale/misleading comment block. An exact affected-hunk BEFORE copy is at `/var/tmp/tched-context-audit-20261024/BEFORE/Cargo.toml.comments.txt`; it is explicitly partial, not a byte-exact full-file copy. The edit tool confirmed the dependency table now proceeds from `# Internal Crates` directly to `op-core`.

Verification and limitations: static review only. Build, Cargo checks/tests, custom smoke, runtime, deployment, sealing, grant, credential, commit, and staging actions were not performed; per assignment these are prohibited and coordinator-owned. No runtime/deployment claim is made. No unresolved in-scope finding.
