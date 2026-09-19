# Identity Handoff Boundaries

This document is the authoritative boundary statement for the corrected
identity handoff. It has three parts: non-negotiable architecture boundaries,
the repudiated design path, and the external integration boundaries that this
mission documents but does not deploy.

## 1 · Non-negotiable architecture boundaries

1. **Oracle decoy is the sole incoming WireGuard termination point.** The
   main host has no incoming identity WireGuard interface; never add
   `wg-lan` or any host WG interface.
2. **No NetMaker transport remains.** The decoy's standalone `wg0` terminates
   human WireGuard; its standalone `wgcf-ingress` carries Xray egress through
   WARP. The retired static NetMaker carrier must not be recreated.
3. **Assertion carriage is INNER.** The short-lived Ed25519-signed
   `OracleIdentityAssertion` rides as gRPC metadata
   (`x-oracle-identity-assertion-bin`) inside the existing TLS channel.
   Xray is a passthrough and never sees plaintext identity state.
4. **`op-grpc-bridge` is the sole validator and the application
   authorization boundary.** Validation order: parse → trusted decoy key →
   signature → expiry → replay cache → configured transport binding →
   HumanPrincipal resolution → existing capability gate. Production WARP uses
   the trusted-decoy-signature binding; every step remains fail-closed.
5. **Human identity, WireGuard key, login session, workspace container, and
   display alias are separate concepts.** A workspace container is not the
   human. System containers are never users. The display alias is
   display-only and never authoritative.
6. **The authenticated client request triggers resolution.** No handshake
   watchers, polling loops, D-Bus watchers, or background expiry tasks (the
   replay cache purges lazily on access). SSH login alone is administration;
   it does not mint or carry OIA1 identity.
7. **Xray live config exists only at `/etc/xray/xray_config.json` inside the
   xray container.** Models do not write or reload xray directly.
   `op-xray-daemon` remains lifecycle-only.
8. **PluginSchema is the source of truth.** Identity operations are methods
   on the `human_principal` plugin via the generated gRPC surface
   (`PluginService` → D-Bus → `MutationEngine`). No hand-written per-plugin
   proto, no direct backend RPC, no second identity control plane.

## 2 · Repudiated design path (do not reintroduce)

The rejected spec `claude-redo/netmaker-xray-identity-handoff/` proposed the
following, all of which are forbidden in this architecture:

| Rejected mechanism | Why it is rejected |
|---|---|
| `wg-lan` host WireGuard interface | Violates boundary 1; WG terminates only at the Oracle decoy |
| `op-identity-shuttle` handshake-watcher service | Polling-based; violates boundary 6 |
| `TransportBindingIndex` (src_ip → pubkey SHM table) | Source-IP assertion is not a cryptographic binding; 180 s race window |
| Per-peer OpenFlow identity tagging (`NXM_NX_REG` tags) | Datapath theater; the real gate remained the IP lookup |
| Per-registration identity containers as verifiers | Turns a network segment into a trusted intermediary; verification belongs at the single existing gate |
| Xray header injection into TLS | Impossible without xray becoming a TLS-terminating MITM |

These tokens are enforced by the negative topology gates
(`crates/op-grpc-bridge/tests/negative_topology_gates.rs`,
`scripts/check-identity-topology.sh`).

## 3 · External integration boundaries (documented, NOT deployed by this mission)

The mission implements both sides LOCALLY: the decoy issuer as a library plus
a local decoy simulator for E2E tests over real TLS on ephemeral localhost
ports. The following are external assumptions that production deployment must
satisfy; they are documented here and tested only through their local
stand-ins.

### 3.1 Oracle decoy (EXTERNAL)

- Terminates the human's WireGuard tunnel on `wg-opdbus` (`10.0.0.1/24`);
  the kernel verifies the peer. The issuer listens on that inner address
  (`10.0.0.1:51888`).
- Issues the signed assertion for the exact `/32` peer selected by the kernel
  (decoy Ed25519 key identified by
  `decoy_key_id`) with TTL ≤ 900 s.
- The client that requested the assertion carries the returned OIA1 envelope
  as `x-oracle-identity-assertion-bin` on its inner tonic request. Neither
  WGCF nor Xray creates or injects that metadata.
- **Client pickup (2026-09-19).** The handoff is atomic at WG ingress and the
  client, not the transport, carries it: the issuer serves the same envelope
  two ways — `GET /assertion` (base64 text, the bridge's Rust acquisition
  client) and `GET /identity/self` (JSON `{door, identity:{kind:"oia1",
  assertion}, expires_at}` with CORS for the configured UI origins,
  `--allow-origin` / `OP_DECOY_ISSUER_ALLOWED_ORIGINS`, door from `--door` /
  `OP_DECOY_ISSUER_DOOR`). The 3tched UI resolves identity in order: injected
  by the host page → same-origin `/api/identity/self` (local SID1) →
  `<issuer>/identity/self` over the human's tunnel (`__3TCHED_CONFIG__.issuer`
  / `VITE_OPDBUS_ISSUER`) → pasted at the connect gate. It keeps the assertion
  in memory only, forwards it unchanged over WARP to the `:8090` door, and
  re-fetches it `RENEW_LEAD_SECONDS` before `expires_at`, swapping the
  credential on the live door client without reconnecting.
- The decoy's verifying keys are provisioned to the bridge's trust store
  (`OP_DECOY_TRUST_STORE`, default `/etc/opdbus/decoy-trust.json`) by an
  operational process outside this mission.

### 3.2 Assertion transport (EXTERNAL)

- Exactly one WARP tunnel carries decoy→host Xray traffic. The decoy's
  FwMark rule steers that traffic into `wgcf-ingress`; the decoy's user-facing
  `10.0.0.1` VIP reaches the host over that leg. **The host is never a
  WireGuard peer of the decoy** — the former `mkdrt` link (host peer on
  `wg-opdbus`, tunnelled inside WARP) is retired (2026-09-19) and gated by
  `host_is_never_a_wireguard_peer_of_the_decoy`.
- WARP and Xray replace the human WireGuard inner source address. Deployments
  therefore set `OP_ORACLE_ASSERTION_SOURCE_BINDING=trusted-decoy-signature`:
  the bridge binds identity to the provisioned decoy Ed25519 key, short
  assertion lifetime, one-time nonce, and registered human WireGuard key.
- `exact-peer-ip` remains the strict default for transports that genuinely
  preserve the human inner source address. Unknown configuration values fall
  back to that strict mode.
- WARP is transport-level policy; it is not identity.

### 3.2a The EMQX doorbell (reactive delivery, decided 2026-09-19)

Owner decisions: **no broker ACL tables, no MCP over MQTT, nothing
authoritative on the broker; SHM is the one source of truth.** EMQX Cloud
"Agents" was evaluated and rejected (hosted on EMQX Cloud/GCP; puts agent
definitions, runs and credentials off-host). What remains of EMQX in the
identity picture is a *doorbell*:

- A knowledge push is first committed by the MutationEngine as a durable
  `ctl_plane_chatbot_knowledge` row and the `knowledge` child of the
  session's private SHM document (the same owned effect as an episode; the
  door's private StateSync view wakes). Only then does the bridge call the
  declared `emqx.publish_wake` method, which publishes `{"seq": revision}` —
  nothing else — to `opdbus/wake/<owner-key>`, `owner-key =
  sha256(principal, session, genesis)`. The broker never sees an identity or
  a value; retain is off.
- The UI (`3tched.com`) connects to the `wss:wake` listener with **client id
  = owner key** (from `ctl_plane_chatbot.get_doorbell`) and **password = its
  own door credential** (`oia1:<base64>` from the decoy pickup, fresh per
  CONNECT because the nonce is one-time; or `sid1:…` inline for provisioned
  container sessions). On a wake it re-reads `list_knowledge` through the
  `:8090` door.
- The exhook (`wake_doorbell.rs`) answers `client.authenticate` and
  `client.authorize` with **functions of the presented identity, never a
  policy lookup**: the credential is verified exactly as the door verifies it
  (`AssertionValidator` / `authenticate_sid1_wire` with transport scope
  `mqtt`, registry + revocation), must derive the client id, and a doorbell
  client may only SUBSCRIBE to its own topic while an anchored, unexpired
  sled still derives it. Every other client is denied the `opdbus/`
  namespace and otherwise left to the broker's own rules. Tombstoning a sled
  kicks its owner key's connection (`emqx.kick_client`).
- Operator steps (not performed by workers): provision the EMQX `wss`
  listener at `10.0.0.3:8084` with the svc0 TLS material (`EmqxState::declared`
  lists it); existing SID1 sleds were minted with scope `dbus,grpc,mcp` and
  must be re-minted to gain `mqtt` before a container session can use the
  doorbell (browsers use OIA1 and are unaffected).

### 3.2b Event-triggered agent runs (OD-63, 2026-09-19)

The loop of EMQX Cloud "Agents" (trigger → run → tools → run record),
hosted here and inside the identity model. Spec:
`docs/specs/od-63-event-triggered-agents/`.

- **Triggers are sealed `persona` plugin state** (`list_triggers` /
  `set_trigger` / `delete_trigger`, durable runtime document with CAS
  revisions): `{topic_filter, agent_session_id, instructions,
  allowed_methods, max_payload_bytes, max_actions, timeout_ms}`. Filters can
  never reach `opdbus/` or `$` topics and must start with a literal level.
- **The broker only delivers the event.** The exhook's `message.publish`
  (already loopback mTLS) matches enabled triggers and enqueues a bounded
  run; the payload is read only on a match and only up to the trigger's
  bound; the publish verdict stays the broker's.
- **A run executes under an agent identity sled.** The trigger's
  `agent_session_id` must resolve to an anchored, unexpired sled whose
  principal is registered and unrevoked (`verified_session`) or the run is
  dropped before any effect — no fallback actor. Every action is a plugin
  method dispatched through `dispatch_method_call_with_identity` with the
  agent's `(principal, session, genesis)`, so it carries the agent's
  `sealed_id_digest` and must pass the agent's own grants and the sealed
  method contract's declared capability; the trigger's `allowed_methods` is
  a further allow-list. The model's answer is untrusted; the publisher is
  never an identity.
- **The run record is an episode in the agent's own private scope**
  (`ReasoningEpisode.run: RunRecord{status, trigger_id, topic_ref (digest),
  queued_ms, duration_ms, tokens_in, tokens_out, actions[]}`), committed
  through the same durable + private SHM path as chat episodes. Statuses:
  queued / running / succeeded / dropped / failed / timeout.
- Operator steps: mint the agent's sled like any principal (register its key,
  anchor a genesis), materialize its grants, then `set_trigger`.

### 3.2c Vectorize on mutation (OD-64, 2026-09-19)

Owner decisions: the semantic shuttle is **not optional**; every Cozo write
path is vectorized; a mutation with a gRPC payload is vectorized on mutation;
the noise filter is **the sealed plugin schema that created the data**.

- Every chain event is already a Snowball timing block. `ChainVectorProjector`
  (bridge) wakes on each audit write and runs `ChainVectorIndex::project_pending`
  chain-first: embedding → `vectors/` subvolume (+ `vec-N.bin.meta.json`
  sidecar: host load from `/proc/loadavg` + PSI, `queued_ms`, `embed_ms`,
  policy) → `btrfs send/receive` → Qdrant on the replica (`ingest_received`).
  Origin upsert only under `OPDBUS_CHAIN_VECTORS_UPSERT_ORIGIN=1`. A block is
  durable before its vector exists: an embedder/Qdrant outage delays the
  index, never the control plane; the loop retries loudly and drains backlog.
- `SchemaProjectionPolicy`: the embedding text is the method's (or signal's)
  schema-described arguments — declared fields with the descriptions the
  schema author wrote, `x-secret` subtrees dropped, undeclared keys ignored
  (`op_state_store::schema_projection`). Hashes, event ids, raw timestamps
  and identity metadata never enter the text; they go to the Qdrant payload
  as allow-listed filters (`actor_id`, `session_id`, `sealed_id_digest`,
  `capability_id`, `decision`, `subid`, `schema_version`, `vector_meta`).
  Unsealed/retired plugins embed only the block header.
- Durable chatbot writes are announced into the chain under the principal
  through declared signals (`ctl_plane_chatbot.episode_committed`,
  `knowledge_committed`, payload = the committed document's schema), so
  episodes (chat and agent runs) and knowledge pushes are vectorized on
  commit through the same projector. `memory_store` already upserts
  `user_memory`; with the mandatory shuttle it always does.
- `CognitiveMcpServer::new_durable` fails closed without Qdrant + a Voyage
  key: cognitive methods report Unavailable until they are reachable.
- Retrieval from the chain collection is cross-principal at the collection
  level; readers **must** filter by `actor_id`/`sealed_id_digest` payload
  fields (retrieval stays principal-scoped — §OD-47).

### 3.2d Memory binding to the selected chat mode (OD-65, 2026-09-19)

Owner rules: "it will always be principal; projects are second class,
controlled by the principal"; modes are personal (with user-defined
sub-spaces), project, control_plane, accountability, notebooklm, and
ui_render which has **no memory**; the mode is what the principal selected
in the chat interface.

- The selection is state: `project.select_context{mode, project_id?,
  personal_namespace?}` validates through `resolve_context` (grants,
  membership, control-plane grant, selected notebook) and persists per
  session in `chat_context_selection`; `get_context` re-resolves it on every
  read, so a revoked membership or grant takes effect immediately and a
  selection that stops resolving fails closed (no silent fallback).
- Namespaces (`memory_namespace_for`): `session/{s}/personal[/{name}]`,
  `project/{id}`, `session/{s}/control_plane`, `session/{s}/accountability`,
  `session/{s}/notebooklm/{notebook}`, and none for ui_render. `{s}` is the
  sled session id — the principal's account-lifetime identity — so every
  partition sits under the principal.
- Every `memory_*` call is bound at the door (`memory_binding`): the
  namespace comes from the selection, never the caller. A caller-supplied
  `namespace` is honoured only in personal mode and only as a bare
  `[a-z0-9_-]` sub-name; anything else is replaced. Writes are refused in
  read-only modes (accountability; a project `Reader`); everything is
  refused in ui_render. `memory_list_namespaces` shows only the principal's
  partitions, the bound one, and the shared `agent_pieces`.
- A turn's recall (`context_bundle`) and `specialist` read the bound
  namespace; in ui_render nothing is recalled and no pushes are injected.
  Episodes (the ledger) stay principal-scoped in every mode.
- Project members are read/write by default; a per-member read-only toggle
  is wishlist (OD-66).

### 3.3 Xray container (EXTERNAL, unchanged)

- Passthrough only: SNI/protocol sniffing, no TLS termination for this
  traffic, no header injection, no identity logic.
- Live config only at `/etc/xray/xray_config.json` inside the container;
  models never write or reload xray.

## 4 · Mission safety boundaries (workers NEVER violate)

- No deploy, no sudo, no `/etc` edits, no service restarts, no live-host
  mutation. Cargo tests only.
- Never read credential files in `~/` (`master_key.txt`, `regkeys.txt`,
  `token.txt`, `netmk-rollback-*`).
- Rust-first: no new Python; scripts are shell.
- All cargo commands use `CXXFLAGS="-include cstdint"`; bridge lib tests run
  with `-- --test-threads=1` (pre-existing env/SHM race).
