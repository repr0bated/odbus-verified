# etched::hackathon — what to do before going further

Owner direction, 2026-10-30. The chatbot is the root of this system. The
hackathon subsidiary is a clone of that chatbot, named `etched::hackathon`.
The clone is gated on `etched0`, with internet only and no network path to
3tched. Every method is a file on its 3tchedFS mount.

One repo comes first. Do not register the clone, start it, or retarget the
mount until that consolidation is the tree you mean to keep. This file is the
piece that goes in first.

Checked on the host 2026-10-30. **Proven** means a command showed it that day.
**Planned** means the owner still has to do it.

## What you need to do

1. Finish bringing the trees into the one repo. Leave the clone stopped while
   you do that.
2. Make `etched::hackathon` a real identity. The bridge will not do this.
   `provision_container` stops with: prepare the shared physical seed, the
   private writable leaf, and the container outside the bridge; the bridge
   cannot compose, mount, or copy an identity root. A new public key is
   accepted only when it is already an active registered principal. Register
   public key `Wycws7Udq0XTSN7I45JPP+CL9ZS4MxhXlhM4zAn8FX0=`, then run the
   external seed provision so the sled binds session
   `cc59ab57-d94a-8334-222b-8077ef033320` to Incus instance `etched-hackathon`.
   Do not edit sled or Cozo rows by hand. Do not add a Btrfs member from
   login or start.
3. Prove the binding. A `get_own_identity` call on a mount whose
   `--lungs-session` is `cc59ab57-d94a-8334-222b-8077ef033320` must return that
   session. Until it does, leave `/etc/sv/subsidiary-hackathon-3tchedfs/run`
   on the root session `b0c777e3-0955-cd8b-3d01-ca12998cae28`.
4. After that proof, change `SESSION=` in that run script to the clone
   session and restart `subsidiary-hackathon-3tchedfs`. A call as Linux user
   `3tched` through `/home/3tched/Hackathon/3tchedfs` must then return the
   clone session.
5. Internet on `etched0` is a later step. The bridge is down, fail-mode
   `secure`, with no address and no uplink, so the clone's NIC cannot reach
   3tched and cannot reach the internet. Do not rebuild `etched0` with
   `ovs-vsctl` (the 2026-10-28 correction in
   `docs/src/operations/network.md`). `openflow.write` stays withheld until
   enforcement refuses a flow on any bridge other than `etched0`. Do not put
   the old `hackbr0` NIC back on the root chatbot.

Leave the root chatbot as `3tchedCtlChatbot`, session
`b0c777e3-0955-cd8b-3d01-ca12998cae28`. It is the root, not the subsidiary.

## New

New on 2026-10-30, because the sealed identity path cannot assign the key:

| Item | Value |
|---|---|
| Display name | `etched::hackathon` |
| Incus name | `etched-hackathon` (a name cannot contain `::`) |
| State | Stopped. **Proven.** |
| Session | `cc59ab57-d94a-8334-222b-8077ef033320`, Blake3 `derive_session_id` of the new public key |
| Public key | `Wycws7Udq0XTSN7I45JPP+CL9ZS4MxhXlhM4zAn8FX0=` |
| Private key | `/root/etched-hackathon.key` inside the container, mode 600, root. The host copy was removed. |
| Disk | Its own root volume on pool `3tched`. The root chatbot's persist volume is not attached. |
| Profile | `3tched-lungs`, so a later start can see the lungs socket |
| NIC | `eth0`, bridged, parent `etched0` only. Not `ovsbr0`, not `hackbr0`. |
| Sled | Not registered. This container is not yet an identity. |

The container is a wholesale copy of `3tchedCtlChatbot` with the name and
keys changed. It is not a seed provision and it is not a second root.
Super-admin is a role above root, and it belongs to this second chatbot.
The core chatbot remains root. It keeps the WireGuard server key, so it
cannot be a peer. This VPS has no WireGuard, so that root identity stays
sealed and detached.

A root command runs only when two super-admins agree. The chatbot does not
execute that command. It delegates the command to authorized agents.

Accountability is a chat window. It runs a semantic search over the
vectorized mutation record and asks the chatbot who, when, why, and what
caused it. The chatbot is put on the stand as the first witness. The
existing accountability mode is that window: a read-only ledger view. The
session ledger is the record that gets vectorized.

The next piece is seeing the chatbot authenticate. Before it answers as
first witness, the window shows that authentication: the session derived
from its key. A witness that has not authenticated does not answer.

## Old and existing

These were already on the host. They were not replaced.

| Item | What it still is |
|---|---|
| Root chatbot | `3tchedCtlChatbot`, running, display name `control-plane-chatbot`, session `b0c777e3-0955-cd8b-3d01-ca12998cae28`, public key `Env5zPMmM1Zhypu5/DY/yU4PRYW/4AOEq2KZ4OSc+18=`, persist volume `3tchedCtlChatbot-persist`. **Proven** still running after the copy. |
| Hackathon mount | `/home/3tched/Hackathon/3tchedfs`, runit `subsidiary-hackathon-3tchedfs`, Linux user `3tched` (uid 1001). **Proven** mounted. `SESSION=` is still the root session. |
| `etched0` | OVS bridge, fail-mode `secure`, no address, no uplink, no extra ports. Created 2026-10-28 with `ovs-vsctl`, so it has no identity and no receipt. |
| `hackbr0` | The 2026-10-28 internet-only Incus bridge. It is down. Its NIC is not on the root container and not on the clone. |
| Other instances | `3tched-container` and `chatbot-template-sysprep` are stopped. `qdrant-recovered` is running. None of them is the subsidiary. |
| Identity rule | The sled is the identity. Session id is derived from the public key. The bridge verifies storage; it does not compose a root. |

The 2026-10-28 docs said the hackathon subsidiary *was* `3tchedCtlChatbot`.
That sentence described the mount that is still live. It is not the owner
model as of 2026-10-30.

## What changed in the old

Brought forward only far enough for the clone to exist beside the root.
Nothing below re-keyed the root or moved the live mount.

| Old thing | Change on 2026-10-30 |
|---|---|
| `3tchedCtlChatbot` | Copied to `etched-hackathon` with `incus copy --instance-only`. The source was left running, on the same session, with the same public key and the same persist volume. |
| Copy's inherited identity labels | Display name, public key, session id, and `lxc.uts.name` set to the clone. `user.opdbus.creation_digest` and `user.opdbus.creation_operation` unset so the copy does not claim the root's creation receipt. |
| Copy's `persist` device | Removed, so the clone does not mount `3tchedCtlChatbot-persist`. |
| `etched0` | The bridge was not edited. The clone's Incus NIC names it as parent. While the clone is stopped, `etched0` still has no extra ports. |
| Hackathon run script and mount | Unchanged. Calls as `3tched` are still the root chatbot. |
| `docs/SUBSIDIARY-SLED.md`, `docs/RUNTIME.md`, `docs/DEV-CYCLE.md` | The old "the subsidiary is the root container" lines now point here. |
| `docs/src/operations/network.md` | A 2026-10-30 note records the clone NIC. The 2026-10-28 `etched0` and `hackbr0` notes stay as history. |

## Also on the host, not the subsidiary

Stale release-kit copies from 2026-10-18 and 2026-10-23 were removed because
their `op-release` binaries did not match the installed kit. The installed
kit that remains is `/usr/local/libexec/opdbus-release/op-release`,
`op-release-launch`, and `/etc/init.d/op-release-activate`, with
`manifest.sha256` matching those three. Slot B
`release-640f40aa-7b7a-446e-a614-3aecb7771d90` was left mounted. The
unfinished journal at `quiescent-delete:489` for candidate
`release-97d42309-3d63-4227-ac48-23f492466652` was left in `slots.json`.
That journal is not this subsidiary.
