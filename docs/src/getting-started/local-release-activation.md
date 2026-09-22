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

Keep at most five release/cache subvolumes, including temporary send and
receive copies. Count paths under the release root explicitly: `btrfs
subvolume list -o` on an ordinary directory lists descendants of its
containing subvolume, including unrelated Incus storage. Existing unrelated
subvolumes are migration debt, not permission to delete storage. Do not add
subvolumes for recovery records or runtime state. Retain the previous active
release for rollback; retire only explicitly identified obsolete releases.

## Prepare

Build the native glibc workspace, prepare its mail and pinned UI/native
artifacts, and explicitly seal/persist with the freshly built `opblob` and
the service's schema-shaping environment. Preserve the previous catalogs and
review additions/removals before a live reseal. `build-golden.sh` accepts
`OP_PERSISTED_CATALOG_SOURCE` for isolated release preparation.

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

## Transfer

Run these as deliberate operator steps after checking the subvolume budget
and the golden manifest. The release ID contains only letters, digits,
hyphens and underscores, and must not already exist at either destination.

```sh
release_id="$(date -u +%Y%m%dT%H%M%SZ)-$(git rev-parse --short HEAD)"
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
| `libexec/3tched/<helper>` | `/usr/local/libexec/3tched/<helper>` for activated service dependencies |
| `sv/<service>/{run,check,finish,log/run}` | Corresponding files in `/etc/runit/sv/<service>` |
| `etc/opdbus/{capability-grants,mcp-audience-policy,mcp-toolsets}.json` | Corresponding protected policy files in `/etc/opdbus` |
| `etc/op-dbus/zeroclaw-runtime.toml` and skill bundle | Corresponding bootstrap files in `/etc/op-dbus` |
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

## Roll back

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
