# Incident Response

## 2026-10 btrfs fiasco recovery — seed/upgrade partition rebuild

Live handoff for the in-progress recovery from the severe btrfs fiasco
(system was recovered from a near-total loss; original recovery used
`testdisk` plus about 3 hours of manual btrfs surgery). This section is
updated as the recovery continues across sessions — treat it as the current
state, not a historical record, until the chatbot identity is fully
re-provisioned and this note says so.

### Root cause (confirmed)

The original fiasco traced to loop-device UUID collisions across the
identity provisioning images (legacy `identity.img` / dedicated
`leaf_device` scheme). A separate, unrelated regression (OD-104) also
existed in the OVS bridge setup, where `tun5` was being incorrectly
enslaved into `ovsbr0`; that has been fixed as part of the 2026-10 bridge
rebuild (`op-ovsbr0-setup --seed-only`, `ovsbr0-cutover` — see
[Network Operations](network.md) and `deploy/runit/retired-services`
for the exact scope of what was retired. The WARP/MASQUE SSH tunnel itself
— `op-warp-ssh-host`/`-routes`, `wgcf` — is a deliberate "looks like
ordinary WARP traffic" security design and must stay live; it was
mistakenly retired once mid-recovery and restored).

### Current state: new isolated storage

To prevent a repeat of the UUID collision, new partitions were created with
distinct UUIDs:

- `/dev/sdc2` — `opdbus-seed`, UUID `80a9c05a-8084-4df0-b4fb-bbc7da671eee`,
  `btrfstune -S 1` SEEDING flag set. Currently has only a bare `@`
  subvolume — not yet populated.
- `/dev/sdc3` — `opdbus-upgrade`, UUID `5a88e7cd-9701-40a7-8301-ef076761886c`,
  bare `@` subvolume, plain rw.

### Open question: are the 7 "siblings" subvolumes or directories?

[Shared Agent Files and Chatbot Alias](agent-files.md) /
[Shared Seed Provisioning](shared-seed-provisioning.md) describes the
seed's top-level layout as `@`, `@agent_memory`, `@agent_rules`,
`@agent_shared`, `@agent_skills`, `@agent_soul`, `@agent_user`. The
`@`-prefix naming looks like the conventional btrfs subvolume-naming
convention, and earlier work in this recovery assumed they are separate
subvolumes.

That assumption was challenged mid-recovery: seeds (SEEDING-flagged
filesystems) may not support creating new subvolumes at all, in which case
the 6 siblings are most likely plain directories inside the seed's single
`@` subvolume. **This must be confirmed empirically** — run
`btrfs subvolume list` against a known-good reference seed — before
building out `/dev/sdc2`'s layout. Do not trust the doc's `@`-prefixed
naming alone as proof they're subvolumes.

### Still unresolved — where the current seed source lives

No `btrfs send` has been run yet. Checked so far and ruled out as sources:

- `core-seed.img` / `router-core-seed.img` (loop-mounted read-only to
  inspect) — wrong structure.
- The current golden release's
  `/opt/op-dbus/releases/<release-id>/usr/share/op-dbus/agents` — plain
  config dirs, not the 7-sibling content.

The actual current seed (whatever is presently backing the chatbot/other
identities, however stale) has not yet been physically located. Check
`btrfs filesystem show`, the chatbot identity's stale
`seed-identities/<id>/state.json`, and `op-identity-root seed-inspect`
output to find it before attempting any `btrfs send` onto `/dev/sdc2`.

Per the operator's own description of the mechanism: *"for each container
the btrfs send to the upgrade partition establishes the pair — that's how
upgrades happen, then it's an overlay."* So the same pairing step also
needs to happen for `/dev/sdc3` once the seed side is settled.

### Chatbot identity — paused, highest caution

The `chatbot` identity (service identity
`b0c777e3-0955-cd8b-3d01-ca12998cae28`, native alias `3tchedCtlbot`) is the
identity being re-provisioned, reusing its existing keys. Its
`seed-identities/<id>/state.json` is from an old, incompatible schema
(dedicated `leaf_device`/`cozo_id` fields, missing `layout`/`seed_fsid`/
`instance_uuid`/`boot_id`). **This is believed to be the exact point where
the original fiasco occurred.** Do not fabricate missing fields to force it
into the new schema — reconcile only with explicit direction, and confirm
the subvolume-vs-directory question above first.

### Also still pending

- `disk-reclaim` script has a stale path (`/srv/git/odbus` instead of the
  renamed `odbus-verified`) and is not scheduled anywhere (cron/runit).
- `tun5`/`tun_id: 5` naming should be renamed to avoid reading as GRE —
  update `/etc/op-dbus/warp-ssh.json` and dependents once the service is
  otherwise stable.
- `op-grpc-bridge` and `tched-router` need to be verified end-to-end once
  the seed work above lands.
- Cozo system graph import (17,064 nodes / 17,356 edges) via
  `cozo.replace_system_graph` — queryable only through D-Bus once
  `op-grpc-bridge` is actually running (there is no standalone Cozo CLI).
