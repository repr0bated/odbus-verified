# Tasks — ZeroClaw schema-materialized surface

Order is fixed (R6): the `zeroclaw` tree is built from the schema and proven on
the host, then sealed, then the hand-rolled copy is surveyed and removed against
the tree. No removal task starts before the host proof (Phase 3) is recorded.
The spec surveys the removal; it does not delete code.

The materialized form is the FUSE `zeroclaw` tree plus the blob — not a Rust
plugin. No task generates a `PluginSchema`/`MethodDecl`, infers a taxonomy, or
reuses the schema-freezing import.

Cargo/build/test execution is delegated to the Luna worker per repo policy; the
primary agent owns source, decisions, and the survey/report.

---

## Phase 1 — Schema from the installed binary

- [ ] 1.1 Read the authoritative schema from `/usr/bin/zeroclaw` at runtime
  - Invoke `zeroclaw config schema` and feed its JSON directly into the
    redistributor as a runtime input. Do NOT check a copy of that output into the
    repository — a stored copy is the 26,032-line contract again.
  - Invoke `zeroclaw --help` and each command's `--help`; feed the command set
    with argument names into the redistributor. Command words preserved verbatim
    (incl. `self-test`); no renamed names.
  - Compute the schema content digest and the binary version `zeroclaw 0.8.4` as
    provenance to carry into the blob; the schema body itself is not persisted
    outside the sealed blob.
  - _Requirements: R1.1, R1.2, R1.3, R1.4_

- [ ] 1.2 Build the schema redistributor (scaffolding, not a plugin generator)
  - Thin code that takes the binary's print as a runtime input (from 1.1) and
    writes it into the `zeroclaw` object's files: `schema.json`, and per command
    `input.schema.json`/`output.schema.json`/`metadata.json`.
  - It redistributes schema into the existing object layout. It does NOT emit a
    `PluginSchema`/`MethodDecl`, does NOT infer `side_effect`/
    `required_capability`/`subid` from command groups, does NOT reuse
    `gen_tched_router_config_surface --import-json`, and does NOT read from a
    checked-in schema file.
  - Carry the version + schema digest as provenance into the sealed blob.
  - _Requirements: R1.1, R1.4, R1.5, R2.2, R2.6_

## Phase 2 — Build the `zeroclaw` FUSE tree from the schema

- [ ] 2.1 Populate the `zeroclaw` object from the schema via the existing builder
  - Feed the namespace builder the binary's schema so it emits
    `plugins/zeroclaw/operations/<command>/{call,input.schema.json,
    output.schema.json,metadata.json}` and the `/operations/zeroclaw/` symlink
    mirror. Command words preserved verbatim.
  - Leave the inode allocation and call-file layout alone; no redesign.
  - _Requirements: R2.1, R2.2, R2.4, R3.1_

- [ ] 2.2 Wire the `call` to exec the installed binary
  - A write to `plugins/zeroclaw/operations/<command>/call` validates args
    against the operation's `input.schema.json`, then execs
    `/usr/bin/zeroclaw <command> <args>` under the caller's verified
    identity/capability, folding the result into the receipt and resulting
    state.
  - Do NOT route the call through plugin dispatch; do NOT add a `MethodDecl`
    adapter. Preserve identity/capability gating on every operation.
  - _Requirements: R2.3, R2.5_

- [ ] 2.3 Error paths for the call
  - Reject invalid args before exec; capture nonzero exit into the output
    envelope and receipt; surface capability denial as a permission boundary.
  - _Requirements: R2.3, R2.5_

## Phase 3 — Seal the tree, then prove on the running host (gate for everything after)

- [ ] 3.1 Seal the tree's schema into the blob (build-before-seal direction)
  - Hand the schema that materialized in the `zeroclaw` tree to `op-blob
    seal-shm` with the service's schema-shaping env. Do NOT seal a Rust plugin
    and project the mount from it. Confirm live/persisted catalog hashes match
    and generation advanced.
  - _Requirements: R3.1, R3.2, R3.3, R3.4_

- [ ] 3.2 Confirm `plugins/zeroclaw` in the host mount is the binary's schema
  - On `/run/mount/3tchedFS` (served by `/opt/op-dbus/current/bin/threetched-fs`),
    confirm `plugins/zeroclaw/schema.json` equals `zeroclaw config schema` and
    the `operations/<command>/` set equals the `zeroclaw --help` command set.
  - _Requirements: R4.1, R4.2_

- [ ] 3.3 Perform one gated `call` through the host mount
  - Exec a safe command (e.g. `status` or `self-test`) via the operation's
    `call`; confirm it runs the installed binary and returns through the receipt
    and resulting state, identity enforcement intact. No container.
  - _Requirements: R4.3, R4.4_

- [ ] 3.4 Record the host proof
  - Write what was invoked and observed to the spec's report location. This
    record is the gate; no Phase 4 task starts until it exists.
  - _Requirements: R4.5, R6.2_

## Phase 4 — Mark the survey against the tree, then remove (removal gated on Phase 3)

- [ ] 4.1 Resolve the blast survey against the proven tree
  - Take the design's survey table as provisional. With the `zeroclaw` tree
    live, mark every row **stay** or **remove**. A row stays only if it is
    identity/capability enforcement with no ZeroClaw behavior, or a layer
    separate from the mirror (FUSE layer, sealer, unrelated plugins). The survey
    does not decide the replacement — the tree does.
  - _Requirements: R5.1, R5.2, R5.3, R5.5_

- [ ] 4.2 Remove the mimic surface (only after 3.4 exists)
  - Remove the method table, routes, runtime, and mimic tests; remove the
    `tched_router` plugin with them; remove the embedded `CONFIG_SCHEMA`
    (`tched_router_contract.rs`) so the binary's live print is the only schema
    authority. These come out — not folded into a generator.
  - _Requirements: R5.1, R5.4, R6.1.3_

- [ ] 4.3 Resolve the review rows
  - For secret-policy, admission, and catalog-binding: keep only
    identity/capability/generic logic with no ZeroClaw behavior; remove the rest.
  - _Requirements: R5.3_

- [ ] 4.4 Verify after removal
  - Luna runs build + `/verify`; confirm the `zeroclaw` tree unchanged and
    `tched_router` gone; the live host `call` still works.
  - _Requirements: R5.4_

---

## Sequence note — container, guest systemd unit, guest socket (not tasks here)

After the host FUSE `call` path works, the guest path reuses the same `zeroclaw`
directory and blob over the guest lungs socket, adding no second schema and no
second method table. This is a sequencing note for a later revision, not a task
in this one (R6.1.4, R6.3).

### Out of scope (restated for the executor)
Reimplementing ZeroClaw; a regenerated `PluginSchema`/`MethodDecl` artifact;
inferring a taxonomy; reusing `gen_tched_router_config_surface --import-json`;
inventing or renaming command names; a frozen hand-copied schema; starting from
the container; guest unit/socket as tasks; rewriting the FUSE inode layer;
extending the OpenWrt gateway-wiring spec; btrfs seed/overlay/send-receive; and
deleting code inside this spec document (the spec surveys removal — removal waits
on the host proof).
