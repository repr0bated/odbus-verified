# Dev cycle: blob → mount → identity

**The method is the blob.** In 3tched a plugin *is* its schema, sealed into a
blob; a method exists if and only if its blob is in the sealed catalog. The
identity (the chatbot / the subsidiary) never calls code directly: it sees the
sealed methods as typed files in its own 3tchedFS mount, calls them there, and
every call is admitted, checked and receipted by the platform. Every new
method built for the hackathon follows this path — that is the dev cycle.

```text
 this repo (new logic)        platform registration        sealed catalog
 ┌───────────────────┐       ┌────────────────────┐       ┌──────────────────────────┐
 │ crate: types,     │──────▶│ plugin schema +    │──────▶│ <plugin>.<hash16>.blob   │
 │ rules, tests      │       │ dispatch (op-dbus) │ seal  │ (/dev/shm/opdbus/...)    │
 └───────────────────┘       └────────────────────┘       └────────────┬─────────────┘
                                                                        │ projected by 3tchedFS
                                                                        ▼
 receipt ◀── bridge: kernel/credential identity, ◀── call file ◀── identity's mount
 (operation id,      exact capability, commit          (write args,   operations/<plugin>/<method>/call
  chain event)                                          read result)
```

## The three relationships

| Relationship | What it means | Where to see it |
|---|---|---|
| **blob ↔ method** | A method exists only as a sealed blob; its schema is the single source of its arguments, results and required capability | `ls /dev/shm/opdbus/plugin-blobs/` (72 blobs today); `opblob inspect <file.blob>` |
| **blob ↔ mount** | 3tchedFS projects each sealed method as `operations/<plugin>/<method>/{call,metadata.json,input.schema.json,output.schema.json}`; a new blob appears in every identity's mount, with no mount change | `/home/3tched/Hackathon/3tchedfs/operations/` (as `3tched`) |
| **mount ↔ chatbot** | The identity acts **only** through its mount, by writing to call files. There is no MCP path: MCP was removed and replaced by 3tchedFS. `3tched-fs toolsets --publish` only announces the sealed method list on EMQX; it does not call anything | call-file helper `3tched-callfile` |

## The UI is in the same cycle

The UI is **derived from the blobs**, never written against a separate API:

| Step | What happens |
|---|---|
| Screens | Rendered from a plugin's **sealed schema** (`schema_renderer.render_schema`) — a new method's blob gives it a form and result view automatically |
| Components | Only components declared in the catalog sealed in the `json_render` blob; a generated spec is **validated against that catalog** before it is shown |
| Actions | Call the **same sealed method** for whoever is acting: a human through the door (the one generic `PluginService/CallMethod`, with their own credential); the identity through its mount's call file. Same blob, same capability check, same receipt |
| Results | Every action shows its **receipt** (operation id, outcome, capability) — and a refusal shows the missing capability |

So a new method needs no UI code to appear: seal the blob, and the UI can
render and call it. Hand-written UI is only for composition and the
governance components (receipt, refusal, approval request, redaction notice).
No MCP anywhere in the UI path.

## Each plugin is a schema feed

A sealed plugin is not only callable — it **publishes itself**. Subscribers
(the UI, the identity) receive one stream per plugin over the bridge's
`Subscribe`:

| Frame | Carries |
|---|---|
| schema frame (`SCHEMA_MIGRATION`) | the plugin's sealed schema and its schema hash, so a consumer can render forms and validate before calling |
| state frames | committed values per object path, each with a **generation** number (older generations are ignored) and the catalog hash |
| tombstones | deletions, so a removed object never reappears from an older frame |
| withheld | private state the subscriber is not entitled to stays withheld (`withheld-or-unavailable`) |

The UI keeps feed data under `/plugins` and `/pluginFeeds` **read-only**: a
UI can never write state into a feed; only a sealed method call changes it.

## How state is read

State has one writer: **MutationEngine**, which commits the result of each
admitted call and only then notifies subscribers. Readers never see an
uncommitted value.

| Reader | How |
|---|---|
| Live consumers (UI, identity) | the plugin's **schema feed** (above) — values derived from committed state, never from a request payload |
| Point-in-time | committed state in shared memory, `/dev/shm/opdbus/state/<plugin>.json` (root-only; e.g. `incus`, `identity_sled`, `chat_router`, `cron`, `json_render`) |
| Through the mount | the plugin's read operations (`operations/<plugin>/<read-method>/call`). The mount's `plugins/<id>/value.json` files are meant for this but currently return EIO — use read operations |

## How state is archived

Nothing is overwritten without a record; history is kept in layers:

| Layer | Where | What it holds |
|---|---|---|
| Receipts / chain | MutationEngine chain events (`operation_id`, `event_id`, `event_hash`, hash-linked) | who did what, under which capability, with which outcome |
| Snowball | `/var/lib/opdbus/snowball/{timing,vectors,state,snapshots}` — Btrfs subvolumes | a streaming audit trail of plugin footprints, automatic read-only snapshots with rolling retention, replicable with btrfs send/receive |
| Vectors | Qdrant `snowball_footprints` (~10k entries) | the archived footprints embedded for semantic search ("what changed in X last week?") |
| Sealed catalog | `/var/lib/opdbus/plugin-blobs` (persisted copy of the live catalog) | which methods existed, by blob hash |

For the hackathon this is what makes "every action is attributable" checkable:
a receipt in the UI points at a chain event, the snowball keeps the state it
produced, and the Web3 index can anchor the chain head.

## Steps for a new method (e.g. `approvals.request_approval`)

1. **Logic, in this repo.** Types, rules and tests in a crate here (e.g.
   `approvals/`). No I/O. This is the judged, new work.
2. **Register as a plugin, on the platform.** A thin schema + dispatch module in
   `/srv/git/odbus-verified/crates/op-plugins/src/state_plugins/<name>.rs`,
   registered in `default_registry.rs`, calling this repo's crate. Follow that
   repo's `AGENTS.md` (plugin is the schema; no parallel schema files). Keep it
   an adapter: decisions stay in this repo's crate.
3. **Seal a development catalog, without touching live.** Build, then
   `opblob seal-plugins <dev-catalog-dir>` and inspect the new blob
   (`opblob inspect`). The live catalog is unchanged.
4. **Release.** The only deploy path is the platform release
   (`./activate-golden.sh` → slot activation). After it, the blob is in the live
   catalog and the operation file appears in the subsidiary's mount
   automatically.
5. **Grant.** The identity gets the method's exact capability (an owner-approved
   grant change). Without it, the call is refused with the capability named —
   which is itself demonstrable.
6. **Call and verify.** As the identity: write arguments to the call file,
   read the result from the same handle, keep the `operation_id` and
   `event_hash` as evidence.

## Mounts today (2026-10-28)

| Mount | Identity | Status |
|---|---|---|
| `/home/3tched/Hackathon/3tchedfs` (host) | Hackathon subsidiary `<subsidiary-session>` | mounted, runit `subsidiary-hackathon-3tchedfs` |
| `/run/mount/3tchedfs`, `/run/mount/threetched` (inside the identity container) | same identity | mounted |
| `/run/mounts/3tchedFS`, `/run/mounts/threetched` (host) | owner `<owner-session>` | **not mounted** — fstab entries missing since the reboot; owner's decision to restore |

Known limits: live state reads (`plugins/*/value.json`) return EIO — use
operations; the `3tched-fs call --mount` CLI cannot drive guest-kernel mounts —
use the call files directly. Do not use the leftover `3tched-fs mcp`
subcommand or the bridge's `/mcp` endpoint: MCP is retired in favour of the
mount.
