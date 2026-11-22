# Identity container provisioning (antix1)

Untracked operator procedure. Lives at `/srv/git/odbus-verified/deploy/provisioning/`
next to `deploy/scripts/provision-container.sh`, with a copy in
`/home/jeremy/provisioning/`. Not committed. Every step is `sudo`, idempotent where
possible, never deletes a seed or a container, and starts nothing unless `START=1`.

## How the bridge finds a container (why this exists)

* The host `op-grpc-bridge` scans `/var/lib/incus/storage-pools/$OP_CONTAINER_POOL/custom/`
  for `default_identity-<session>` volumes (`crates/op-grpc-bridge/src/runtime_identity_owner.rs:305-329`).
* For each one (`adopt_identity_volume`, same file `:331-356`) it requires:
  `session_id` in the sled == volume name; **exactly one** Incus instance whose config
  has `user.opdbus.session_id == <session>` (`op-plugins/src/state_plugins/incus.rs:1723-1731`;
  otherwise the warning `session … has N containers, expected one`, `:339`);
  that instance is a container carrying `user.opdbus.wireguard_public_key`, the key derives
  the session; `issued_identity.iid1` verifies against the decoy trust store; `sealed_id`
  equals the SID1. Then it commits the Cozo identity record (`commit_volume_identity`, `:358-408`).
* It runs at bridge startup (`runtime.rs:413`) **and** whenever a container connects to the
  host `lungs.sock` and asks to bind (`runtime_lungs_identity.rs:268-277`) — so a container
  provisioned for a session with no identity record yet is adopted on first contact; the
  host bridge does not need a restart.
* **Re-provisioning an existing identity is not adopted.** Once the Cozo record holds an
  instance, adoption requires the same instance name and the same `volatile.uuid`
  (`runtime_identity_owner.rs:496-499`, `startup import cannot replace an existing native
  identity UUID`). Deleting a container and running `30-provision-container.sh new` with the
  same keys creates a new UUID, so the host bridge refuses it. There is no sealed route that
  re-provisions the chatbot from `@seed` on `3tched-pool`. The sealed `identity_sled`
  `creation_mode: "replace_chatbot"` path (`runtime_identity_owner.rs:1106-1120` →
  `runtime_identity_clean.rs:573-760`) does not create a container. It adopts an existing,
  operator-prepared, stopped instance described by a root-written plan at
  `/var/lib/op-dbus/identity-provision-plans/<session>.json` (`CleanPlan`, `clean.rs:56-79`:
  physical seed device and generation, leaf image, seven member subvolumes, instance and ETag).
  `plan_sha256` is that file's SHA-256, not a preflight output. `native_proof` requires the
  plan's pool to use the `dir` driver (`clean.rs:467-471`); the only plan writer,
  `deploy/scripts/prepare-chatbot-adoption.sh`, targets the retired `identity-roots` GPT layout.
  `inspect_provision_preflight` only reports the expected revision and the previous instance.
  Both methods need the configured chatbot key (`clean.rs:588-592`) and a verified caller holding
  `mut.service.identity-sled.container.provision@v2` with its own committed identity
  (`runtime_identity_seed.rs:84-111`). `upgrade_own_native_root` upgrades the caller's own human
  root and requires `OP_IDENTITY_GOLDEN_*` to already name the new seed
  (`runtime_identity_upgrade.rs:70-78`); it does not move that pin.
* Reference script for the whole flow: `deploy/scripts/provision-container.sh` (systemd
  Ubuntu seed, names the chatbot `3tchedChatbot`, which on antix1 collides with the old
  stopped container — hence the `NAME` parameter here).

## Steps

### LXD subsidiary from the golden snapshot

```sh
./deploy/provisioning/30-provision-container.sh --subsidiary 3tchedChatbot \
    --name kiroween --lxd-pool <pool> --image <full-image-fingerprint> --dry-run
```

The image must already be published privately in LXD from the golden live
snapshot. Its `opdbus.golden.commit` and `opdbus.golden.subvolume` properties
must match the golden state file. Remove `--dry-run` to create the stopped
instance through the mounted `lxd` plugin. The subsidiary has a root disk
and a parent reference. It stays pending until its own identity is admitted.
After admission, grant policy can assign the child principal to
`$SUBSIDIARY`. Root inside the subsidiary is UID 0 in that container's own
user namespace. The isolated ID mapping keeps that authority inside the
container.

| Authority | Scope |
|---|---|
| Guest UID 0 | Manage local users, packages, services, and files on the subsidiary's writable volume. |
| `$SUBSIDIARY` | `cap.identity.sled.self.read@v1` for the admitted child principal's own identity. The group name and LXD labels confer no authority by themselves. |
| Application access | Grant exact capabilities to the child principal after checking its approved scope. For example, the sealed `json_render.read` capability covers reading and validating the UI schema. Keep it separate from the baseline group. |
| Host operator | Provision, attach devices and networks, admit the child identity, assign memberships, start, snapshot, and roll back through the sealed operations. |

Check each application grant against the parent allowance and the child's
required methods. Broad capabilities such as LXD instance management, host
shell, secrets, grants, and network administration belong outside this group.
An own-instance management capability would need server-side validation of
the verified identity against the target on every call before it could be
granted.

Pending instances have a read-only root disk, no inherited profiles or
additional devices, an isolated ID map, and a start lock. The provisioner
disables nesting, the LXD guest API, and syscall interception, and sets
limits of 2 CPUs, 2 GiB hard memory, and 512 processes. It reads the expanded
instance configuration back after creation.

Admission is a separate step. Before lifting the start lock, verify the
child's own Decoy-issued identity, attach its writable overlay and
identity-bound 3tchedFS projection, and verify any dedicated OVN egress
against the backend deny policy. Keep host management sockets and parent
storage outside the subsidiary. The creation command does not perform
admission or start the instance.

`tests/mock-lxd.sh` is an offline fixture for the image lookup, stopped
creation, operation wait, and instance readback. It reads a fixture
`image.json` and writes `instance.json` under `LXD_MOCK_FIXTURE`. Its
responses are marked `mock: true` and carry no live admission.

| step | script | what |
|---|---|---|
| 1 | `10-seed-acquire.sh check` | **Case A**: a seed already exists (`images/@seed` Ubuntu 26.04 systemd, `images/@antix-seed` Debian trixie runit). |
| 1 | `10-seed-acquire.sh receive <file>` | **Case B1**: `btrfs receive` a sent seed (e.g. `@seed-send` from sdb3). Received ro subvolume kept; a writable copy becomes the seed. |
| 1 | `10-seed-acquire.sh image <raw.img>` | **Case B2**: copy the rootfs out of a raw image (e.g. `/boot-disk/blob/seed-blob.img`, read-only loop). |
| 1 | `10-seed-acquire.sh debootstrap [suite]` | **Case B3**: fresh Debian minbase + runit (fresh install / DR / client wants another image). |
| 2 | `20-seed-overlay.sh [--toolchain]` | Add the extras to a **runit** seed, then seal it read-only (details below). Takes a ro backup snapshot `images/<seed>.pre-overlay-<stamp>` first. |
| 2b | `25-build-in-chroot.sh` | Release design: cargo build inside a chroot of the seed OS (needs `20-seed-overlay.sh --toolchain`), then re-run step 2 with `BIN_*` pointing at the chroot build. **Required for `@antix-seed`** (glibc below). |
| 3 | `30-provision-container.sh new <pubkey> <iid1> [NAME]` | New container from the sealed seed + new identity sled volume, labels, devices. |
| 3 | `30-provision-container.sh complete <NAME>` | Existing container that already has its sled (e.g. `3xcontrolchatbot`): verify sled, set labels, add missing devices. |
| 3b | `35-refresh-binaries.sh <NAME>` | Existing stopped container: copy current `3tched-fs` (dup-path fix), `op-grpc-bridge`, `op-tunnel-door`, `op-identity-accept` into its rootfs; old files kept as `.pre-<stamp>`; refuses if the rootfs glibc is too old. |
| 3c | `sv/threetched-gateway` → `pair-threetched-gateway.sh` (via `sv/threetched-pair`) | **Host, before any bridge**: zero claw (`threetched`) in gateway mode on `127.0.0.1:8084`, then pair once → `/etc/op-dbus/secrets/threetched-runtime-token`. See "Zero claw gateway + pairing". |
| 4 | `START=1 …` / `incus start <NAME>` | Start. In-container runit brings up mounts → bridge; host bridge adopts the sled on the first lungs bind. |
| 5 | `40-verify.sh [NAME]` | Read-only: seeds ro state, sled→container count (must be 1), devices, in-container service status, threetched gateway health + runtime-token file presence/mode (never its contents). |

Select the seed with `SEED_NAME=@antix-seed` (default) or `SEED_NAME=@seed`. Settings
(binary paths, pool, NextDNS version/checksum) are in `lib.sh`.

"Registering with Incus": the seed is not an Incus *image*; it is a read-only subvolume
in Incus volume layout (`metadata.yaml` + `rootfs/`) under `images/` on the pool.
Containers are `incus create --empty` placeholders swapped for a writable CoW snapshot of
the seed (`op-container from-seed` for `@seed`, `crates/op-container/src/seed.rs:56`;
the same swap by hand for other seeds, in step 3).

## What the image carries (step 2, runit seed)

Port of `deploy/seed/rootfs/` (systemd units) to runit (`runit/`):

| runit service | replaces | does |
|---|---|---|
| `core-services/90-opdbus.sh` (stage 1) | `tmpfiles.d/opdbus-sockets.conf` | `/run/opdbus -> /opt/run-mounts/opdbus`, mountpoints, `/dev/shm/opdbus/state`, hostname |
| `opdbus-lo` | `opdbus-lo.service` | **networking**: no NIC; `10.0.0.2/32` on `lo` for the door |
| `door-tls-init` | same | door TLS pair generated inside the container; cert exported to host `/run/opdbus/ct/<name>/door.crt` |
| `3tchedfs` | `3tchedfs.service` | capture + mount `3tchedFS` (surface `3tched`) at `/run/mounts/3tchedFS` over the host lungs socket |
| `threetchedfs` | `threetchedfs.service` | capture (`--include-plugin threetched`) + mount `threetched` (surface `3tched`: guests do not ship the threetched CLI) at `/run/mounts/threetched`; needs the duplicate-path-fixed `3tched-fs` (host `3tched-fs-dupfix-b37bffa2`) |
| `op-grpc-bridge` | `op-grpc-bridge.service` | waits for both mounts + lo + TLS, loads `/etc/op-dbus/bridge.env`, runs the existing `op-grpc-bridge` binary on `10.0.0.2:8090` |
| `op-tunnel-door` | same | chatbot only (tunnel device present): splices host door socket into the container door |
| `threetched-gateway` (`sv/`, host template) | `deploy/runit/threetched` (`daemon`) | zero claw **gateway mode** on `127.0.0.1:8084` with `/var/lib/zeroclaw` |
| `threetched-pair` (`sv/`, host template) | (new) | waits for the gateway, pairs once, writes the runtime token, marks `/run/threetched-paired` |
| `nextdns` | (new) | official NextDNS client; profile + device read from the identity sled at start (below) |

Each service has `log/run` → `svlogd -tt /var/log/<service>`. Base packages checked/installed
in a chroot: `iproute2 openssl fuse3 util-linux runit curl`. Binaries copied in:
`3tched-fs`, `op-grpc-bridge`, `op-tunnel-door` (`/usr/local/libexec/op-dbus/bin/`),
`op-identity-accept` (`/usr/local/bin/`), `nextdns` (`/usr/bin/`).

The Ubuntu/systemd `@seed` already carries the systemd versions of these units
(`deploy/seed/README.md`); it has no `nextdns` service yet.

### Devices per container (step 3)

| device | source (host) | path (container) |
|---|---|---|
| `fuse` | `/dev/fuse` | `/dev/fuse` |
| `opdbus-run` | `/run/opdbus/ct/<name>` | `/opt/run-mounts/opdbus` (shift) |
| `lungs` | `/run/opdbus/lungs` | `/opt/run-mounts/opdbus/lungs` (ro, shift) |
| `shm` | `/run/opdbus/shm/<name>` (1777, `opdbus/plugin-blobs` pre-created) | `/dev/shm` (shift) |
| `catalog` | `/var/lib/opdbus/plugin-blobs` (sealed catalog) | `/dev/shm/opdbus/plugin-blobs` (ro, shift) |
| `decoy-trust` | `/etc/op-dbus/decoy-trust.json` | same path (ro, shift) |
| `sled` | pool volume `identity-<session>` | `/agent/user` (ro) |
| `tunnel` | `/run/opdbus/tunnel` | `/opt/run-mounts/opdbus/tunnel` (chatbot key only) |

Mount layering: no mount covers another. Each parent is mounted before anything under it:
`opdbus-run` before `lungs`/`tunnel`, and `shm` before `catalog` (Incus orders disks by path).
Without the `shm` disk, systemd (systemd seed) mounts its own tmpfs on `/dev/shm` at boot,
covering the catalog, and both 3tchedFS units fail their manifest check. A tmpfs from
`raw.lxc` does not work: Incus writes raw.lxc after its disks and LXC attaches idmapped
disks later, so the catalog target is missing. Host dirs are recreated at boot by
`deploy/runit/boot-run/S96-run-mounts.stage1.sh`.

Config: `user.opdbus.session_id`, `user.opdbus.wireguard_public_key`, `security.idmap.isolated=true`,
unprivileged, no nesting, `boot.autostart=false`, `raw.lxc`: `lxc.uts.name = <session>` (**hostname**),
`lxc.mount.auto = cgroup2:force`, plus `lxc.init.cmd = /sbin/runit-init` for runit seeds.

### Identity sled (`identity-<session>`, mounted ro at `/agent/user`)

`session_id`, `issued_identity.iid1` (decoy-minted, delivered over WARP), `sealed_id`
(SID1 installed by `op-identity-accept`), `ledger/` (written by the host bridge),
`wireguard_pubkey` (public key; written by step 3 `new`). Nothing is minted on antix1:
a new container needs its IID1 from the decoy (`op-decoy-issuer`, which seals the NextDNS
profile and device in; device defaults to the session id).

### NextDNS

* Install: official release tarball `nextdns_<ver>_linux_amd64.tar.gz` from
  github.com/nextdns/nextdns, SHA-256 pinned in `lib.sh` (1.50.2). Equivalent official
  apt route: `https://repo.nextdns.io/deb stable main` with key `https://repo.nextdns.io/nextdns.gpg`.
* At start, `runit/nextdns/run` reads the sled: `op-identity-accept --issued /agent/user/issued_identity.iid1
  --pubkey <key> --trust-store /etc/op-dbus/decoy-trust.json --out /agent/user/sealed_id`
  verifies the IID1 and prints `session_id profile_id device_name`
  (`crates/op-identity/src/bin/op-identity-accept.rs:13,77-82`; same source the old
  `3tched-seed-launch` used). With `--out` = the existing identical `sealed_id` nothing is written.
  The key comes from `/agent/user/wireguard_pubkey`, else `identity_sled.json`.
* Runs `nextdns run -listen 127.0.0.1:53 -report-client-info -profile <id> -forwarder https://dns.nextdns.io/<id>/<device>`.
  nextdns has **no device-name flag**; the device name rides on the DoH path (as the old
  `nextdns.conf.template` did). The ID is never hardcoded or echoed by the run script.
* ⚠ Flag: the container has no NIC (chatbot has no internet; `ID-N` get egress only via
  `ct-egress-relay`), so upstream DoH needs that egress path. Pointing the container's
  `/etc/resolv.conf` at `127.0.0.1` (template `/etc/resolv.conf.opdbus`) is left to the operator.
  nextdns' own log lines may include the profile in its DoH URL; `/var/log/nextdns` stays in the container.

### Runtime token  (blocker resolved by step 3c)

The bridge reads `THREETCHED_RUNTIME_ENDPOINT` (default `http://127.0.0.1:8084`),
`THREETCHED_RUNTIME_AGENT` (default `dashboard`) and the token from
`/etc/op-dbus/secrets/threetched-runtime-token` (root-owned, 0400/0600, `[A-Za-z0-9._~-]`,
≤ 8192; `deploy/runit/op-grpc-bridge/run:82-109`). Nothing in the repo generates it: it is
the bearer token the zero claw gateway issues when a client pairs. `pair-threetched-gateway.sh`
produces it (below). Never use `zeroclaw-gateway.token` for it. Until step 3c has run, the
token is absent and chat runtime calls are refused.

## Zero claw gateway + pairing

Zero claw = `/usr/local/libexec/op-dbus/threetched/threetched` (`zeroclaw 0.8.4`; binary side
spelled `threetched`, nothing renamed). It runs **only as a gateway**: all backend model
traffic goes through it, so no `agent`/standalone mode. Config/data dir `/var/lib/zeroclaw`
(`/var/lib/threetched` is a symlink to it): `config.toml` `[gateway] host=127.0.0.1 port=8084
require_pairing=true allow_public_bind=false`; `paired_tokens` are stored encrypted with
`/var/lib/zeroclaw/.secret_key`.

**Gateway command** (`sv/threetched-gateway/run`):

    threetched gateway start --config-dir /var/lib/zeroclaw --host 127.0.0.1 --port 8084 --log-level warn

run as root under `env -i` (HOME=/var/lib/zeroclaw/home, ZEROCLAW_CONFIG_DIR=/var/lib/zeroclaw).
Sources: `threetched gateway start --help`; ZeroClaw `src/main.rs:4013-4021` (`start` →
`run_gateway_if_enabled`; unlike `gateway restart`, `main.rs:3848-3864`, it does not shut down
another gateway). Readiness `check`: `GET /health` (`crates/zeroclaw-gateway/src/lib.rs:1640`).
The run script refuses to start if something already listens on 8084.

**Pairing** (`pair-threetched-gateway.sh`, run by `sv/threetched-pair` or by hand as root):

1. `POST http://127.0.0.1:8084/admin/paircode/new` → `{"pairing_code": …}`. Localhost-only;
   generates a fresh one-time code **without revoking** existing tokens
   (`lib.rs:1638`, `3984-4120`; CLI equivalent `threetched gateway get-paircode --new`).
   Plain `GET /admin/paircode` / `get-paircode` returns no code once any client is paired —
   antix1 already has paired tokens, so `--new` is the right call.
2. `POST /pair` with header `X-Pairing-Code: <code>` → `{"paired":true,"persisted":true,"token":…}`
   (`lib.rs:1642`, `2242-2348`); the gateway persists the token hash encrypted in `config.toml`.
3. Verify with `GET /api/status` + `Authorization: Bearer <token>` (`api.rs:241-248`).
4. Write atomically: temp file `install -o root -g root -m 0600` in `/etc/op-dbus/secrets`,
   then `mv` over `threetched-runtime-token`.

Code and token live only in 0600 temp files and curl `-H @file` headers (never argv, stdout or
logs). An existing valid, accepted token → no-op (pair once). `--force` pairs an additional client;
revoke an old one with `threetched gateway get-paircode --rotate-device <id>`.

**Ordering:** `threetched-gateway` up (`sv check` = `/health`) → `threetched-pair` (token written,
`/run/threetched-paired`) → `op-grpc-bridge`. The bridge-side wait is in `runit/op-grpc-bridge/run`
(image) and as a snippet for the host bridge in `sv/op-grpc-bridge.gateway-dep/README`.

**Install (not done; templates only):** copy `sv/threetched-gateway` and `sv/threetched-pair` to
`/etc/sv/`, `pair-threetched-gateway.sh` to `/usr/local/libexec/op-dbus/provisioning/`, add the
snippet to the bridge run script, then link into the runsvdir.

**Uncertain / check before enabling:**
* The repo's own service (`deploy/runit/threetched/run:354-355`) runs `threetched daemon` on the
  same port 8084 (daemon = gateway + channels + heartbeat + cron, per `daemon --help`) and patches
  provider keys into the config at every start. Run **one** of them, not both. The gateway template
  relies on provider settings already persisted in `config.toml`.
* ZeroClaw source read is the golden-run checkout (`/var/tmp/golden-run-20261118-native-recovery.0NqTu0/source`);
  its CLI help matches the installed binary, but its exact commit vs the binary was not proven.
* The gateway is loopback on the host. A bridge **inside** a NIC-less container cannot reach host
  `127.0.0.1:8084`; container use needs a relay (like `ct-egress-relay`) or a gateway in the image.
* `@antix-seed` still carries the earlier `runit/op-grpc-bridge/run` (no gateway wait); refresh with
  `UNSEAL=1 20-seed-overlay.sh` when approved.

### gRPC libraries

Both are **required**:

1. **Tonic** (hyperium/tonic; workspace currently `tonic 0.12` + `prost`, `tonic-web`,
   `tonic-health`, `tonic-reflection`) — what the current `op-grpc-bridge` binary is built on.
2. **The official gRPC Rust implementation**, crate `grpc` **0.9.0**, repo
   https://github.com/grpc/grpc-rust (crates.io lists that repo for `grpc` 0.9.0).

These are Rust crates compiled into the binaries, not runtime packages: the image needs
nothing extra to *run* the bridge. If the image builds the bridge in-chroot (approved
release design: cargo inside a chroot of the subvolume OS), use `20-seed-overlay.sh
--toolchain`: `build-essential pkg-config protobuf-compiler libprotobuf-dev libssl-dev clang
cmake git` + rustup (stable). **Follow-up (not authorized yet):** `grpc` is not a dependency
in the repo; adding it is a Cargo/code change for Jeremy to approve.

## glibc constraint (found 2026-11-18)

Host-built `op-grpc-bridge` (f5ab3fd6) needs `GLIBC_2.43`. Ubuntu 26.04 `@seed` has 2.43 (OK);
Debian trixie `@antix-seed` has 2.41 → the bridge **cannot run** there with host binaries
(`3tched-fs`, `op-tunnel-door`, `op-identity-accept`, `nextdns` do). Fix = step 2b
(build in the seed's chroot), or an AntiX/Debian base with glibc ≥ 2.43.

## Checklist from the failed `3xcontrolchatbot-seedtest` (read-only inspection, 2026-11-18)

Ubuntu seed, systemd. Its syslog: `3tched-fs: Error: open bounded source
/dev/shm/opdbus/plugin-blobs/.manifest.json: No such file or directory` (704×), both
`3tchedfs`/`threetchedfs` failed, so `op-grpc-bridge.service` failed with `result 'dependency'`.
Its config has only `fuse`, `opdbus-run`, `root`. Missing, all now covered by step 3:
`catalog` device (the direct cause), `lungs` device, identity `sled`, `decoy-trust`,
`user.opdbus.session_id`/`wireguard_public_key`, `security.idmap.isolated`, `lxc.uts.name`;
plus the in-image threetched duplicate-path fix (step 2 copies the fixed `3tched-fs`).

## Rollback

* Seed overlay: `btrfs property set -ts images/<seed> ro false`, then replace from the
  `images/<seed>.pre-overlay-<stamp>` snapshot (manual; never automatic).
* Labels/devices: `incus config unset <n> user.opdbus.session_id user.opdbus.wireguard_public_key`,
  `incus config device remove <n> <dev>`; prior config saved in `runs/<n>.config.before-<stamp>.yaml`.
