# Runtime_Record — identity-chat-interface Task 1.2

Recorded: 2026-11-09T09:32:41Z (read-only discovery only, Req 1.6: no
login, start, stop, restart, logout, mount, unmount, grant, storage,
identity or deployment change). Unobserved items are recorded as absent.

| Surface | Command | Observed output | Result |
|---|---|---|---|
| `3tched` view | `ls -ld /run/mounts/3tchedFS` | `dr-xr-xr-x 2 root root 0` | present |
| `3tched` view contents | `ls /run/mounts/3tchedFS` | `capabilities operations plugins relationships view.json` | present |
| `threetched` view | `ls -ld /run/mounts/threetched` | `dr-xr-xr-x 2 root root 0` | present |
| `threetched` view contents | `ls /run/mounts/threetched` | `capabilities operations plugins relationships view.json` | present |
| Catalog generation | `cat /run/mounts/3tchedFS/view.json` | `source_catalog_generation: 216`, `source_catalog_hash: 3391876719b148ebe8595f2fbb1cccf485bae3b60ecf7ba6480ed8b1b6ddaf05` | present (owner-brief said generation 144 — superseded) |
| Lungs_Socket (host) | `ls -l /run/opdbus/lungs/lungs.sock` | `srw-rw-rw- 1 root root 0 Nov 8 21:27` | present |
| Lungs_Socket (guest path) | `ls -l /run/3tched-lungs.socket` | `No such file or directory` | absent on host |
| Door_8090 | `ss -tlnp` | `LISTEN 10.0.0.2:8090` | present |
| Op_Web_Connector (:8080) | `ss -tlnp` | `LISTEN 10.0.0.2:8080` | present |
| Mount helper | `grep upstream_plugins deploy/runit/libexec-3tched/mount-3tchedfs` | line 35 `upstream_plugins=threetched` | source fix present |
| Branches (Incus) | `incus list --format csv -c n,s` | `CockpitBot3tched,RUNNING`; `qdrant-recovered,RUNNING`; `seed-rw,STOPPED`; `subsidiary-dev,RUNNING`; `subsidiary-probe,RUNNING` | observed; Branch↔mount mapping NOT established |
| In-container egress | — | initially recorded here from the host; superseded by the in-container observation below | corrected below |
| 3tchedW3_Identity (B-03) | — | no such identity observed in any read-only surface above | absent (unobserved) |

## In-container observation (owner correction, 2026-11-09)

Egress is observed from inside the container, and egress auth/credentials
are obtained in the FUSE filesystem — not from the host, and never by the
chat code holding raw credential material.

Observed 2026-11-09 (read-only `incus exec CockpitBot3tched` listings;
no operation was called):

| Surface | Command | Observed output | Result |
|---|---|---|---|
| In-container `3tched` view | `incus exec CockpitBot3tched -- ls /run/mounts/3tchedFS/` | `capabilities operations plugins relationships view.json` | present |
| In-container catalog | `cat /run/mounts/3tchedFS/view.json` | `source_catalog_generation: 144`, hash `ec5afd4e691f509b7975ba7f504a34fb1877c7ed28c1da152d2caf4bf3fd81ff` | present (matches owner brief; host view is generation 216 — a different view) |
| In-container Lungs_Socket | `ls -l /run/opdbus/lungs/lungs.sock` | `srw-rw-rw- 1 root root 0 Nov 8 21:27` | present inside container; `/run/3tched-lungs.socket` absent |
| In-container network | `ip -brief addr; ip route` | only `lo 127.0.0.1/8`; no route output | no direct IP egress |
| Egress via FUSE operations | `ls /run/mounts/3tchedFS/operations/` | `large_language_model/stream_generate`, `model_registry/{list_models,list_providers,maintain_credentials,native_list_providers,refresh,set_model_profile}`, `proxy_server/{configure,start_proxy,stop_proxy}` | egress path is Operation_File calls through the lungs socket |
| Credential handling via FUSE | `ls /run/mounts/3tchedFS/operations/identity_sled/` + `model_registry` ops | `identity_sled/{get_own_identity,refresh_credential,get_session_history,...}`, `model_registry/maintain_credentials` | provider/identity credentials are obtained and used behind the FUSE operation surface; the chat interface must never receive raw credential values (Secret_Guard) |
| In-container identity operations | `ls /run/mounts/3tchedFS/operations/identity_sled/` | `get_own_identity` present among 18 operations | a mounted Branch namespace exists for `CockpitBot3tched` |

Effect on blockers: B-06/B-07 (no egress path) are narrowed — the egress
path is FUSE operation calls (above), not container IP egress. Which
model-provider call satisfies the structured tool-call requirement
(D-04) is still a design decision, not settled by this observation.
Task 1.3 still requires actually calling `get_own_identity` through the
mount to identify the Bound_Identity; that is an operation call, not a
listing, and remains gated pending execution authorization.

## Blockers

- **B-02 (partially cleared at host level)**: a mount and the host
  Lungs_Socket exist for the current host user, contrary to the design-time
  record. Per-Branch mounts, and whether any Branch namespace is available
  to the Chat_Interface, remain unobserved.
- **B-01**: host views are mounted; the helper defect is not re-tested
  (mount action forbidden in Task 1). Recorded as observed-mounted, not
  re-verified.
- **Task 1.3 remains BLOCKED**: Bound_Identity identification requires a
  `get_own_identity` Receipt through a mounted Branch's Operation_File.
  No Branch↔mount mapping is established, and Task 1 is read-only
  discovery; making that call is an operation, gated by B-01/B-02 per
  Branch. Bound_Identity is therefore recorded as **unidentified** for
  every discovered container, and every task that runs the Chat_Interface
  under a Branch stays in proposed status (Req 1.8).
