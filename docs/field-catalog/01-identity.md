# Slice 1 — Identity, session, principals, keys

Source: FldIdentity field inventory (read-only, cited). Integrated verbatim; canonical proposals are proposals, not renames applied to code.

Sealed/live verification is pending (door unreachable) — stated once, per contract. Read-only inventory; no files changed.

## identity_sled — `ContainerIdentitySled` / `IdentitySledState` (`crates/op-plugins/src/state_plugins/identity_sled.rs`), v4.7.0:295, methods:528-700

|field_as_found|source (file:line)|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity (public\|hash-only\|private)|notes|
|---|---|---|---|---|---|---|---|
|session_id|identity_sled.rs:49|String|id (sessdecl), session_id|session_id|opaque_id (UUID-form, blake3-KDF-derived)|private|Container name = session_id, derived from WG pubkey; never caller-supplied on write (WriteIdentityInput:356-359)|
|wireguard_pubkey|identity_sled.rs:52|String|human_pubkey, pubkey, public_key|wireguard_pubkey|base64 X25519 pubkey, 32B decoded|private|Cryptographic root of identity; base64 (not hex)|
|interface|identity_sled.rs:55|String|iface (op-identity wireguard.rs:29)|wireguard_interface|string|private|WG interface name|
|peer_ip|identity_sled.rs:58|Option<String>|allowed_ip, tunnel_inner_ip, peer_ip|peer_ip|IP addr string|private|Unbounded string where an IP type belongs|
|mutation_index|identity_sled.rs:61|u64|mutation_index|mutation_index|u64|hash-only|Monotonic accountability-loop position|
|genesis|identity_sled.rs:67|Option<String>|hashed_footprint (serde alias), session_genesis (SID1)|session_genesis_hash|lowercase hex blake3 (32B)|hash-only|Minted once at arrival; alias `hashed_footprint` is legacy v2|
|trace_id|identity_sled.rs:70|String|trace_id|trace_id|opaque_id|hash-only||
|schema_version|identity_sled.rs:73|u32|schema_version|schema_version|u32|public||
|vector_id|identity_sled.rs:76|String|vector_id|vector_id|opaque_id|private|Semantic embedding id|
|sealed_id|identity_sled.rs:84|Option<String>|sealed_id, SID1|sealed_id|sid1:<unpadded-base64url>|private|MutationEngine-authored only; never caller-supplied/rebuilt; contains full SealedId claims|
|principal_kind|identity_sled.rs:90|Option<String>|principal_kind (SID1, different meaning), actor class|actor_class|enum: human\|service|private|⚠️ Same name, different semantics vs SID1 `principal_kind=wireguard-principal` (see mis-normalized)|
|btrfs_device|identity_sled.rs:97|Option<SledBtrfsDevice>|btrfs_device|btrfs_device|record|private|See sub-record below|
|instance|identity_sled.rs:104|Option<IncusInstance>|instance, native instance|instance|record (Incus shared/api)|private|Full Incus definition inlined|
|session_started_at|identity_sled.rs:107|i64 (unix s)|session_started_at, created_at|session_started_at|RFC3339 UTC (from unix s)|private||
|last_seen_at|identity_sled.rs:110|i64 (unix s)|last_seen_at|last_seen_at|RFC3339 UTC|private||
|active|identity_sled.rs:115|bool|active|active|bool|private|Also power policy (running vs parked), not identity validity|
|expires_at|identity_sled.rs:124|Option<i64>|expires_at|expires_at|RFC3339 UTC|private|Current visit/term expiry; lifelong accounts renewed|
|arrival_timestamp|identity_sled.rs:128|i64 (unix s)|arrival_timestamp, issued_at (SID1 equality)|arrival_at|RFC3339 UTC|hash-only|Immutable first-authenticated-mutation time; genesis input|
|chain_head_at_arrival|identity_sled.rs:131|String (hex)|chain_head_at_arrival, chain_head_hash|chain_head_at_arrival_hash|lowercase hex hash (16B native / 32B legacy)|hash-only|Genesis input|
|catalog_hash_at_arrival|identity_sled.rs:134|String (hex)|catalog_hash_at_arrival, catalog_hash, schema_catalog_hash|catalog_hash_at_arrival|lowercase hex sha256/blake3 32B|hash-only|Genesis input|
|head_timestamp_at_arrival|identity_sled.rs:137|i64|head_timestamp_at_arrival, head_timestamp|head_timestamp_at_arrival|RFC3339 UTC|hash-only|Genesis input|

`SledBtrfsDevice` (identity_sled.rs:165-183): `device_path:167 String`, `mount_point:170 String`, `btrfs_uuid:173 String`, `cozo_id:178 String`, `attached:182 bool` — all `private`; canonical: `btrfs_uuid`→uuid, `cozo_id`→`cozo_owner_id: opaque_id`. Never the shared seed (comment:166).

`SessionEvent` (identity_sled.rs:188-203): `session_id:190 String`, `seq:192 u64`, `kind:194 String` (unbounded, should be enum), `subid:197 String` (OSCAL subid), `content:200 String` (JSON-as-string, unbounded — provenance payload, `private`), `created_at:202 i64` → `created_at: RFC3339 UTC`. State `events:217 Vec<SessionEvent>` explicitly "remains private" (214-215).

Method I/O (identity_sled.rs:308-523): `GetIdentityInput.session_id:311` (empty=host), `WriteIdentityInput.wireguard_pubkey/interface/peer_ip/btrfs_device:359-366`, `PrepareContainerInput{wireguard_pubkey,operation_id}:371-372`→`PrepareContainerOutput{session_id,name,native_uuid,operation_id,phase,replayed}:376-382`, `ProvisionContainerInput{wireguard_pubkey,plan_sha256:393}` (64-hex, regex-pinned), `RepairOwnRootInput.plan_sha256:440`, `InspectOwnRootRepairPreflightOutput{session_id,genesis,revision,revision_operation_id,btrfs_device,instance_etag,instance_uuid,expected_first_start_idmap_sha256:338,image_fingerprint,source_pool,source_pool_source,native_status,root_subvolume}:329-344`, `RefreshCredentialOutput{refreshed,session_id,transport_scope}:350-353`, `RecordSessionEventInput{session_id,kind,subid,content}:492-500`→`{session_id,seq}:521-523`, `GetSessionHistoryInput{session_id,limit:509 u32 ≤4096}`. Capabilities (`required_capability`, 528-752): legacy dotted `identity_sled.read/write`, `identity_sled.inventory.read`, `identity_sled.credential.refresh` vs new-style `cap.identity.sled.self.read@v1`, `cap.identity.sled.root_repair@v1`, `cap.identity.sled.physical_root_migrate@v1` — mixed capability naming conventions in one plugin.

Metadata projection (`identity_sled_metadata.rs:38-52`): only `session_id, wireguard_pubkey, principal_kind, mutation_index, genesis, trace_id, schema_version, active, expires_at, arrival_timestamp, chain_head_at_arrival, catalog_hash_at_arrival, head_timestamp_at_arrival` survive; `sealed_id`/events/instance/btrfs deliberately withheld (5-6, 13-15) — this is the de-facto public/hash-only subset, though file warns StateSync public exposure is still withheld (5).

## human_principal — `HumanPrincipal` (`human_principal.rs:28-57`), methods:168-297

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|principal_id|human_principal.rs:32|String|principal_id, user (sessdecl)|principal_id|opaque_id (UUID-form, blake3-KDF "op-identity human-principal v1")|private|Derived from pubkey, never caller-supplied (30)|
|human_pubkey|human_principal.rs:36|String|wireguard_pubkey, pubkey, public_key|wireguard_pubkey|base64 X25519, 32B|private|Unique across active+revoked; root of identity|
|display_alias|human_principal.rs:39|String|display_alias, label, name|display_alias|string|public|Display-only, NEVER authoritative/resolvable (37); unique among active only|
|registered_at|human_principal.rs:41|i64|registered_at, created_at|registered_at|RFC3339 UTC|hash-only||
|revoked_at|human_principal.rs:45|Option<i64>|revoked_at|revoked_at|RFC3339 UTC|hash-only|Permanent tombstone; revoked key can never re-register|
|principals|human_principal.rs:56|Vec<HumanPrincipal>|principals|principals|array|private|State root|

Methods: `RegisterKeyInput{human_pubkey,display_alias}:172-176`, `RevokeKeyInput{human_pubkey}:187`, `SetAliasInput{principal_id,display_alias}:196-199`, `ResolveKeyInput{human_pubkey}:209` (aliases explicitly rejected), `GetPrincipalInput{principal_id}:221`; outputs wrap `principal: HumanPrincipal/Option`. Capabilities: `human_principal.write` (mutations, 244-265), `human_principal.read` (reads, 274-295) — dotted-legacy style.

## sess_decl — `SessionDeclaration` (`sessdecl.rs:37-76`, file is `sessdecl.rs`, plugin name `sess_decl`)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|id|sessdecl.rs:39|String|session_id, id|session_id|opaque_id|private|⚠️ Renamed from session_id; from verified context only, never caller args|
|user|sessdecl.rs:41|String|principal_id, user|principal_id|opaque_id|private|⚠️ `user` = stable principal_id (41); ambiguous name|
|label|sessdecl.rs:43|String|display_alias, label, name|display_label|string ≤128 (DeclareSessionInput:50)|public|Caller-selected, no auth meaning|
|declared_at|sessdecl.rs:44|String|declared_at, registered_at|declared_at|RFC3339 UTC|hash-only|Already RFC3339 via `Utc::now().to_rfc3339()` (138) — the only RFC3339 timestamp in slice|
|created / revision (outputs)|sessdecl.rs:66-67,75|bool / u64|revision, mutation_index|created / revision|bool / u64|hash-only|Idempotent re-declare|
|principal_id, session_id (SessDeclAccessContext)|sessdecl.rs:80-81|String|—|principal_id, session_id|opaque_id|private|Bridge-supplied verified context; owner key = `sess_decl:sha256("op-dbus.sess-decl.owner.v2\0"‖len‖principal_id‖len‖session_id)` (244-260) — hash-only routing key|

Capabilities: `sess_decl.read:358`, `sess_decl.write:368`. Explicitly does not mint identity or grant auth (345, 385-386).

## keypair — `Keypair`/`KeypairInfo` (`keypair.rs:47-139`, `keypair/agent.rs`)

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|name|keypair.rs:54,113|String|name, device_name, label|keypair_name|string|public|State-side and output-side duplicate struct|
|algorithm|keypair.rs:62,121|String|algorithm|algorithm|enum (ssh-ed25519, etc.)|public|Unbounded string where enum belongs|
|public_key|keypair.rs:70,129|Option<String>|public_key, wireguard_pubkey, human_pubkey|public_key|string `<algo> <base64-blob>`|public|SSH format, not bare base64 — different encoding convention from WG pubkeys|
|present|keypair.rs:78,138|bool|present, active|present|bool|public|Agent-advertised only; "does not assert signing permitted or private material exportable" (131-132)|
|fingerprint (output only)|keypair.rs:106|String|fingerprint, image_fingerprint|public_key_fingerprint_hash|SHA-256 hex/base64 of SSH wire encoding|hash-only|Output-side only — state `Keypair` lacks it|
|source (output)|keypair.rs:92|String|source, source_pool_source|source|string|public||

Single method `list_keypairs` (capability `cap.service.keypair.list@v1`:244 — new-style cap in an otherwise legacy plugin), empty input. **No private-key field anywhere**: agent adapter "never requests signatures or private key material" (agent.rs:3). This is the correct secret posture.

## keyring — `KeyringState`/`CollectionInfo` (`keyring.rs:29-44`), methods:257-276

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|collections|keyring.rs:31|Vec<CollectionInfo>|collections|collections|array|private||
|default_collection|keyring.rs:33,252|Option<String>|default_collection|default_collection_path|D-Bus object path string|private||
|path|keyring.rs:39|String|path|collection_path|D-Bus object path|private||
|label|keyring.rs:40|String|label, display_alias|label|string|public|Display label from Secret Service|
|locked|keyring.rs:41|bool|locked|locked|bool|hash-only||
|created|keyring.rs:42|u64 (unix s)|created, created_at|created_at|RFC3339 UTC|hash-only|⚠️ u64 here vs i64 unix timestamps everywhere else|
|modified|keyring.rs:43|u64|modified, last_seen_at|modified_at|RFC3339 UTC|hash-only||

Capability `keyring.read:263-273` only; apply/rollback refused "for security reasons" (175-177, 225-227). **No secret item fields declared at all** — plugin surfaces only collection metadata; secret values never enter the schema (correct: private-by-absence).

## persona — `Persona` (`persona.rs:31-207`), methods:468-575

|field_as_found|source|declared_type|naming_variants_seen|canonical_name_proposal|canonical_type|sensitivity|notes|
|---|---|---|---|---|---|---|---|
|agent_type|persona.rs:37,128,144,157,178|String|agent_type|agent_type|opaque_id (enum-like)|public|The persona "identity" key; code-defined catalog, not a principal|
|name|persona.rs:44,146|String|name, display_alias, label|display_name|string|public||
|description|persona.rs:51,149|String|description|description|string|public||
|category|persona.rs:58|String|category|category|enum: execution\|persona\|orchestration|public|Free string despite closed doc-set (53)|
|operations|persona.rs:66|Vec<String>|operations|operations|array<string>|public||
|operation_schemas{name,description,input_schema,output_schema}|persona.rs:81-88|String/String/Value/Value|input_schema, args|operation_schemas|array<record>|public|Unbounded JSON Values|
|status / persona_count (state)|persona.rs:101,108|String / usize|status, phase|status / persona_count|enum / u64|public||
|credentials (AuthenticateInput)|persona.rs:159|HashMap<String,String>|credentials|credentials|map<string,string>|**private — secret**|⚠️ Unbounded secret map in a method arg; dispatch always returns `authenticated:false, token:None` (318-321) so it is accepted-then-discarded dead surface|
|token (AuthenticateOutput)|persona.rs:170|Option<String>|token, oauth_token|auth_token|string|**private — secret**|Never populated (always None)|
|operation, path, query, args (ExecuteInput)|persona.rs:180-190|String/Option|operation, args|operation / args|enum / JSON|private|Free-form execution surface|
|success, data, message, metadata (ExecuteOutput)|persona.rs:197-206|bool/Value/Option|success, data|success / data|bool / JSON|private|Unbounded result payload|

Capabilities: `persona.read:475`, `persona.write:487-500`, `persona.execute:511`. Note `set_identity`/`authenticate` are declared Mutation but dispatch bails "catalog is read-only" (290) / returns false — declared-vs-live method drift.

## op-identity crate (`crates/op-identity/src/`)

**`SealedId` (SID1)** — `sealed_id.rs:59-82`: `principal_id:61 String` (grant/audit actor), `principal_kind:63 String` = `wireguard-principal` (namespace — ⚠️ conflicts with sled actor-class meaning), `session_id:64`, `wireguard_pubkey:65`, `session_genesis:67` (never a grant key), `trace_id:68`, `schema_version:69 u32`, `issued_at:70 i64`, `expires_at:72 i64` (**0 = lifelong**, sentinel vs sled's `Option`), `arrival_timestamp:73 i64`, `chain_head_at_arrival:74`, `catalog_hash_at_arrival:75`, `head_timestamp_at_arrival:76 i64`, `transport_scope:78 String` (comma-joined list — mis-normalized), `nextdns{profile_id:34 (6-char canonical), device_name:35}:81`. All `private`; sealed with SHA-256 integrity seal (129) — integrity only, not encryption. `session_projection.rs:131-147` exact-matches all claims against the sled.

**`SessionIdentity`** — `session_projection.rs:20-59`: field-for-field mirror of the sled scalar subset (session_id:22, wireguard_pubkey:24, principal_kind:35, mutation_index:37, genesis:39 alias `hashed_footprint`, trace_id:41, sealed_id:44, schema_version:46, active:48, expires_at:50, arrival_timestamp:52, chain_head_at_arrival:54, catalog_hash_at_arrival:56, head_timestamp_at_arrival:58) plus selector matching on session_id‖pubkey‖trace_id‖genesis (178-183) — one selector field accepting four different id kinds.

**`Session`** — `session.rs:72-80`: `session_id:73`, `pubkey:74` (⚠️ short name for wireguard_pubkey), `user_email:75 Option<String>` (**PII, private**), `oauth_token:76 Option<String>` (**secret, private — never expose/hash-only at most**), `token_expires_at:77 Option<DateTime<Utc>>` (⚠️ only chrono-typed timestamp in slice), `created_at:78`, `last_seen_at:79 DateTime<Utc>`. Derivations: `derive_session_id` (42-48) and `derive_principal_id` (62-68) both blake3-KDF first-16-bytes as UUID string, domain-separated contexts — so session_id and principal_id are same format, different namespaces. `session_proof` (29-31) = blake3 hex of session_id.

**`UserRecord` (IdentityVault)** — `identity_vault.rs:32-38`: `pubkey:33`, `user_email:34` (**PII, private**), `allowed_ip:35` (vs peer_ip/tunnel_inner_ip), `created_at:36`, `provisioned_at:37 DateTime<Utc>`. Vault file 0600, explicitly "no private keys, no recovery seeds, no OAuth refresh tokens" (5-7).

**`OracleIdentityAssertion` (OIA1)** — `oracle_assertion.rs:69-82`: `human_pubkey:71`, `issued_at:73 i64`, `expires_at:75 i64`, `nonce:77 [u8;16-ish]` (replay-cache key, hash-only), `tunnel_inner_ip:79 IpAddr` (⚠️ only properly IP-typed field in slice), `decoy_key_id:81 String`; `SignedAssertion.signature:125 [u8;64]` Ed25519 (**private** — signature value itself is public-proof material, treat hash-only downstream). Decoy issuer signing seed loaded from file, 32B Ed25519 (`bin/op-decoy-issuer.rs:67-79`) — **secret, server-side only**.

**Keys/recovery** — `registration.rs:14-17 WireGuardKeyPair{private_key, public_key}`: `private_key` is **SECRET — local-only, must never cross the door, never be hashed into provenance** (base64 X25519 StaticSecret); `public_key` → canonical `wireguard_pubkey`. `recovery.rs`: `seed:25 [u8;32]` / BIP39 `mnemonic` (32-44) — **SECRET, local-only 0600, server never sees it** (9-12); `derive_keypair` (54-65) blake3-KDF → X25519. `generate_magic_link_token:31` (registration.rs) — **secret token**, unbounded length param. `gcloud_auth.rs:70` returns `(oauth_token String, expires DateTime)` from env/antigravity-cache/gcloud/ADC — secret in transit only.

**Genesis** — `session_genesis.rs:41-47 mint_genesis(wg_pubkey[32], chain_head_hash[], head_timestamp i64, catalog_hash[32], arrival_timestamp i64) -> [u8;32] blake3`, hex-encoded for storage. Canonical: `session_genesis_hash: lowercase hex blake3-256`.

**No Ethereum/secp256k1 wallet field exists in this slice.** The only key types are X25519 (WireGuard), Ed25519 (decoy/OIA1 signing), and SSH agent keys (public-only). The spec's bound secp256k1 wallet (design spec:31-34, 119) has no field here yet — `wallet_address`/`secp256k1_pubkey` would be new fields, not renames.

## (a) Duplicate / synonym fields across the slice
- **WireGuard pubkey**: `wireguard_pubkey` (sled/SID1/SessionIdentity), `human_pubkey` (human_principal/OIA1), `pubkey` (Session, vault, PeerInfo), `public_key` (WireGuardKeyPair, SSH Keypair — different encoding: SSH `<algo> <b64>` vs bare base64). One canonical: `wireguard_pubkey`.
- **Session id**: `session_id` everywhere except sessdecl's renamed `id`.
- **Principal id**: `principal_id` except sessdecl's `user`.
- **Genesis**: `genesis` (sled/projection) = `session_genesis` (SID1) = alias `hashed_footprint` (legacy).
- **Display name**: `display_alias`, `label`, `name` (persona/keypair/device_name) — three fields, same role.
- **Peer/allowed IP**: `peer_ip`, `allowed_ip`, `allowed_ips: Vec`, `tunnel_inner_ip`, `endpoint` (depreciated PeerInfo).
- **Timestamps**: `registered_at`/`created_at`/`declared_at`/`arrival_timestamp`/`issued_at` (SID1 issued_at is *defined equal* to arrival_timestamp, session_projection.rs:143), `session_started_at`/`created_at`.
- **Struct duplication**: `SessionIdentity` ≈ sled scalar subset ≈ `identity_metadata_projection` output; `Keypair` ≈ `KeypairInfo` (output adds `fingerprint`/`source`); `WireGuardIdentity`/`PeerInfo` duplicated in `wireguard.rs:19,89` and deprecated `depreciated-wg.rs:8,13` (note misspelled filename).
- **Whole-record duplication risk**: sled is authoritative; SessionIdentity/metadata projection are derived mirrors that must not become second sources.

## (b) Mis-normalized fields
- `principal_kind` means two incompatible things: actor class `human|service` (sled/projection) vs identity namespace `wireguard-principal` (SID1) — explicitly warned in both files (identity_sled.rs:85-90, session_projection.rs:28-33). Rename sled field → `actor_class`.
- Timestamps: i64 unix-seconds (most), u64 unix (keyring `created/modified`), `DateTime<Utc>` (Session/UserRecord/gcloud), RFC3339 string (sessdecl `declared_at` only). Canonical target RFC3339 UTC requires conversion at the boundary everywhere except sessdecl.
- `expires_at`: `Option<i64>` (sled) vs `i64` with `0=lifelong` sentinel (SID1) — same fact, two encodings, bridged by `unwrap_or(0)` (session_projection.rs:142).
- Hashes as unbounded `String` without format pins except where schemars regex was added (`plan_sha256`, `expected_first_start_idmap_sha256` are pinned 64-hex; `genesis`, `chain_head_at_arrival`, `catalog_hash_at_arrival`, `trace_id`, `vector_id` are not). Genesis/chain-head also mix blake3 vs "hex hash" docs and 16B vs 32B chain heads (session_genesis.rs:23-26).
- Free strings where enums/ids belong: `SessionEvent.kind`, `Persona.category`, `Keypair.algorithm`, `principal_kind/actor_class`, `transport_scope` (comma-joined multi-value in one string), `native_status`/`phase`/`status`.
- `content: String` (SessionEvent:200) and `args/data/metadata: Value` (persona) — JSON smuggled as string/unbounded blobs; provenance-hostile.
- Capability naming: legacy dotted (`identity_sled.read`, `human_principal.write`, `persona.read`, `keyring.read`, `sess_decl.*`) vs versioned (`cap.identity.sled.self.read@v1`, `cap.service.keypair.list@v1`) — mixed within identity_sled itself. For tagged routing, capabilities must normalize to the `cap.*@v1` form.
- `session_id` values are UUID-formatted but are blake3-KDF outputs (not RFC-4122 versioned UUIDs) and, on the sled, also equal the container/Incus instance name (identity_sled.rs:46-47, 98-100) — one string serving as id, container name, and (via selector, session_projection.rs:178-183) interchangeable with pubkey/trace_id/genesis as a lookup key.
- Persona `credentials: HashMap<String,String>` + `token` — secret-shaped fields on a read-only catalog plugin whose dispatch never authenticates; should be removed, not normalized.
- Case/format: all Rust fields are already snake_case; anomalies are semantic (renames `id`/`user`, sentinel-vs-Option, type width), not casing. Only filename-level: `depreciated-wg.rs` (misspelling of deprecated).

## (c) Tagged-routing / provenance relevance
- **Provenance anchor chain**: `genesis`/`session_genesis` + `arrival_timestamp` + `chain_head_at_arrival` + `catalog_hash_at_arrival` + `head_timestamp_at_arrival` are the genesis-mint input set — the exact fields a provenance entry must carry (hash-only) to bind an identity to the event-chain head and schema catalog at arrival.
- **Receipt linkage**: `operation_id` + `revision_operation_id`/`expected_revision_operation_id` + `committed_revision`/`revision` (identity_sled and sessdecl outputs) are the CAS/receipt handles an index entry can point back to; `SessionEvent{seq, kind, subid, content, created_at}` is the per-session append-only ledger feeding it.
- **Routing identity**: `principal_id` is the grant/audit actor (sealed_id.rs:60); tags must never substitute — capability strings (`cap.identity.sled.*@v1` family) are the sealed grant surface, and their mixed legacy/versioned naming is the main normalization debt for the tag→control→operation map.
- **Selective disclosure boundary already exists**: `identity_metadata_projection` (identity_sled_metadata.rs:38-52) is the hash-only/public-safe subset; `sealed_id` (SID1) is the private credential that must stay server-side and never be tagged, anchored, or routed as data — clients forward it opaquely only.
- **Secrets that must remain private (never in tags, index entries, or chain data)**: WG `private_key` + BIP39 mnemonic/seed (local-only), decoy Ed25519 signing seed, `oauth_token`/gcloud tokens, magic-link tokens, persona `credentials`/`token`, keyring secret values (absent from schema by design), SSH private keys (absent by design). `user_email` is PII-private. Public/hash-only candidates: WG pubkey hashes, genesis, chain/catalog hashes, fingerprints, principal/session ids (hash-only per design boundary: identity mappings stay private).
