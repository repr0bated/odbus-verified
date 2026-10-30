# Chatbot Migration — Leaf Subvolume Complete

## Live binding (authoritative)

| Field | Value |
|------|-------|
| Session | `b0c777e3-0955-cd8b-3d01-ca12998cae28` |
| Leaf device | `/dev/sdb2` |
| Label | `leaf-root` |
| FSID | `bc95536f-1d38-46ad-b87e-2637c5c548ec` |
| SEEDING | **cleared** (not a seed) |
| Subvolume | `@identity/b0c777e3-0955-cd8b-3d01-ca12998cae28` (id 262) |
| Mount | `.../seed-identities/<session>/volume` |
| Cozo mutation_index | 3 |
| Chatbot loop | **removed** |

## Self-validation

Guest wrote and host confirmed:

- `/agent/user/.self-verify` → `chatbot-leaf-ok-20261003T012126Z`
- `/opt/run-mounts/persist/.self-verify` → `persist-ok`

## Source follow-ups (not blocking live leaf)

- `op-network` sole-leaf `ensure_device_removed` no-op (uncommitted)
- Schema comments: leaf/subvolume, no seeding (uncommitted)
- Fold guest disk devices / bridge binary into next golden (no hand-stage)
