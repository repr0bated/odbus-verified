# Provisioning diagnosis — provisional

Full semantic corpus review remains incomplete. This report integrates saved source investigations; it does not establish present runtime state. No implementation, build, test, deployment or provisioning was performed.

Owner correction supersedes the earlier storage-layer inference: **the seed is the backing; `btrfs device add` supplies the writable member enabling the sprout's read-write operation, as it did before.** Parent verified historical `op-identity-root:1172–1196` at commit `26a4ad132fbbd4523b05724060bfdeb9ca661824`. See `../owner-seed-device-add-correction.md` and `../jev-seed-mechanism-correction.json`. Do not evaluate exposed Incus subvolumes as though each must itself be a standalone device-add operand.

## Finding

The existing workflow contains disconnected completion boundaries and incompatible storage contracts. Native creation is not storage establishment, authoritative adoption or startup. Replacing the image-creation command alone cannot satisfy the latest no-host-loop requirement: the Rust adoption/start proofs explicitly require loop mappings and regular-file leaf identity.

Evidence: `recovery/BackendProof.md`, original snapshots `identity_clean_provision/storage.rs:259–369`, `partitioned.rs:54–65`; `recovery/SourceChain.md`, source/installed coverage tables.

Current preparation/binding drafts select the newer pool and upgrade FSID while synchronization retains the older pool and FSID. Installed application helpers and the installed base boot service also differ from source. Application golden staging does not update the base-owned boot service. These are concrete integration defects, not proof that every historical blocker was genuine or remains current.

Evidence: source `deploy/scripts/bind-instance-root.sh:27–30,79–81`, `sync-instance-upgrade.sh:34–80`; installed `etc/sv/identity-roots/run:33–39`; source `deploy/lib/golden/build-golden.sh:95–99,123–127`, as cited in saved source reports.

## Storage decision boundary

No host loop devices is unconditional, including pool backing and seed views. Existing loops are conflicts, not exemptions or permission to detach them. Preserve the demonstrated sequence: immutable seed backing → `btrfs device add` of the writable member → read-write sprout mount → inherited subvolume views and separate upgrade replica. Distinguish backing, member, Cozo identity association and exposed subvolumes. The parent withdraws the implication that an operand-type statement alone rules out the owner's overall subvolume arrangement. A loop-free implementation still needs concrete tracing and proof; neither success nor impossibility is established by that correction alone.

Upstream documents the Linux 6.17+ restriction on mounting the same seed block device independently and through sprouts concurrently. Determine how that applies to the actual intended backing/view arrangement; do not turn it into an unverified demand to replace the owner's previously working mechanism or retire the seed. No storage operation is authorized.

Evidence: `recovery/NativeFeasibility.md` §§5–9; https://btrfs.readthedocs.io/en/latest/Seeding-device.html .

## Evidence limits

The preceding runner's HTTP502 was a provider failure, not a provisioning result. Mechanical parsing of 42 histories is not semantic review. Packet reviews and additional JEV judgments remain outstanding. Historical start reports do not prove cold restoration or current completion. Saved `HistoricalScripts.md` ends mid-citation; its claim of complete reading is not a complete preserved detailed report.
