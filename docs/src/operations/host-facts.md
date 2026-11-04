# Host facts

The one place for live facts about this host and the decoy. Check here before
asking the owner. When you verify or correct a fact, change it here and date it.
Never put private keys, tokens or passwords in this file.

Facts below were checked on 2026-11-04 unless noted. An entry marked UNKNOWN
needs the owner once, then gets filled in here.

## Decoy (Oracle VPS, the only WireGuard server)

| Fact | Value |
| --- | --- |
| Addresses that answer SSH | `10.0.0.1`, `129.80.64.127` (the only public IP, per OCI) |
| Stale address | `129.213.201.220`, times out, not on the instance |
| WARP address | `100.96.0.2`, mesh node `decoy-wg-mesh`, routes `10.0.0.0/24` +3 (owner, Cloudflare mesh list). This host is `3tched-ovs-mesh`, `100.96.0.3`. SSH to it from this host fails with "No route to host", via `10.200.0.254` on `ovsbr0` |
| SSH login that works from this host | `sudo -u jeremy ssh -i ~jeremy/.ssh/decoy_key -o IdentitiesOnly=yes ubuntu@10.0.0.1`. `ubuntu` has passwordless sudo |
| Live `wg-opdbus` public key | `wNhmhGDpdkScmSdcHnzhr/1vmTIL5wxLXcve/jwk9xE=`, private key in `/etc/wireguard/wg-opdbus-server.key` on the decoy |
| OCI access | `~jeremy/.local/share/oci-cli/bin/oci`, instance name `decoy`. `/opt/oci-cli` is broken. Run Command does not work: agent plugins report `INVALID` |

## The demo

The demo shows the embodiment of the control-plane chatbot, enabled by 3tchedFS
(owner, 2026-11-04). The identity acts by writing to its mount's call files and
keeps the receipt. A chat reply alone does not show this.

## Control-plane chatbot

| Fact | Value |
| --- | --- |
| Incus instance | `wheelhouse` (renamed from `CtlChatbot3tched`, owner 2026-11-04) |
| Identity requirement | The same keypair as the decoy `wg-opdbus` server (owner) |
| Correct key | `wNhmhGDpdkScmSdcHnzhr/1vmTIL5wxLXcve/jwk9xE=`, read from the live decoy |
| Correct session | `54291774-4b0a-68b0-b55c-dc544900a8de`, derived with `op-identity session-id v1` |
| Correct principal | `29858abd-4489-5b69-67b9-c5ee951ed6f2`, derived with `op-identity human-principal v1` |
| Attachment | Lungs socket only, `/run/opdbus/lungs/lungs.sock` (owner) |
| Name still used by the `chatbot-identity` service | `3tchedCtlChatbot`, not yet updated |
| Required standing | Unprivileged container, in the 3tched `$ROOT` group and the Linux root group (owner). "Root" is not `security.privileged`, and the prior chatbot was unprivileged |
| `wheelhouse` devices | Instance-level `runtime-run` (tmpfs at `/run`) and `persist` (volume `3tchedCtlChatbot-persist`), plus `lungs-socket` from the profile. `lungs.sock` is visible inside. It has no `user.opdbus.*` identity config yet |
| Who uses this identity | The owner, as the chatbot identity: "my keys are the chatbot's" (owner) |
| Sled role | Captures the identity state and puts it on the `@user` subvolume (owner). Not built yet |
| Browser peer | `OP_BROWSER_CONTROL_PLANE_PEER_KEY=lnF9h…` in `/etc/op-dbus/environment`, the decoy's only peer, at `10.0.0.5`. A signed proof from it selects the chatbot identity |
| Identity the bridge runs on now | `Env5z…`, session `b0c777e3-0955-cd8b-3d01-ca12998cae28`. Not the required key |
| Installed release limit | `61c2827` cannot provision `wNhm…` or accept the name `wheelhouse`. Needs a source change and a release |

Other public keys still recorded for the chatbot. All are wrong for it. `14ja…`
is an unused rotation file on the decoy, and its pinned IDs lost trailing
characters: the full values end `…d63a9ff90e11` and `…bd999410e11a`.

| Source | Public key |
| --- | --- |
| Installed bridge default | `Env5z…` |
| `/etc/op-dbus/environment` | `14ja…`, with a truncated session and principal ID |
| `/etc/op-dbus/secrets/chatbot-wg.key` | derives to `V94Y…` |

## Sockets

`/run/opdbus/grpc.sock` and `/run/ghostbridge/container.sock` should not exist
(owner). The installed bridge recreates them because `crates/op-grpc-bridge/src/server.rs`
defaults to them. The removal is uncommitted in `/srv/git/odbus-remove-drift-sockets`
and is not released. Deleting the files does not last.

## Storage images

Both images are in `/run/mounts/seed-3tched`, on LV `3tched-vg/3tched-seed`.

| Image | Layout |
| --- | --- |
| `seed.img` | No partition table. Blank Btrfs `3tched-base` at 1 MiB with no seeding flag. Ext4 `3tched-upgrade` at block 2883840 (4 KiB blocks), which cannot take a `btrfs receive` |
| `persist.img` | GPT. Partition 1 ext4 `persist_ext4`, partition 2 Btrfs `persist_btrfs`. Both empty. Attached once, on `loop2`. Not mounted on the host and not attached to `wheelhouse` |

Purpose of the `persist.img` partitions (owner, 2026-11-04):

- `persist_ext4` is for RocksDB. It is ext4 so RocksDB performs better and does not fight Btrfs copy-on-write. It is not a Cozo store.
- `persist_btrfs` is added with `btrfs device add`, run inside Incus.

Btrfs guard, tested 2026-11-04 with throwaway images. The guard is in GLOBAL
mode and trusts only listed cgroup and filesystem pairs.

| Where | Mount | Subvolume create | `btrfs device add` |
| --- | --- | --- | --- |
| Host root | Works | Denied | Not tried |
| Unprivileged container | Denied | Not reached | Not reached |
| Unprivileged, mount interception for `btrfs,ext4` | Works | Denied | Denied |
| Privileged container | Works | Denied | Denied |

## Network

| Fact | Value |
| --- | --- |
| `ovsbr0` addresses | `10.0.0.2/32`, `10.200.0.1/24`, `100.96.0.3/32` |
| Public address | `188.68.58.237/22` on `pub0` |
| op-web page | `http://10.0.0.2:8080/`. `10.0.0.4`, as written in `AGENTS.md`, is not on this host and does not answer from the decoy |
| Door | `https://10.0.0.2:8090`, answers from this host and the decoy |
| Route to `10.0.0.1` | via `10.200.0.254` on `ovsbr0` |
| Route to `129.80.64.127` | `reality0` |
| `gre-decoy` | Not present on this host, although `AGENTS.md` describes it |
| Service manager | runit, on Devuan. `AGENTS.md` still says Artix |
