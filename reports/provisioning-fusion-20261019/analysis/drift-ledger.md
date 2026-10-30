# Drift ledger — provisional

Source findings below are grounded in preserved reports. Conversation anchors from CASE are discovery leads until their original packets are semantically reviewed; they are not a completed chronology.

| Requirement | Observed divergence | Evidence | Required disposition |
|---|---|---|---|
| No host loops | Shell allocation and Rust proof require image/loop-backed members; existing pool loop is also a conflict | BackendProof: storage.rs:259–369; partitioned.rs:54–65; NativeFeasibility §§6–8 | Coordinated backing/proof change; no exemption or automatic detachment |
| Preserve actual seed/sprout | Parent extrapolated a device-add operand fact into a verdict about the overall exposed-subvolume arrangement | Latest owner correction; historical op-identity-root:1172–1196; ../jev-seed-mechanism-correction.json | Withdraw that inference. Seed is backing; btrfs device add supplies writable member. Trace backing, Cozo association, exposed subvolumes and upgrade separately; retain no-host-loops. |
| One complete workflow | Native creation has a durable receipt separate from storage, adoption and start | BackendProof architecture, native_container_creation.rs:80–89; Cozo native_creation.rs:130–133 | Reconcile same operation and UUID, then prove remaining stages |
| Coherent storage identities | New pool/FSID in preparation/binder, old pins in synchronization | SourceChain; bind-instance-root.sh:27–30,79–81; sync-instance-upgrade.sh:34–80 | One explicit owner-approved storage descriptor, not blind constant replacement |
| Boot restoration | Installed base service uses old single-chatbot early-return path; source differs | Installed etc/sv/identity-roots/run:32–38; source run:39–58,76–125 | Separate base-owner delivery receipt; application staging does not install it |
| Correct guest views | Binder validation and image metadata lag two-view preparation | SourceChain §§guest contract and metadata | Align both FUSE views without changing admission or exporting credentials |
| Root binding proves identity | Binder proves less than sync/adoption: replica FSID and complete effective binds absent | SourceChain, bind-instance-root.sh:77–84,275–280 | Strong observation before adoption; no weakened Rust checks |
| Durable resume | Retained sync intent prevents unsafe cleanup but is not complete recovery | sync-instance-upgrade.sh:95–105; BackendProof failure table | Phase-specific reconcile; never repeat ADD_DEV or POST from uncertain outcome |

## Conversation review still required

CASE identifies numbering/display overcomplication, imagined blockers, writable-leaf/overlay-upper conflation, component-only audit, real later admission failures, subsequent repairs, queued/live-delegation steering and the final broader loop prohibition. These must be traced through actual actions and propagation before marking this ledger complete. Historical successful startup does not establish cold reboot or today's topology.
