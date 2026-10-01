# Identity Cozo reconciliation investigation

Recorded 2026-09-30T19:21:56Z by Codex, task `/root/graph_release_inventory`.
Source HEAD: `46c8a19050cd5eb1bee8db7d62e303db141671df`, with shared uncommitted work.

Owner authorized the existing human identity route ("use mine"). Reads used
`/usr/local/bin/threetched-fs`, schema-only local capture, MCP stdio, and
`/run/opdbus/lungs.sock` bound to session
`414fbc56-03fb-20ab-758a-bcc7a7cbdc0e`. No credential was copied, minted,
persisted, or printed. A jq filter removed credential/key/genesis content
before tool output. The installed binary resolves to
`/opt/op-dbus/golden/bin/threetched-fs`; the older PATH-shadowing
`/usr/local/sbin/threetched-fs` lacks this MCP surface.

## Supported read receipts

`identity_sled__get_identity` succeeded for both requested sessions as the
authorized human. The records below are Cozo-backed identity metadata, not
proof of actual container power, mounts, or deployment:

| Field | Human | Chatbot |
| --- | --- | --- |
| Session | `414fbc56-03fb-20ab-758a-bcc7a7cbdc0e` | `b0c777e3-0955-cd8b-3d01-ca12998cae28` |
| Principal kind | human | service |
| Active / mutation index | true / 0 | true / 0 |
| Embedded instance name | same session UUID | same session UUID |
| Embedded status / pool | Running / `seedpool-414fbc56` | Running / `3tched-storage` |
| Device | `/var/lib/opdbus-runtime/identities/replacement-414fbc56/seeded-leaf.img` | `/var/lib/opdbus-runtime/identities/chatbot/fstorage.img` |
| Mount | `/var/lib/opdbus-runtime/identities/replacement-414fbc56/seeded-root` | `/var/lib/opdbus-runtime/identities/chatbot/persist` |
| Btrfs UUID | `806e2801-8be0-4b0d-919c-3e319452667b` | `ec388ad3-607d-455c-ad0e-3dc4fe2fdbb4` |
| Cozo owner key | human session UUID | `ec388ad3-607d-455c-ad0e-3dc4fe2fdbb4` |
| Attached | true | true |

The human's `identity_sled__inspect_own_root_repair_preflight` returned:
`own root repair preflight requires the parked, anchored container owner`.
That guard runs before revision and storage-journal checks. Neither successful
get reply includes revision or journal metadata, so exact current journal
presence remains unverified. The schema has no list-sessions or journal-read
method. These two reads do not prove there are only two rows.

Earlier unbound `zcall identity_sled get_identity` and subsequent public
`zcall props identity_sled` both returned `Access denied`; caller principal and
exact missing capability were not exposed. No credential substitution, private
database read, grant change, or protected mutation followed these denials.

## Source conclusions

* `identity_sled_dispatch.rs` validates/authenticates the embedded instance's
  session UUID name and container type. Pool/devices/native UUID differences
  do not themselves invalidate SID1. Preserve canonical names at cutover;
  restore them before SID1/lungs proof. Display aliases can stay short.
* `write_identity` derives the target from its public key and supports updating
  Btrfs metadata on an existing anchored identity. It preserves authored SID1,
  key, genesis, actor class and embedded Incus metadata. It does not verify a
  supplied `attached` field against the kernel; `attach_btrfs_device` supplies
  the existing dedicated-mount readback path.
* `identity_transactions.rs` fences incomplete journals and rejects changed
  Btrfs binding after completed provision/repair, except exact repair commit.
  Journal deletion must never be used to unlock a write.
* Private identities use `IDENTITY_SLED_COZO_DB_PATH`, default
  `/var/lib/op-dbus/identity-cozo`; general `cozo` methods use the cognitive
  store, not this private authority.
* Archived Incus names are not separate identity-store keys. Preserve the
  current human/chatbot UUID rows and their journals when removing obsolete
  container names. No public identity deletion/GC method exists. The unused
  tombstone helper preserves revision/events/journals; legacy deletion omits
  journals and rejects versioned rows. Never use `identity-sled-bind`.

## Dedicated-filesystem lifecycle correction

Existing `SledBtrfsDevice` documentation describes dedicated mounted,
bind-ready persistence, never an Incus-pool extra device. The prior lifecycle
contradicted that contract: activation called ADD_DEV when backing membership
was absent; stopping called RM_DEV for a still-present registered member.
This is reached by D-Bus registration/logout, human login/idle parking, and
chatbot startup, including already-active identities without journals.

Bounded source changes in `identity_sled_dispatch.rs` replace both membership
mutations with the existing native dedicated-mount verifier. Activation
verifies before guest start. Guest stop retains the externally managed
filesystem and commits `attached=true` after successful kernel readback.
Post-stop verification failure reports reconciliation and keeps committed
inactive policy without claiming a successful observed stop. Host storage
mount/teardown remains owned by runit/operator preparation. No schema, grant,
credential, container-name, or journal contract changes were added.

Three disposable injected-verifier regressions cover successful start/stop
with retained mount and identity claims, missing mount before start, and lost
mount after stop. Cargo/rustfmt verification is delegated to Luna; this receipt
does not claim those pending checks passed. No source deployment, live Cozo
write, identity cleanup, container action, reseal, or service restart occurred
in this investigation. Read-only MCP processes exited; the temporary empty
state directory was removed. The parent-owned schema-only store remains.

## Follow-up at 2026-09-30T20:18:59Z

The root-owned operator helper `/var/tmp/owner-identity-call-20260930.sh`
passed `bash -n` and two fresh owner-lungs GETs. Sanitized receipts are retained
only in the root-private `/var/tmp/owner-identity-read-20260930` directory.
Each response contained `success=true` and one identity metadata entry; an
explicit forbidden-field check found no SID1, genesis, or public-key field.
The filter includes safe event receipt fields and exact old storage binding,
interface and peer metadata for the parent's cutover/rollback. No writes or
attach operations were invoked. Own-human GET can activate an inactive human
in source; the observed human remained active with mutation index zero.

The existing PluginV1 exposes GetProperty and GetAllProperties, not GetState.
Those property readers use the manifest-pinned public-state policy regardless
of a lungs-bound caller. The captured identity schema has empty public policy.
These properties therefore do not supply an authorized private account list;
the two requested GETs still do not establish a total row count.

Parent-approved inventory source is now applied and frozen in four files:
`op-cozo-store/src/{identity_inventory.rs,lib.rs}`,
`op-plugins/src/state_plugins/identity_sled.rs`, and the existing owned
`op-grpc-bridge/src/identity_sled_dispatch.rs`. The typed read method has the
separate exact capability `identity_sled.inventory.read`. It selects only
bounded metadata columns, includes keyset pages and coherent counts for
identity rows, revisions, revision-only tombstones and journal-only IDs, and
exposes only storage journal phases. It does not select credentials, keys,
genesis values, instance JSON, storage paths or event content. The public-state
policy and all deletion surfaces remain unchanged. Source formatting and the
three store, one schema and one route/capability regression tests are delegated
to Luna; no pending check is represented here as a pass.

Startup source review confirms that only chatbot activation verifies its old
dedicated mount before the bridge serves clients. Local-human bootstrap
preserves its anchored credential and actor class without native mount
verification or container activation. The old live lifecycle must never see a
new populated binding: the parent will activate the corrected binary and
matching catalog with both old bindings retained, then invoke the authorized
write/attach/canonical-cutover sequence. No journal deletion may unlock a
rejected CAS. Stored Incus pool/device fields on non-journal identities remain
birth/stale metadata; the proposed inventory does not report them as runtime.

The initial prestart FUSE-in-root recommendation is superseded by the parent's
installed-LXC observation: recursive root idmapping rejects its FUSE child
with EINVAL. The parent and storage agent now own the verified start-host hook
and detached-mount-FD design. This child did not prototype or mutate guest
mounts. All Rust build/test work and release activation remain separately
owned; this follow-up records source implementation, not deployment.

## Storage binding and future provisioning source audit

The dedicated filesystem binding should retain the canonical receipt's
top-level `volume` as `btrfs_device.mount_point`, the actual writable leaf as
`device_path`, and its sprout FSID. Preserve the existing `cozo_id` exactly.
`SledBtrfsDevice` documents a host-mounted dedicated filesystem; canonical
provisioning stores its top-level plan mount, and lifecycle/attach only verify
that filesystem. The physical helper creates guest private persistence at
`volume/guest/opt/run-mounts/persist` and a separate `/agent/memory` subvolume.
These paths do not move the bridge's memory database: its durable cognitive
owner uses `COGNITIVE_MCP_DB_PATH`, and identity persistence uses
`IDENTITY_SLED_COZO_DB_PATH`. No storage metadata edit was performed here.

The subscriber script's identity branch still generates/copies WireGuard
credentials, prepares profile/socket disks, uses legacy ADD_DEV/RM_DEV storage,
prints a UUID token, and swallows some failures. Its current system branch is
separate. The identity branch never invokes canonical identity provisioning;
its local metadata cannot establish an authoritative account or SID1.

A minimal replacement can prepare physical roots only for already-authoritative
sessions, requiring and verifying the existing session ID/public key through
the supported owner route. It must preserve credentials, refuse recreate or
implicit cleanup, prepare an empty stopped disk-only instance, invoke the
selected physical seed/lungs helpers, then reconcile through supported
write/attach operations. This is not yet implemented in the script.

Entirely new account provisioning remains distinct debt: the existing typed
`provision_container` requires an absent identity, registered key, absent
native target/pool, and a pinned regular-file seed plan with an identity
profile and separate shifted persistence disk. The physical helper assumes a
preexisting empty dir-backed instance and shared physical seed, and installs a
start-host hook that the existing provision plan rejects as `raw.*` config.
Connecting these layouts needs a bounded extension of the existing canonical
provisioning backend, preserving MutationEngine minting and its journal/CAS,
schema/seal/grant and native readback boundaries. A shell wrapper must not
become an identity issuer.

There is no supported public identity-row purge or tombstone operation. The
unused internal tombstone helper preserves revision/journal recovery evidence;
the legacy delete helper rejects versioned rows and is not an operator route.
Principal key revocation is separately supported but does not delete identity
rows or containers. Current keys and both current identities must be retained.
An authoritative inventory can establish that cleanup is a no-op or identify
extra records for a later reviewed retirement design; two GETs cannot.

## Follow-up at 2026-09-30T20:54:05Z: subscriber source implemented

The preceding proposal is now implemented in
`deploy/scripts/provision-workspace-subscriber.sh`, with disposable stubs in
`deploy/scripts/tests/test-provision-identity-mode.sh`. The default branch's
legacy credential generation/copy, UUID token, socket/profile disk, ADD/RM and
recreate/delete paths are removed. The prior standalone system function is
preserved. An explicit session, public key and authorized operator session are
required; only an already anchored identity with a canonical stored instance
can proceed. The exact anchor predicate checks genesis, arrival timestamp and
chain head. SID1 is never required or inspected: the backend deliberately
redacts it for cross-session GET and all mutation results, so absence from a
reply does not imply a missing credential.

Before native allocation, an actual successful metadata inventory call gates
the corrected deployed lifecycle and exact operator grant. Schema-only capture
uses an empty private state directory. Selected helper, FUSE binary, catalog
and sibling/default hook are canonicalized and root-controlled; helpers receive
the same selected artifacts. Every MCP response remains transient until a
strict metadata allowlist; errors and anchor/credential bytes are discarded.

Root preparation and native checks precede supported write/attach and final
authoritative readback. The original owner key, interface and peer remain
unchanged. Normal mode requires an inactive target: supported touch starts its
canonical native guest and commits active state, then GET/native readback prove
the result. Existing-active targets otherwise skip native start in source;
they are admitted only for `--no-start` maintenance. That mode preserves prior
active metadata while requiring the guest to remain stopped. After complete
binding/power proof only, `seed-enable-boot` receives true for normal mode or
false for restore-root/FUSE-only boot policy. Failures never receive that marker
and never trigger automatic cleanup.

Both shell suites pass, as do scoped Bash syntax and diff checks. Fixtures cover
redacted mutation replies, malformed anchors, missing/mismatched instance/key,
active-target admission, existing native names, helper/native failures,
intervening metadata changes, write/attach/touch denial, final power readback,
completion-publication failure, preserved owner/interface/peer, ordering and
secret/error sentinel exclusion. Logs are
`/var/tmp/subscriber-system-tests-20260930.log` and
`/var/tmp/subscriber-identity-tests-20260930.log`. These are stub workflows, not
live provisioning proof. No real native, Cozo or lungs mutation was executed by
this child. The five delegated inventory tests also pass: store3, schema1 and
bridge1, with bridge log `/var/tmp/identity-inventory-final-20260930/op-grpc-bridge.log`.
Rust/Cargo and release activation remain owned by Luna and the parent.

The final read-only storage review found no remaining subscriber source
blocker. Startup sequence remains lifecycle release/catalog first with old
bindings/native identities retained, no boot completion markers and storage
service disabled; new inventory then precedes any canonical cutover. A
chatbot storage journal or invalid original mount/native plan can deny startup
before the owner door opens; that error must be reconciled without changing
bindings or deleting journals.

Inventory phases require strict interpretation. No journal on a target permits
the ordinary binding path subject to CAS. Incomplete journals fence unrelated
writes; `identity-committed` journals still make changed storage binding JSON
immutable. A completed phase is not cutover clearance. Present row counts are
account records; union-key counts include tombstones/journal-only recovery
evidence and do not imply additional live accounts. Counts alone also cannot
exclude journals on the two current identities; collect their page entries.

The existing repair API cannot target this shared physical-root layout. Its
version3 plan requires a dedicated Btrfs Incus pool and separate shifted
persist disk beside the top-level mount; the native preflight requires a
missing root in the original Btrfs pool and empty old persistence. Native seed
verification requires root-owned regular seed/leaf files. The physical helper
uses a shared block seed, a dir-backed root and persistence inside the guest.
Existing root-repair journals also make a fresh repair invocation return
reconciliation required. Any actual target journal therefore needs its
legitimate recovery/migration design reviewed before this cutover; no such
extension or journal deletion was implemented here.

## Final activation provider risk check (source only)

Bridge startup constructs a native runtime client and restores committed
projections; it does not choose a provider/model or patch the native config.
The control-plane request path observes its dedicated native agent's current
selection and pins that configured provider/model into the request. The
OpenCode-only dashboard filter explicitly excludes the control-plane agent,
and the existing `custom.opencode_*` profiles remain supported. There is no
production startup requirement for `opencode.go_control_plane`. Native
provider/model/config PATCH operations remain admitted explicit methods.

Preserving the live native router executable, service and config therefore
does not introduce a startup reset of its working custom control-plane route.
The earlier Go-versus-Zen route mismatch remains unresolved and unchanged.
Request-time native endpoint/ACK compatibility still needs final workflow
proof; this source review is not an end-to-end chat check. Source references:
`server.rs:849`, `mutation_engine.rs:2798`, `control_plane_routing.rs:65`,
`tched_router_runtime.rs:203,1074,2102`, and explicit selection methods at
`mutation_engine.rs:4987`. No credential/config contents, live mutation or
Cargo invocation were used for this check.

## Catalog generation136 to204 schema audit (read only)

All68 plugin IDs remain. The audit extracted only section1 SCHEMA_JSON from
each old/new OPBLOB01 after header/tag/range validation; each raw section's
SHA256 equals its own manifest hash. Object-key-normalized schemas match for
63 plugins, including adc, users, Qdrant and tched_router. New schema bytes use
insertion order where old bytes used sorted keys: Cargo feature unification
enables serde_json preserve_order/indexmap, contradicting the sealer's stated
BTreeMap assumption. SHA256 algorithm and timestamp metadata are not the cause.

The five differing normalized documents contain: Cozo2.1 adds seven graph
methods/three capabilities; identity4.2 adds inventory/its separate capability;
model-registry1.1 adds optional native-provider/API-prefix result metadata to
three existing methods; elevation1.0.1 documents intentional root-password
authorization; json-render3.0 changes only default component prop-name list
ordering (sorting those lists makes its full schema equal). Existing identity
and Cozo methods/fields remain equal; no native-router schema change occurred.

Latest GNU bridge compiled method evidence was compared structurally against
new sealed version/fields/methods for all68 plugins: zero mismatches, including
json-render arrays. Consumers pin stored manifest/section bytes or compare
structural declarations; raw stored digest validation remains coherent. Old
immutable 3tchedFS views must be recaptured to discover new inventory/graph
operations. Catalog changes do not reinterpret SID1 arrival hashes as current
catalog hashes; identity checks compare authored credential metadata with its
stored arrival stamp.

Root-private schema-only evidence is retained at
`/var/tmp/catalog-schema-review-20260930.8FMNPZ`; no blob identity/private-state
sections were extracted. The ordering weakness is recorded in SIGNALS for
follow-up. No artifact or compiled authority was normalized/resealed by this
child, no Rust/Cargo edits occurred, and this audit does not establish live
activation or chat compatibility.

## Supported physical-root migration (source implementation; deployment pending)

Actual owner inventory reported human `414fbc56-03fb-20ab-758a-bcc7a7cbdc0e`
with completed seed provision and no root repair. Ordinary binding replacement
correctly remains fenced. The new `migrate_own_physical_root` operation is
restricted to that operation's verified human owner, with exact capability
`cap.identity.sled.physical_root_migrate@v1`. It never formats, mounts, starts,
renames or deletes native resources. It adopts an already prepared STOPPED
canonical target after native and read-only kernel verification.

A root-owned SHA256-pinned plan lives at
`/var/lib/op-dbus/identity-physical-migration-plans/SESSION.json`, maximum64KiB.
The source schema provides `inspect_own_physical_root_migration_preflight {}`
on the same owner capability. Its response gives safe revision/op-ID, prior
binding and exact historical-journal digests; operators need no private Cozo
read and no SID1/genesis/key payload to author a plan. Plan fields are:

```
version: 1
session_id: preflight.session_id
expected_revision: preflight.expected_revision
expected_revision_operation_id: preflight.expected_revision_operation_id
previous_binding: preflight.previous_binding
previous_root_repair_sha256: preflight.previous_root_repair_sha256
previous_seed_provision_sha256: preflight.previous_seed_provision_sha256
binding: {device_path: leaf, mount_point: canonical-volume,
          btrfs_uuid: sprout-fsid, cozo_id: exact-prior-key, attached: true}
seed_device: resolved root-controlled physical block path
seed_fsid: helper-state.seed_fsid
seed_generation: numeric helper-state.seed_generation
leaf_inode: numeric helper-state.leaf_inode
leaf_size: numeric helper-state.leaf_size
guest_subvolume_id: numeric btrfs inspect-internal rootid VOLUME/guest
native_root: helper-state.root (canonical /var/lib/incus/.../rootfs)
pool_name: native root disk's pool name
pool_source: exact native GET pool config.source
instance: complete official Incus GET instance JSON for STOPPED canonical name
```

The plan contains no authored identity fields. Commit preserves key, SID1,
genesis, class, interface, peer, term, Cozo owner key, active policy and all
unrelated fields. A single Cozo transaction checks the current revision and
both exact old journals, archives old bindings/native metadata/provenance,
records new current physical provenance and advances revision/index. Original
journal rows remain unchanged; generic CAS/write remains fenced. Only one
migration is admitted initially. Deterministic plan-derived operation identity
supports replay after paired-publication failure without repeating native
work; replay re-verifies kernel/native state and recovers the pending SHM
publication. Inventory exposes migration revision/digest only.

Physical verification pins shared seed FSID/generation/SEED flag, root-owned
private leaf inode/size, exact read-only seed and writable leaf loop backing,
two-member ancestry, top-level volume, guest subvolume and native root bind.
It validates protected agent parent/soul/rules/shared RO and private
user/memory/skills/persist RW. Native admission requires matching canonical
UUID, dir pool, profile-free/nicless root, exact layout/config and three masked
mutation-ioctl denies with effective default seccomp protection. New lifecycle
uses this persisted current provenance and verifies it at every start/stop;
historical incomplete journals remain a denial. No ADD_DEV/RM_DEV occurs.

Operator call order after matching release/catalog/grant activation:
`identity_sled__inspect_own_physical_root_migration_preflight {}`; preserve its
filtered safe result; prepare and pin the private plan after canonical native
cutover; `identity_sled__migrate_own_physical_root {"plan_sha256":"SHA256"}`;
then inventory + authoritative get/native proof before publishing boot-ready.
The owner caller helper's output filter must explicitly include the preflight
fields and migration receipt. Do not rely on a successful response alone.
Root owns actual plan, native cutover, grant, deployment and completion proof.
