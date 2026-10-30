# Prepared seed disk — final runtime evidence

Task: identity-container-provision-20261024. Agent: Codex /root.
Storage preparation completed: 2026-10-25T02:52:24Z.
LiteLLM offline addition completed: 2026-10-25T03:16:40Z.
Source commit: 86f03d2dede738e2d780ab789dd5800b58678479; existing WIP preserved.

The owner's requested seed transfer, patch subvolume, offline guest preparation
and OS partition shrink are complete. This is not completed identity provisioning.

| Object | Verified state |
| --- | --- |
| Outer LV | /dev/mapper/3tched--vg-3tched--seed, 42949672960 bytes, ext4 |
| Outer mount | /run/mounts/seed-3tched; persisted UUID5573f463-435c-4a21-8e92-d048524eea44 |
| Whole disk file | /run/mounts/seed-3tched/seed.img; 42949672960 logical bytes |
| Current allocated bytes | 9318735872 (8.68GiB), sparse; after LiteLLM addition |
| Root filesystem and p1 | 11849957376 bytes (11.04GiB), start sector2048 |
| Sizing | measured minimum10776215552 +1073741824 headroom;1MiB aligned |
| Root inspection | /run/mounts/seed-3tched/rootfs, loop1p1, @, writable |
| Patch p2 | start41945088, size41940959 sectors; existing empty writable@; unmounted |
| P2 unchanged SHA256 | 36925297daab668e82eab9593d17907eeb5eb542f3676663b8b662218b4aa8c5 |
| Other GPT fields | unchanged; only p1.size reduced; whole file not truncated |
| Seven members | @ and @agent_user RW; other five agent members RO; UUIDs preserved |
| Guest software | attempt8 3tched-fs,71-plugin catalog, unmodified upstream threetched, two FUSE systemd units staged |
| JEV runtime | LiteLLM1.105.1,117 locked distributions, config and enabled guarded systemd unit staged; no key;2.07GB root space available |
| Shared defaults | six agent members populated, copied machine identifiers neutralized; private state empty |
| Seal / boot / identity | unsealed; no guest boot, new SID1, ID-0, grant/role conversion or hostsoftware deployment |

Filesystem shrink preceded partition shrink. Remapping and root remount succeeded.
The first final inspection rejected legacy standalone nologreplay; its failed
receipt remains. A separate reviewed inspection used ro,rescue=nologreplay,
verified empty property-RW@, unmounted p2 and matched its whole byte hash. No
resize or partition edit was repeated. No fsck, scrub, tests, Clippy or formatting
check ran; operational readbacks are not a test-suite pass.

Receipts:

- litellm-{resolve,build,install}-completion.json / litellm-preparation.json
- litellm-image-stat.json / litellm-space.txt / litellm-source.sha256
- guest-exec-and-litellm.md / litellm-review.md
- storage-resume-completion.json / storage-resume-result.json
- storage-rollback-verified.json
- guest-refresh-completion.json / guest-refresh-result.json / guest-lungs-client.json
- storage-shrink-initial-completion.json (retained inspection failure)
- storage-shrink-completion.json / storage-shrink-result.json (successful final verification)
- storage-shrink-size.json / storage-final-gpt.json / storage-patch-at.txt
- attempt8-build-receipt.json / attempt8-catalog-receipt.json / attempt8-artifacts.sha256
- attempt8-semantic-delta.json / attempt8-guest-input.json

Root-private execution records remain in:

- /var/tmp/identity-container-storage-resume-20261025.6eBmCIsL
- /var/tmp/identity-container-guest-refresh-20261025.M1nbxprF
- /var/tmp/identity-container-storage-shrink-20261025.gSyp8Vzb
- /var/tmp/identity-container-shrink-inspection-20261025.3TYzS94k

Retain /var/lib/cargo-build/staging/seed-backup-raw.img,
identity-container-provision-20261024.v2FToSfM/{current-lv-40GiB.raw.img,seed.img},
failed stage identity-container-provision-20261024.YGUjoYQv, and old public guest
software in identity-container-provision-20261024-guest-refresh.2vweWyCp.

Next broader work remains in status.md/HANDOFF.md: truthful base acceptance
(original archive receipt absent), image-aware host mount/provisioning bindings,
namespace-safe boot send/receive integration, owner-held sealing, coherent host
release and actual registered key/two-human approvals. No alternate identity or
permission bypass is authorized. Mounted NotebookLM operation discovery exists;
its data/value files returned EIO during the final listing, so no new remote
memory-sync receipt is claimed. Repository receipts are current.
