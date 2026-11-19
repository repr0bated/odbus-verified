# Implementation Plan — provisioning `--system` and `--subsidiary`

Spec id: `provisioning-system-subsidiary`. Design: `design.md`.
Requirements: `requirements.md` 1–9. Not started: implementation waits until
after the current build (owner, 2026-11-19). One task = one commit on `dev`,
pushed fast-forward after a fresh fetch/rebase, hooks on. No task runs
provisioning, creates/starts/exports containers, edits grants, or touches the
bridge, 3tchedFS, pools, the seed, the overlay or sealed blobs.

- [ ] 1. Step runner, mode parsing and test harness
  - `deploy/provisioning/lib-plan.sh`: `plan_host`, `plan_incus`, `plan_fuse`
    (schema check, capability from `metadata.json`, `--confirm-side-effects`
    for mutations, uncertain-receipt stop), numbered dry-run output, secret
    redaction for file inputs.
  - `30-provision-container.sh`: `--system`, `--subsidiary`, `--dry-run` /
    `DRY_RUN=1`, mutual exclusion; `new`/`complete` unchanged.
  - `deploy/provisioning/tests/` bats harness: PATH stubs, forbidden stubs,
    fake mount tree fixture.
  - Tests: exclusivity, dry-run executes nothing, `new`/`complete` untouched.
  - _Requirements: 1.1–1.7, 9.1, 9.2_

- [ ] 2. Function manifest format and validation
  - `lib-system.sh`: load and validate `system-functions/<function>.json`
    (name regex, closed key set, network modes, limits).
  - Class deny-list and coarse-capability check against operation metadata.
  - Manifests `system-functions/dns-forwarder.json` (no operation) and
    `system-functions/datapath-health.json` (one read operation).
  - Tests: missing/invalid manifest, name mismatch, two operations, each
    denied class, coarse capability with and without ack.
  - _Requirements: 3.1–3.7_

- [ ] 3. `--system` plan: locked-down instance with no identity
  - `incus.create_instance` (`source_mode=empty`) payload with the design
    config set, seed swap step, service `down` files in the container root,
    labels, root-only devices plus manifest-declared extras, `START=1` via
    `incus.start_instance`.
  - Refuse identity options; never plan sled, IID1, `/agent/user`, `fuse`,
    `lungs`, `catalog`, `decoy-trust`, `tunnel`, `opdbus-run`.
  - Tests: exact config/device sets, no identity artefacts, `nic` without ACL
    refused, golden `system-dns-forwarder.dry-run.txt`.
  - _Requirements: 2.1–2.4, 4.1–4.7, 9.3_

- [ ] 4. `--system` grant delta and projection gate
  - Grant delta proposal for `system:<function>` (exactly one capability),
    never applied; projection device when present.
  - Real run with an operation and no projection refused with the named
    contract; no-operation functions unaffected.
  - Tests: delta content, refusal message, no grants file written.
  - _Requirements: 5.1–5.4_

- [ ] 5. `--subsidiary` preflight
  - `lib-subsidiary.sh`: parent existence, sled/session/label checks, single
    instance per session, refuse system/subsidiary parents, parent capability
    names only; refuse `START=1`.
  - Tests: each refusal, no mutation planned on refusal.
  - _Requirements: 6.1–6.3, 1.5_

- [ ] 6. `--subsidiary` staging and export plan
  - `incus.snapshot_instance`, native `incus copy` (gap G1), identity device
    removal and label unset via `incus.remove_device` / `incus.modify_instance`,
    native `incus export` into the 0700 directory, size/SHA-256 receipt,
    `incus.delete_instance` of the staging copy only.
  - Tests: identity devices stripped before export, parent untouched, gap G1
    recorded in the receipt.
  - _Requirements: 7.1–7.3, 7.6_

- [ ] 7. `--subsidiary` LXD import plan
  - `lxd.get_profile` / `lxd.create_profile` (`opdbus-subsidiary`, refuse a
    differing existing profile), `lxd.create_instance_from_backup` with the
    backup shown as a file placeholder, `lxd.get_instance` +
    `lxd.update_instance` for profile and labels, never start.
  - Tests: no `lxc`/`lxd` binary invoked, payloads valid against the sealed
    schemas in the fixture, golden `subsidiary-ID-1.dry-run.txt`.
  - _Requirements: 7.4, 7.5, 9.3_

- [ ] 8. Child identity binding and grant subset
  - Optional `--child-key`/`--child-iid1`: verify, derive, refuse
    child == parent session, plan the child sled volume, device and labels;
    otherwise `admission=pending` with no sled.
  - Grant delta for the child: parent capabilities minus shell/exec/admin/
    secret classes, read subset by default; never applied; nothing minted.
  - Tests: both paths, excluded classes, no parent identity material copied.
  - _Requirements: 8.1–8.5_

- [ ] 9. Verification and documentation
  - `40-verify.sh`: read-only invariants for `mode=system` (no identity
    devices, manifest hash label matches the source manifest, config set) and
    `mode=subsidiary` (parent link, admission state).
  - `deploy/provisioning/README.md`: both modes, dry-run examples, gaps G1–G3.
  - Final `shellcheck` and full bats run.
  - _Requirements: 9.1–9.4_
