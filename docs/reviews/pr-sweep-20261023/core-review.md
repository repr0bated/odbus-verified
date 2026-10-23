# PR 11–21 independent source review

Task: pr-sweep-20261023. Reviewer: /root/review_core. Recorded: 2026-10-23T21:06:17.536Z.

None of these eleven PR heads is ready to merge independently as reviewed. There are real correctness/security defects, forward dependencies within the extraction stack, and code superseded only by the committed recovery branch. This report is static source review, not a build, test, deployment or full-repository approval.

Reviewed each of 248 unresolved inline threads and the supplied review submission bodies, including seven additional body-only findings. Counts are review threads, not unique defects: 124 confirmed defect; 52 fixed by later stack layer; 33 needs decision; 17 false positive; 22 fixed only in recovery/current worktree.

Compared the exact PR heads below, cumulative stack tip `d46bd4b1483b845e9d2df1afc52c5dd9dab60582`, and committed recovery head `86f03d2dede738e2d780ab789dd5800b58678479`. Unrelated dirty checkout work was not approved. Read AGENTS.md and all 28 mandatory preload files. Source inspection used git show/diff/search; no Cargo, tests, formatting, Clippy, smoke fixture or live operation was run.

## Most consequential blockers

- Schema integrity: op-blob aliases can panic during descriptor production after earlier blobs have already been written; the reduced schema hash path still does not recursively canonicalize keys. #11 is assigned to the application reviewer for repair.
- State durability: Cozo policy-query errors return allow=true; chain-vector replacement is two independent transactions; model registry writes report success after sync failure; conversation count queries use the wrong relation/parameter and quota checks precede owner serialization.
- Identity/admission: capability grants are reread without startup ownership/mode checks; direct Workstack execution lacks sealed workflows admission; compatibility magic-link verification manufactures a success response from pending local state when the authoritative backend fails. No claim is made that the latter creates a valid SID1.
- Privacy: schema projection copies undeclared argument keys and misses some nested combinator redaction; responder wake includes principal/message preview and is sent before the durable human turn commits.
- Storage: Snowball retention ignores configured buckets, transfer failure cleanup/destination tracking can pick unsent parents, independent handles are not locked across transfer lifetime, and old roots are eligible for pruning based only on absolute-path syntax.
- Runtime correctness: mail chunk parsing can panic; external MCP actor lookup aborts at a missing cache entry; complete catalog projection stops at64 entries; rate/owner registries retain terminal entries indefinitely; timeout releases workstack capacity while detached admitted work can continue.

## Dependencies and obsolete layers

The branch chain is linear: #11 → #12 → #13 → #14 → #15 → #16 → #17 → #18 → #19 → #20 → #21, followed by additional stack layers through d46bd4b. Every extraction PR description says it is not independently deployable. A later-layer fix is not a false positive on its original PR head. Keep its thread open until that dependency reaches the PR/base, or document explicit supersession when closing an obsolete extraction PR.

#11's typed alias support requires the bridge argument normalization from #18. #14–#17 require later bridge dispatch/adapters, Incus observation, native config dispatch, workflow execution and manifest changes. #20 depends on later op-chat identity/scoped rollback and spawn APIs; #21 needs the renamed proto source restored in recovery. Detailed evidence for each dependency is in the thread mapping below.

Recovery removes binary_namespace, tched_plugins, tched_skills and the old threetched_fs contract; consolidates the verified native capture; retires GetConfigSchema/embedded and per-section generated contracts. Do not resurrect these surfaces to make an older bot comment pass. Recovery-only fixes must be explicitly incorporated or the superseded PRs retired with references. Neither tip nor recovery currently clears all remaining findings.

The requested old image-based seed provision path is intentionally gated off. Do not remove valid_digest checks or the external host provisioning boundary. Keep OpenFlow's final permit deadline/cancellation check. Preserve the COGNITIVE_* rename; historical COGNITIVE_MCP_* names do not automatically deserve aliases.

Runit path conflict: source uses /etc/sv; current AGENTS prescribes /etc/runit/sv; parent reports live /etc/sv exists and /etc/runit/sv does not. These five threads are now needs-decision, superseding my initial mechanical-path-fix suggestion. Reconcile release manifest, host evidence and owner instruction before changing paths. Registered D-Bus endpoint mismatch is a separate source-confirmed defect.

## Per-PR readiness

| PR | Exact head | Verdict |
|---|---|---|
| [#11](https://github.com/repr0bated/odbus-verified/pull/11) | `951ce4270f426f9a27a3e84ea80437ecfb0cae64` | Blocked: panic/error propagation, reduced canonical JSON, feature-dependent fixture; bridge alias dependency. |
| [#12](https://github.com/repr0bated/odbus-verified/pull/12) | `c12b2974bb4413da8c78a33727a1ba3b5081c3b8` | Blocked: retention/transfer safety, audit projection, runit D-Bus endpoint; path policy conflict. |
| [#13](https://github.com/repr0bated/odbus-verified/pull/13) | `beffeac26791f896504ee7251e2e239c34b565f6` | Blocked: fail-open query, incorrect count, nonatomic replacement, sync error, unsafe migration helpers. |
| [#14](https://github.com/repr0bated/odbus-verified/pull/14) | `f0214f7ada85d8bb5f80e8375837fe46be172410` | Blocked: privacy wake, elevation provider subtree, governance inheritance, runit dispatch; forward and recovery dependencies. |
| [#15](https://github.com/repr0bated/odbus-verified/pull/15) | `ceb80a2a03fb15015a704d71631f72cb1aa5c0b2` | Blocked: chunk panic, reconciliation diff, provider pagination, policy validation; retired capture requires recovery. |
| [#16](https://github.com/repr0bated/odbus-verified/pull/16) | `75aa399a01c6a66f2a216c310bc21530486cf8f6` | Blocked: declared-model validation/version fixture; native capture/retired wrappers need recovery. |
| [#17](https://github.com/repr0bated/odbus-verified/pull/17) | `5334c0ce56413c701c9290c58097414566285d96` | Blocked: runtime dependency, undeclared input bounds, false rollback success; old FUSE contract retired. |
| [#18](https://github.com/repr0bated/odbus-verified/pull/18) | `bdebb2d5531249f4463e53facf89a2f6905534b7` | Blocked: quota ordering, proxy/Workstack admission, magic-link fallback, context loop, advertised missing method. |
| [#19](https://github.com/repr0bated/odbus-verified/pull/19) | `023ae4cfc7c3ecd24dbbd7ae79e614cecd87fba7` | Blocked: grants protection, credential transactions, catalog/wake ordering, physical binding parking and lifecycle cleanup. |
| [#20](https://github.com/repr0bated/odbus-verified/pull/20) | `74f31017c2c560d843354c0f569d521a7663708b` | Blocked: ticket timing/rate accounting, unbounded owner maps, detached timeout admission, graph consistency; op-chat dependencies. |
| [#21](https://github.com/repr0bated/odbus-verified/pull/21) | `5a34fc6e325d5d41e89e97fb1f09337fd37a07d7` | Blocked: missing renamed proto until recovery; GetConfigSchema retirement dependency; ignored child fixture guard. |

## Review-body findings

- **#11: false positive** — `review-body-11-header-assert`, `crates/op-blob/src/blob.rs:296`. seal writes a fixed 64-byte header from fixed-size literals, u16 values and a 32-byte SHA256. The review alleges only a hypothetical future edit; no current length mismatch or attacker-controlled header width exists. Suggested fix: Optional always-on invariant assertion can be a separate hardening change; do not report current corruption without a concrete mismatch.

- **#12: confirmed defect** — `review-body-12-state-key-prefix`, `crates/op-snowball/src/snowball.rs:890`. write_state_validated removes both characters of /* and then tests starts_with. plugin_bar_baz matches plugin_bar/*; retained test even encodes this behavior despite documented path-prefix semantics. Suggested fix: Strip only the final * so the separator remains; update the fixture to use plugin_bar/baz and explicitly reject sibling-prefix keys in strict mode when tests are requested.

- **#13: confirmed defect** — `review-body-13-test-env-safety`, `crates/op-cozo-store/src/lib.rs:2850`. human_principal_db_path_env_override mutates the process environment using unsafe set_var/remove_var with an unaudited FIXME; ordinary Rust tests may execute on multiple threads. The function also assumes the variable was initially unset and removes any prior value. Suggested fix: Test a pure environment-value resolver, or isolate environment mutation in a dedicated subprocess before threads start. A mutex among only selected tests does not establish process-wide environment safety.

- **#18: confirmed defect** — `review-body-18-magic-link-fallback`, `crates/op-grpc-bridge/src/grpc_server.rs:5666`. compatibility_registration_interceptor at621–629 explicitly admits SendMagicLink and VerifyMagicLink. On D-Bus error verify_magic_link retrieves the raw stored token and returns success/new random user_id at5688–5697 without expiry, consumption, domain comparison or authoritative identity issuance. Same branch remains in recovery. This establishes a broken verification response, not proof of a valid minted SID1. Suggested fix: Fail closed when the authoritative registration owner is unavailable; do not treat pending local state as verification. If compatibility registration is retained, authoritative token consumption must be atomic, expiring, domain-bound and tied to the owner-issued identity.

- **#19: confirmed defect** — `BUG_pr-review-job-970818bc14d54f6195cd4be328c4a886_0002`, `crates/op-grpc-bridge/src/mutation_engine.rs:559`. enrich_json_render_catalog calls list_ui_catalog(None,64,false) once and publishes its entries with current revision without following continuation. A catalog larger than64 is silently truncated. Same at cumulative tip and recovery. Suggested fix: Add a store operation returning bounded active names at a single consistent revision, or exhaust pages under a revision-consistency check before committing the projection. Do not silently publish a partial page as complete.

- **#19: confirmed defect** — `BUG_pr-review-job-970818bc14d54f6195cd4be328c4a886_0003`, `crates/op-grpc-bridge/src/mutation_engine.rs:1633`. open_chat_turn reserves the responder turn, spawns MQTT wake at1633–1641, then awaits commit_human_turn at1643. Commit/index failure cancels the turn only after wake can escape. Same at cumulative tip and recovery. Suggested fix: Move wake construction/spawn after successful durable human commit and turn-index verification; wake should carry bounded opaque references, with responder reading canonical state.

- **#19: needs decision** — `SEC_pr-review-job-970818bc14d54f6195cd4be328c4a886_0001`, `crates/op-grpc-bridge/src/mutation_engine.rs:178`. audit_method_arguments intentionally digests x-opdbus-private content while x-secret is removed; authorize_task is explicitly redacted before either path to avoid password guessing. Unsalted private digests do permit guesses, but the body supplies no concrete accepted credential schema incorrectly marked only private. Even the whole-request fallback retains request_hash for most private plugins. Suggested fix: Audit actual credential annotations and low-entropy private data requirements. Mark credentials x-secret and suppress derived hashes where confidentiality requires it. Do not replace the owner-defined private-digest protocol wholesale or claim a demonstrated password leak from this hypothetical alone.

## Explicitly declined or unresolved judgments

- No builds, tests, fmt, Clippy, smoke fixtures or live runtime operations were run. Static expected failures are not executed test results.
- No approval of unrelated dirty working-tree changes, deployment state, active catalog contents, identity storage layout or live FUSE namespace; recovery comparison is committed 86f03d2 only.
- Hidden review flags not included by GitHub settings are unavailable: reported counts include #11 2, #12 2, #13 2, #14 2, #16 2, #17 2, #18 3, #19 4, #20 2, #21 3. They were not silently dismissed.
- PAM callback error ownership and pam_start cleanup allegations need authoritative libpam implementation evidence; no demonstrated leak/invalid free is claimed.
- Btrfs seed/sprout fs_info.num_devices semantics need authoritative kernel/version evidence before changing storage safety checks.
- CORS identity exposure needs exact credential/cross-origin request reachability evidence; do not weaken identity metadata or claim a token leak solely from expose_headers.
- Policy decisions remain for retired COGNITIVE_MCP environment compatibility, legacy config-field deserialization, egress group union/intersection, independent native CLI executor authority, MD5 chain format migration and direct Workstack workflow admission.
- Owner-required /etc/runit/sv versus parent-reported live /etc/sv conflict requires reconciliation; no host path was changed.

Needs-decision entries below are retained as unresolved findings. They are not approvals. Review-settings-hidden flags cannot be assessed from the supplied data. No external reviewer claim of passing tests was adopted.

## Thread-by-thread evidence

### PR #11

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpmYP`, `crates/op-blob/src/descriptor.rs:252`. descriptor.rs build_message uses expect on validate_proto_field_aliases; blobify_canonical remains infallible and enters descriptor synthesis without prevalidation. All three revisions retain it. Suggested action: Validate every candidate method schema before any catalog writes and propagate fallible producer errors; avoid a partial reseal.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpn1m`, `crates/op-blob/tests/canonical_schema.rs:94`. canonical_schema.rs:94 asserts serialized fixtures differ; op-blob Cargo.toml does not enable preserve_order, so default BTreeMap fixtures serialize identically. Static contradiction, not an executed test. Suggested action: Make the fixture feature-independent and exercise insertion ordering explicitly.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tporD`, `crates/op-blob/src/descriptor.rs:252`. descriptor.rs build_message uses expect on validate_proto_field_aliases; blobify_canonical remains infallible and enters descriptor synthesis without prevalidation. All three revisions retain it. Suggested action: Validate every candidate method schema before any catalog writes and propagate fallible producer errors; avoid a partial reseal.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpori`, `crates/op-blob/src/descriptor.rs:273`. Later bridge layer adds normalize_generated_arguments/normalize_message_keys in grpc_server.rs:198–268 and build-time authored json_name handling; typed proto names are restored before schema validation. Suggested action: Integrate #18 with #11; retain the normalization and authored names.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tporv`, `crates/op-blob/src/lib.rs:92`. schema.rs:118–122 still serializes to_value directly, while full-schema lib.rs path canonicalizes keys. Reduced-schema hashes depend on unified preserve_order feature. Suggested action: Reuse canonicalize_json_object_keys in PluginSchema::canonical_json.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpo3R`, `crates/op-blob/src/descriptor.rs:272`. Later bridge layer adds normalize_generated_arguments/normalize_message_keys in grpc_server.rs:198–268 and build-time authored json_name handling; typed proto names are restored before schema validation. Suggested action: Integrate #18 with #11; retain the normalization and authored names.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpo3T`, `crates/op-blob/tests/canonical_schema.rs:94`. canonical_schema.rs:94 asserts serialized fixtures differ; op-blob Cargo.toml does not enable preserve_order, so default BTreeMap fixtures serialize identically. Static contradiction, not an executed test. Suggested action: Make the fixture feature-independent and exercise insertion ordering explicitly.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpo3V`, `crates/op-blob/src/descriptor.rs:252`. descriptor.rs build_message uses expect on validate_proto_field_aliases; blobify_canonical remains infallible and enters descriptor synthesis without prevalidation. All three revisions retain it. Suggested action: Validate every candidate method schema before any catalog writes and propagate fallible producer errors; avoid a partial reseal.

- **needs decision** — `PRRT_kwDOUgxzzc6tpo3Y`, `crates/op-blob/src/descriptor.rs:566`. The comment invokes an obsolete mandatory /verify policy. Current owner explicitly makes fmt and checks on-request only; no formatting check was run. Suggested action: Do not run or claim fmt for this sweep without owner request; record formatting as unverified.

### PR #12

- **needs decision** — `PRRT_kwDOUgxzzc6tpqZB`, `crates/op-state-store/src/event_chain.rs:724`. event_chain.rs:1101–1113 uses MD5; this is pre-existing audit format, not replaced by the transport-attribution diff. Hash migration changes historical verification. Suggested action: Prepare a versioned hash migration preserving legacy verification; do not silently rehash existing events.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpqZE`, `crates/op-snowball/src/snowball.rs:89`. snowball.rs:86–95 ignores policy and now and retains one counter; the declared retention API exposes configurable bucket counts. Suggested action: Restore configured retention buckets while pinning transfer source and parent.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpqZH`, `crates/op-snowball/src/snowball.rs:3199`. snowball.rs:86–95 ignores policy and now and retains one counter; the declared retention API exposes configurable bucket counts. Suggested action: Restore configured retention buckets while pinning transfer source and parent.

- **false positive** — `PRRT_kwDOUgxzzc6tpqZL`, `crates/op-cache/src/btrfs_cache.rs:1562`. btrfs_cache.rs:1555–1558 uses create_new(true). O_CREAT|O_EXCL atomically refuses an existing final symlink; the asserted final-component symlink overwrite is not possible. Suggested action: No fix needed for the alleged symlink attack.

- **false positive** — `PRRT_kwDOUgxzzc6tpqZN`, `crates/op-cache/src/btrfs_cache.rs:984`. decode_embedding:957–982 reads the bincode Vec length prefix, then deserializes the whole buffer with with_fixint_encoding. It is not an extra custom header. Suggested action: Retain the bounded matching decoder.

- **needs decision** — `PRRT_kwDOUgxzzc6tprS4`, `crates/op-core/src/runit.rs:24`. Source sets SV_DIR=/etc/sv and ServiceDef imports it. Current AGENTS explicitly prescribes /etc/runit/sv. Parent reports separate app reviewer observed /etc/sv present and /etc/runit/sv absent on host, contradicting written policy; this reviewer did not inspect live deployment. Suggested action: Reconcile declared deployment manifest, actual runit definition root and owner policy before changing paths. Do not mechanically replace /etc/sv or assume current host layout proves intended release layout.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprTR`, `crates/op-state/src/authority.rs:16`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprTs`, `crates/op-snowball/src/snowball.rs:94`. snowball.rs:86–95 ignores policy and now and retains one counter; the declared retention API exposes configurable bucket counts. Suggested action: Restore configured retention buckets while pinning transfer source and parent.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprUF`, `crates/op-snowball/src/snowball.rs:788`. snowball.rs:759 selects latest prepared local dump; remote_mkdir at788 returns early on error without discarding or marking failed local preparation. Suggested action: Persist sent-parent state per destination; only confirmed received snapshots qualify as parents; reconcile failed preparation.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprUv`, `crates/op-snowball/src/snowball.rs:834`. snapshot flock exists only in preparation closure:752–783; per-instance snapshot_lock is not shared across independent handles; remove_older_local_dumps:831 can prune another active transfer. Suggested action: Serialize dump lifetime per root across handles/processes, or persist explicit in-flight pins.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprVP`, `crates/op-snowball/src/snowball.rs:2078`. remote_ssh_command:2075 waits child before draining piped stderr:2077. Suggested action: Drain stderr concurrently with wait and bound diagnostics.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprVj`, `crates/op-snowball/src/snowball.rs:2166`. received_root_eligible_for_prune:2178 checks only absolute syntax; saved arbitrary roots flow to remote_delete_chain_root:2166. Suggested action: Bind records to destination host, parent, expected dump naming and verified receive identity before deletion.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprVt`, `crates/op-state-store/src/schema_projection.rs:221`. schema_projection.rs:216–220 copies undeclared arguments verbatim into audit output. Suggested action: Fail closed or redact/digest undeclared fields at this audit boundary; do not copy unknown private content.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptFv`, `crates/op-snowball/src/snowball.rs:89`. snowball.rs:86–95 ignores policy and now and retains one counter; the declared retention API exposes configurable bucket counts. Suggested action: Restore configured retention buckets while pinning transfer source and parent.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptFy`, `crates/op-state/src/authority.rs:16`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptFz`, `crates/op-state-store/src/schema_projection.rs:206`. digest_walk:202–206 checks annotations only on resolved node, while child property allOf annotations are not traversed conservatively before scalar copy. Suggested action: Resolve secrecy/privacy over all applicable combinator branches before serializing.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptF2`, `crates/op-snowball/src/snowball.rs:805`. snowball.rs:803–805/822–824 deletes local failed dump only; remote mkdir uses plain mkdir and counter derives from surviving local directories. Suggested action: Use durable unique attempts and reconcile known partial remote targets without deleting uncertain unrelated data.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptF3`, `crates/op-snowball/src/snowball.rs:2148`. received-roots contains only paths; current host is applied to old entries. Parent selection at759 is also destination-independent. Suggested action: Store destination identity and confirmed parent/roots per host and filesystem; full send to a new destination.

- **needs decision** — `PRRT_kwDOUgxzzc6tptF4`, `crates/op-core/src/runit.rs:24`. Source sets SV_DIR=/etc/sv and ServiceDef imports it. Current AGENTS explicitly prescribes /etc/runit/sv. Parent reports separate app reviewer observed /etc/sv present and /etc/runit/sv absent on host, contradicting written policy; this reviewer did not inspect live deployment. Suggested action: Reconcile declared deployment manifest, actual runit definition root and owner policy before changing paths. Do not mechanically replace /etc/sv or assume current host layout proves intended release layout.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptF7`, `crates/op-snowball/src/snowball.rs:747`. operation_guard acquired747 and dropped845 spans both network transfers and retention; same snapshot_lock protects writers. Suggested action: Release writer admission after immutable aligned snapshots; retain a separate dump transfer lock/pins.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptF8`, `crates/op-snowball/src/snowball.rs:2225`. remote_delete_chain_root:2225 sets received snapshot ro false before delete, unnecessarily requiring destructive received_uuid transition. Suggested action: Delete the read-only received subvolume directly after ownership checks.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptF9`, `crates/op-core/src/projection_transactions.rs:231`. projection_transactions.rs:229–232 calls try_reserve_exact for each8192-byte append. Suggested action: Use bounded preallocation from validated length or amortized try_reserve.

- **needs decision** — `PRRT_kwDOUgxzzc6tpu3a`, `crates/op-core/src/runit.rs:24`. Source sets SV_DIR=/etc/sv and ServiceDef imports it. Current AGENTS explicitly prescribes /etc/runit/sv. Parent reports separate app reviewer observed /etc/sv present and /etc/runit/sv absent on host, contradicting written policy; this reviewer did not inspect live deployment. Suggested action: Reconcile declared deployment manifest, actual runit definition root and owner policy before changing paths. Do not mechanically replace /etc/sv or assume current host layout proves intended release layout.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu3h`, `crates/op-state/src/authority.rs:16`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu3l`, `crates/op-snowball/src/snowball.rs:788`. snowball.rs:759 selects latest prepared local dump; remote_mkdir at788 returns early on error without discarding or marking failed local preparation. Suggested action: Persist sent-parent state per destination; only confirmed received snapshots qualify as parents; reconcile failed preparation.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu3p`, `crates/op-snowball/src/snowball.rs:805`. snowball.rs:803–805/822–824 deletes local failed dump only; remote mkdir uses plain mkdir and counter derives from surviving local directories. Suggested action: Use durable unique attempts and reconcile known partial remote targets without deleting uncertain unrelated data.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu3s`, `crates/op-snowball/src/snowball.rs:759`. received-roots contains only paths; current host is applied to old entries. Parent selection at759 is also destination-independent. Suggested action: Store destination identity and confirmed parent/roots per host and filesystem; full send to a new destination.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu3w`, `crates/op-snowball/src/snowball.rs:831`. snapshot flock exists only in preparation closure:752–783; per-instance snapshot_lock is not shared across independent handles; remove_older_local_dumps:831 can prune another active transfer. Suggested action: Serialize dump lifetime per root across handles/processes, or persist explicit in-flight pins.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu3z`, `crates/op-cache/src/btrfs_cache.rs:1196`. btrfs_cache async get_raw/put_raw:1185–1198 synchronously flock, read/write and sync on executor thread. Suggested action: Bound admission first, run the complete filesystem transaction on blocking owner.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu34`, `crates/op-cache/src/numa.rs:1284`. numa test:1284 unconditionally calls read_memory_policy after checking only child seccomp fixture availability. Suggested action: Treat unavailable get_mempolicy as unsupported fixture; do not report native policy verified.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpu38`, `crates/op-snowball/src/snowball.rs:2077`. remote_ssh_command:2075 waits child before draining piped stderr:2077. Suggested action: Drain stderr concurrently with wait and bound diagnostics.

### PR #13

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppao`, `crates/op-cozo-store/src/bin/get-migration-preflight.rs:22`. get-migration-preflight.rs hardcodes session22, reports mutation_index as revision32/42 and suggests fabricated hashes45. Suggested action: Remove this misleading ad-hoc helper or make it call supported authenticated preflight; do not generalize a direct identity-store bypass.

- **needs decision** — `PRRT_kwDOUgxzzc6tppas`, `crates/op-cozo-store/src/external_mcp_sessions.rs:210`. external_mcp_sessions.rs:183–209 has an insert/revocation race, but line187 documents post-persist ingress revocation recheck; this alone does not prove revoked runtime admission. Suggested action: Make persistence transaction atomic if rejecting all post-revocation rows is required; separately inspect ingress recheck before claiming authorization bypass.

- **false positive** — `PRRT_kwDOUgxzzc6tppau`, `crates/op-cozo-store/src/external_mcp_sessions.rs:36`. canonical_session_id hashes namespaced bindings and explicitly sets version8 and RFC variant; no v4-only caller is identified. Suggested action: No UUID bit change.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppaw`, `crates/op-cozo-store/src/lib.rs:770`. Cozo evaluate_mutation:770–773 returns allow:true on query error. Suggested action: Return deny/error on failed compliance evaluation.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppax`, `crates/op-cozo-store/src/bin/get-migration-preflight.rs:45`. get-migration-preflight.rs hardcodes session22, reports mutation_index as revision32/42 and suggests fabricated hashes45. Suggested action: Remove this misleading ad-hoc helper or make it call supported authenticated preflight; do not generalize a direct identity-store bypass.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppaz`, `crates/op-cozo-store/src/lib.rs:931`. replace_chain_vector_links:904–932 uses two independent cozo_run transactions. Suggested action: Use one checked transaction and serialize replacements so failed inserts cannot erase prior links or mix batches.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprAQ`, `crates/op-cozo-store/src/chat_response_conversations.rs:805`. count_principal_conversations:805 uses undefined chat_conversation, parameter principal_id instead of principal and invalid count expression. Suggested action: Query *chat_response_conversations with ?[count(conversation_id)] and $principal.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprB6`, `crates/op-cozo-store/src/lib.rs:906`. replace_chain_vector_links:904–932 uses two independent cozo_run transactions. Suggested action: Use one checked transaction and serialize replacements so failed inserts cannot erase prior links or mix batches.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprCi`, `crates/op-cozo-store/src/model_registry.rs:420`. model_registry_write:417–421 logs sync_runtime_document_store error and still returns successful result. Suggested action: Propagate an explicit postcommit durability-unknown error; do not claim rollback or retry-safe failure.

- **needs decision** — `PRRT_kwDOUgxzzc6tprC9`, `crates/op-cozo-store/src/lib.rs:287`. Current owner explicitly renamed cognitive configuration to COGNITIVE_*. These source defaults intentionally omit historical COGNITIVE_MCP_* aliases; source-only backward-compatibility concern does not authorize restoring retired configuration. Suggested action: Check release migration declarations; reject stale configuration explicitly if needed, without resurrecting old routing defaults.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprDT`, `crates/op-cozo-store/src/bin/inventory/mod.rs:1479`. inventory collect_consumers:1472–1479 records depends_on/starts but does not push dependent service onto traversal queue. Suggested action: Traverse dependency services with visited-cycle guard and include their file/blocker closure.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprED`, `crates/op-cozo-store/src/model_registry.rs:457`. record_model_registry_refresh:453–456 rejects successful empty provider catalog, preserving prior available models. Suggested action: Treat successful empty complete inventory as removal/unavailability; keep failures distinct from empty success.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprEY`, `crates/op-cozo-store/src/bin/validate-chatbot-state.rs:48`. validate-chatbot-state checks contains on serialized binding, including /dev/sdb2 and fixed FSID, without matching typed fields. Suggested action: Remove stale migration helper or verify typed expected fields through supported owner.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprE2`, `crates/op-cozo-store/src/lib.rs:769`. Cozo evaluate_mutation:770–773 returns allow:true on query error. Suggested action: Return deny/error on failed compliance evaluation.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprFN`, `crates/op-cozo-store/src/bin/update-chatbot-fsid.rs:57`. update-chatbot-fsid mutates btrfs_device_json and calls put_identity_sled:57, bypassing migration provenance or failing for versioned rows. Suggested action: Retire helper; use authenticated migration with revision, native proof and durable receipt.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprFo`, `crates/op-cozo-store/src/bin/get-migration-preflight.rs:45`. get-migration-preflight.rs hardcodes session22, reports mutation_index as revision32/42 and suggests fabricated hashes45. Suggested action: Remove this misleading ad-hoc helper or make it call supported authenticated preflight; do not generalize a direct identity-store bypass.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprGC`, `crates/op-cozo-store/src/bin/list-genesis.rs:18`. Identity helper binaries call new_persistent, not private identity store constructor; get-migration-preflight:20 and validate-chatbot-state:21 confirm. Suggested action: Use supported identity-bound inspection instead; if maintenance helper retained, enforce private store ownership/mode.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAW`, `crates/op-cozo-store/src/bin/get-migration-preflight.rs:32`. get-migration-preflight.rs hardcodes session22, reports mutation_index as revision32/42 and suggests fabricated hashes45. Suggested action: Remove this misleading ad-hoc helper or make it call supported authenticated preflight; do not generalize a direct identity-store bypass.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAd`, `crates/op-cozo-store/src/bin/update-chatbot-fsid.rs:57`. update-chatbot-fsid mutates btrfs_device_json and calls put_identity_sled:57, bypassing migration provenance or failing for versioned rows. Suggested action: Retire helper; use authenticated migration with revision, native proof and durable receipt.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAi`, `crates/op-cozo-store/src/chat_response_conversations.rs:805`. count_principal_conversations:805 uses undefined chat_conversation, parameter principal_id instead of principal and invalid count expression. Suggested action: Query *chat_response_conversations with ?[count(conversation_id)] and $principal.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAl`, `crates/op-cozo-store/src/lib.rs:907`. replace_chain_vector_links:904–932 uses two independent cozo_run transactions. Suggested action: Use one checked transaction and serialize replacements so failed inserts cannot erase prior links or mix batches.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAp`, `crates/op-cozo-store/src/workflow_executions.rs:133`. workflow_executions:133 sorts RFC3339 strings while validation accepts offsets. Suggested action: Normalize accepted_at to UTC or epoch at insertion.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAq`, `crates/op-cozo-store/src/ui_gallery.rs:575`. ui_gallery:568–577 loads limit+1 full documents, each up to2MiB, then deserializes before truncate. Suggested action: Bound aggregate bytes in retrieval and decoding; expose continuation.

- **needs decision** — `PRRT_kwDOUgxzzc6tpsAs`, `crates/op-cozo-store/src/lib.rs:290`. Current owner explicitly renamed cognitive configuration to COGNITIVE_*. These source defaults intentionally omit historical COGNITIVE_MCP_* aliases; source-only backward-compatibility concern does not authorize restoring retired configuration. Suggested action: Check release migration declarations; reject stale configuration explicitly if needed, without resurrecting old routing defaults.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAu`, `crates/op-cozo-store/src/identity_transactions.rs:1718`. anchored_genesis_compatible:1715–1720 omits schema_content_hash entirely rather than checking compiled replacement hash. Suggested action: Require exact approved compiled hash via dedicated refresh intent; preserve other immutable genesis fields.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsAw`, `crates/op-cozo-store/src/workflow_executions.rs:186`. workflow CAS:177–205 compares only revision/owner_epoch then accepts changed workflow_id and accepted_at. Suggested action: Make workflow identity and original acceptance timestamp immutable across updates.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpxzG`, `crates/op-cozo-store/src/chat_response_conversations.rs:805`. count_principal_conversations:805 uses undefined chat_conversation, parameter principal_id instead of principal and invalid count expression. Suggested action: Query *chat_response_conversations with ?[count(conversation_id)] and $principal.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpxzH`, `crates/op-cozo-store/src/model_registry.rs:421`. model_registry_write:417–421 logs sync_runtime_document_store error and still returns successful result. Suggested action: Propagate an explicit postcommit durability-unknown error; do not claim rollback or retry-safe failure.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpxzJ`, `crates/op-cozo-store/src/lib.rs:907`. replace_chain_vector_links:904–932 uses two independent cozo_run transactions. Suggested action: Use one checked transaction and serialize replacements so failed inserts cannot erase prior links or mix batches.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpxzL`, `crates/op-cozo-store/src/bin/inventory/mod.rs:1633`. inventory:1633 performs single ordered scan; helper argument bindings can be discovered after a helper was scanned. Suggested action: Collect argument bindings first or iterate bounded fixed-point before unresolved-path verdict.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpxzM`, `crates/op-cozo-store/src/ui_gallery.rs:577`. ui_gallery:568–577 loads limit+1 full documents, each up to2MiB, then deserializes before truncate. Suggested action: Bound aggregate bytes in retrieval and decoding; expose continuation.

### PR #14

- **false positive** — `PRRT_kwDOUgxzzc6tpptZ`, `crates/op-plugins/src/state_plugins/elevation.rs:58`. tched_router is canonical registered plugin ID, confirmed in tched_router.rs PLUGIN_NAME and MutationEngine route. Hypothetical typo is absent. Suggested action: No name change.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpptb`, `crates/op-plugins/src/state_plugins/emqx.rs:798`. emqx publish_responder_wake:771–779 sends owner principal and message preview verbatim without input bounds; content-free wake policy elsewhere sends seq only. Suggested action: Publish a bounded opaque notification and retrieve content through designated responder admission.

- **false positive** — `PRRT_kwDOUgxzzc6tpptd`, `crates/op-plugins/src/state_plugins/emqx.rs:1255`. Missing method declaration cannot bypass MutationEngine sealed admission: undeclared methods fail closed. Internal helper call is separate and needs deliberate policy. Suggested action: Do not add a public capability merely to satisfy this claimed bypass; reconcile internal notification owner separately.

- **false positive** — `PRRT_kwDOUgxzzc6tpptf`, `crates/op-plugins/src/state_plugins/emqx.rs:89`. runit_lifecycle is an internal helper called with fixed known action literals; debug_assert omission alone supplies no untrusted action path. Suggested action: Optional enum hardening, not a demonstrated defect.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpptg`, `crates/op-plugins/src/state_plugins/ghostbridge.rs:3`. ghostbridge_schema:316–318 copies current_state environment into defaults/example; schema identity is environment-dependent and includes deployment identity metadata. Suggested action: Use synthetic deterministic schema defaults; observe runtime values only in state.

- **needs decision** — `PRRT_kwDOUgxzzc6tpptj`, `crates/op-plugins/src/state_plugins/identity_sled.rs:248`. arrival_timestamp zero is deliberate unanchored representation; comment supplies no admitted path that can overwrite anchored records, whose Cozo SID1/genesis checks are separate. Suggested action: Check creation validation and fixtures if zero-time production support is intended; do not weaken anchoring.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprL7`, `crates/op-plugins/src/state_plugins/emqx.rs:91`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprMW`, `crates/op-plugins/src/state_plugins/cozo.rs:499`. MutationEngine later layer handles cozo refresh_system_graph specially at4248 before generic cozo dispatcher. Suggested action: Merge dependency through #19; do not duplicate handler in plugin.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprMq`, `crates/op-plugins/src/state_plugins/chat_response.rs:595`. chat_response_dispatch now has list_pending_turns arm:422 using responder principal. Suggested action: Integrate bridge layer #18.

- **needs decision** — `PRRT_kwDOUgxzzc6tprNV`, `crates/op-plugins/src/service_def.rs:11`. Source sets SV_DIR=/etc/sv and ServiceDef imports it. Current AGENTS explicitly prescribes /etc/runit/sv. Parent reports separate app reviewer observed /etc/sv present and /etc/runit/sv absent on host, contradicting written policy; this reviewer did not inspect live deployment. Suggested action: Reconcile declared deployment manifest, actual runit definition root and owner policy before changing paths. Do not mechanically replace /etc/sv or assume current host layout proves intended release layout.

- **needs decision** — `PRRT_kwDOUgxzzc6tprNy`, `crates/op-plugins/src/state_plugins/embedding_model.rs:185`. Current owner explicitly renamed cognitive configuration to COGNITIVE_*. These source defaults intentionally omit historical COGNITIVE_MCP_* aliases; source-only backward-compatibility concern does not authorize restoring retired configuration. Suggested action: Check release migration declarations; reject stale configuration explicitly if needed, without resurrecting old routing defaults.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprOa`, `crates/op-plugins/src/state_plugins/elevation.rs:113`. elevation path matching:111 protects providers and providers.models descendants but excludes other providers descendants. Suggested action: Match all protected provider subtrees according to declared elevation policy.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprO6`, `crates/op-plugins/src/state_plugins/emqx.rs:779`. emqx publish_responder_wake:771–779 sends owner principal and message preview verbatim without input bounds; content-free wake policy elsewhere sends seq only. Suggested action: Publish a bounded opaque notification and retrieve content through designated responder admission.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprPl`, `crates/op-plugins/src/state_plugins/binary_namespace.rs:310`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps4_`, `crates/op-plugins/src/state_plugins/cognitive.rs:26`. Later bridge/cognitive layers use cognitive ID/accessor; recovery restores renamed op-cognitive crate source after stack deletion. Standalone lower layer remains incompatible. Suggested action: Preserve cognitive rename and integrate complete callers; recovery source restoration is required for build.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tps5B`, `crates/op-plugins/src/state_plugins/emqx.rs:91`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps5C`, `crates/op-plugins/src/state_plugins/identity_sled.rs:526`. identity_sled_dispatch now routes own/inventory/delete/preflight/migration methods in verified-session path around2355–2514. Suggested action: Integrate identity bridge layer #19.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps5E`, `crates/op-plugins/src/state_plugins/elevation.rs:363`. state_plugins/mod links elevation and MutationEngine owns TaskElevation, dispatches at3810 and consumes protected tickets after admission. Suggested action: Integrate plugin module and bridge layers; preserve exact capability checks.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps5H`, `crates/op-plugins/src/state_plugins/cozo.rs:498`. MutationEngine later layer handles cozo refresh_system_graph specially at4248 before generic cozo dispatcher. Suggested action: Merge dependency through #19; do not duplicate handler in plugin.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps5M`, `crates/op-plugins/src/state_plugins/chat_response.rs:483`. chat_response_dispatch now has list_pending_turns arm:422 using responder principal. Suggested action: Integrate bridge layer #18.

- **needs decision** — `PRRT_kwDOUgxzzc6tps5N`, `crates/op-plugins/src/state_plugins/emqx.rs:1257`. emqx schema still omits publish_responder_wake while bridge calls internal dispatch directly. Adding a public mutation is not necessarily appropriate; notification ownership needs resolution. Suggested action: Use the existing internal authenticated notification owner or declare a narrowly granted canonical method; do not weaken admission.

- **false positive** — `PRRT_kwDOUgxzzc6tps5P`, `crates/op-plugins/src/bin/gen_tched_router_config_surface.rs:4`. Current owner explicitly retired embedded contract import and per-section generated surface; shared verified native capture on seal is required. Suggested action: Do not restore deleted importer or87 generated getters. Update stale consumers/docs only.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps5S`, `crates/op-plugins/src/default_registry.rs:37`. mongodb_mcp is explicitly in retired plugin IDs; later deployment/source layers remove stale module/provider references. Restoring it would contradict intended retirement. Suggested action: Keep retirement and remove stale callers/toolset/release declarations together.

- **needs decision** — `PRRT_kwDOUgxzzc6tps5U`, `crates/op-plugins/src/service_def.rs:12`. Source sets SV_DIR=/etc/sv and ServiceDef imports it. Current AGENTS explicitly prescribes /etc/runit/sv. Parent reports separate app reviewer observed /etc/sv present and /etc/runit/sv absent on host, contradicting written policy; this reviewer did not inspect live deployment. Suggested action: Reconcile declared deployment manifest, actual runit definition root and owner policy before changing paths. Do not mechanically replace /etc/sv or assume current host layout proves intended release layout.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tps5V`, `crates/op-plugins/src/state_plugins/incus_device.rs:320`. Later incus_observer now populates source_identity, but Device::to_incus_map:599–608 serializes every field, including observer-only source_identity. Suggested action: Exclude source_identity from native device writes; retain observer computation under approved owner.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tps5X`, `crates/op-plugins/src/state_plugins/governance_taxonomy.rs:136`. governance_taxonomy default user role parent_role=admin at136, capability/data-scope traversal unions parent grants. Suggested action: Remove admin inheritance for read-only user and enumerate intended grants.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps5Y`, `crates/op-plugins/src/state_plugins/identity_sled.rs:416`. Later identity_seed_provision sends valid plans into clean_provision which supplies plan/operation/revision/replayed receipt. The old return belongs to retired path blocked before effects. Suggested action: Integrate bridge clean-provision owner; do not restore legacy provisioning.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps9d`, `crates/op-plugins/src/lib.rs:70`. Later bridge/cognitive layers use cognitive ID/accessor; recovery restores renamed op-cognitive crate source after stack deletion. Standalone lower layer remains incompatible. Suggested action: Preserve cognitive rename and integrate complete callers; recovery source restoration is required for build.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps9g`, `crates/op-plugins/src/state_plugins/emqx.rs:43`. mqtt_doorbell_relay later uses EMQX_MQTT_WSS_HOST/PORT instead of removed FABRIC_SVC0_IP. Suggested action: Integrate #19.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps9i`, `crates/op-plugins/src/state_plugins/cozo.rs:498`. MutationEngine later layer handles cozo refresh_system_graph specially at4248 before generic cozo dispatcher. Suggested action: Merge dependency through #19; do not duplicate handler in plugin.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps9k`, `crates/op-plugins/src/state_plugins/chat_response.rs:483`. chat_response_dispatch now has list_pending_turns arm:422 using responder principal. Suggested action: Integrate bridge layer #18.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps9m`, `crates/op-plugins/src/state_plugins/identity_sled.rs:601`. identity_sled_dispatch now routes own/inventory/delete/preflight/migration methods in verified-session path around2355–2514. Suggested action: Integrate identity bridge layer #19.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tps9s`, `crates/op-plugins/src/state_plugins/identity_sled.rs:581`. Later identity_seed_provision sends valid plans into clean_provision which supplies plan/operation/revision/replayed receipt. The old return belongs to retired path blocked before effects. Suggested action: Integrate bridge clean-provision owner; do not restore legacy provisioning.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tps9v`, `crates/op-plugins/src/state_plugins/binary_namespace.rs:199`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tps9x`, `crates/op-plugins/src/state_plugins/binary_namespace.rs:316`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tps90`, `crates/op-plugins/src/state_plugins/binary_namespace.rs:310`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

### PR #15

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppuK`, `crates/op-plugins/src/state_plugins/mail_server.rs:554`. mail_server.rs:168 slices body[pos..pos+size] without checked addition and remaining length validation. Suggested action: Reject truncated/overflowing chunk lengths before slicing; require CRLF framing.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tppuO`, `crates/op-plugins/src/state_plugins/tched_plugins.rs:55`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppuP`, `crates/op-plugins/src/state_plugins/rtnetlink.rs:80`. rtnetlink calculate_diff:116 marks any desired gateway Some as changed. Suggested action: Compare desired gateway with observed current gateway.

- **false positive** — `PRRT_kwDOUgxzzc6tppuS`, `crates/op-plugins/src/state_plugins/json_render_methods.rs:57`. 15+gallery::METHODS.len explicitly tracks gallery additions; adding a gallery method increases both sides. No current wrong count is demonstrated. Suggested action: Optional named baseline constant only; no required functional change.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprP1`, `crates/op-plugins/src/state_plugins/service/methods.rs:10`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprQV`, `crates/op-plugins/src/state_plugins/json_render.rs:864`. MutationEngine now dispatches gallery, registry, routing, schema registry, Snowball and assessment methods; local JsonRenderMethods is not their final owner. Suggested action: Integrate all bridge layers; do not duplicate ownership.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprQ2`, `crates/op-plugins/src/state_plugins/snowball_plugin.rs:486`. snowball schema declares2.1.0 while StatePlugin version remains2.0.0. Suggested action: Use one shared version constant.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprRS`, `crates/op-plugins/src/state_plugins/qdrant.rs:449`. QdrantCollectionParams requires flat QdrantVectorParams; current_state:447 decodes each collection and propagates failure. Suggested action: Model single/named vectors union and preserve complete state publication.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprR4`, `crates/op-plugins/src/state_plugins/model_registry.rs:578`. parse_models_response reads document.data only; model registry refresh has no cursor pagination for has_more response. Suggested action: Fetch all bounded pages before marking inventory complete; otherwise fail as incomplete.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprSS`, `crates/op-plugins/src/state_plugins/schema_tag_registry_methods.rs:819`. apply_policy_update:817–819 installs resolved mappings without PolicyRecord validation used by author_policy. Suggested action: Validate candidate record before replacement.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprSv`, `crates/op-plugins/src/state_plugins/oscal_field_classification.rs:1197`. protection_losses:1195 checks traversal_complete but not boundary_coverage_complete. Suggested action: Require both completeness properties before claiming no protection loss.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprTM`, `crates/op-plugins/src/state_plugins/tched_plugins.rs:179`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **needs decision** — `PRRT_kwDOUgxzzc6tprTk`, `crates/op-plugins/src/state_plugins/model_registry.rs:357`. models_url is operator-controlled arbitrary HTTPS configuration; a key is intentionally assigned to that endpoint. No trusted provider-host authority mapping exists to compare against. Suggested action: Decide whether endpoint host allowlists are a required contract; do not invent hardcoded vendor restrictions.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsXZ`, `crates/op-plugins/src/state_plugins/rtnetlink.rs:479`. Later op-network::rtnetlink::del_ipv4_address returns Result<bool>:315; disposition match compiles against matching source. Suggested action: Integrate support crate layer.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsXa`, `crates/op-plugins/src/state_plugins/service/methods.rs:10`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsXc`, `crates/op-plugins/src/state_plugins/mod.rs:62`. Later bridge removes tched_router_config_surface references and test/export cleanup removes retired MongoDB module imports. Suggested action: Carry full stack plus recovery source restoration; do not restore compatibility exports.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsXh`, `crates/op-plugins/src/state_plugins/tched_plugins.rs:179`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsXk`, `crates/op-plugins/src/state_plugins/tched_plugins.rs:53`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsXn`, `crates/op-plugins/src/state_plugins/snowball_methods.rs:389`. snowball_methods rollback test writes generation2 then rolls back retained generation2; it cannot detect a no-op. Suggested action: Write a distinct current generation after snapshot before rollback assertion, when tests are edited; do not run unrequested tests.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptQU`, `crates/op-plugins/src/state_plugins/service/methods.rs:10`. authority.rs, emqx.rs and service/methods.rs use org.opdbus.v1.plugins.runit; provider op-runit-systemctl still exports org.opdbus.v1.Runit.Systemctl at /org/opdbus/v1/plugins/runit/systemctl. Suggested action: Restore destination/path/interface together in all three clients.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tptQY`, `crates/op-plugins/src/state_plugins/tched_plugins.rs:53`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **needs decision** — `PRRT_kwDOUgxzzc6tptQf`, `crates/op-plugins/src/state_plugins/model_registry.rs:401`. validate_provider omits credential gate combinations, but bridge refresh/list explicitly validates gates. The public parse function contract versus downstream validation needs a single declared boundary. Suggested action: Centralize credential gate validation if parse_providers_toml promises fully validated providers; keep current fail-closed downstream gates.

- **needs decision** — `PRRT_kwDOUgxzzc6tptQk`, `crates/op-plugins/src/state_plugins/routing_decision_methods.rs:718`. routing_decision loops egress rules and denies on any mismatch; whether policies compose conjunctively or disjunctively is security policy, not inferable from this comment. Suggested action: Owner must establish union/intersection semantics before altering authorization.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tptQp`, `crates/op-plugins/src/state_plugins/notebooklm.rs:28`. Later release stages NLM at /usr/local/libexec/op-dbus/bin/nlm and runtime wrapper uses that location; old /usr/local/bin recommendation is stale for cumulative release. Suggested action: Integrate packaging layer, do not restore obsolete fallback alone.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptQv`, `crates/op-plugins/src/state_plugins/qdrant.rs:448`. QdrantCollectionParams requires flat QdrantVectorParams; current_state:447 decodes each collection and propagates failure. Suggested action: Model single/named vectors union and preserve complete state publication.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptQz`, `crates/op-plugins/src/state_plugins/schema_tag_registry_methods.rs:742`. tag update lookup:739–742 checks only tag_id; registry validation separately requires unique names. Suggested action: Reject a requested name owned by another tag before mutation.

### PR #16

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpp01`, `crates/op-plugins/src/state_plugins/tched_router.rs:3138`. tched_router PLUGIN_VERSION=4.4.1 but config_contract_break_bumps_plugin_version still expects4.3.0, including recovery:3101. Suggested action: Update assertion to intentional contract version; do not report test pass.

- **false positive** — `PRRT_kwDOUgxzzc6tpp08`, `crates/op-plugins/src/state_plugins/tched_skills.rs:659`. jeremy is the explicitly configured runtime account and a username is not a credential. chpst failure is checked, not silently ignored. Suggested action: No security fix based on CWE798; optional shared constant.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpp09`, `crates/op-plugins/src/state_plugins/tched_router.rs:2456`. set_role_model_handler:2604–2639 accepts any nonempty model_id and emits changed-role signal without declared catalog membership unlike primary selection. Suggested action: Validate selected model against authoritative declared catalog before role mutation.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpp1B`, `crates/op-plugins/src/state_plugins/threetched.rs:148`. Recovery changes runtime schema readers to sealed catalog and makes shared capture an explicit build/seal step; original OnceLock unbounded ambient capture is replaced. Suggested action: Carry shared capture changes and use paired candidate binary/manifest during build/seal.

- **needs decision** — `PRRT_kwDOUgxzzc6tpp1C`, `crates/op-plugins/src/state_plugins/threetched.rs:197`. Selector check admits only equal configured path in several captured clap representations, including nested array. No alternate path bypass shown. Suggested action: Confirm captured CLI shape if rejecting nested array; do not describe it as a privilege escalation.

- **needs decision** — `PRRT_kwDOUgxzzc6tpp1D`, `crates/op-plugins/src/state_plugins/tched_router.rs:273`. Renaming cognitive_endpoint is a breaking persisted representation; current clean-cutover owner direction disallows automatically restoring old compatibility schema. Suggested action: Use explicit migration/rejection of old records if supported; decide based on current persisted contract, not bot alias suggestion.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpp1G`, `crates/op-plugins/src/state_plugins/tched_router.rs:2123`. tched_config_dispatch maps GetMcpConfig to NativeSection(mcp):37 before synchronous fallback. Local dispatcher test classification may still need alignment. Suggested action: Retain native adapter; align declaration-coverage test to actual owner path.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpriu`, `crates/op-plugins/src/state_plugins/tched_skills.rs:369`. Later MutationEngine arms dispatch threetched_fs and tched_skills/tched_plugins/threetched at4359–4429. Recovery consolidates/names tched_fs. Suggested action: Integrate bridge owner and recovery naming/consolidation; no new parallel adapters.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprjD`, `crates/op-plugins/src/state_plugins/tched_router.rs:2179`. Later MutationEngine::dispatch_tched_router_owned explicitly handles all three catalog-route methods at5278/5341. Suggested action: Integrate #19 owner adapter.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprjo`, `crates/op-plugins/src/state_plugins/tched_router_paths.rs:11`. Later tched-router run script and golden packaging use /usr/local/libexec/op-dbus/threetched/threetched and matching threetched manifest. Suggested action: Integrate release layout; do not restore zeroclaw names.

- **false positive** — `PRRT_kwDOUgxzzc6tprkV`, `crates/op-plugins/src/state_plugins/tched_skills.rs:693`. tched_skills run moves stdin into async write branch; Some(mut stdin) is dropped when branch completes before try_join awaits child exit. EOF is delivered. Suggested action: No stdin fix for alleged retained handle. Legacy wrapper is removed in recovery.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprk4`, `crates/op-plugins/src/state_plugins/tched_router.rs:2418`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprlE`, `crates/op-plugins/src/state_plugins/tched_skills.rs:607`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprla`, `crates/op-plugins/src/state_plugins/tched_skills.rs:663`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprt4`, `crates/op-plugins/src/state_plugins/threetched.rs:22`. Later FUSE layer provides library target and threetched_surface/fuse_process modules used by plugin. Suggested action: Integrate support crate layer.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprt7`, `crates/op-plugins/src/state_plugins/tched_router.rs:2465`. Later MutationEngine::dispatch_tched_router_owned explicitly handles all three catalog-route methods at5278/5341. Suggested action: Integrate #19 owner adapter.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprt8`, `crates/op-plugins/src/state_plugins/tched_router_generation.rs:25`. tched_router_generation_native MAX_PROMPT_BYTES is256KiB at cumulative/recovery source; raised request bound is now matched. Suggested action: Integrate later native runtime layer.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprt-`, `crates/op-plugins/src/state_plugins/tched_router.rs:3138`. tched_router PLUGIN_VERSION=4.4.1 but config_contract_break_bumps_plugin_version still expects4.3.0, including recovery:3101. Suggested action: Update assertion to intentional contract version; do not report test pass.

- **false positive** — `PRRT_kwDOUgxzzc6tpsTL`, `crates/op-plugins/src/state_plugins/tched_router.rs:2074`. Current owner explicitly retired embedded contract import and per-section generated surface; shared verified native capture on seal is required. Suggested action: Do not restore deleted importer or87 generated getters. Update stale consumers/docs only.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsTN`, `crates/op-plugins/src/state_plugins/tched_router.rs:2164`. tched_config_dispatch maps GetMcpConfig to NativeSection(mcp):37 before synchronous fallback. Local dispatcher test classification may still need alignment. Suggested action: Retain native adapter; align declaration-coverage test to actual owner path.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsTP`, `crates/op-plugins/src/state_plugins/tched_router.rs:2417`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsTT`, `crates/op-plugins/src/state_plugins/tched_skills.rs:658`. Legacy binary_namespace/tched_plugins/tched_skills implementations persist at stack tip but files and router facades are removed in recovery. Do not repair/reintroduce deleted wrappers. Suggested action: Carry reviewed recovery consolidation into merge result; ensure current threetched owner covers retained contracts.

- **needs decision** — `PRRT_kwDOUgxzzc6tpsTX`, `crates/op-plugins/src/state_plugins/threetched.rs:225`. Current owner mandates external binary-backed plugin and observed execution path; generic no-subprocess rule conflicts with implemented canonical admitted native command executor. A direct process alone is not evidence of bypass before MutationEngine admission. Suggested action: Do not replace established native executor with invented service; resolve architecture against approved migration and current host/container execution identity.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsTZ`, `crates/op-plugins/src/state_plugins/tched_router.rs:3136`. tched_router PLUGIN_VERSION=4.4.1 but config_contract_break_bumps_plugin_version still expects4.3.0, including recovery:3101. Suggested action: Update assertion to intentional contract version; do not report test pass.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsTa`, `crates/op-plugins/src/state_plugins/tched_router_paths.rs:11`. Later tched-router run script and golden packaging use /usr/local/libexec/op-dbus/threetched/threetched and matching threetched manifest. Suggested action: Integrate release layout; do not restore zeroclaw names.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsTb`, `crates/op-plugins/src/state_plugins/tched_skills.rs:321`. Recovery changes runtime schema readers to sealed catalog and makes shared capture an explicit build/seal step; original OnceLock unbounded ambient capture is replaced. Suggested action: Carry shared capture changes and use paired candidate binary/manifest during build/seal.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsTd`, `crates/op-plugins/src/state_plugins/tched_skills.rs:734`. Later MutationEngine arms dispatch threetched_fs and tched_skills/tched_plugins/threetched at4359–4429. Recovery consolidates/names tched_fs. Suggested action: Integrate bridge owner and recovery naming/consolidation; no new parallel adapters.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsTg`, `crates/op-plugins/src/state_plugins/tched_router_contract.rs:17`. Recovery removes embedded provisional contract and uses verified shared full config schema capture on every seal. Suggested action: Carry recovery capture implementation; do not regenerate obsolete embedded constant.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsTk`, `crates/op-plugins/src/state_plugins/tched_router.rs:2173`. Later MutationEngine::dispatch_tched_router_owned explicitly handles all three catalog-route methods at5278/5341. Suggested action: Integrate #19 owner adapter.

### PR #17

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tppOf`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:362`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **needs decision** — `PRRT_kwDOUgxzzc6tppOg`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:174`. Duplicate WorkflowsState defaults are maintainability debt, no present divergent values established. Suggested action: Optional delegate current_state to Default in a scoped cleanup, not a merge-blocking regression.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tppOo`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:357`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tppOq`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:183`. WorkflowsPlugin rollback returns Ok with supports_rollback=false; direct accidental invocation reports a nonexistent effect. Suggested action: Return explicit unsupported rollback error.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpq9z`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:361`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpq93`, `crates/op-plugins/src/state_plugins/threetched_fs_contract.rs:5698`. threetched_fs_contract file removed/replaced by recovery tched_fs contract/projection; obsolete enumeration table should not be repaired in isolation. Suggested action: Review current tched_fs declaration and preserve canonical names.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprMJ`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:362`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprMg`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:357`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprpo`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:361`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprpp`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:357`. Later MutationEngine routes workflows and cognitive workflow aliases through WorkstackRunner; descriptor-only plugin helper is not runtime owner. Suggested action: Integrate bridge workflow adapter; keep old journal retired.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprpq`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:38`. StartWorkflowInput.params remains Option<Value> with prose-only limits; downstream runner validates but sealed contract lacks declared bounds. Suggested action: Encode bounded workflow structures/constraints in authoritative schema while retaining runtime checks.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprpr`, `crates/op-plugins/src/state_plugins/workflows_plugin.rs:37`. Later audit_method_arguments consumes args schema, digests x-opdbus-private and includes workflows/cognitive in whole-request private fallback. Suggested action: Integrate audit layer, then fix combinator/unknown-field weaknesses separately.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprpu`, `crates/op-plugins/src/state_plugins/threetched_fs_contract.rs:16054`. Later MutationEngine arms dispatch threetched_fs and tched_skills/tched_plugins/threetched at4359–4429. Recovery consolidates/names tched_fs. Suggested action: Integrate bridge owner and recovery naming/consolidation; no new parallel adapters.

### PR #18

- **false positive** — `PRRT_kwDOUgxzzc6tpqOc`, `crates/op-grpc-bridge/src/grpc_server.rs:2784`. Comment describes the deletion of an unsafe fixed default as a correct fix. Empty endpoint now rejects. Suggested action: No change.

- **false positive** — `PRRT_kwDOUgxzzc6tpqOd`, `crates/op-grpc-bridge/src/grpc_server.rs:265`. normalize_message_keys mutates original map by removing known proto keys and reinserting authored names; unknown keys are never removed. Suggested action: Keep later exact schema validation.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpqOe`, `crates/op-grpc-bridge/src/external_mcp_session.rs:246`. external_mcp_session:246 uses ? on missing context in loop; additional resolve lookup uses ok()?? and can also abort scan. Suggested action: Skip unavailable/nonmatching entries instead of terminating candidate search; retain ambiguity rejection.

- **needs decision** — `PRRT_kwDOUgxzzc6tpqOi`, `crates/op-grpc-bridge/src/grpc_server.rs:284`. descriptor_schema_node depth exhaustion returns no normalization; malformed/deep schemas are still validated. No successful incorrect dispatch is demonstrated. Suggested action: Return explicit depth/unsupported mapping error if preserving diagnosability is required; do not loosen validation.

- **needs decision** — `PRRT_kwDOUgxzzc6tpqOl`, `crates/op-grpc-bridge/src/grpc_server.rs:1508`. Outer CORS is permissive and exposes metadata names, but a malicious origin still needs valid credential request and inner CORS applies. Comment does not prove credential disclosure. Suggested action: Align CORS layers to configured allowed origins; assess exact header sensitivity and request credential path before labeling leak.

- **needs decision** — `PRRT_kwDOUgxzzc6tprO3`, `crates/op-grpc-bridge/src/identity_clean_provision/storage.rs:155`. fs_info checks num_devices1, later enumerator expects seed+leaf; Btrfs seed devices are a separate device set, so simple arithmetic is not proof of rejection. Recovery retains this verifier. Suggested action: Verify kernel ioctl semantics for seed/sprout with authoritative receipt before changing identity-storage checks; no test run authorized.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprPp`, `crates/op-grpc-bridge/src/chatbot_episode_state.rs:29`. Later runtime uses COGNITIVE_DB_PATH consistently with guard, under current COGNITIVE rename. Suggested action: Integrate runtime rename; do not restore obsolete variable alone.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprQP`, `crates/op-grpc-bridge/src/chat_response_dispatch.rs:481`. start_conversation counts before acquiring publisher.admit; insert does not atomically enforce quota. Suggested action: Move count/check under per-principal admission or enforce in same Cozo transaction.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprQd`, `crates/op-grpc-bridge/src/agent_ui_proxy.rs:115`. agent_ui_proxy::route selects Host and forwards before gRPC interceptor without caller admission when enabled. Suggested action: Require existing identity/capability admission for private proxy or remove unsupported bypass surface.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpr84`, `crates/op-grpc-bridge/src/chat_response_dispatch.rs:481`. start_conversation counts before acquiring publisher.admit; insert does not atomically enforce quota. Suggested action: Move count/check under per-principal admission or enforce in same Cozo transaction.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tptSH`, `crates/op-grpc-bridge/src/chatbot_episode_state.rs:28`. Later runtime uses COGNITIVE_DB_PATH consistently with guard, under current COGNITIVE rename. Suggested action: Integrate runtime rename; do not restore obsolete variable alone.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSI`, `crates/op-grpc-bridge/src/grpc_server.rs:1387`. WorkstackService mounted directly; execute calls facade.caller identity only, not workflows.write sealed admission. Per-child checks do not authorize orchestration itself. Suggested action: Retire direct handwritten surface or forward through canonical sealed workflows.start_workflow admission.

- **needs decision** — `PRRT_kwDOUgxzzc6tptSL`, `crates/op-grpc-bridge/src/grpc_server.rs:1333`. Workstack reflection registers full descriptor plus explicit service name. Need version-specific tonic-reflection list/lookup semantics; no claim all listed names leak without inspecting dependency source. Suggested action: Prefer derived service-only descriptor if retained; overall unsealed service removal is already required.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSN`, `crates/op-grpc-bridge/src/freedesktop_dispatch.rs:596`. freedesktop_dispatch XML advertises CallWithOperationId at596; no zbus implementation method found at any reviewed revision. Suggested action: Implement canonical admitted operation-id handler or remove advertisement and dependent unsupported call path.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSQ`, `crates/op-grpc-bridge/src/agent_ui_proxy.rs:121`. agent_ui_proxy forward:121 checks browser origin only for websocket requests; ordinary requests bypass origin enforcement. Suggested action: Apply exact-origin checks for every browser Origin-bearing mutation request.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptST`, `crates/op-grpc-bridge/src/chat_response_dispatch.rs:481`. start_conversation counts before acquiring publisher.admit; insert does not atomically enforce quota. Suggested action: Move count/check under per-principal admission or enforce in same Cozo transaction.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSX`, `crates/op-grpc-bridge/src/catalog_ordinary_profile.rs:156`. catalog_ordinary_profile reads provider key_file (documented model-list auth) and writes same secret into inference profile uri/api_key. Suggested action: Declare separately scoped inference auth or verify same authorized authority before reuse; support public-list providers.

- **needs decision** — `PRRT_kwDOUgxzzc6tptSb`, `crates/op-grpc-bridge/src/identity_clean_provision/partitioned.rs:90`. Bridge verifier/observer directly reads kernel mount/metadata. This is maintenance/native observation implementation, while AGENTS broadly forbids parallel live-state reads; source does not establish whether this observer is approved owner. Suggested action: Resolve ownership with existing storage/Incus plugin; do not invent a parallel service or bypass identity to fix review.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSf`, `crates/op-grpc-bridge/src/identity_clean_provision.rs:283`. CleanPlan validate permits any disk-typed device; only root pinned and two named v4 exclusions. Host path disk under another name passes. Suggested action: Require exact approved device set and pins for every device before adoption.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSj`, `crates/op-grpc-bridge/src/chat_response_state.rs:73`. ChatResponsePublisher recover:471 locks map mutex for complete asynchronous recovery instead of selected owner mutex, blocking all principal admissions. Suggested action: Factor owner-lock lookup and hold only selected owner guard.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptSl`, `crates/op-grpc-bridge/src/accountability_search.rs:36`. accountability_search::search directly forwards query/limit; only prose declares4000-byte/1–20 input constraints. Suggested action: Validate byte and numeric bounds before external work; encode schema constraints.

### PR #19

- **false positive** — `PRRT_kwDOUgxzzc6tpp_P`, `crates/op-grpc-bridge/src/identity_seed_provision.rs:730`. Legacy provision path is deliberately retired and guarded by require_external_shared_seed_provisioning; invalid digest must fail. Removing the digest guard is not a valid fix. Suggested action: Delete unreachable obsolete code when scoped, but never enable legacy provisioning.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpp_R`, `crates/op-grpc-bridge/src/interceptor.rs:1907`. load_capability_grants:1848 uses unprotected fs::read each request while protected startup reader validates ownership/mode. Suggested action: Use protected reader per grant reload and fail closed on ownership/mode/symlink violation.

- **false positive** — `PRRT_kwDOUgxzzc6tpp_U`, `crates/op-grpc-bridge/src/openflow_permit.rs:120`. Final post-await deadline and cancellation checks deliberately require permit current at execution boundary. Tokio timeout may return Ready at/after deadline; removing final gate weakens authorization. Suggested action: Keep fail-closed deadline check.

- **needs decision** — `PRRT_kwDOUgxzzc6tpp_Y`, `crates/op-grpc-bridge/src/interceptor.rs:67`. Bound oracle source may be decoy/WireGuard peer rather than human principal; valid signed proof, assignment and target SID1 are independently checked. Missing human row alone is not proof of unregistered credential acceptance. Suggested action: Confirm source registration contract with oracle authority; do not require human enrollment indiscriminately.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprXq`, `crates/op-grpc-bridge/src/lungs_socket.rs:26`. Later deployment helpers op-identity-root/op-lungs-check use /run/opdbus/lungs/lungs.sock; recovery preserves matching socket path. Suggested action: Integrate packaging defaults as a unit.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprX7`, `crates/op-grpc-bridge/src/lungs_socket.rs:482`. World-connectable socket accepts bounded unauthenticated negotiation; recovery even performs initial peek before spawning worker. Local peers can repeatedly consume admission/time. Suggested action: Apply supported peer-origin screening before reserving long-lived slots and isolate negotiation concurrency; preserve container caller access.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpto2`, `crates/op-grpc-bridge/src/identity_sled_dispatch.rs:1341`. park sets attached=false on migrated identity while Cozo physical-root binding immutability compares complete JSON; recovery still retains mismatch risk. Suggested action: Keep immutable physical binding, store logical attachment separately or allow exactly that field in checked intent.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpto5`, `crates/op-grpc-bridge/src/model_registry_dispatch.rs:219`. load_providers calls routing readiness validator requiring inference_url_prefix for every enabled discovery provider. Suggested action: Require routing fields only when creating route; discovery catalog remains valid independently.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpto8`, `crates/op-grpc-bridge/src/model_registry_dispatch.rs:887`. maintain_credentials reads, mutates and atomic-renames full provider document without shared transaction lock/CAS; concurrent writes lose changes. Suggested action: Serialize complete read-modify-write-readback, normalize/validate first, ideally share refresh lock.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptpA`, `crates/op-grpc-bridge/src/model_registry_dispatch.rs:999`. OAuth slot stored trimmed but readback compares untrimmed operation.slot; multiple operations same provider validated against final state. Suggested action: Normalize slots and reject conflicting duplicate operations before writing.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptpB`, `crates/op-grpc-bridge/src/model_registry_dispatch.rs:782`. binding_target_from checks enabled but not credential off; cached available model rows still bind. Suggested action: Validate credential gate and refuse Off before route selection.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tptpD`, `crates/op-grpc-bridge/src/model_registry_dispatch.rs:654`. list filters configured_providers before include_unavailable, hides retained removed-provider history. Suggested action: Allow requested historical rows with unavailable status and no routable authority.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tptpF`, `crates/op-grpc-bridge/src/lungs_socket.rs:27`. Later deployment helpers op-identity-root/op-lungs-check use /run/opdbus/lungs/lungs.sock; recovery preserves matching socket path. Suggested action: Integrate packaging defaults as a unit.

- **needs decision** — `PRRT_kwDOUgxzzc6tptpH`, `crates/op-grpc-bridge/src/incus_observer.rs:56`. Bridge verifier/observer directly reads kernel mount/metadata. This is maintenance/native observation implementation, while AGENTS broadly forbids parallel live-state reads; source does not establish whether this observer is approved owner. Suggested action: Resolve ownership with existing storage/Incus plugin; do not invent a parallel service or bypass identity to fix review.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpt8l`, `crates/op-grpc-bridge/src/lungs_socket.rs:27`. Later deployment helpers op-identity-root/op-lungs-check use /run/opdbus/lungs/lungs.sock; recovery preserves matching socket path. Suggested action: Integrate packaging defaults as a unit.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpt8q`, `crates/op-grpc-bridge/src/json_render_gallery_dispatch.rs:335`. Gallery records persist profile_sha256 only, no exact sealed plugin schema hash. A broader reseal leaves active entries apparently current. Suggested action: Persist/validate manifest-pinned plugin schema hash on candidates and promoted entries.

- **needs decision** — `PRRT_kwDOUgxzzc6tpt8u`, `crates/op-grpc-bridge/src/incus_observer.rs:56`. Bridge verifier/observer directly reads kernel mount/metadata. This is maintenance/native observation implementation, while AGENTS broadly forbids parallel live-state reads; source does not establish whether this observer is approved owner. Suggested action: Resolve ownership with existing storage/Incus plugin; do not invent a parallel service or bypass identity to fix review.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpt8y`, `crates/op-grpc-bridge/src/mcp_frontend.rs:1224`. handle_batch unconditionally collects jsonrpc_body for id-less failed messages, and error helper returns null-id body. Suggested action: Determine valid notification status before dispatch and suppress error responses for notifications.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpt84`, `crates/op-grpc-bridge/src/model_registry_dispatch.rs:973`. maintain_credentials reads, mutates and atomic-renames full provider document without shared transaction lock/CAS; concurrent writes lose changes. Suggested action: Serialize complete read-modify-write-readback, normalize/validate first, ideally share refresh lock.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpt89`, `crates/op-grpc-bridge/src/identity_root_repair.rs:1135`. verify_old_persist_empty:1133 reads old.mount_point without verifying expected mount/device/FSID first. Suggested action: Require supported storage-owner mount proof before emptiness; absence fails closed.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpt8-`, `crates/op-grpc-bridge/src/mcp_frontend.rs:312`. External contexts are registered in engine; no forget_session_context on MCP DELETE/SSE close/expiry found; session_closed performs close-check only. Suggested action: Track external scope leases and remove context on final close/expiry without dropping active shared leases.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpt8_`, `crates/op-grpc-bridge/src/nlm_cli.rs:14`. Later release stages NLM at /usr/local/libexec/op-dbus/bin/nlm and runtime wrapper uses that location; old /usr/local/bin recommendation is stale for cumulative release. Suggested action: Integrate packaging layer, do not restore obsolete fallback alone.

### PR #20

- **needs decision** — `PRRT_kwDOUgxzzc6tpqG7`, `crates/op-grpc-bridge/src/pam_auth.rs:83`. calloc and free_responses correctly zero/free earlier entries. Error output-pointer initialization can be hardened, but assertion PAM frees uninitialized *resp on callback failure lacks implementation evidence. Suggested action: Set *resp=null after validating pointer; verify PAM contract before claiming exploitable double free.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpqG_`, `crates/op-grpc-bridge/src/task_elevation.rs:232`. TaskElevation authorize captures now before potentially slow PAM and uses it for failure window and ticket expiry after PAM. Suggested action: Take fresh completion timestamp for failure/issue.

- **needs decision** — `PRRT_kwDOUgxzzc6tpqHB`, `crates/op-grpc-bridge/src/pam_auth.rs:98`. calloc and free_responses correctly zero/free earlier entries. Error output-pointer initialization can be hardened, but assertion PAM frees uninitialized *resp on callback failure lacks implementation evidence. Suggested action: Set *resp=null after validating pointer; verify PAM contract before claiming exploitable double free.

- **false positive** — `PRRT_kwDOUgxzzc6tpqHD`, `crates/op-grpc-bridge/src/server.rs:887`. Schema-content refresh failure and unavailable container storage have different invariants. Failing startup on corrupted identity schema does not contradict allowing independent storage recovery. Suggested action: Do not make identity integrity refresh best-effort without specific recovery design.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpqHE`, `crates/op-grpc-bridge/src/workstack_service.rs:208`. Workstack EpisodeSink inserts owners on Started and never expires/removes completed map entry. Suggested action: Bound terminal status retention or move historical ownership to durable scoped store.

- **needs decision** — `PRRT_kwDOUgxzzc6tpqHG`, `crates/op-grpc-bridge/src/tched_router_runtime.rs:484`. Ack check keyed to chat currently matches authenticated A2A chat call site; comment hypothesizes a future different operation, not an existing bypass. Suggested action: Use typed operation/auth policy if extending endpoints; no blanket ack requirement for unrelated config endpoints.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpru7`, `crates/op-grpc-bridge/src/routing_store.rs:203`. routing_store owners map has hard max_owners and never evicts idle slots; session scopes eventually exhaust map. Suggested action: Evict only inactive unreferenced slots or use weak-owner lifetime accounting.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprvM`, `crates/op-grpc-bridge/src/request_registry.rs:201`. request_registry open uses requests.len while fulfilled entries retained until TTL; global capacity includes completed requests. Suggested action: Exclude fulfilled entries from open capacity and bound retained replay tombstones separately.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprvX`, `crates/op-grpc-bridge/src/system_graph_projection.rs:352`. system_graph_projection selects expanded_devices only if nonempty, inventing mounts from stale local devices on authoritative empty effective list. Suggested action: Prefer present expanded inventory even when empty; distinguish absent from empty.

- **needs decision** — `PRRT_kwDOUgxzzc6tprvn`, `crates/op-grpc-bridge/src/pam_auth.rs:165`. pam_start error branch does not call pam_end, but libpam commonly cleans failed initialization itself. Non-null error handle needs upstream proof before marking leak. Suggested action: Check linked PAM implementation; use RAII for every successfully initialized handle.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tprv0`, `crates/op-grpc-bridge/src/workstack_service.rs:229`. Later op-chat workstack_executor::execute_rollback uses execute_scoped(&ctx.scope) at1025. Suggested action: Integrate matching op-chat layer.

- **needs decision** — `PRRT_kwDOUgxzzc6tprwN`, `crates/op-grpc-bridge/src/workstack_service.rs:377`. Shared definition registry can change between register/execute; requests select committed definitions rather than arbitrary caller payload. Cross-session definition substitution severity depends on concurrent committed-definition changes. Suggested action: Execute the captured admitted definition directly or pin revision; do not claim arbitrary malicious payload injection without a writer path.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsvm`, `crates/op-grpc-bridge/src/workstack_service.rs:355`. WorkstackService mounted directly; execute calls facade.caller identity only, not workflows.write sealed admission. Per-child checks do not authorize orchestration itself. Suggested action: Retire direct handwritten surface or forward through canonical sealed workflows.start_workflow admission.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsvn`, `crates/op-grpc-bridge/src/workstack_service.rs:377`. Later ExecutionContext::new allocates workstack.id plus uuid::Uuid::new_v4, eliminating seconds-based collision. Suggested action: Integrate matching op-chat layer.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsvq`, `crates/op-grpc-bridge/src/request_registry.rs:295`. request_registry open uses requests.len while fulfilled entries retained until TTL; global capacity includes completed requests. Suggested action: Exclude fulfilled entries from open capacity and bound retained replay tombstones separately.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpsvs`, `crates/op-grpc-bridge/src/workstack_service.rs:377`. Later orchestration/services/workstack.rs uses tokio::spawn at292/304, not spawn_local. Suggested action: Integrate matching op-chat layer.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsvu`, `crates/op-grpc-bridge/src/workstack_service.rs:200`. Completed event spawns untracked commit_chatbot_episode and only logs validation/storage failures:197–207; terminal event not coupled to durable episode. Suggested action: Make terminal persistence part of owned completion and report unknown/error honestly; avoid automatic replay of effects.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsvw`, `crates/op-grpc-bridge/src/system_graph_dispatch.rs:83`. system_graph_dispatch:81–84 rechecks plugin IDs only; catalog hash change with same IDs is not caught. Suggested action: Pin and recompare manifest catalog hash before publication.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpsvx`, `crates/op-grpc-bridge/src/tched_config_engine.rs:320`. Recovery removes GetConfigSchema declaration and legacy dispatch; old stack still exposes stale state-backed schema. Suggested action: Carry recovery removal and matching tests; do not restore duplicate schema authority.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpsvy`, `crates/op-grpc-bridge/src/workstack_service.rs:146`. Workstack EpisodeSink inserts owners on Started and never expires/removes completed map entry. Suggested action: Bound terminal status retention or move historical ownership to durable scoped store.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpvyN`, `crates/op-grpc-bridge/src/server.rs:1325`. deploy/config/routing-limits.json exists at cumulative tip and recovery. Suggested action: Integrate release configuration layer.

- **fixed by later stack layer** — `PRRT_kwDOUgxzzc6tpvyQ`, `crates/op-grpc-bridge/src/workstack_service.rs:20`. Later op-chat defines ExecutionControl/ScopeVerifier/WorkstackEventSink and scoped executor methods; inspected current imports and implementations. Suggested action: Integrate matching op-chat source layer.

- **false positive** — `PRRT_kwDOUgxzzc6tpvyT`, `crates/op-grpc-bridge/src/tched_catalog_binding.rs:212`. Exact #20 file already expects openai.go in catalog_binding_distinguishes_platforms_and_path_boundaries, not custom.zen. Comment describes different source revision. Suggested action: No fix to inspected assertion; inspect separate runtime fixture if still relevant before changing expectations.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpvyW`, `crates/op-grpc-bridge/src/workstack_runner.rs:432`. workstack_runner:419–432 times out waiting on detached MutationEngine owner task then drops worker permit while underlying admitted work can continue. Suggested action: Tie permit lifetime to actual owned work settlement; report unknown without freeing active capacity early.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpvyY`, `crates/op-grpc-bridge/src/responder_wake.rs:51`. Responder-specific MQTT topic has no matching authenticated subscribe branch; hook permits only own wake/toolsets in opdbus namespace. Suggested action: Use designated-responder admission for bounded content-free notifications through existing broker hook.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tpvya`, `crates/op-grpc-bridge/src/task_elevation.rs:232`. TaskElevation check_lockout before spawn_blocking PAM does not reserve attempt; failure count updated after call. Suggested action: Serialize PAM verification per principal or reserve attempt atomically before launch.

### PR #21

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tpqvr`, `crates/op-grpc-bridge/tests/negative_topology_gates.rs:45`. Recovery restores crates/op-cognitive/proto/cognitive.proto at the test-pinned path. Stack tip removed old cognitive crate without replacement. Suggested action: Carry full recovery op-cognitive source restoration; no old-path alias needed.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprHd`, `crates/op-grpc-bridge/tests/negative_topology_gates.rs:45`. Recovery restores crates/op-cognitive/proto/cognitive.proto at the test-pinned path. Stack tip removed old cognitive crate without replacement. Suggested action: Carry full recovery op-cognitive source restoration; no old-path alias needed.

- **fixed only in recovery/current worktree** — `PRRT_kwDOUgxzzc6tprHg`, `crates/op-grpc-bridge/tests/tched_router_backing.rs:1549`. Recovery removes GetConfigSchema declaration and legacy dispatch; old stack still exposes stale state-backed schema. Suggested action: Carry recovery removal and matching tests; do not restore duplicate schema authority.

- **confirmed defect** — `PRRT_kwDOUgxzzc6tprHj`, `crates/op-grpc-bridge/tests/p0_principal_group_lifecycle.rs:291`. ignored principal_group_lifecycle_child unconditionally calls checked_fixture_path; explicit --ignored executes without parent marker. Suggested action: Return harmlessly when helper marker absent or use a dedicated child executable.

## Handoff

Machine-readable mapping: core-triage.json. Parent authorized a new isolated worktree from d46bd4b for narrow confirmed fixes after this report, excluding op-blob/opblob ownership. Those future fixes will be recorded separately with commit IDs and will not retroactively change this reviewed-head verdict. Build requirements will be sent to parent; no Cargo may run concurrently. No pushes, merges or deployment were performed by this reviewer.

## Authorized bounded repair handoff

Repair branch `fix/core-pr-sweep-20261023` in `/home/jeremy/.codex/worktrees/29f8/odbus-verified` starts at the cumulative stack tip. It has five commits, is clean, and has not been pushed or merged.

| Commit | Change | Inline thread IDs |
|---|---|---|
| `d6fc695` | Cozo fail-closed policy, atomic links, sync/empty inventories, transactional quota | `PRRT_kwDOUgxzzc6tppaw`, `PRRT_kwDOUgxzzc6tprE2`, `PRRT_kwDOUgxzzc6tppaz`, `PRRT_kwDOUgxzzc6tprB6`, `PRRT_kwDOUgxzzc6tpsAl`, `PRRT_kwDOUgxzzc6tpxzJ`, `PRRT_kwDOUgxzzc6tprAQ`, `PRRT_kwDOUgxzzc6tpsAi`, `PRRT_kwDOUgxzzc6tpxzG`, `PRRT_kwDOUgxzzc6tprCi`, `PRRT_kwDOUgxzzc6tpxzH`, `PRRT_kwDOUgxzzc6tprED`, `PRRT_kwDOUgxzzc6tprQP`, `PRRT_kwDOUgxzzc6tpr84`, `PRRT_kwDOUgxzzc6tptST` |
| `b5cba28` | Complete catalog names and committed content-free wakes | `PRRT_kwDOUgxzzc6tpptb`, `PRRT_kwDOUgxzzc6tprO6` |
| `6782380` | Authoritative registration and MCP actor scan | `PRRT_kwDOUgxzzc6tpqOe` |
| `8cf33e1` | Elevation timing/concurrency/provider protection | `PRRT_kwDOUgxzzc6tpqG_`, `PRRT_kwDOUgxzzc6tpvya`, `PRRT_kwDOUgxzzc6tprOa` |
| `036ca82` | Chunk bounds, declared role models and version fixture | `PRRT_kwDOUgxzzc6tppuK`, `PRRT_kwDOUgxzzc6tpp09`, `PRRT_kwDOUgxzzc6tpp01`, `PRRT_kwDOUgxzzc6tprt-`, `PRRT_kwDOUgxzzc6tpsTZ` |

The two #19 body-only catalog/wake findings are implemented in b5cba28; #18 body-only registration fallback is implemented in6782380. PAM output initialization in8cf33e1 is defensive hardening, not proof the alleged invalid-free existed. Mapping entries retain their reviewed-head disposition and carry a separate remediation object so repair-branch implementation is not mistaken for integration.

Source diff inspection only; no Cargo, tests, formatting, Clippy or smoke fixture. Working tree clean after five commits. No push, merge or deployment.

Build request: As jeremy through existing JEV routing, one Cargo at a time, reuse /var/lib/cargo-build/odbus-verified: cargo build --locked --target x86_64-unknown-linux-gnu -p op-cozo-store -p op-plugins -p op-grpc-bridge. Bind Cargo/RUSTC/RUSTDOC/RUSTUP_TOOLCHAIN consistently if invoking a toolchain binary directly. No workspace build or tests requested.

Remaining: All unmapped findings remain unresolved. Snapshot retention/transport, schema audit projection, grants ownership/read race, migration helpers/binding, direct Workstack/proxy admission, provider credential transaction issues, unbounded registries and recovery-only retired modules remain blockers. A pending chat turn remains visible to polling before human commit; the wake repair addresses only premature notification. PAM pam_start cleanup allegation remains unproven.

Follow-up safety review: `2f322e0` retains fail-closed external MCP actor lookup when principal registry reads fail. Only absent contexts/principals are skipped. Repair branch now has six commits, head `2f322e0`, clean. Thread `PRRT_kwDOUgxzzc6tpqOe` records this supplemental commit. No build/tests run.

## Deployment cross-review and explicit identity selector

Reviewed f811665 and d2243a5 statically. Host-session invoke/provision wrappers use supported flags and same-handle receipts; quoted shell arguments preserve values, and no retry/fallback was introduced. Notebook readiness remains fresh local capture only; failed capture keeps last-good summary but records failure/removes ready marker. Qdrant check proves current TCP reachability only. Bearer header stdin removes bearer values from curl argv; existing Cloud Code token stdout/default-mode file behavior remains uncorrected.

Blocking guest cron issues were missing prepared executable path and missing typed guest-mode CLI selection. Deployment commit745b799 selects the guest's prepared /usr/local/bin/threetched-fs and --expected-mode guest-kernel. Parent-authorized CLI commit174cdca97fd2355cdb499a5ea8e0f5b4c37b1393 in clean29f8 adds --expected-mode host-session|guest-kernel|credential for call/invoke/describe/mcp. Explicit modes call open_for_mode; host-session requires an expected session; guest/credential reject session overrides; legacy no-mode path remains host-only. Existing mounted receipt validation remains unchanged. Source-only parser/selector regressions were added, not run. This addresses PR22 threads PRRT_kwDOUgxzzc6tps_U and PRRT_kwDOUgxzzc6tpsV1.

The generic host helper's3tched-fs default depends on the recovery rename: cumulative stack manifest still builds threetched-fs. Preserve explicit THREETCHED_FS selection or integrate the rename/install manifest; source does not prove deployment.

Cross-tranche blocker: b5cba28's content-free {request_id} wake needs the responder consumer to stop requiring/logging owner_key/owner_principal_id. Parent informed and owns that adaptation. No privacy fields should be restored to the broker payload.

At cumulative stack naming: cargo build --locked --target x86_64-unknown-linux-gnu -p threetched-fs --bin threetched-fs. After recovery rename, use its manifest tched-fs/3tched-fs. No build run.

## Published core dispositions

Fresh published PR heads were inspected and 11 remaining direct-fix threads received source-linked replies and were resolved (5 on #13, 2 on #16, 1 on #18, 3 on #20). PAM replies distinguish defensive output initialization from the unproven leak allegation. Already-resolved mappings were skipped. Later-layer-only and unresolved findings remain open.

- [#12 triage](https://github.com/repr0bated/odbus-verified/pull/12#issuecomment-6375215834) at `b1a88b570b0877b3591e0910ad29b2a6e6c4e5d7`.
- [#13 triage](https://github.com/repr0bated/odbus-verified/pull/13#issuecomment-6375216106) at `2ea37e634c326e5b2f52cf8753b41fb319fe0a93`.
- [#14 triage](https://github.com/repr0bated/odbus-verified/pull/14#issuecomment-6375216372) at `d99e59c8779c6d4d001ba0e6be037ed37fcb69dd`.
- [#15 triage](https://github.com/repr0bated/odbus-verified/pull/15#issuecomment-6375216696) at `ba4b6102ce2871b1008e07688645edc4f19fa9b1`.
- [#16 triage](https://github.com/repr0bated/odbus-verified/pull/16#issuecomment-6375216982) at `08179bcfaf003f07a7afdb56cc2bd173b691b2e6`.
- [#17 triage](https://github.com/repr0bated/odbus-verified/pull/17#issuecomment-6375217229) at `7045eb0e05cd6caa4661ef0204096769639811db`.
- [#18 triage](https://github.com/repr0bated/odbus-verified/pull/18#issuecomment-6375217551) at `5ac499a32b64dd91fd47bb6088f432e5d35236bb`.
- [#19 triage](https://github.com/repr0bated/odbus-verified/pull/19#issuecomment-6375217888) at `fe67db2bc96d8654b2c65dd60db92b20bd840b10`.
- [#20 triage](https://github.com/repr0bated/odbus-verified/pull/20#issuecomment-6375218218) at `1e139df0d14d67657b99e48637fa271d43a64ffe`.
- [#21 triage](https://github.com/repr0bated/odbus-verified/pull/21#issuecomment-6375218514) at `259f32b4c26250cefedf110e51f00c41dd08a878`.

Public comments report the scoped GNU integration build in progress under the parent task; no tests, formatting, Clippy, runtime fixtures or deployment were performed by this reviewer. Publication receipts and exact replies are in core-triage.json.publication.

## Bounded PR22 follow-up

- `e2f676a408602846839a807fb05c620b2811f568`: Reduced union validation, collision-safe MCP catalog construction, and correct nested reference scope. Threads: PRRT_kwDOUgxzzc6tpr0p, PRRT_kwDOUgxzzc6tpr1u, PRRT_kwDOUgxzzc6tps_S, PRRT_kwDOUgxzzc6tpsV4.
- `0b75c0a681b05720197f8726310188e52758d80f`: Typed FUSE clients verify mounted pins/declarations, leaving host catalog access and admission with daemon. Threads: PRRT_kwDOUgxzzc6tps_V, PRRT_kwDOUgxzzc6tpsVz.
- `d83a1fa8fb9f8cac0207896479a458558dce5f23`: Opt-in generated consumer and crate-relative tracked imports; explicit sealed artifact preparation. Threads: PRRT_kwDOUgxzzc6tpsVy.

Source inspection only; worktree clean after commits. No Cargo, tests, fmt, Clippy, generation, mount fixture or deployment executed.

Remaining:

- PRRT_kwDOUgxzzc6tpsV3: inode identity hashes only path; kind transitions need new inode incarnation and retention of opened-file state, coordinated with FUSE lifecycle owner. No partial hash-only change.
- PRRT_kwDOUgxzzc6tps_Q: identity-local stdio MCP versus single public door is an unresolved policy distinction; no admission bypass added.
- General anyOf specialization is deliberately reported unsupported rather than coercing a reduced overlapping union into an exclusive sum.
- generated-example (and --all-features) requires sealed-derived generated/plugin_types.rs first. Explicit --use-example generator output remains checkout-local; do not commit regenerated machine-specific absolute paths.
- Typed FUSE paths are source-reviewed only; guest operation/receipt and reseal rejection behavior are not runtime-verified.

Build request: After current Cargo worker exits, as jeremy through existing JEV routing/shared target: cargo build --locked --target x86_64-unknown-linux-gnu -p threetched-fs --bin threetched-fs --example typed_specialize. Apply recovery package/binary rename to tched-fs/3tched-fs at that integration revision. Do not enable generated-example without sealed artifact preparation. No tests/fmt/Clippy request.

Next: Parent independent source review, integration/restack, scoped build, and public thread replies only after exact published heads contain fixes.

## Parent prompt/code-collection cross-review

Source-only review of 3b4634a and6498360 against current authoritative instructions. No Cargo, tests or runtime operation. The new network-policy raw string agrees with policy, but get_fixed_prompt still includes contradictory retired lifecycle/deployment claims from the unchanged base (lines35–36,152); generated tool summary still advertises Systemd (408). The TOPOLOGY assertion at483 is now stale; local-release-activation.md assertion was already stale. Parent notified for bounded follow-up.

No source-level blocker found in6498360 collection selection or its new regression source: actual profile fields/types and Result propagation match, validation precedes activity effects, and unauthorized/empty selections fail closed. This is not compilation or runtime proof.

## Prompt follow-up, validator cross-review and PR26 repair

The b942 prompt follow-up addresses all four prior findings. Scoped validator commits f0190e6, d2d9e7e,3bdbf62 and4b1a1d7 have no source-level blocker identified; local dependency APIs/semantics and cfg boundaries inspected. No Cargo/tests/runtime/retrieval. Same-instance annotation cycles, complete cross-draft behavior, arbitrary-precision numbers and Windows execution are not approved by this bounded review.

PR26 repair `2ff21fcf7c141e724978fb385060d579c1cafa9a` preserves every trusted system-role message in provider system fields in order. The reminder boundary consolidates only recognized leading repository reminder prefixes, preserving other content and message/tool roles/order. Anthropic, Gemini both request paths, Antigravity and GCloud ADC use the shared ordered extraction. Source-only idempotence/multi-system/Antigravity conversion cases added, not run. Threads PRRT_kwDOUgxzzc6tprPq, PRRT_kwDOUgxzzc6tprSl. Clean29f8; parent source review/build/integration pending.

## 2026-10-23 22:05 UTC follow-up — /root/review_core

Source HEAD b8a182d71537ac3a11c091af3f08577f4742f7cb in isolated29f8 is clean. Eight core findings and five relay threads have bounded source repairs, mapped in core-triage.json core_followup. Relay033bb4c preserves clean half-close, refuses occupied/stale paths, and caps connections globally at configurable64 with RAII reservation through both pumps and failed dials. Core0d5cb06/57b9b0c/69d921b/a8ffd2e/a3885ae/5c29f40/b8a182d preserve workflow attribution, exclude observer metadata from Incus writes, validate policy replacements, reject incomplete coverage, avoid unchanged route writes, report unsupported rollback, bound evidence search, and preserve absent-vs-empty effective Incus inventory through typed observation.

No Cargo, tests, formatting, Clippy, custom fixtures or runtime operations executed. Source cases are unrun. Parent review/integration/scoped GNU builds and publication remain required; not deployed.

Five false positives were replied to with exact fresh-head source links then resolved: PR14tpptd4364609705/tpptf4364609990; PR16tpp084364610247; PR19tpp_P4364610555; PR20tpqHD4364610799. Full IDs/URLs and source heads are in publication.false_positive_followup. Ten previously resolved false positives were left untouched. PR14tps5P and PR16tpsTL remain unresolved: current owner policy declines restoring embedded import and per-section methods, but published layer source still contains embedded contracts/stale directions. The original blanket false-positive classification is qualified; shared verified capture reconciliation is still required.

Independent bounded source reviews491b407/cb5ebb6/6a1a0f8/3e22b27 found no blocking defect. No runtime/performance/cross-platform or broader lifecycle approval is implied.

## 2026-10-23 22:16 UTC shared capture reconciliation — /root/review_core

Standalone commit `dbe061a43cbeb94b957e63f6b7a395fa9cf1a593`, based on parent integration353d000, is clean on `fix/shared-capture-20261023` in isolated29f8. It uses the exact committed86f recovery capture/helper implementation as reference, removes the embedded upstream document, and explicitly prepares a single verified capture in bridge contract generation and opblob sealing. The checked blob candidate preflight remains before catalog open. Retained native_config, MCP, model_routes, skills and plugins derive that shared capture; runtime schema/state consumers read sealed blobs without lazy discovery. Typed CLI command/flag coverage includes captured aliases. Retired GetConfigSchema is removed while GetProviderCatalog and retained adapters remain.

Maps PR14tps5P and PR16tpsTL to owner-policy reconciliation, pending parent owner-doc correction and publication; it does not restore the retired importer or per-section RPCs. PR21tprHg is directly addressed by removing the stale declaration/capability/dispatch. No thread was resolved before publication of these source changes.

Parent source review found no production blocker. Read-only source searches show only opblob seal and bridge build initialize production capture; derived caches cannot run independent discovery. Source cases now explicitly prepare capture where they author contracts. MCP positive facade cases still need an isolated sealed catalog during a future requested test run. No Cargo, tests, fmt, Clippy, custom fixtures, capture, runtime catalog access, or deployment executed.

Build inputs exist at `/var/lib/cargo-build/odbus-verified/threetched-artifacts/tched-router/{threetched,manifest.json}`. Set THREETCHED_CAPTURE_BIN and THREETCHED_CAPTURE_MANIFEST to those absolute paths and TCHED_ROUTER_CONFIG_DIR=/var/lib/tched-router, preserving default runtime binary/manifest paths. SHA25622e121f5eb1b66352138b714cc3d40d9eb977ed0c13bd6316f693a11a875a75e matches both candidate and installed manifests. The manifests record recovery branding patchd950152; stack source.pin currently records6e35f93. This verifies the pair but does not establish current-stack release provenance. Parent notified; no binary or patch modified.

Required next step is parent integration and scoped GNU build of op-plugins/op-grpc-bridge as jeremy through existing JEV routing and shared cache, then exact-head publication. Full mapping, build environment and limitations are recorded in core-triage.json.capture_reconciliation.

## 2026-10-23 22:24 UTC bounded reader repairs — /root/review_core

Three independent commits follow dbe061a in clean29f8; no capture-file drift:

- `fa283b1e8a9cc1cdd1bf43047c0a28e36c73e36e` fixes PR21tprHj: unrequested standalone ignored child returns before grants/fixture access. A supplied marker still requires existing private-file validation; the parent still verifies the execution sentinel.
- `e1fa947dcb540fc708952075415f22c965c6ac6b` fixes PR15tprRS/tptQv: single and named vector configurations share one untagged typed union. The collection-list name is bound before decoding the upstream detail response, which does not supply a name. Shape source: https://qdrant.tech/documentation/manage-data/vectors/#named-vectors and https://api.qdrant.tech/api-reference/collections/get-collection. Other Qdrant DTO fields, including quantization configuration, are outside this bounded repair and not certified for every backend shape.
- `13f9a8dc80cf33c9153e20a1afded3ba7e88e8ed` fixes PR13tpsAp: history pages exact-scope metadata under the existing shared owner mutex, compares Chrono instants with descending execution-ID ties, then fetches only selected documents. All authored relation writes found use that mutex. Original immutable accepted_at bytes are preserved; no schema migration. Metadata transferred/parsed is O(history), with200-row pages plus a shortlist<=200; final documents<=200. Database query performance was not measured.

Source cases cover Qdrant single/named/empty maps, missing detail name and malformed values; workflow offset equivalence, pre-epoch dates, nanosecond separation, ties, immutable timestamp spelling and205-row cursor traversal. None were executed. No Cargo, tests, formatting, Clippy, custom fixtures or runtime/deployment operations. Parent review, scoped GNU build, integration and fresh-head publication remain required.

Fresh GitHub readback confirmed all five previous false-positive evidence replies still present and their threads resolved: PR14tpptd reply4364609705, PR14tpptf4364609990, PR16tpp084364610247, PR19tpp_P4364610555, PR20tpqHD4364610799. Full readback results are in core-triage.json.publication.false_positive_readback; no duplicate replies or resolutions were sent.

## 2026-10-23 22:35 UTC remaining source repairs and readiness

`6e416dd` rejects incomplete/malformed model-list pagination metadata, preserving the last complete inventory on refresh failure; it does not implement cursor fetching. `eef93f2` aligns Snowball version2.1.0. `f200a6a` requires the configured service UID/GID on non-root retained skill/plugin execution, matching the full threetched runner. `7ee5028` plus `238e745` replaces unbounded namespace process waits/output with the existing bounded group runner, adds nonblocking request input with1MiB bound, and rejects early exit before complete request delivery after cleanup/reap. Output remains16MiB and deadline600s. Source cases are unrun. tpsTP remains partial/open because synchronous dispatch and native control-plane policy remain; caller cancellation was not fixed.

PR12–21 readiness is fully listed in core-triage.json.readiness_followup against integration0db0ef8 plus explicitly separate newer commits. None is independently merge-ready into its immediate predecessor on current evidence. Main remaining groups: PR12 audit privacy and snapshot lifecycle; PR13 migration helpers/schema-hash compatibility/aggregate gallery reads; PR14 source_identity observation and service-layout conflict; PR15 retained native policy/verification gaps; PR16 synchronous native authority and source scope; PR17 undeclared workflow parameter bounds; PR18 direct Workstack/UI proxy admission and provider/clean-plan boundaries; PR19 protected grants reload, credential transactions and identity/storage proofs; PR20 detached ownership, registry retention and catalog drift. New repaired groups are separately recorded, not presented as unresolved again.

PR21 child and retired-method source defects are repaired, but `negative_topology_gates` still needs `crates/op-cognitive/proto/cognitive.proto` from PR25, commit2eec486. The combined stack contains that file; PR21 alone does not. A successful cumulative build must not be reported as standalone PR21 source closure.

Two prior claims are explicitly corrected: tps5V has no production observer computation of source_identity; only omission from native requests was repaired, so the thread remains partial/open. Canonical runit destination/path/interface is present cumulatively from PR29 source layer18 commit690f057, while earlier layer heads lack it; the previous cumulative mismatch claim was stale. Do not resolve those lower-layer dependency threads before their own heads/base include the provider change.

Independent source reviews of Pointer00a36dc and draft-compatibilityac9b6ad found no blocking defect. Pointer cache borrows remain owned by shared Arc/OnceLock, and equality/hash/order/serialization remain encoded-pointer based. additionalItems is restricted to defining drafts; dependencies remains a documented compatibility extension supported by pinned optional corpus inputs. No tests or conformance suite executed. Cycle review is pending its source commit.

## 2026-10-23 22:45 UTC final bounded follow-up

Inventory commit `6f2cbbf7844926e1fc736ff0500d0d31d0178fac` repairs PR13 `tpxzL`. Service/helper/config text is captured once with the existing bounded reader. Each pass starts with the pre-scan graph and prior argument/template bindings; only a stable pass supplies edges, derived nodes and completeness diagnostics. Earlier unresolved arguments therefore cannot leave stale blockers. The scan fails closed at 256 passes, 45000 argument bindings or template destinations, or 64 MiB of captured text. Existing per-file 1 MiB bounds remain. Two source cases cover reverse lexical helper calls, late template destinations, genuinely unresolved arguments and non-convergence; neither was executed. Worktree29f8 is clean. Parent source review, integration, GNU inventory binary build and exact-head publication remain required. This repair does not close the separate tprDT service-dependency traversal finding.

Final independent review of validator `b9ca56c` plus `57c66c` found no additional source-level blocker. Reference traversal preserves resolver/draft/vocabulary, inline traversal matches current filter constructors, and the root filter family remains consistent with generic Self::new through cross-draft references. Current-path identities permit sibling reuse; child-instance edges are excluded. The follow-up rejects malformed2019 recursiveRef, caps active depth256, and avoids copying whole schemas into diagnostics. This is an explicit structural-cycle/depth compile restriction, not arbitrary fixed-point semantics or a broad dynamic-reference conformance claim. Local referencing0.29.1 source and source cases were reviewed; no Cargo/tests/runtime were run.

The final PR12–21 readiness conclusion remains unchanged: no complete lower layer is independently ready into its immediate predecessor on this evidence. The JSON table distinguishes repaired groups, policy/architecture blockers, unexecuted coverage, and later-layer dependencies. In particular, PR21 still needs PR25 cognitive.proto in its actual head/base; cumulative compilation cannot close that lower-layer source dependency. PR14tps5V and PR16tpsTP remain explicitly partial/open. The five false-positive reply/resolution receipts remain confirmed in publication.false_positive_readback. No deployment occurred in this worker.

## Final parent and deployment cross-review

Parent commit `8b482d4b333c6fe240ac764d3c3fec2fc0783101` now repairs the separate PR13tprDT consumer dependency closure. Source review found no blocker: crossing a declared release-owned dependency propagates files and blockers with own_lifecycle=false; direct file traversal retains true. Visiting (id,flag) pairs preserves direct intent even after an indirect visit, and terminates service cycles. Direct deps/starts stay unchanged for the downstream planner. This supersedes the previous paragraph calling tprDT an unresolved source defect; build and publication remain parent-owned.

Deployment commit `2837d0cd07903b4b45b588cce5f305952ee98a0b` has no blocker in its narrow checked-ioctl ENOENT handling. The restored kernel patch hashes exactly to pinned a684f8c19b92a73e1dec7f88563413daf0799e2ff3957b1de494de3d953d91a2. Its wrapper routes initial ENOENT through the absent-generation census after input/FSID checks; linked, mismatched and busy generations fail, and post-unlink errors become EIO. The private Rust decoder normalizes only return -1/ENOENT, preserving other errors and rejecting unexpected returns. Cleaner synchronization and recorded-name/journal reconciliation still follow both success outcomes. This is source-contract review, not proof of running ABI support, kernel concurrency safety, deployment or runtime behavior. tpujj remains open; no Cargo/tests/runtime were run.

## Qdrant build failure follow-up

Parent build BUILDFAIL02e3 exposed an integration failure in the earlier named-vector repair: finite Map cannot retain the vector record constraints, so bridge schema generation correctly rejected it. Commit `5d4f7ec68827862308504d964c2d815b390da2c3`, directly based on frozen integration `b91ac1406efe3e707f201d26050cf33dc5ec39ab`, annotates only QdrantCollectionParams.vectors for the existing exact-schema field representation. The authored Single/Named anyOf and record constraints remain the single source of truth; no guard or validation was weakened and no wire normalization was introduced.

Unrun source coverage calls production schema generation, compares both adapters, checks retained definitions/uint64 format, and checks rebased export plus whole-state exact validation for single, named, empty and malformed vector values. Clean branch fix/qdrant-union-build-20261023 in29f8. Parent source review and op-plugins/op-grpc-bridge GNU rebuild with the existing capture environment remain necessary. No Cargo, tests, formatting, Clippy, fixtures or deployment ran in this worker. PR15tprRS/tptQv source mapping now includes this build follow-up; prior source review was not a compilation pass.

## Fresh relay and PR16 findings

PR28vy8BO is a confirmed normal-restart defect. `mail-port-fabric` execs the relay with outbound-smtp.sock, while the prior process had no graceful shutdown cleanup. Commit `587829d615fd7f77b08cf3a1a493092cf5512e62`, based on2cc653d, installs SIGTERM/SIGINT handlers that only store an atomic flag. Main stops and joins nonblocking listener workers before exit. Their owned UnixEndpoint closes its listening descriptor before removing a matching socket device/inode. Startup rollback uses the same ownership guard. Existing paths remain refused, and detected replacements remain untouched.

This is limited to the supported graceful lifecycle under a trusted service-controlled directory. POSIX has no atomic compare-and-unlink, so the inode guard is not protection against a directory-renaming adversary. No directory permissions or grants were changed. Crash/SIGKILL leftovers and cleanup I/O errors require explicit operator cleanup. Active connection workers terminate with the process as before; no idle timeout was introduced. Stop/restart, replacement and partial-startup source cases were added but not run. Parent review, isolated GNU relay build and publication remain pending; no Cargo/tests/runtime/deployment in this worker.

Fresh PR16vy86W at16eb8193 mixes expected retirement with a real lower-layer mismatch: the old bridge handler and backing test remain there after GetConfigSchema is removed. Restoring that method contradicts owner policy. Current2cc653d contains only retirement comments in Rust/proto, so classify as authorized retirement plus later-layer cleanup dependency and keep open until the actual lower head/base contains that cleanup. PR16vy87I is likewise a real lower-head build dependency fixed in the cumulative stack: build.rs44 explicitly prepares the shared capture before collecting schemas. Do not resolve it solely from a cumulative build.

Independent source review of validator1aac993 found no blocker in its4096-visit annotation preflight budget. Recursive and inline branches, including boolean leaves, consume the same counter without context-insensitive memoization. Documentation explicitly allows rejecting large valid DAGs and does not claim a global compilation-time bound. Source cases were reviewed, not executed.

## Atomic checkpoint directory preflight

PR25vzITN is repaired in `87b19a8198e7e05115a561d7af1dbcae831904da`, based on0465337 with only chain_vectors.rs changed. The existing checkpoint branch retains its nontruncating write-open and now checks the canonical parent before initializing the chain/Qdrant index. A single blocking task opens that directory, exclusively creates a unique mode0600 sibling, closes/removes it, and syncs the parent. Any failure aborts initialization; failure to remove the probe reports its retained path. The existing checkpoint is never rewritten or replaced by preflight, including corrupt or foreign contents, and existing root/UUID validation is unchanged.

No directory permissions or ownership were altered. The task keeps create/cleanup together despite async cancellation; operator changes can still revoke directory access after preflight. Source cases for preserved checkpoint bytes and unusable parent paths were added but not run. No Cargo/tests/probes/runtime were executed. Clean29f8; independent app/root review, scoped build and exact-head publication are pending.
