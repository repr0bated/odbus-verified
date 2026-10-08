# Documentation Maintenance

## Source-backed codebase learning

The onboarding artifacts are the [System Overview](../architecture/overview.md),
[Crate Map](../architecture/crate-map.md), and
[workspace dependency graph](../architecture/workspace-graph.json), followed by
the [Plugin Execution Baseline](../architecture/plugin-baseline.md) and
[tched_router trace](../architecture/tched-router-baseline.md). They were
created with three independent source readers and a coordinating integration
pass, following the general dependency-mapping/shared-context workflow publicly
described by a local, inspectable workflow. The process maps source evidence
without relying on a hosted onboarding platform.

The graph is a declared Cargo dependency graph. It is not a complete symbol graph
or a service inventory. The narrative maps plugin, binary, transport and storage
relationships which Cargo cannot express. Future sessions must read these files
and revalidate affected source; the artifacts do not refresh themselves.

### Repeat the learning pass

1. Read `AGENTS.md`, all nine mandatory skills, and recent relevant `SIGNALS.md`
   rows. Record owner corrections as constraints. **Source that violates an
   explicit invariant is a defect; do not rewrite the invariant to match it.**
   In particular, the plugin IS the schema and its sealed blob IS the plugin;
   restoring a separate plugin `schemas/` tree is a defect, not accepted authority.
   The bridge module named `schema_loader` reads manifest-pinned sealed blobs;
   it is not permission to add a JSON-file schema fallback.
2. Record `git rev-parse HEAD` and `git status --short`; preserve existing work.
   Inventory workspace membership with Cargo metadata. Exclude archived copies
   from primary source evidence. Account for external binary-backed plugins
   and the separate UI explicitly.
3. When the user authorizes subagents, divide work by architectural boundaries.
   Assess difficulty and select the cheapest suitable model and reasoning effort
   for each bounded task, as required by the owner's `AGENTS.md` preference.
   Routine inventory, documentation and verification use the low-cost worker;
   escalate only for a specific difficulty. Keep the coordinator available for
   user communication. This pass used plugin/runtime, identity/state, and
   build/deployment readers, followed by per-plugin readers and cross-checks.
   Each child repeats mandatory preload and returns facts with source path,
   symbol, line, evidence type, and uncertainties. Readers make no runtime
   changes. The coordinator handles cross-boundary questions and consolidation.
   Every source registration must appear once, including retired and uncompiled
   declarations. When enumerating dispatch coverage, include guarded match arms
   such as Incus's two method-specific cases.
4. Trace representative requests through admission, validation, dispatch,
   persistence and response encoding. Ask readers to check disagreements.
   Distinguish **owner intent**, **observed source**, **inference**, **test
   evidence**, and **live evidence**. An interface name or success envelope
   alone does not prove backend behavior.
5. Update the existing overview and crate map. Put observations in `SIGNALS.md`
   and actionable gaps in `WISHLIST.md`; do not create a competing task board.
   Preserve limitations and uninspected areas rather than claiming total
   understanding.
6. Validate changed links and graph counts. Run the repository `/verify`
   equivalent below and report its actual outcome. Test success does not prove
   production deployment or erase a known architectural defect.

### Refresh only affected context

Compare against the map's recorded commit **and** inspect working-tree changes.
For each changed file, revisit its owning package, reverse dependency edges,
shared contracts, and relevant runtime path. Manifest changes require graph
regeneration; implementation changes require rereading the affected symbols.
User corrections can invalidate a map even without a code change.

For binary-backed plugins, check the plugin contract and upstream provenance;
do not create a second authoritative schema store. For `tched_router`, the
upstream executable's name is `zeroclaw`; the framework/plugin/scaffolding uses
`tched_router`. Keep the control-plane chatbot, retired Waypipe and the
model-agnostic routing role distinct from historical package names.

Apply the owner's network topology when interpreting source coverage: WireGuard
terminates at the decoy, this server has no WireGuard, and WARP transports
identity. Classify missing handlers by intended deployment role before proposing
repairs. A source-inventory count is not a count of required server capabilities;
an unwired `wgcf` scaffold does not establish that WARP transport is broken.

This pass did not build an automatic indexer, populate a vector database, or
install a watcher. The code retrieval implementation is a separate source path
described in the overview. Use retrieved chunks as evidence only after checking
their version and relevance against the current checkout.

### Preparing a fresh repository

The baseline can guide a fresh source manifest, but it is not yet a proof that
a reduced repository builds or supplies a complete release snapshot. Retain
the selected workspace's dependency closure and record external artifacts
explicitly. Required inputs currently include Cargo manifests/lockfile, crate
sources, build scripts, source proto inputs, dispatch templates, policy fixtures,
and the `vendor/rovs-openflow` patch. Generated transport files derive from the
plugin; they are not another authoritative contract.

In particular, retain the tracked generator input
`crates/op-plugins/src/bin/tched_router_config_surface.dispatch.inc` and the
test policy fixture `deploy/security/capability-grants.json`. The indexing
allowlist is not a source packaging manifest; excluded files remain in a full
checkout.

The repaired router owns one imported contract in
`crates/op-plugins/src/state_plugins/tched_router_contract.rs`; its 81 section
schemas and 89 config methods are derived from that value. The generator takes
an explicit upstream import, and the bridge loader consumes sealed blobs only.
Revalidate the clean build after the repair with no `schemas/` dependency; do
not treat the historical baseline fingerprints as current source proof. See
[tched_router's trace](../architecture/tched-router-baseline.md).

The non-workspace `op-gallery-gen` source still references the removed
`schemas/json-render` exports. It is excluded from this workspace verification
and must not be added to a fresh source manifest without reconciling its catalog
inputs with the active UI's catalog. Historical export files are not a reason
to recreate a parallel plugin schema directory.

The active UI and autonomous backing binaries are separate dependencies. A
source-only checkout does not supply their release artifacts. Record the exact
inputs and provenance required by the intended snapshot; existing package
version files can be evidence, while historical preparation scripts do not
establish the snapshot's required contents. Credentials and minted SID1 values
are provisioned runtime state and do not become source files.

For the tched_router backing binary, retain the complete
`vendor/tched-router/` pin, patch set, manifests and `build-native.sh`. The
helper emits `target/native-artifacts/tched-router/{zeroclaw,manifest.json}`
without installing it. Golden preparation validates and stages those files,
the runtime bootstrap configuration and the default skill bundle. A clean
source checkout still needs either the exact pinned source/cache/archive or
network access to fetch it; the built binary is not checked into source.

**Deployment is btrfs send/receive of the prepared snapshot; there is no
deployment script.** Prove two boundaries separately: a disposable clean
checkout builds/tests using only its declared inputs, and its prepared snapshot
contains the intended binaries, static UI, service definitions and configuration
before a btrfs transfer to a disposable receiver is validated. No clean repository
or transfer was created by this learning pass. A generic-dispatch count does not
decide which features belong in that snapshot.

### Regenerate the declared workspace graph

Run from the repository root. This captures manifests and binary entrypoints,
not runtime state. Review `git status --short` alongside the result; `git_head`
alone does not identify uncommitted source content.

```sh
set -eu
codebase_metadata=$(mktemp)
codebase_graph=$(mktemp)
trap 'rm -f "$codebase_metadata" "$codebase_graph"' EXIT HUP INT TERM
cargo metadata --locked --no-deps --format-version 1 > "$codebase_metadata"
jq --arg root "$PWD/" \
   --arg head "$(git rev-parse HEAD)" \
   --arg day "$(date -u +%F)" '
  {
    schema_version: 1,
    observed_date: $day,
    git_head: $head,
    scope: "Working-tree manifests; declared workspace dependencies, not a resolved feature graph, symbol call graph, or live service inventory",
    nodes: ([.packages[] | {
      id: .name,
      manifest: (.manifest_path | ltrimstr($root)),
      binaries: [.targets[] | select(.kind | index("bin")) | {
        name,
        source: (.src_path | ltrimstr($root)),
        required_features: (."required-features" // [])
      }]
    }] | sort_by(.id)),
    edges: ([.packages[] as $p | $p.dependencies[] |
      select(.path != null) | {
        from: $p.name, to: .name, kind: (.kind // "normal"), optional, target
      }] | sort_by(.from, .to, .kind))
  }
' "$codebase_metadata" > "$codebase_graph"
jq -e '.nodes | length > 0' "$codebase_graph" >/dev/null
cat "$codebase_graph" > docs/src/architecture/workspace-graph.json
```

The current graph has only workspace-internal path dependencies. If a future
manifest adds an external path dependency, represent it explicitly instead of
assuming it is a workspace member. Crates.io dependencies, patches and resolved
feature selection require `Cargo.lock`/Cargo resolution as additional evidence.

### Verification

The available repository helper implements formatting, all-target/all-feature
Clippy with warnings denied, and workspace tests, aggregating failures:

```sh
sh scripts/check-workspace.sh verify
```

The September 13 learning pass used this command with output at
`/var/tmp/odbus-learning-verify.log`: exit 0, formatting and Clippy passed,
**1,126 tests passed, zero failed, eight ignored**. Local documentation file links,
graph endpoints/source paths, counts and changed-file whitespace also passed.
The mdBook executables were unavailable, so the rendered book was not validated.
These results are historical pre-repair evidence, separate from architecture
coverage. No release build, catalog reseal, service restart, live schema
comparison, provider invocation, source/installed-binary comparison or full
repair verification is implied.

A later intermediate repair checkpoint recorded **1,132 passed, zero failed,
eight ignored**, plus formatting and Clippy with warnings denied, in
`/var/tmp/odbus-repair-complete-verify.log`. The working tree changed after that
checkpoint. Treat both logs and the `post_repair_verification` section in the
[revision receipt](../architecture/plugin-baseline-revision.json) as historical
evidence. The root final gate must validate the current source; a clean checkout
and release snapshot require their own validation.

## Build the documentation book

This book uses [mdBook](https://rust-lang.github.io/mdBook/).

```bash
cd docs
mdbook serve --open
```

[`book.toml`](../../book.toml) configures `mdbook-mermaid` and
`mdbook-linkcheck2`; broken local links are errors. Source links in onboarding
pages are relative to the repository and can also be checked directly from the
checkout. If the mdBook executables are unavailable, report that limitation
rather than claiming a rendered book was validated.
