# Requirements Document — `op-mcp-door`: external MCP endpoint on the door, FUSE behind it

Status: APPROVED FOR IMPLEMENTATION. Owner request of 2026-11-19 ("write then
implement agentically, stop if there is a blocker I need to resolve").
Implementation authorizes source, tests and repo-only deploy templates. It
authorizes no seal, reseal, activation, service install or start, grant
change on the live host, or edit of the bridge, 3tchedFS, `op-tunnel-mux` /
`op-tunnel-door`, ZeroClaw, `cozo-ublk`, pools, the seed or the overlay.

Spec id: `mcp-door-adapter`
Spec home: `.kiro/specs/mcp-door-adapter/`
Source material: the cognitive/MCP audit of 2026-11-19 (§5 door design, §6
capabilities, grants and the public class), plus the owner corrections of
2026-11-19 recorded in `design.md` "Owner corrections".

Related specs, which this one does not override:
`.kiro/specs/identity-bound-fuse-application-migration/` (identity on the
mount, no automatic retry after an uncertain outcome),
`.kiro/specs/3tchedfs-threetchedfs/` and
`.kiro/specs/3tched-fs-mount-consolidation/` (mount names, surfaces, call
files), `.kiro/specs/ai-sdk-door-adapter/` (OIA1/SID1 identity on the door).

## Introduction

External clients (Cursor, other IDEs, LLM agents) lost every cognitive tool
when the bridge's `/mcp` route was unwired in `66f1d99` ("Fresh bridge",
2026-10-29). Internal traffic now runs entirely over 3tchedFS call files. The
door (TLS ingress) is still needed for external connections, and is now
quiet.

`op-mcp-door` is a new, standalone binary and crate. It is the external MCP
endpoint on the door: MCP Streamable HTTP over TLS only, on its own port by
default. Behind it there is only FUSE: every tool call becomes one write to
a `plugins/<p>/operations/<m>/call` file on the caller's mount and one
receipt read from the same handle. The door never speaks gRPC, D-Bus, or any
bridge socket itself, and does not depend on the lungs socket. What serves a
FUSE call behind the mount (the bridge, over D-Bus or gRPC) is not the door's
concern.

In scope: tool generation from the mounted operation schemas; two endpoints
(`/mcp/full` curated, `/mcp/loader` five meta-tools); a toolsets file
generated from the catalog; a capability class taxonomy with a `public` class
and a hard deny-list; one `authorize()` used by every list, search, describe,
load and call path; a door-side grant store scoped by identity, tool,
arguments, time and call count; per-call elevation tickets for `shell`,
`exec`, `admin` and `secret`; an append-only JSONL audit log; runit service
and log-service templates under `deploy/`.

Out of scope: routing inside `op-tunnel-door` (ALPN/SNI demux or a new
splice), any bridge change, any 3tchedFS change, resealing capabilities,
SID1 verification, starting the bridge, Qdrant or NotebookLM, and installing
anything on the live host.

## Requirements

### Requirement 1 — TLS-only door endpoint

**User Story:** As the owner, I want the external MCP endpoint to accept only
TLS connections, so that no credential or tool result ever crosses the door
in plaintext.

#### Acceptance Criteria

1. WHEN `op-mcp-door serve` starts THEN THE SYSTEM SHALL listen only through a
   TLS acceptor and SHALL NOT open any plaintext listener.
2. IF the TLS certificate or private key file is missing, unreadable, empty
   or unparsable THEN THE SYSTEM SHALL refuse to start and exit non-zero.
3. IF the private key file is readable by group or other THEN THE SYSTEM SHALL
   refuse to start.
4. WHEN a client sends plaintext HTTP to the listener THEN THE SYSTEM SHALL
   close the connection without producing any HTTP response.
5. THE SYSTEM SHALL offer no option, flag or environment variable that
   disables TLS.
6. WHERE no explicit certificate path is configured THE SYSTEM SHALL use the
   door's existing certificate convention
   (`THREETCHED_TLS_CERT_FILE` / `THREETCHED_TLS_KEY_FILE`, default
   `/var/lib/op-dbus/door-tls/door.crt` / `door.key`).
7. THE SYSTEM SHALL never write key material, assertion bytes or bearer
   values to stdout, stderr or the audit log.
8. WHERE no listen address is configured THE SYSTEM SHALL bind a port
   separate from the bridge door (`127.0.0.1:8091`).

### Requirement 2 — MCP Streamable HTTP

**User Story:** As an external MCP client, I want a standard Streamable HTTP
endpoint, so that Cursor and other LLM clients can connect with a URL and
headers.

#### Acceptance Criteria

1. WHEN a client POSTs a JSON-RPC `initialize` request THEN THE SYSTEM SHALL
   reply with server info, a supported protocol version and an
   `Mcp-Session-Id` header.
2. WHEN a request other than `initialize` arrives without a known
   `Mcp-Session-Id` THEN THE SYSTEM SHALL reject it (400 when missing, 404
   when unknown).
3. WHEN a client sends a JSON-RPC notification THEN THE SYSTEM SHALL answer
   202 with no body.
4. WHEN a client sends GET to an MCP endpoint THEN THE SYSTEM SHALL answer 405.
5. WHEN a client sends DELETE with a session id THEN THE SYSTEM SHALL end
   that session.
6. IF a request carries an `Origin` header that is not on the configured
   allow-list THEN THE SYSTEM SHALL answer 403.
7. IF a request body exceeds the configured limit or is a JSON-RPC batch THEN
   THE SYSTEM SHALL reject it without dispatching any tool.
8. WHEN a session's credential on a later request resolves to a different
   principal than at `initialize` THEN THE SYSTEM SHALL reject the request.

### Requirement 3 — FUSE-only dispatch

**User Story:** As the owner, I want every door call to go through the
caller's 3tchedFS mount, so that the door adds no second control path.

#### Acceptance Criteria

1. WHEN an authorized `tools/call` arrives THEN THE SYSTEM SHALL open
   `plugins/<encoded-plugin>/operations/<encoded-method>/call` on the mount
   mapped to the caller, write the canonical arguments once, fsync, and read
   the terminal receipt from the same handle.
2. THE SYSTEM SHALL NOT open any gRPC channel, D-Bus connection or lungs
   socket.
3. IF the mount's `.3tchedfs.json` does not declare format `3tchedfs/1` and
   the expected identity mode THEN THE SYSTEM SHALL refuse calls on that
   mount.
4. IF the mount changes during a call, the write or fsync fails without a
   terminal diagnostic, or the receipt is missing or malformed THEN THE
   SYSTEM SHALL return an "unknown outcome" error and SHALL NOT retry.
5. IF the receipt is a diagnostic (`error`) THEN THE SYSTEM SHALL return it
   as a tool error attributed to the plugin and method.
6. IF the receipt's plugin, method, subid, required capability, side effect
   or idempotence differs from the mounted metadata THEN THE SYSTEM SHALL
   treat the outcome as unknown.
7. WHEN arguments fail the operation's `input.schema.json` THEN THE SYSTEM
   SHALL reject the call before opening the call file.
8. WHEN a mutating operation is called without `confirm_side_effects` THEN
   THE SYSTEM SHALL reject it before opening the call file.

### Requirement 4 — Tools generated from operation schemas

**User Story:** As an MCP client, I want typed tools derived from the sealed
operation schemas, so that argument errors are caught before a call.

#### Acceptance Criteria

1. WHEN the door starts THEN THE SYSTEM SHALL build its catalog from
   each `plugins/*/operations/*/metadata.json` (its `args` and `returns` are the
   documents also mounted as `input.schema.json` and `output.schema.json`)
   and SHALL NOT require a plugin-level `schema.json` with `.methods`.
2. THE SYSTEM SHALL name hot tools by their bare method name and every other
   tool `<plugin>__<method>`, restricted to `[A-Za-z0-9_-]` and at most 64
   characters, with a stable hash suffix when truncated.
3. THE SYSTEM SHALL set `readOnlyHint` from `side_effect == read` and
   `destructiveHint` for delete, kill, remove, purge and similar mutations.
4. IF an operation directory lacks readable metadata THEN THE SYSTEM SHALL
   skip that operation and record it, without failing startup.

### Requirement 5 — Toolsets and tiers

**User Story:** As a Cursor user with a 50-tool limit, I want hot tools
always loaded and the rest grouped, so that I choose what fills my slots.

#### Acceptance Criteria

1. THE SYSTEM SHALL define tier 1 as the seven memory tools and tier 2 as the
   ten hot tools (`code_search`, `code_context`, `code_index`,
   `search_own_events`, `sequential_thinking`, `context_manager`,
   `specialist`, `schema_read`, `notebook_query`, `workflow_query`), with the
   coding tools and the knowledge tool (`search_own_events`) mandatory
   (owner decision 2026-11-19; `oscal_subids` moved to the `cognitive` set).
2. WHEN `op-mcp-door toolsets generate` runs THEN THE SYSTEM SHALL write a
   toolsets file from the mounted catalog containing the tiers, the domain
   toolsets and exactly one `cognitive` toolset holding every `cognitive`
   operation that is in neither tier.
3. WHERE a toolsets file is configured and readable THE SYSTEM SHALL use it;
   otherwise THE SYSTEM SHALL generate the toolsets in memory at startup.
4. IF a tool named in a tier is absent from the catalog THEN THE SYSTEM SHALL
   omit it and report it in `list_toolsets`.
5. THE SYSTEM SHALL NOT place a tier-1 memory tool in any toolset other than
   `memory`, including when a hand-edited toolsets file lists one.
6. THE SYSTEM SHALL mark the `cognitive` toolset `lazy` and SHALL NOT generate
   any other toolset (persona, coding, reasoning or whole-plugin) that groups
   `cognitive` operations.
7. WHILE any collection a vector-backed hot tool depends on (coding: the seven
   voyage-4 retrieval collections; knowledge: `snowball_footprints`) is absent,
   empty or unreadable THE SYSTEM SHALL keep the tool listed and SHALL answer
   its calls with a `pending_revectorization` status, writing nothing to FUSE
   and consuming no grant use or elevation ticket.
8. THE SYSTEM SHALL determine vector readiness with read-only requests and
   SHALL NOT create collections, write points or start re-vectorization.
9. WHEN `list_toolsets` is called THEN THE SYSTEM SHALL include each vector
   gate's status.
10. THE SYSTEM SHALL keep `deploy/config/toolsets/cognitive.toolsets.json`
    (the manifest for the FUSE toolset projection) consistent with its tiers,
    `cognitive` set and vector gates, and a test SHALL fail when they differ.

### Requirement 6 — Two endpoints

**User Story:** As a Cursor user, I want a curated endpoint and a loader
endpoint, so that I get typed hot tools and can still reach every tool.

#### Acceptance Criteria

1. WHEN a client lists tools on `/mcp/full` THEN THE SYSTEM SHALL return the
   authorized members of tier 1, tier 2, the selected toolsets and the meta
   tools `list_toolsets` and `load_toolset`.
2. THE SYSTEM SHALL accept toolset selection by path (`/mcp/full/<a>+<b>`),
   query (`?toolsets=a,b`) or header (`X-MCP-Toolsets: a,b`).
3. IF a selection would list more than 50 tools THEN THE SYSTEM SHALL refuse
   it with an error that names the requested count and the cap.
4. WHEN `load_toolset` would exceed 50 tools on `/mcp/full` THEN THE SYSTEM
   SHALL refuse it and leave the session unchanged.
5. WHEN a client lists tools on `/mcp/loader` THEN THE SYSTEM SHALL return
   exactly `list_toolsets`, `search_tools`, `describe_tool`, `call_tool` and
   `load_toolset`.
6. WHEN `call_tool` names a tool THEN THE SYSTEM SHALL apply exactly the same
   authorization, validation, confirmation and elevation as a direct call.
7. THE SYSTEM SHALL NOT expose any generic dispatcher other than `call_tool`.

### Requirement 7 — Capability classes and the public class

**User Story:** As the owner, I want every tool classified by risk, with a
public class that needs no grant, so that anonymous clients can discover but
never act.

#### Acceptance Criteria

1. THE SYSTEM SHALL assign every tool exactly one class from `public`,
   `read`, `write`, `invoke`, `exec`, `shell`, `network`, `storage`, `admin`
   and `secret`, deterministically from its metadata.
2. THE SYSTEM SHALL classify a tool as `public` only when it is on the public
   allow-list, is a read, is idempotent, and passes the hard deny-list.
3. THE SYSTEM SHALL keep every memory tool, every tool with a `command`
   argument, and every `write`, `exec`, `shell`, `network`, `storage`,
   `admin`, `secret` and `invoke` tool out of `public`, even if the allow-list
   names it.
4. WHILE a caller is unauthenticated THE SYSTEM SHALL list, describe, search,
   load and call only public tools.
5. WHEN a public tool is called THEN THE SYSTEM SHALL rate-limit it per
   client address.

### Requirement 8 — One authorization function

**User Story:** As the owner, I want one policy decision everywhere, so that
no meta-tool can bypass a grant.

#### Acceptance Criteria

1. THE SYSTEM SHALL route `tools/list`, `search_tools`, `describe_tool`,
   `list_toolsets`, `load_toolset`, `call_tool` and direct `tools/call`
   through one `authorize()` function.
2. IF `authorize()` denies a tool THEN THE SYSTEM SHALL hide it from every
   listing and SHALL NOT write any call file.
3. WHEN a caller names an unknown, hidden or denied tool THEN THE SYSTEM
   SHALL return the same `forbidden_or_unknown` error.
4. IF authorization cannot be evaluated (grant document unreadable or
   invalid) THEN THE SYSTEM SHALL deny everything except public tools.

### Requirement 9 — Door-side grants

**User Story:** As the owner, I want grants scoped by identity, tool,
arguments, time and count, so that a grant gives exactly what I approved.

#### Acceptance Criteria

1. THE SYSTEM SHALL read the durable grant document read-only, with the same
   contract as the bridge (exact principal keys, closed `$` groups, no `*`).
2. THE SYSTEM SHALL read ephemeral grant records from a root-only directory,
   each scoped to a principal, a tool or toolset, an optional argument
   constraint, `not_before`, `expires_at` and `max_calls`.
3. WHEN an ephemeral grant has expired, is not yet valid or has no calls left
   THEN THE SYSTEM SHALL treat it as absent.
4. WHEN a call is allowed by an ephemeral grant THEN THE SYSTEM SHALL consume
   one call before the FUSE write.
5. IF arguments violate a grant's argument constraint THEN THE SYSTEM SHALL
   deny the call.
6. THE SYSTEM SHALL allow toolset-scoped grants only for `public`, `read` and
   `invoke` tools.
7. THE SYSTEM SHALL accept a `shell` grant only from an ephemeral record, never
   from the durable document.

### Requirement 10 — Per-call elevation tickets

**User Story:** As the owner, I want shell, exec, admin and secret calls to
need a one-time ticket bound to the exact arguments, so that a standing grant
is never enough for a dangerous call.

#### Acceptance Criteria

1. WHEN a `shell`, `exec`, `admin` or `secret` tool is called THEN THE SYSTEM
   SHALL require an elevation ticket bound to the principal, the tool and the
   SHA-256 of the canonical arguments.
2. WHEN a ticket is used THEN THE SYSTEM SHALL consume it atomically before
   the FUSE write, so it can never be used twice.
3. IF a ticket is expired, older than its maximum lifetime of 300 seconds, or
   bound to other arguments THEN THE SYSTEM SHALL deny the call.
4. THE SYSTEM SHALL never list `shell` or `exec` tools in `core` or in any
   default selection.

### Requirement 11 — Audit log

**User Story:** As the owner, I want every decision in an append-only log, so
that I can reconstruct who called what.

#### Acceptance Criteria

1. WHEN any call decision is made (allow or deny, including public) THEN THE
   SYSTEM SHALL append one JSON line with time, session, principal, endpoint,
   path (direct or `call_tool`), tool, class, capability, grant basis,
   decision, argument hash, confirmation, ticket reference, receipt event id,
   outcome and latency.
2. THE SYSTEM SHALL record full arguments only for `public` and `read` tools,
   and never for `secret` tools.
3. THE SYSTEM SHALL write audit lines in append mode only.
4. THE SYSTEM SHALL ship a runit service with a log service as repository
   files under `deploy/`, and SHALL NOT install them.

### Requirement 12 — Identity

**User Story:** As the owner, I want the door to know who is calling from the
existing OIA1 assertion, so that grants and mounts are per principal.

#### Acceptance Criteria

1. WHEN a request carries an OIA1 assertion (header
   `x-oracle-identity-assertion-bin` or `Authorization: Bearer`) THEN THE
   SYSTEM SHALL verify its Ed25519 signature against the configured decoy
   verifying keys and its validity window.
2. IF the assertion is malformed, badly signed, expired, or its WireGuard key
   has no registered principal THEN THE SYSTEM SHALL treat the caller as
   unauthenticated.
3. IF the same assertion nonce is presented for a second session THEN THE
   SYSTEM SHALL reject it.
4. WHEN a principal is authenticated THEN THE SYSTEM SHALL dispatch its calls
   only to the mount mapped to that principal.
5. IF a principal has no mapped mount THEN THE SYSTEM SHALL deny every
   non-public call.
