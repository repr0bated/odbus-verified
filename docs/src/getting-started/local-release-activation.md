# Local release activation

The clean baseline receives releases on this Artix/runit host. Golden
preparation is separate from deployment. The deployment boundary is a
read-only snapshot transferred by `btrfs send/receive`; installed executable
paths then reference the received filesystem through one `current` symlink.

## Paths and ownership

| Path | Purpose |
|---|---|
| `/opt/op-dbus/golden` | Writable preparation subvolume; never an execution target |
| `/opt/op-dbus/send/<release-id>` | Temporary read-only source snapshot |
| `/opt/op-dbus/releases/<release-id>` | Received read-only release |
| `/opt/op-dbus/current` | Atomic symlink selecting the received release |
| `/opt/op-dbus/recovery/<activation-id>` | Ordinary directory holding first-migration originals and an activation receipt |

Retention is configurable per storage owner; there is no global five-subvolume
cap. Budget free bytes for preparation, send/receive and a recovery reserve
before starting writes. Count paths under the release root explicitly: `btrfs
subvolume list -o` on an ordinary directory lists descendants of its
containing subvolume, including unrelated Incus storage. Existing unrelated
subvolumes are not permission to delete storage. Preserve the category,
timing, vector and state subvolumes owned by cache/Snowball. Retain the previous active
release for rollback; retire only explicitly identified obsolete releases.

## Prepare

Build the native glibc workspace, prepare its mail and pinned UI/native
artifacts, and explicitly seal/persist with the freshly built `opblob` and
the service's schema-shaping environment. Preserve the previous catalogs and
review additions/removals before a live reseal. `build-golden.sh` accepts
`OP_PERSISTED_CATALOG_SOURCE` for isolated release preparation.

After extracting deterministic UI assets into the prepared tree, set
`dashboard/index.html` to the preparation timestamp before taking the read-only
snapshot. Archive timestamps are epoch-based; preserving them on this HTTP
entry point can cause a browser's old `If-Modified-Since` request to receive
304 and retain an obsolete bundle. Keep the archive and content hashes intact,
and verify an old conditional request returns 200 after activation.

For the first clean baseline, move the previous golden contents into an
ordinary recovery directory before repopulating the existing golden
subvolume. This prevents untracked legacy binaries, provider trees and
container files from becoming release inputs. Do not move nested subvolumes
or the golden subvolume itself. The intentionally staged `bin/nlm` wrapper
is additional to the Cargo binaries and must remain.

Resolve target secrets and the native router before activating the bridge.
The `tched-router` service uses the pinned `/usr/bin/zeroclaw` executable,
loopback port 8084 and private mutable configuration. Its authenticated
generation credential must be issued by that same gateway and persisted
with its corresponding native token hash. A token from a temporary gateway
is not valid for the production gateway. This native backend provisioning
does not introduce OP-DBUS UI pairing.

Task elevation uses the static PAM service file `deploy/pam/op-dbus-elevate`,
staged into the release and activated as `/etc/pam.d/op-dbus-elevate`. For
each exact task, the authenticated caller authorizes the request by entering
the root account password; PAM checks that password and account validity.
The existing verified caller-principal and capability-grant checks remain
in force, and the resulting one-use authorization ticket is bound to that
principal, session, plugin, method and arguments. The workflow has no
per-principal Unix-user map prerequisite.

Control Plane inherits its provider from the native `control_plane` agent.
Set the public service variable `TCHED_ROUTER_CONTROL_PLANE_MODEL_PROVIDER`
in the prepared service environment to an existing native `family.alias`
profile, for example `openai.control_plane`, `anthropic.control_plane`, or
`opencode.go_control_plane` (the default, OpenCode Go). The referenced profile owns
the model and credentials and must already be configured through the native
configuration owner; the variable contains no credential. Startup rejects an
unknown profile before changing the current role. Control Plane does not
require OpenCode inventory or a fixed model list. The UI reads the same
provider-keyed Cozo catalog for every mode, including Control Plane. Its
protected Control Plane selection changes the selected profile's actual
model and the dedicated agent reference together; routing metadata is not
a separate model eligibility list. Personal's provider and model remain
independent. Named chatbot roles retain the `chatbot_500` runtime
profile regardless of provider selection.

The provider list uses `provider_id` as its stable Cozo key (legacy `id`
input remains accepted). Do not rename stored providers to native router
families. Registry model records carry a separate `native_provider_id`, an
exact ordinary-selection `family.alias`; the picker keeps the Cozo key and
sends the native binding only when applying that exact catalog model.
`native_profile_overrides` maps a model's recorded alternative wire profile
to an explicitly provisioned ordinary profile. An unmapped alternative has
no native binding. Control Plane continues to use its separate
`control_plane_profile` and protected mutation. Binding metadata does not
prove that a profile exists in the running native configuration: reconcile
its endpoint and key ownership before activation. The checked-in ordinary
bindings currently cover the Go and Cheaper Inference bootstrap profiles;
other provider bindings still require verified native provisioning.

`inference_url_prefix` records the expected HTTPS origin and API path
namespace in the same provider metadata. Go uses
`https://opencode.ai/zen/go`; Zen's `https://opencode.ai/zen/v1` is a distinct
pay-as-you-go platform and must not be substituted for the owner's Go
subscription. The UI verifies the configured route URI against this
namespace before applying or generating, with a path-segment boundary.

The registry schema changes require a matching newly sealed backend release.
Refreshing the npm UI alone cannot add these fields to the running catalog.
The database remains owned by `model_registry`; 3tchedFS operations call that
owner. Do not symlink its private Cozo directory to the identity's FUSE API
mount or to another store: the private opener rejects symlink traversal.

## Transfer

The container's 3tchedFS executable, agent defaults and vendor service template
belong in its read-only identity seed. The owner maintains that filesystem
directly during a deliberate maintenance window; no separate seed preparation
script is retained.
They must not be copied into the identity's writable overlay at startup, which
would hide subsequent seed upgrades. See [agent files](../operations/agent-files.md)
for preparation, persistent overrides and the separate seed activation boundary.

Run these as deliberate operator steps after checking the capacity budget
and the golden manifest. The release ID contains only letters, digits,
hyphens and underscores, and must not already exist at either destination.
Use a root Bash shell (`sudo bash`) from the repository root for this block,
including creation of the transfer lock under `/run`.

```sh
release_id="$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)"
# Budget the complete receive, temporary overhead and a 16 GiB reserve.
# Serialize local transfers so they cannot both spend the same free space.
exec 9>/run/opdbus-release-transfer.lock
flock -n 9
candidate_bytes=$(sudo du -sb /opt/op-dbus/golden | awk '{print $1}')
available_bytes=$(df -B1 --output=avail /opt/op-dbus | tail -1)
[ "$available_bytes" -ge "$((candidate_bytes * 2 + 17179869184))" ] || {
  echo 'Insufficient free space; no snapshot or receive started' >&2
  exit 1
}
sudo install -d -m 0755 /opt/op-dbus/send /opt/op-dbus/releases
sudo btrfs subvolume snapshot -r /opt/op-dbus/golden "/opt/op-dbus/send/$release_id"
set -o pipefail
sudo btrfs send "/opt/op-dbus/send/$release_id" |
  sudo btrfs receive /opt/op-dbus/releases
sudo cmp "/opt/op-dbus/send/$release_id/MANIFEST" "/opt/op-dbus/releases/$release_id/MANIFEST"
sudo btrfs property get "/opt/op-dbus/releases/$release_id" ro
sudo btrfs subvolume show "/opt/op-dbus/releases/$release_id"
```

Record the manifest digest, source UUID and received UUID. Check the
received binary/config/catalog/UI digests before selecting it. A completed
transfer alone does not activate a release.

The first local baseline was transferred as
`20260922T001242Z-dacc6dd`. Its received UUID references source UUID
`6131ecd3-87f4-174d-be41-b2aed69b3c4a`. The activation receipt and 66 explicit
host bindings are retained in
`/opt/op-dbus/recovery/activation-20260922T001242Z-dacc6dd/`; transfer and
catalog receipts are in `/var/tmp/opdbus-release-P8UxXV5ctf/`.

The preceding EMQX release was `20260922T013431Z-1f1b772`. Its source UUID
is `46fb8811-8f29-4f40-8547-56869fb16cde`, and
the received filesystem records that UUID. It contains the native binaries
built at `a01411c`, the unchanged pinned UI, and the final ACL correction.
All 66 plugin IDs remain; only the `identity_sled` schema changed. The live,
persisted and received catalog manifests match. The production UI client
received wake sequence 3 and reread scoped knowledge in 108 ms; a wildcard
subscription was refused. Metadata-only evidence is retained under
`/var/tmp/opdbus-release-P8UxXV5ctf/activation-20260922T013431Z-1f1b772/`.

The final cognitive MCP/EMQX baseline is `20260922T023328Z-2f48e86`, selected
by `current`. The native GNU workspace release built at `2f48e86` in 9m28s;
69 focused tests passed. Source UUID and received UUID both record
`887d3eae-da94-e545-86e1-27cde332bf3f`. All 66 plugin IDs remain, with only
`cognitive_mcp` changing schema; received, persisted and SHM catalog manifests
match SHA-256 `66660da6bb4cfb4487c13dbf80b3549a074bcc44f421a32da78969b752d65149`.
The running bridge, web and grants binaries match the release. Both bridge
sockets remain root-owned mode 0600, and all four affected services pass
runit readiness checks.

The live test stored disposable account-scoped advice, received EMQX wake 5,
and used the production UI client to reread a new owned knowledge row
containing that exact advice. Knowledge count and revision advanced 4 → 5.
The wildcard subscription was refused, and disposable memory was deleted.
Public MCP memory round trips, protected identity metadata refusal, three
workflow facades and replacement of forged context-session hints also passed.
The positive and negative MQTT tests run sequentially: they share the
owner-derived MQTT client ID, so running them concurrently disconnects the
positive test's subscription.

Final evidence is `/var/tmp/opdbus-closeout-20260922/baseline-receipt.json`;
activation metadata is under
`/var/tmp/opdbus-release-P8UxXV5ctf/activation-20260922T023558Z-20260922T023328Z-2f48e86/`.
The obsolete `dacc6dd` received filesystem was archived as a protected,
digest-checked btrfs stream under `/opt/op-dbus/recovery/closeout-20260922/`
before retiring it. Five managed subvolumes remain, including both compatible
preceding releases. Full `/verify` remains cancelled by the owner; historical
Qdrant recovery (OD-68) and exhaustive retained-method audit are separate work.

The following historical release used a five-subvolume cap. The owner withdrew
that global cap on 2026-09-23: cache and Snowball retain their own Btrfs
subvolumes and configured snapshot retention. Never flatten runtime storage
or delete history to satisfy that former cap.

For a transfer that avoids retaining a temporary source snapshot, send it to
a protected ordinary stream file first. After successful send, record the
stream digest and source UUID, remove only that temporary source snapshot,
then receive the completed stream. Verify the received UUID, read-only flag
and contents before activation. This release used that sequence to preserve
the existing received filesystems without exceeding five subvolumes.

## Activate

First migration creates per-file links from established host paths into
`/opt/op-dbus/current`. Preserve every replaced file, directory or symlink
and record its destination in the recovery receipt. Move originals rather
than copying new binaries into the live installation. Create replacement
links under temporary names in each destination directory, then rename
them into place.

| Received path | Host path |
|---|---|
| `bin/<name>` | `/usr/local/bin/<name>` for the explicit release binary inventory and `nlm` |
| `usr/bin/zeroclaw` | `/usr/bin/zeroclaw` |
| `usr/share/op-dbus/{native,ui}-manifests/<file>` | Same absolute host path |
| `usr/share/op-dbus/agents` | `/usr/share/op-dbus/agents` (initial shared agent bundle) |
| `libexec/3tched/<helper>` | `/usr/local/libexec/3tched/<helper>` for activated service dependencies |
| `sv/<service>/{run,check,finish,log/run}` | Corresponding files in `/etc/runit/sv/<service>` |
| `etc/opdbus/{capability-grants,mcp-audience-policy,mcp-toolsets}.json` | Corresponding protected policy files in `/etc/opdbus` |
| `etc/op-dbus/zeroclaw-runtime.toml` and skill bundle | Corresponding bootstrap files in `/etc/op-dbus` |
| `etc/pam.d/op-dbus-elevate` | `/etc/pam.d/op-dbus-elevate` |
| `etc/emqx/{emqx.conf,base.hocon,acl.conf}` | Corresponding files in `/etc/emqx` |
| `usr/local/share/op-dbus/dashboard/{index.html,assets}` | Same host paths; preserve other dashboard mounts |
| `opt/emqx` and pinned provider installations | Explicit matching host provider paths |

Keep runit service directories and their `supervise/` and log-supervision
state writable. Never symlink the whole service directory into a read-only
release. Keep `/etc/op-dbus` and `/etc/emqx` writable: secrets, issued TLS
files, native gateway pairing state and runtime configuration are not
overwritten by a directory-level switch. Keep identity, Cozo, Qdrant,
Snowball, replay and broker data in their existing durable locations.

Once links and prerequisites are ready, stop the affected application services
before changing the binary/catalog pair. For this bridge/identity release:

```sh
sudo sv down emqx op-web op-grpc-bridge opdbus-grants
sudo ln -s "releases/$release_id" /opt/op-dbus/.current-next
sudo mv -Tf /opt/op-dbus/.current-next /opt/op-dbus/current
```

Activate the received persisted catalog with `opblob persist <received
catalog> /var/lib/opdbus/plugin-blobs`, then `opblob stage-shm`. Startup
consumes that catalog and must never reseal or restore old private SHM state.
Bring up `opdbus-grants` and the bridge before EMQX and op-web, checking their
readiness with `sudo sv check`. Deliberately restart only changed services:
the native router when its configuration changes, grants materialization,
the bridge, EMQX, and op-web. Preserve existing runlevel symlinks, adding only
the explicitly enabled application service when absent. Do not restart or
activate staged network, OVS, uplink, DHCP, session-bus, Xray or container
definitions as an incidental part of application activation.

Check that running executable digests match the received release, live and
persisted catalog manifests match, authenticated MCP works, and the UI
receives its own EMQX wake before reading private committed knowledge.
Report any remaining backend-data failure separately from deployment.

For releases carrying `usr/share/op-dbus/graphs/release.json`, verify its file
digest against `MANIFEST`. Golden preparation regenerates this inventory but
does not import it into the running database. After activating the matching
Cozo contract, use an explicitly authorized identity to call
`cozo.replace_system_graph` with the received snapshot and expected revision
zero for its new release namespace. Runtime observations are a separate
`cozo.refresh_system_graph` mutation. Read, import and refresh have separate
capabilities; an MCP reminder grants none of them. See
[System Dependency Graph](../operations/system-graph.md) for provenance,
coverage limits and query examples. Never use the offline loader against the
live database.

### First migration socket cleanup

The former live bridge readiness script changed socket permissions, which
changed their ctime after the bridge recorded its ownership lease. The new
readiness script is observational. On the first activation, the bridge can
therefore reject the old socket with `existing socket does not match the
previous bridge lease; refusing cleanup`.

Stop the bridge with `sv`, retain the socket metadata and lease files in the
activation receipt, and check each exact bridge socket independently. Only
unlink a root-owned socket after confirming it has no live listener and a
connection attempt is refused. The two bridge paths on this host are
`/run/ghostbridge/container.sock` and `/run/opdbus/grpc.sock`. Do not remove
other shared-directory sockets, weaken directory permissions, or bypass
the bridge's ownership checks. Restart the bridge after this one-time
operator cleanup; it records ownership of the newly created sockets.

### Existing identity credentials and MQTT

New identities include the `mqtt` transport scope. An older persisted SID1
may contain only `dbus,grpc,mcp`; restarting the bridge deliberately preserves
those authored bytes. Do not edit the credential or weaken the broker check.
The identity owner uses a dedicated durable transport-refresh transaction;
ordinary identity compare/exchange still rejects anchored credential changes.

For an existing human identity, invoke the canonical
`identity_sled.refresh_credential` method with `{}` and the exact
`identity_sled.credential.refresh` capability using that human's authenticated
session. The release grants this capability only to the configured local
human. The method resolves the target from verified session metadata and
accepts no caller-selected identity or credential fields. MutationEngine
authors the supported transport upgrade, commits it through Cozo and private
SHM, and returns a receipt without credential bytes.

Fetch the newly authored credential through the existing identity-header
helper or `/api/identity/self` afterward. The previous SID1 stops
authenticating. Principal, session, genesis, trace and arrival claims are
preserved, so the knowledge owner key and history remain the same. Repeating
the call with a current credential is idempotent. The advertised doorbell URL
must include `/mqtt`, matching the EMQX WebSocket route.

The static ACL must leave identity clients unmatched so ExHook can authorize
their exact owner topic. Do not restore a terminal `{deny, all}` file rule:
the file authorizer runs before ExHook and stops the hook chain on a match.
Keep `authorization.no_match = deny` as the final denial default. The only
static allow is the existing generated local publisher account.

## Coding retrieval deadlines

Coding retrieval has one total budget covering embedding and collection
queries. Defaults are 5 seconds for completion, 10 seconds for search, and
30 seconds for deep retrieval. Operators may set
`COGNITIVE_MCP_RETRIEVAL_DEADLINE_MS`, or override a mode with
`COGNITIVE_MCP_COMPLETION_DEADLINE_MS`, `COGNITIVE_MCP_SEARCH_DEADLINE_MS`,
or `COGNITIVE_MCP_DEEP_DEADLINE_MS`. Values are bounded to 100–120,000 ms.
Search reports failure when its budget expires; context returns an explicit
`retrieval_error` with empty results. Failure of every requested collection
also reports an error. Partial results remain available when at least one
collection query succeeds.

### Cognitive tool contracts

The baseline binds memory identity and coding activity to the authenticated
session. Memory callers cannot replace the reserved `_identity` record or
select another session through request fields. Memory and coding results are
checked against their admitted output schemas before being returned.

The six language/workflow agent facades require their own
`cognitive_mcp.<tool-name>` capabilities. A generic `cognitive_mcp.invoke`
grant does not authorize these MCP targets. The baseline policy grants those
six capabilities only to the existing local human principal; memory deletion
uses the existing `cognitive_mcp.memory.write` grant. Deploy this policy with
the matching cognitive schema and binary.

## Roll back

Physical identity leaves require the dedicated-mount lifecycle implementation.
Before registering such a leaf in Cozo, activate and verify the release that
checks its mounted filesystem and retains Btrfs membership when the guest
stops. Logical Cozo attachment may become false; this never means device removal.
The older implementation used device removal on stop. Once a populated
physical-seed leaf is registered, do not roll the bridge back to that older
implementation. Retain a compatible fixed bridge for recovery; an older
catalog or filesystem snapshot alone does not make the old lifecycle safe.
Runtime identity data must not be restored from an old backup to bypass its
revision or recovery journals.

After the first credential refresh, the durable identity store may contain a
`transport-refresh` revision. Any rollback binary must understand that revision
kind. The earlier `dacc6dd` release does not, so keep it as a pre-refresh recovery
artifact; do not activate it against the refreshed store or restore old identity
data to make it boot. Use a compatible replacement release for later rollback.

For subsequent releases, atomically restore `current` to the previous
compatible received release, restore its persisted catalog and stage it into SHM, then
restart the same application services. For the first migration, restore the
original paths from the activation receipt instead. Restore any deliberately
changed native router config/token pair together. Runtime data is never
rolled back as part of binary/config rollback.

After successful receive and activation, the temporary source snapshot may
be deleted by its exact recorded path to free a subvolume slot. Keep the
received active tree read-only. Never prune the active or designated rollback
release, or unrelated Incus/cache storage.
