# System dependency graph

The system graph is a versioned inventory in the existing cognitive Cozo store.
Its typed methods belong to the `cozo` PluginSchema and use the normal
authenticated D-Bus/MutationEngine path. They do not require arbitrary
CozoScript access. The new `system_graph_state`, `system_graph_nodes` and
`system_graph_edges` relations leave existing `graph_node`/`graph_edge` and
private memory relations intact.

This describes source implemented on 2026-09-30. The new backend contract,
grants and reminders require a prepared, sealed release and normal activation;
editing these sources does not update the running bridge.

## Evidence and freshness

Keep these inventories separate:

| Inventory | Meaning | Update authority |
| --- | --- | --- |
| Source | Dependency declarations from a specified source export | Explicit typed import |
| `release:<commit>-<stamp>` | Files and relationships in a prepared filesystem | Golden preparation, followed by authenticated import |
| `runtime:public` | Pinned plugin contracts and committed public SHM | `cozo.refresh_system_graph` |

Each receipt includes a revision, canonical snapshot digest, node/edge counts,
input identifiers, generation time, coverage and limitations. Each edge carries
declared, observed or inferred evidence and a locator. A generation timestamp
does not establish the age of a backend observation. A staged binary or an
enabled-service declaration does not prove a running process. Runtime capture
does not independently identify the active release.

For runtime provenance, `inputs.sealed_catalog_sha256` currently contains the
manifest's stored `catalog_hash` (the BLAKE3 catalog fold), despite the input
key's name. Compare it with that field in the received, persisted and SHM
manifests, not with the SHA-256 of the manifest file. Activation receipts
record executable and file digests separately.

The runtime collector reads one bounded committed SHM snapshot, validates it
against the manifest-pinned schemas, and applies both the existing public
projection policy and domain allowlists. It never probes processes, invokes
subprocesses, reads service configuration files or imports private content.

The adapter represents plugin methods, capabilities and dependencies, container
devices, Btrfs hierarchy, Qdrant summaries, mail resources and declared
vectorization, memory, cognitive, compliance and UI relationships **when its
inputs support those claims**. Current exposure limits matter:

- Incus public state withholds identity container names and devices. Detailed
  container relationships remain unknown in this shared graph. When names are
  withheld, the Incus plugin node records aggregate instance counts by approved
  status and type, with an explicit privacy limitation. These counts do not
  assign identities or establish per-container dependencies.
- The current Btrfs plugin does not publish a filesystem/subvolume inventory.
  Source support for projecting that inventory is not a live observation.
- Qdrant summaries require an allowed public projection. A reserved collection
  name does not prove a collection exists.
- Private episodes, conversation text, vector payloads, credentials and mail
  content are excluded. Private episode and memory stores remain distinct.
- Prepared container archives are artifact nodes; their internal configuration
  files are not unpacked by the collector.

## Golden preparation hook

`deploy/runit/build-golden.sh` requires the built `system_graph_inventory`
binary. After staging and writing the release manifest, it generates:

```text
usr/share/op-dbus/graphs/release.json
```

The hook validates the complete graph and writes it atomically, then records
the graph path and file SHA-256 in `MANIFEST`. The graph output subtree and
the manifest's own digest are excluded to avoid self-reference. The collector
records file hashes and conservative static references without storing
configuration bodies or environment values. Protected files are not read.

This hook runs at each golden preparation. It does not open the live Cozo
database, activate a release or restart a service. After activation, an
authorized operator imports the received artifact through
`cozo.replace_system_graph`; a new namespace uses `expected_revision: 0`.
Prepared artifacts and active runtime observations retain separate provenance.

For an offline source export conversion, from the repository root:

```sh
cargo run -p op-cozo-store --bin system_graph_inventory -- \
  --source-graph docs/crate-graph/crate-graph.json \
  --output /var/tmp/source-graph.json \
  --namespace source:workspace \
  --generated-at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
```

Use the actual source export path. The output's parent directory must already
exist. This converts an export; it does not regenerate or validate the export's
source claims. The existing `scripts/crate-graph.sh` produces the legacy source
export. Its additive `.cozo` output is not the replacement/update contract.
New source exports record `worktree_dirty`; the converted receipt exposes
`source_worktree_dirty` directly. A dirty tree or a legacy export without that
marker must not be described as an exact checkout of `source_commit`.

`cargo run -p op-cozo-store --example load_crate_graph -- SNAPSHOT.json`
validates an import in a fresh in-memory store. The example also accepts an
explicit offline RocksDB directory and expected revision. Never point it at a
running service's database.

## Scoped reads and updates

| Methods | Required capability |
| --- | --- |
| `list_system_graphs`, `system_graph_status`, `query_system_graph_nodes`, `query_system_graph_edges`, `traverse_system_graph` | `cap.data.cozo.system-graph.read@v1` |
| `replace_system_graph` | `cap.data.cozo.system-graph.write@v1` |
| `refresh_system_graph` | `cap.data.cozo.system-graph.refresh@v1` |

Use the existing identity-bound 3tchedFS MCP connection. The generated direct
tool name is `cozo__<method>`; compact clients discover and execute the same
plugin method through their normal meta-tools. The previous
`cap.data.cozo.relations.list@v1` grant lists relation names only and does not
grant these methods. Neither a discovery reminder nor a visible tool grants
permission.

Start with `cozo__list_system_graphs` arguments:

```json
{"after_namespace":null,"limit":20}
```

Read the selected receipt's coverage, limitations and revision before using
its dependencies. Node queries accept a kind filter and a continuation cursor:

```json
{"namespace":"runtime:public","revision":1,"after_id":null,"kind":null,"limit":100}
```

Use the returned revision and `next_after_id` for subsequent pages. An incoming
traversal answers which nodes depend on a chosen node, subject to the graph's
actual relation directions:

```json
{"namespace":"runtime:public","revision":1,"start":"plugin:cozo","direction":"incoming","relations":[],"max_depth":3,"max_nodes":100,"max_edges":300}
```

Use an actual returned node ID and revision. Traversal returns explicit
`truncated` and `truncation_reasons`; a partial traversal cannot establish that
no other dependants exist. Namespace listing is consistent within each page,
but does not freeze the set of namespaces across several requests.

Updates replace exactly one namespace atomically, including removal of stale
nodes and edges. `expected_revision` must match; stale readers/writers fail
instead of combining generations. Snapshot provenance kind and prepared
release identity cannot change within a namespace. Imported snapshots cannot
claim runtime ownership or observed edge evidence; only committed runtime
refresh can produce those observations.

Limits are 50,000 nodes, 200,000 edges and 64 MiB per snapshot; 256 namespaces;
1,000 rows and 2 MiB per response page. Traversal is bounded by depth, node,
edge, byte, scan and time limits. No automatic pruning deletes old namespaces.
At the namespace limit, imports fail explicitly and require a reviewed
retention decision.

## Agent reminders

The graph reminder belongs at model-request construction, so ordinary history
compaction cannot permanently remove it. It is one line, not the graph itself;
agents query details only when needed. The reminder must accurately distinguish
available access from discovery or missing access, preserve the authenticated
caller, and never grant permission or import graph data.

The shared line is:

> System graph: discover cozo.list_system_graphs through identity-bound 3tchedFS; check provenance and coverage; grants apply.

Repository-managed direct LLM requests receive it in their system message. The
pinned native router provider wrapper adds it on every inference through the
canonical in-process provider factory, including autonomous tool iterations,
streaming and calls after compaction.
It is added to request copies, so stored conversation history does not grow by
another copy every turn. User or tool text cannot suppress the system reminder.
This requires rebuilding the pinned native artifact with the checked-in patch;
an already-running older native executable does not acquire the hook.

Native CLI adapters receive the line on each invocation, but the router cannot
control hidden model turns inside those external CLI agents. Directly
constructed providers outside the canonical factory need equivalent wiring.

3tchedFS also advertises the graph in MCP initialization instructions when the
loaded Cozo schema exposes its read method. That is a discovery aid, not a
per-turn hook: an external IDE controls its own prompts and must install a
matching turn hook to repeat the reminder on every inference.
