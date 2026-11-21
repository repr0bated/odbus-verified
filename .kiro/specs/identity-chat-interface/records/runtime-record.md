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

Architecture note (owner, 2026-11-09): the FUSE FS is a runtime mirror
of the SHM. Corroborated read-only: host `/dev/shm/opdbus/` holds
`plugin-blobs/` (72 sealed blobs), `state/`, `capability-grants.json`,
`credentials/`; the same layout is visible inside the container. The
mounted views (`plugins/<p>/{schema.json,value.json,object.blob.json,
object.hash,capabilities.json,operations}`) are the projection of the
sealed blob catalog plus live, MutationEngine-committed SHM state —
`view.json`'s `source_catalog_hash`/`source_catalog_generation` name
that SHM catalog. Consequences for implementation: reading a mounted
file is reading the SHM mirror, not an independent store; the mirror is
never a persistence target (Partitioned_Store and records live
elsewhere); authoritative state change happens only through an
Operation_File call, after which the bridge commits state to SHM before
notifications, and the mirror reflects it.

Effect on blockers: B-06/B-07 (no egress path) are narrowed — the egress
path is FUSE operation calls (above), not container IP egress. Which
model-provider call satisfies the structured tool-call requirement
(D-04) is still a design decision, not settled by this observation.
Task 1.3 still requires actually calling `get_own_identity` through the
mount to identify the Bound_Identity; that is an operation call, not a
listing, and remains gated pending execution authorization.

## Task 1.3 call attempt (owner: "call it", 2026-11-09)

`identity_sled` is mounted in the container at
`/run/mounts/3tchedFS/plugins/identity_sled/`; mount declaration
`.3tchedfs.json` shows `mount_identity: { kind: "guest_kernel" }`,
catalog generation 144, `local_call_owner_uid: 0`.

Attempted calls, in order:

1. `3tched-fs call identity_sled get_own_identity --mount
   /run/mounts/3tchedFS --args "{}"` (in-container deployed binary):
   refused before dispatch — `Error: mount must declare the expected
   operation identity mode`. The deployed binary has no `--guest-kernel`
   flag (that flag is WT/newer, B-09).
2. Direct Operation_File call per D-44 (write `{}` once, fsync, read the
   terminal receipt on the same handle), as root (the declared local
   call owner): write succeeded; fsync returned `EACCES`, and the
   terminal diagnostic on the handle was:

   ```json
   {
     "plugin": "identity_sled",
     "method": "get_own_identity",
     "stage": "register",
     "error": "identity registration refused: org.freedesktop.DBus.Error.AccessDenied: lungs identity binding refused",
     "operation_id": null
   }
   ```

3. Same direct call as `ubuntu` (uid 1000): open refused (`EACCES`,
   call file is `0600 root`).

Result: the call was executed exactly once through the mount and was
**denied at identity registration**. No receipt, no Public_Identity_Fields.
Bound_Identity for `CockpitBot3tched` remains **unidentified**; B-02 is
now precisely located: the `incus exec` root context is not the
identity-bound operator for this mount, and no other caller context was
tried (trying other identities to route around a denial is forbidden).
Task 1.3 stays BLOCKED until the bound operator context performs the
call or the owner authorizes the Branch run (task 19.14).

Owner note (2026-11-09): the container is not finished developing.
`CockpitBot3tched` is a development container, not a finished
Deployment_Target. The identity-binding refusal above is therefore
expected at this stage and is not a defect in the mount or the identity
sled. Development of the Chat_Interface proceeds against the synthetic
fixtures and in-process harness (tasks 10.3, 14.6); live container
integration stays in the blocked task-19 group until the container is
finished and the owner authorizes a Branch run.

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
