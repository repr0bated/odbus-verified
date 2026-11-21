# Requirements Document — provisioning `--system` and `--subsidiary` modes

Status: SPEC ONLY, NOT APPROVED FOR IMPLEMENTATION. Owner request of
2026-11-19 ("take care of provisioning script adding --system (no identity and
gated tightly to the single function) and --subsidiary that (export?) incus
identity container > lxd. there is a very detailed plugin for lxd").
Implementation waits until after the current build (owner, 2026-11-19).

Spec id: `provisioning-system-subsidiary`
Spec home: `.kiro/specs/provisioning-system-subsidiary/`
Source material: a fresh read of `origin/dev` `8da14f6`
(`deploy/provisioning/*`, `deploy/scripts/provision-container.sh`,
`deploy/scripts/provision-workspace-subscriber.sh`,
`crates/op-plugins/src/state_plugins/lxd.rs`) and a read-only listing of the
live mount `/run/mounts/3tchedFS/plugins/{incus,lxd}/operations/*/metadata.json`
on antix1. Nothing was provisioned, exported or called.

Related specs, which this one does not override:
`.kiro/specs/mcp-door-adapter/` (tool classes, deny-list, grants and tickets),
`.kiro/specs/identity-bound-fuse-application-migration/` (identity on the
mount; Wonder's shared FUSE contracts for identity binding, capability checks,
toolset projection and receipts), `.kiro/specs/cockpit-container/` (identity
sled).

## Introduction

`deploy/provisioning/30-provision-container.sh` provisions identity
containers today (`new`, `complete`): every container gets an identity sled
(`identity-<session>` at `/agent/user`), an IID1/SID1, `/dev/fuse`, the host
`lungs` socket, the sealed catalog and the decoy trust store. This feature adds
two modes to that script:

1. `--system <function>`: a system container with **no identity** that runs
   exactly one declared function, with at most one control-plane capability,
   a locked-down instance configuration, minimal devices and no general network.
2. `--subsidiary <parent>`: a copy of an existing Incus identity container,
   exported and imported into LXD through the sealed `lxd` plugin's FUSE
   operations, bound to a **child identity** derived from (not copied from) the
   parent, with grants that are a strict subset of the parent's.

Both modes have a dry-run that prints every planned Incus/LXD call, host step
and FUSE call payload without executing anything.

Out of scope: running provisioning on any host; creating, starting, exporting
or importing real containers; changing grants (the script only proposes grant
deltas for owner approval); minting identities (the decoy mints); the bridge,
3tchedFS / `threetched-fs`, FUSE code, pools, the seed, the overlay, ZeroClaw,
`cozo-ublk`, the kernel and sealed blobs; installing an LXD daemon; resealing
capabilities; changing `provision-workspace-subscriber.sh --system`.

## Requirements

### Requirement 1 — mode selection and dry-run

**User Story:** As the operator, I want `--system` and `--subsidiary` as
explicit modes of the existing provisioning script with a dry-run, so that I
can review exactly what would happen before anything changes.

#### Acceptance Criteria

1. WHEN the script is called with `--system <function>` or `--subsidiary <parent>` THEN THE SYSTEM SHALL run only that mode and SHALL leave the `new` and `complete` behaviour byte-for-byte unchanged.
2. IF more than one of `new`, `complete`, `--system`, `--subsidiary` is given THEN THE SYSTEM SHALL exit non-zero before any external call.
3. WHEN `--dry-run` is given (or `DRY_RUN=1`) THEN THE SYSTEM SHALL print, in order, every planned `incus`/host step and every FUSE call as `plugin.method` plus its JSON payload, and SHALL execute no mutating command and write no call file.
4. WHILE in dry-run THE SYSTEM SHALL still perform the same input validation and fail with the same messages as a real run.
5. WHEN not in dry-run THEN THE SYSTEM SHALL require root and SHALL start nothing unless `START=1`, as `new` does today.
6. IF any step fails THEN THE SYSTEM SHALL stop, leave any created object stopped for inspection, and never delete or recreate automatically.
7. THE SYSTEM SHALL print no secret material (private keys, tokens, IID1 contents) in normal, dry-run or error output.

### Requirement 2 — `--system`: no identity

**User Story:** As the owner, I want system containers to carry no identity,
so that a single-purpose service can never act as a principal.

#### Acceptance Criteria

1. WHEN `--system` runs THEN THE SYSTEM SHALL create no identity volume, no sled device, no `/agent/user`, no IID1/SID1, no `user.opdbus.session_id` and no `user.opdbus.wireguard_public_key`.
2. IF any identity option (`--pubkey`, `--iid1`, `--session-id`, `--child-key`, `--child-iid1`) is given with `--system` THEN THE SYSTEM SHALL exit non-zero before any external call.
3. THE SYSTEM SHALL NOT attach `/dev/fuse`, the host `lungs` socket, the sealed catalog, the decoy trust store, the door tunnel or `/run/opdbus/ct/<name>` to a system container.
4. THE SYSTEM SHALL label the instance `user.opdbus.mode=system`, `user.opdbus.system_function=<function>` and `user.opdbus.function_manifest_sha256=<sha256>`, so the host bridge's identity adoption never matches it.

### Requirement 3 — `--system`: one declared function

**User Story:** As the owner, I want each system container tied to one named
function declared in a reviewed manifest, so that its purpose and reach are
fixed in source.

#### Acceptance Criteria

1. WHEN `--system <function>` runs THEN THE SYSTEM SHALL load `deploy/provisioning/system-functions/<function>.json` and refuse if it is absent, not valid against the manifest format, or its `function` field differs from `<function>`.
2. IF `<function>` does not match `^[a-z][a-z0-9-]{1,30}[a-z0-9]$` THEN THE SYSTEM SHALL refuse.
3. THE SYSTEM SHALL enable exactly one in-container service (the manifest's `service.runit`) and mark every other seed service down in the container's own root, never in the seed.
4. THE SYSTEM SHALL allow at most one control-plane operation (`plugin.method`) per manifest.
5. WHEN the manifest names an operation THEN THE SYSTEM SHALL read that operation's sealed `metadata.json` from the mount and use its `required_capability` as the function's only capability.
6. IF that operation is in the `shell`, `exec`, `admin` or `secret` class (as defined in `mcp-door-adapter` design "Classes") THEN THE SYSTEM SHALL refuse.
7. IF the operation's capability is coarse (not of the form `cap.<…>@v<N>`, e.g. `dnsresolver.write`) THEN THE SYSTEM SHALL refuse unless the manifest sets `coarse_capability_ack: true`, and SHALL print the other operations that share that capability.

### Requirement 4 — `--system`: locked-down instance

**User Story:** As the owner, I want system containers locked down by
default, so that a compromised function cannot escalate.

#### Acceptance Criteria

1. THE SYSTEM SHALL set `security.privileged=false`, `security.nesting=false`, `security.idmap.isolated=true`, `boot.autostart=false` and apply no profiles.
2. THE SYSTEM SHALL keep the default syscall deny list, deny compat syscalls and disable every syscall interception (`mknod`, `mount`, `bpf`, `setxattr`, `sysinfo`), and SHALL disable the guest API.
3. THE SYSTEM SHALL drop the capabilities listed in design "Locked-down configuration" through `raw.lxc`.
4. THE SYSTEM SHALL set `limits.cpu`, `limits.memory` and `limits.processes` from the manifest, with design defaults when absent.
5. THE SYSTEM SHALL attach only the root disk, plus at most: the function's operation projection (Requirement 5), one read-only config directory under `/etc/op-dbus/system/<function>/`, and the network attachment the manifest declares.
6. WHEN the manifest network mode is `none` THEN THE SYSTEM SHALL attach no NIC and no relay socket.
7. WHEN the manifest network mode is `relay` or `nic` THEN THE SYSTEM SHALL attach only the declared relay sockets or one NIC on the declared managed network with the declared ACL, and SHALL refuse `nic` without an ACL.

### Requirement 5 — `--system`: grant scoped to the function

**User Story:** As the owner, I want a system function's grant to be exactly
its one capability, and to approve it myself, so that grants never widen
silently.

#### Acceptance Criteria

1. THE SYSTEM SHALL NOT write, edit or materialize any grants document.
2. WHEN the manifest names an operation THEN THE SYSTEM SHALL write a grant delta proposal for the service principal `system:<function>` containing exactly that one capability, and print its path.
3. IF the per-function operation projection required by the design is not available on the host THEN THE SYSTEM SHALL refuse a real run for a function with an operation and name the missing contract; a function without an operation SHALL still be provisionable.
4. THE SYSTEM SHALL expose the operation to the container only through that projection, never through a full 3tchedFS mount, the lungs socket or the catalog.

### Requirement 6 — `--subsidiary`: parent preflight

**User Story:** As the operator, I want the script to prove the parent is a
real identity container before copying it, so that only admitted identities
get subsidiaries.

#### Acceptance Criteria

1. WHEN `--subsidiary <parent>` runs THEN THE SYSTEM SHALL verify that the parent exists in Incus, has a `sled` device, a `user.opdbus.session_id` equal to the sled's `session_id`, and is the only instance with that session label.
2. IF the parent is a system container or already a subsidiary THEN THE SYSTEM SHALL refuse.
3. THE SYSTEM SHALL read the parent's grants by capability name only and never print grant document internals beyond the capability list.

### Requirement 7 — `--subsidiary`: export and import into LXD

**User Story:** As the owner, I want the parent copied into LXD through the
sealed `lxd` plugin, so that LXD is driven by the same governed control plane.

#### Acceptance Criteria

1. THE SYSTEM SHALL copy (never move) the parent: the parent's root, config, sled and labels SHALL be unchanged except for one snapshot that is kept and listed in the receipt.
2. THE SYSTEM SHALL export from a stopped staging copy that has no identity devices (`sled`, `decoy-trust`, `lungs`, `catalog`, `fuse`, `opdbus-run`, `tunnel`) and no `user.opdbus.*` labels.
3. WHERE an Incus operation exists in the `incus` plugin THE SYSTEM SHALL call it through `plugins/incus/operations/<method>/call`; WHERE none exists THE SYSTEM SHALL use the native `incus` command and list it in the receipt as a plugin gap.
4. THE SYSTEM SHALL perform every LXD step through `plugins/lxd/operations/<method>/call` and SHALL never invoke `lxc` or `lxd` directly.
5. THE SYSTEM SHALL import into LXD with a locked-down profile equivalent to the identity profile (unprivileged, no nesting, isolated idmap) and SHALL NOT start the subsidiary.
6. WHEN the export file is written THEN THE SYSTEM SHALL store it 0600 in a 0700 root-owned directory, record its size and SHA-256 in the receipt, and remove the staging instance.

### Requirement 8 — `--subsidiary`: child identity and grants

**User Story:** As the owner, I want a subsidiary to be its own principal
derived from the parent, so that the parent's identity is never duplicated.

#### Acceptance Criteria

1. THE SYSTEM SHALL NOT copy the parent's sled, IID1, SID1, WireGuard key or session id to the subsidiary.
2. WHEN `--child-key <pubkey>` and `--child-iid1 <file>` are given THEN THE SYSTEM SHALL verify the IID1 against the decoy trust store, derive the child session, refuse if it equals the parent session, and plan the child sled and labels including `user.opdbus.parent_session_id`.
3. IF the child key or IID1 is absent THEN THE SYSTEM SHALL leave the imported subsidiary stopped, labelled `user.opdbus.admission=pending`, with no sled.
4. THE SYSTEM SHALL write a grant delta proposal for the child principal whose capabilities are a subset of the parent's and exclude every `shell`, `exec`, `admin` and `secret` class capability, and SHALL NOT apply it.
5. THE SYSTEM SHALL mint nothing locally.

### Requirement 9 — tests and verification

**User Story:** As a developer, I want both modes covered by offline tests,
so that they can be reviewed without touching a host.

#### Acceptance Criteria

1. THE SYSTEM SHALL pass `shellcheck` on every changed script.
2. THE SYSTEM SHALL have bats tests that run with stubbed `incus`, `btrfs`, `op-container`, `op-identity-accept` and a fake operation tree, and that fail if any forbidden command runs or any identity artefact appears in `--system` mode.
3. THE SYSTEM SHALL have golden dry-run outputs for one `--system` and one `--subsidiary` invocation.
4. WHEN `40-verify.sh` sees a system or subsidiary container THEN THE SYSTEM SHALL check its mode-specific invariants read-only instead of the identity checks.
