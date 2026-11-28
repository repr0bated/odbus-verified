# Implementation Plan: cockpit-container

## Overview

This plan implements the cockpit-specific integration for the existing main 3tched control-plane cockpit `3tchedCockpitBot` (owner rename from `CockpitBot3tched`, 2026-11-09). It preserves the existing main identity, session, Native_UUID, grants, credentials, storage, mounted views, and host boundary. It does not create a cockpit, add 3tchedW3 or Web3 work, perform subsidiary LXD work, or add unrelated hackathon functionality.

Implementation uses Rust for all repository-owned cockpit code (identity binding, registry, evidence pipeline, execution, credential safety, and authorization). TypeScript is reserved exclusively for the GUI in `/srv/git/3tched.com`. Every task that can cause a Live_Change remains proposed until an exact Owner_Authorization is present.

## Execution contract and deliverables

- This is a proposed implementation DAG, not authorization to run it. All checkboxes remain unchecked until their own deliverables exist. Source completion never implies built, tested, deployed, or cutover-complete status.
- Start with read-only source/runtime discovery. Do not modify requirements, design, `.config.kiro`, or any `cockpit-lxd-standalone` file under this plan. Requirements references below are acceptance-criterion IDs in this cockpit's `requirements.md`.
- The authoritative Chat_Interface specification is `.kiro/specs/identity-chat-interface/` here. Its generic Rust server/core lives in `crates/op-chat-interface`; only browser Chat_UI/Canvas is TypeScript/Svelte. Cockpit-specific binding, registry, evidence, execution and authorization live under `crates/op-grpc-bridge/src/cockpit/`. The combined owner integrates both without duplicating generic chat components or adapting legacy chat.
- Evidence deliverables belong under `records/cockpit-container/`: baseline, runtime, blockers, status, legacy inventory, removal manifest, contract snapshots, authorizations, and action plans. Reuse existing record formats/writers where compatible. Source discovery procedures and command plans are development tooling, not product transports.
  - Evidence deliverables (tests, fixtures, property checks) belong under `crates/op-grpc-bridge/tests/` and `tests/` in this repository. Synthetic fixtures are test inputs only, never product fallback data.
- Each leaf task is complete only when its named component/procedure and evidence deliverables satisfy every listed criterion. Implementation leaves corresponding verification stages pending until section 14; a blocked task records the missing prerequisite and dependent IDs, with no stub, partial substitute, or guessed contract.
- `Depends on` lists are authoritative; the waves at the end are their topological ordering. Tasks in the same wave are only eligible for parallel execution when they do not edit the same file. Use one owner per shared file and respect the other repositories' handoff rules.
- Product tests, review, lint, type checking, build verification, and smoke execution remain post-deployment and explicitly requested, as specified by this design. Read-only discovery and validation of this planning document are not product verification. An applied deployment may remain `deployment-unconfirmed`; that status must not prevent the checks needed to confirm it.
- Owner architecture clarification (2026-11-09): the Rust Chat_Route runs on the VPS host, using host Mounted_Views bound to the existing main identity/session. Browser UI and server share the private `10.0.0.2` origin over WireGuard via the decoy, not Door_8090/Tonic. Both Vercel SDKs remain browser-side; this is not Vercel hosting. Preserve Tonic's independent backend duties and the cockpit guest's lungs-only boundary. B-33, B-30, D-47 and identity-chat Task 24 remain reserved; do not choose their contracts or implement dependent work.

## Tasks

- [ ] 1. Establish evidence-record foundations and read-only discovery
  - [ ] 1.1 Implement typed evidence record schemas and secret-safe writers
    - Create complete Rust models and validators for `Baseline_Record`, `Runtime_Record`, `Blocker_Record`, `Legacy_Chat_Inventory`, `Owner_Authorization`, and `Status_Report` under the cockpit integration record layer.
    - Implement atomic replacement for singleton records, append-only blocker writing, credential-class redaction, and independent proposed/implemented/built/tested/deployed stage calculation.
    - Reject records containing Credential_Material or digests computed from Credential_Material.
    - **Depends on:** none.
    - _Requirements: 2.4, 18.1, 17.7, 17.8, 18.1, 18.4, 18.6, 22.1, 22.7, 22.8_

  - [ ] 1.2 Implement the pinned-source baseline collector and operation-path tracer
    - Add a read-only collector that records accepted 40-character commit pins, paths, symbol signatures, file digests, callers, and source-only/not-found classifications for every pinned interface.
    - Trace 3tched and threetched Operation_File writes separately through 3tched-fs, PluginV1/lungs, backend dispatch, Attempt_Record creation, terminal recording, Receipt construction, and same-file return.
    - Stop each trace at its first unresolved transition, record the last proven symbol, and mark dependent view work deployment-unconfirmed without modifying the pinned dependency components in `/srv/git/odbus-verified`.
    - **Depends on:** 1.1.
    - _Requirements: 2.1, 2.2, 2.3, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9, 8.10, 8.11, 8.12, 8.13, 23.2, 23.3, 23.4_

  - [ ] 1.3 Implement the read-only cockpit runtime collector
    - Collect cockpit name, Main_Session, Native_UUID, sealed principal, grants identity, Seed, Leaf, Btrfs members, exactly the Seven_Subvolumes, host channels, listeners, mount paths, distinct kernel mount sources, owning processes, both catalog pins, and Guest_Kernel_Identity observations.
    - Classify mounted projection, committed-SHM projection, guest identity, tracker evidence, Receipt, and Authoritative_State as distinct evidence classes; never infer completion from a mount, state read, or successful filesystem write.
    - Use passive read-only mounted projections and supported observation interfaces at this stage; record missing evidence rather than invoking an unestablished operation path. Add no login, lifecycle, mount, SHM, D-Bus, backend, or container-runtime mutation path.
    - **Depends on:** 1.1.
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9, 2.12, 4.1, 4.2, 4.3, 4.13, 10.1, 10.2, 10.3, 14.1, 14.6, 22.8_

  - [ ] 1.4 Run the collectors and write the initial Baseline_Record, Runtime_Record, blockers, and stage report
    - Execute only the read-only collectors from 1.2 and 1.3, with no build, test, review, lint, type-check, smoke, reseal, deployment, restart, mount, or lifecycle command.
    - Record observed evidence, source-only evidence, and not-found evidence without upgrading any deployment status.
    - Leave every dependent task proposed when a required source transition, mounted view, identity binding, or catalog pin cannot be established.
    - **Depends on:** 1.2, 1.3.
    - _Requirements: 1.6, 1.7, 1.8, 2.4, 2.11, 8.11, 8.12, 22.1, 22.6, 22.7, 22.8_

  - [ ] 1.5 Resolve source-contract and integration prerequisites before dependent implementation
    - Record the exact catalog-declared call paths and same-handle protocol, capability source, guest describe interface, authenticated attribution fields, hash verification contract, tracker/receipt read operations, and method-specific authoritative-state reads for each view. No invented field, capability, method, path, or grant is allowed.
    - Resolve the bootstrap ordering: observe both views and kernel attribution, derive only the granted descriptors needed for the tracked identity read, execute it with full evidence, then expose the session/tools. Configuration supplies expectations, never caller authority.
    - Record the operation-ID/confirmation contract explicitly: Requirement 16.2 binds confirmation to `operation_id`, while design D-05 creates that ID during FUSE admission and D-08 permits a pre-admission request ID. Require a source-proven reservation/binding contract satisfying the requirement; otherwise block mutation implementation and return the conflict for requirements/design clarification. Never substitute a UI/request ID or modify pinned dispatch to manufacture compliance.
    - Record how finite supporting tracker, receipt-recovery, and state reads retain their own authenticated attempt/terminal/Receipt evidence without recursively calling the subject-completion pipeline. Resolve any conflict between D-05's supporting-read rule and Requirement 8.4 before implementing dependent completion behavior.
    - Confirm the identity-chat-interface boundary supplies the needed integration contracts and an existing browser/provider delivery path within the lungs-only boundary. Missing chat functionality remains owned by its spec; this plan adds no listener or alternate channel.
    - Record how deployment artifacts are produced under the prescribed deferred-verification policy, separately from build activation and post-deployment build verification. If no approved existing artifact-production path exists, block deployment rather than install missing or unverified substitute artifacts.
    - **Done when:** the baseline names supported contracts and an executable bootstrap/evidence sequence; every unresolved discrepancy has a blocker and the exact dependent tasks remain proposed. Recording a blocker does not release those tasks.
    - **Depends on:** 1.4.
    - _Requirements: 2.1, 2.2, 3.1, 3.9, 3.10, 5.1, 5.2, 6.4, 6.6, 8.9, 8.11, 8.12, 8.4, 11.1, 14.1, 14.4, 16.2, 19.2, 22.7, 23.2, 23.9_

- [ ] 2. Implement exact cockpit identity binding and preservation guards
  - [ ] 2.1 Implement exact existing-cockpit target selection
    - Accept only the observed `3tchedCockpitBot` binding whose session is `b0c777e3-0955-cd8b-3d01-ca12998cae28` and whose principal and Native_UUID match the initial Runtime_Record.
    - Produce a Blocker_Record and no dependent action for partial matches, cross-matches, another container, or any attempt to create, clone, rename, reimage, rebuild, reseed, repurpose, or duplicate the cockpit.
    - Keep the expected binding in cockpit-owned configuration/evidence, not in identity-neutral chat source.
    - **Depends on:** 1.5.
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 1.7, 1.8, 10.12_

  - [ ] 2.2 Implement CockpitIdentityBinder over the tracked 3tched Operation_File path
    - Bind session start only from `identity_sled.get_own_identity` in `/run/mounts/3tchedFS`, using Guest_Kernel_Identity and no caller-supplied SID1 path or session override.
    - Require full principal and Main_Session equality before exposing the fresh identity-neutral Chat_Interface or any tool.
    - Require the identity read itself to pass the complete tracker, Receipt, and finite Authoritative_State pipeline before the chat session starts.
    - **Depends on:** 2.1, 4.6.
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.9, 3.10, 5.1, 6.1, 8.1, 8.2, 8.3, 8.4, 11.1, 11.2, 18.8, 21.3_

  - [ ] 2.3 Implement identity rechecks and cross-identity refusal
    - Recheck the bound identity and unchanged Catalog_Pin before every confirmed mutation.
    - Refuse any request that targets another identity, session, principal, credential, or cockpit and preserve all unrelated pending calls.
    - Emit a secret-free blocker and end dependent execution on any attribution mismatch.
    - **Depends on:** 2.2, 5.1.
    - _Requirements: 3.4, 3.6, 3.7, 3.8, 6.13, 16.2, 16.3, 16.6, 16.7, 17.4, 18.1, 18.8_

- [ ] 3. Implement the strict dual-view catalog and grant registry
  - [ ] 3.1 Implement mounted catalog and capability readers for both simultaneous views
    - Read plugin, method, argument schema, result schema, Required_Capability, side effect, `subid`, call path, catalog hash, and generation from both mounted views by local file I/O.
    - Treat 3tched-fs values as projections of committed op-dbus SHM and provide no direct SHM open/read/write API.
    - Require equal catalog hash/generation and distinct kernel mount sources while retaining per-view availability evidence.
    - **Depends on:** 1.5, 2.1.
    - _Requirements: 2.5, 2.6, 2.7, 2.8, 4.1, 4.2, 4.3, 4.6, 4.13, 5.5, 13.4_

  - [ ] 3.2 Implement exact runtime-owner classification and registry derivation
    - Classify operations by what runs them: 3tched code uses the 3tched view and `/usr/bin/threetched` operations use the threetched view.
    - Offer exactly the uniquely exposed, correctly assigned methods whose Required_Capability is held; treat missing or unrecognized side effects as mutation.
    - Refuse duplicate-view, wrong-view, absent, and forbidden threetched-name entries and record blockers without routing around defects.
    - **Depends on:** 3.1.
    - _Requirements: 3.5, 4.4, 4.5, 4.6, 4.7, 4.8, 4.11, 4.12, 12.2, 13.1, 13.3, 13.4, 13.6, 13.8_

  - [ ] 3.3 Implement catalog-pin invalidation and threetched upgrade isolation
    - Re-read both catalog pins immediately before model exposure, confirmation release, and operation dispatch.
    - Invalidate stale descriptors and pending confirmations and require full dual-view re-derivation when either pin changes.
    - Preserve unchanged 3tched method definitions, schemas, capabilities, and routing when only threetched declarations change; refresh their shared Catalog_Pin without treating pin metadata as a method-definition change.
    - Implement invalidation as a registry/gate contract, independent of the fully wired Chat_Interface; wire session cleanup later in 6.1 and 6.2.
    - **Depends on:** 3.2.
    - _Requirements: 4.9, 13.7, 16.7_

- [ ] 4. Implement the only permitted operation transport and completion evidence pipeline
  - [ ] 4.1 Implement root-confined same-file Operation_File invocation
    - Resolve only the catalog-declared call path beneath the pinned Matching_View root, open without following escaping links, write canonical validated input, fsync, seek, and read the result and Receipt from the same file descriptor.
    - Reject missing, unreadable, schema-invalid, traversal, encoded-separator, symlink-escape, and root-escape paths before writing.
    - Include no direct D-Bus, op-dbus client, SHM, backend-store, raw container-runtime, alternate transport, cross-view fallback, or product MCP path.
    - **Depends on:** 3.2.
    - _Requirements: 4.4, 4.5, 4.10, 5.1, 5.2, 5.5, 5.6, 5.7, 5.8, 14.5, 15.1, 15.2, 15.3, 15.4, 15.5, 15.6_

  - [ ] 4.2 Implement same-pin argument and result schema validation
    - Validate canonical arguments against the descriptor argument schema before file open and validate terminal results against the result schema from the unchanged same Catalog_Pin.
    - Return typed, secret-safe failures without confirmation or dispatch when input is invalid and incomplete evidence when a returned result is invalid.
    - **Depends on:** 3.3, 4.1.
    - _Requirements: 5.3, 5.4, 16.1, 18.1, 18.3, 18.5_

  - [ ] 4.3 Implement tracker evidence reading through granted Operation_Files
    - Read tracker records only through a granted operation in the original Matching_View and only for the subject `operation_id`.
    - Normalize only source-proven fields for caller identity/session, plugin, method, `subid`, Catalog_Pin, outcome, event references, and hash-chain linkage; never synthesize missing attribution.
    - Require one Attempt_Record before terminal outcome and exactly one Terminal_Record.
    - **Depends on:** 1.5, 4.2.
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.11, 6.12, 8.9, 8.1, 8.2_

  - [ ] 4.4 Implement fail-closed TrackerEvidenceEvaluator
    - Require exact attribution equality among the subject Tool_Call, Attempt_Record, Terminal_Record, and Receipt for `operation_id`, caller identity/session, plugin, method, `subid`, and Catalog_Pin; compare outcome only between the Terminal_Record and Receipt, since an attempt has no terminal outcome.
    - Validate attempt/terminal ordering, record hashes, preceding-record hashes, and Receipt attempt/terminal event references.
    - Return `incomplete` plus a Blocker_Record for missing, duplicate, invalid, or mismatched evidence; accept no local log, UI state, write success, process status, state projection, or model output as a substitute.
    - **Depends on:** 4.3.
    - _Requirements: 2.9, 2.10, 2.11, 6.7, 6.8, 6.9, 6.10, 6.13, 6.14, 6.16, 8.1, 8.2, 8.3, 8.6, 8.7, 8.8, 8.11_

  - [ ] 4.5 Implement finite post-terminal Authoritative_State reading
    - Resolve a granted owner-state Read_Tool_Call from the same catalog and Matching_View and execute it through the full tracked Operation_File path after the subject terminal record.
    - Carry the supporting read's own operation ID, Attempt_Record, exactly one Terminal_Record, Receipt, and observed state, correlated to `subjectOperationId`. Verify its own attribution tuple rather than forcing its operation ID/method to equal the subject's. Use the contract resolved in 1.5; never recurse into subject completion or independently report this supporting read as a completed operator call.
    - Keep the subject incomplete when no granted read exists, the read evidence is incomplete, or state does not reflect the subject terminal outcome.
    - **Depends on:** 4.4.
    - _Requirements: 2.10, 6.1, 8.4, 8.5, 8.9, 8.10, 12.4, 20.7_

  - [ ] 4.6 Implement one TrackedToolExecutor pipeline for every read and mutation
    - Enforce `Operation_File -> 3tched-fs -> PluginV1/lungs -> backend -> Attempt_Record -> exactly one Terminal_Record -> Receipt -> matching Authoritative_State` for every Tool_Call in either view.
    - Report complete only after 4.4 and 4.5 succeed; distinguish model output, UI state, tracker evidence, Receipt, and Authoritative_State in the returned status model.
    - Never dispatch via another view, identity, transport, direct backend, SHM, container runtime, or local substitute after any result.
    - **Depends on:** 3.3, 4.2, 4.5, 5.1, 5.4.
    - _Requirements: 4.10, 5.1, 5.2, 5.6, 6.1, 6.2, 6.15, 8.1, 8.2, 8.3, 8.4, 8.5, 8.11, 8.12, 12.3, 12.4, 13.5, 13.6, 20.5, 20.6, 20.7_

- [ ] 5. Implement mutation, denial, unknown-outcome, and credential safety
  - [ ] 5.1 Implement server-side single-use mutation confirmation
    - Hold validated canonical bytes with the source-proven `operation_id` binding resolved in 1.5, principal, Main_Session, plugin, method, `subid`, non-secret input digest, and Catalog_Pin. Missing reservation/binding support blocks this task; no request-ID substitution is permitted.
    - Accept only an exact, explicit operator approve event; atomically consume one confirmation for one call and reject model text, tool output, message text, filesystem state, mismatches, expiry, replay, and stale pins.
    - Execute the held bytes rather than client-returned bytes and preserve unrelated pending calls.
    - **Depends on:** 2.1, 4.2.
    - _Requirements: 16.1, 16.2, 16.3, 16.4, 16.5, 16.6, 16.7, 16.8_

  - [ ] 5.2 Implement sticky denial handling and blocker recording
    - Stop a denied Tool_Call, mark its tool unavailable for the rest of the chat session, reject later requests without writes, and preserve unrelated pending calls unchanged.
    - Record operation attribution, Catalog_Pin, missing capability or policy, redacted error, and timestamp without credential bytes or credential-derived digests.
    - Make no retry or fallback through another view, file, transport, identity, direct store, raw container-runtime command, or local substitute.
    - **Depends on:** 1.1, 4.6, 5.1.
    - _Requirements: 4.10, 18.1, 18.2, 17.3, 17.4, 17.5, 17.6, 17.7, 17.8_

  - [ ] 5.3 Implement finite Unknown_Outcome reconciliation without subject replay
    - On absent or unverifiable terminal evidence, preserve the original `operation_id` and Matching_View and issue only granted tracker, declared receipt-recovery, and authoritative-state reads.
    - Classify from the Terminal_Record only when one attempt, exactly one terminal, one matching Receipt, and matching state are established.
    - Otherwise return incomplete, write a blocker, and send zero repeat subject writes.
    - **Depends on:** 4.6, 5.2.
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7_

  - [ ] 5.4 Integrate fail-closed credential checks at every cockpit output boundary
    - Apply the fail-closed credential check rules to model context, logs, errors, telemetry, UI output, persisted state, evidence projections, runtime records, blockers, and streamed chunks.
    - Withhold complete items on detection, checker error, or timeout; record only credential class, blocked channel, main identity display name, and timestamp.
    - Provide a repository scanner that reports credential classes and locations without matched values.
    - **Depends on:** 1.1.
    - _Requirements: 18.1, 18.2, 18.3, 18.4, 18.5, 18.6, 18.7_

- [ ] 6. Wire the fresh identity-neutral Chat_Interface into the cockpit
  - [ ] 6.1 Implement cockpit-specific glue around identity-neutral chat components
    - Connect the existing fresh Chat_Interface to CockpitIdentityBinder, DualViewRegistry, confirmation gate, TrackedToolExecutor, evidence recorder, and status reporter without importing, copying, wrapping, calling, or adapting legacy chat code.
    - Keep generic chat source free of cockpit-specific principal IDs, sessions, grants, capabilities, credentials, and plugin allowlists.
    - Expose no MCP, direct D-Bus/SHM/backend/container-runtime call, alternate transport, extra host channel, listener, or inbound network door.
    - **Depends on:** 2.3, 3.3, 4.6, 5.1, 5.2, 5.3, 5.4.
    - _Requirements: 3.5, 5.5, 11.1, 11.2, 11.3, 14.1, 14.2, 14.3, 14.4, 14.5, 15.1, 15.2, 15.3, 15.4, 15.5, 15.6, 18.1, 18.2_

  - [ ] 6.2 Implement session termination, confirmation cleanup, and evidence-stage UI status
    - End or refuse a session on identity mismatch, catalog mismatch that cannot be re-derived, missing view, or binding denial; discard only that session's confirmations.
    - Render distinct labels for model output, local UI state, Attempt_Record, Terminal_Record, Receipt, Authoritative_State, denial, unknown, incomplete, and complete.
    - Derive status from evidence only and keep proposed, implemented, built, tested, and deployed stages independent.
    - **Depends on:** 6.1.
    - _Requirements: 2.8, 2.9, 2.10, 2.11, 8.12, 16.7, 17.3, 17.4, 22.1, 22.2, 22.5, 22.6, 22.8_

- [ ] 7. Implement storage and image provisioning
  - [ ] 7.1 Implement 3-image pre-rootfs layout specification
    - Document the 2-partition GPT image (p1: Btrfs rootfs with `btrfstune -S 1`, p2: sparse ext4 with `chattr +C`), sparse Btrfs image for read/write rootfs sprout via `btrfs device add` (no loop), and upgrade overlay Btrfs (only `@` subvolume).
    - Record that the identity sled is a separate `content_type: filesystem` Btrfs subvolume on the `3tched` pool.
    - Images live on the same partition where rootfs resides; no loop devices used.
    - **Depends on:** none.
    - _Requirements: 10.1, 10.2, 10.3_

  - [ ] 7.2 Implement image file management procedures
    - Implement image rename procedures (CockpitBot3tched.img → 3tchedCockpitBot.img, etc.) and validation that images are present before container recreation.
    - Include no code that creates/reimages/reseeds the Cockpit; container recreation is a separate Live_Change requiring Owner_Authorization.
    - **Depends on:** 6.1.
    - _Requirements: 1.5, 1.8, 10.12_
- [ ] 8. Implement storage and lifecycle preservation
  - [ ] 8.1 Implement tracked lifecycle snapshot reads and exact comparison
    - Read Seed identity, Leaf identity, Btrfs member set, Seven_Subvolumes, Identity_Binding, Native_UUID, Grant_Set identity, credential identity, host channels, and listeners before and after each authorized lifecycle operation.
    - Require exact equality for completion and write an incomplete outcome plus blocker on any difference.
    - Use only granted tracked reads through Matching_View Operation_Files; expose no direct Btrfs, SHM, D-Bus, or container-runtime mutation API.
    - **Depends on:** 2.2, 4.6.
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.11, 14.7, 14.8_

  - [ ] 8.2 Implement the authorization-gated existing-cockpit lifecycle procedure
    - Accept only exact Owner_Authorization for the named lifecycle action and existing cockpit binding.
    - Cover login, start, stop, restart, logout, recovery, and failed-start cleanup only through the existing source-traced lifecycle contracts. Start includes the tracked `identity_sled.touch_session` operation; it is not assumed to replace the entire runtime start procedure. A missing lifecycle interface blocks that action, never authorizes a raw runtime command.
    - Include no code that adds/removes Btrfs members, creates/deletes/renames/replaces subvolumes, recreates the cockpit, or repairs by reimage/reseed.
    - **Depends on:** 8.1, 10.1.
    - _Requirements: 10.7, 10.8, 10.9, 10.10, 10.12, 19.1, 19.2, 19.3, 19.5, 19.7_

- [ ] 9. Inventory clean-slate chat code and preserve non-chat boundaries
  - [ ] 9.1 Implement complete legacy chat candidate discovery and caller tracing
    - Discover source, module/build membership, registration, sealed schema, generated code, service, route, UI, tests, fixtures, current documentation, and callers in the relevant repositories.
    - Record path, symbol, accepted pinned commit, callers, and evidence without modifying pinned source or live state.
    - Include `ctl_plane_chatbot` coupling and `/usr/bin/threetched`-backed services as candidates requiring classification rather than assuming their disposition from names.
    - **Depends on:** 1.2.
    - _Requirements: 11.4, 11.5, 11.7, 12.1, 13.1, 13.2, 13.8, 21.1_

  - [ ] 9.2 Implement Jev-assisted inventory classification and anomaly recording
    - Run the credential precheck, send only non-secret source/text, and ask Jev to classify each candidate as `legacy_chat_remove`, `ctl_plane_chatbot_keep_and_decouple`, `threetched_service_keep`, `identity_grant_storage_lifecycle_keep`, or `unclear_owner_decision_required`.
    - Record question, model, answer, probabilities, confidence, and judged source paths; treat confidence below 0.6 as inconclusive and confirm every disposition from source evidence.
    - Preserve unclear items and create a blocker rather than writing a stub, placeholder, compatibility shim, or partial removal.
    - **Depends on:** 8.1.
    - _Requirements: 11.4, 11.7, 13.1, 13.2, 13.8, 17.7, 18.1_

  - [ ] 9.3 Write the final Legacy_Chat_Inventory and removal manifest
    - Produce an exhaustive, secret-free inventory and a machine-readable removal manifest containing only source-confirmed `legacy_chat_remove` items.
    - Exclude the main identity, session, grants, credentials, storage, lifecycle bindings, `ctl_plane_chatbot`, and threetched services from removal.
    - Record forbidden historical threetched names as defects and leave affected operations unavailable until separately authorized correction.
    - **Depends on:** 8.2.
    - _Requirements: 11.4, 11.7, 12.1, 13.1, 13.2, 13.8, 17.7, 18.1_

- [ ] 10. Retain and decouple ctl_plane_chatbot behind a dedicated authorization
  - [ ] 10.1 Implement the ctl_plane_chatbot contract snapshot and diff gate
    - Snapshot every plugin method, argument/result field, record field, side effect, capability, `subid`, registration, and sealed schema entry before source modification.
    - Accept an after-contract only when changes are limited to removing threetched-service type, constant, size-limit, and plugin-identifier coupling.
    - Block every unrelated contract difference before build activation or reseal.
    - **Depends on:** 1.2, 8.3.
    - _Requirements: 12.1, 12.5, 12.6, 12.9_

  - [ ] 10.2 Decouple ctl_plane_chatbot from threetched-service internals after exact Owner_Authorization
    - Modify only the source files and symbols named by a matching authorization for pinned-source decoupling; reject an expired authorization when it carries an expiry.
    - Define complete plugin-owned or neutral 3tched types and limits, preserve all unrelated methods and fields, and leave no compatibility shim, stub, placeholder, dummy data, or partial implementation.
    - Keep the plugin registered as 3tched code and do not alter identity, grants, storage, lifecycle, tracker, 3tched-fs, or threetched internals.
    - If authorization is absent, mismatched, or expired, write the proposed status/blocker and make no pinned-source change. Before changing files, claim ownership under the shared-repository handoff rules and ensure no overlapping build, seal, or activation is running.
    - **Depends on:** 9.1, 10.1.
    - _Requirements: 12.1, 12.5, 12.6, 12.9, 19.1, 19.2, 19.3, 19.4, 19.5, 23.1, 23.2_

  - [ ] 10.3 Integrate retained ctl_plane_chatbot through the generic registry and executor
    - Discover the retained plugin from the 3tched-view sealed catalog and expose it without a plugin-specific transport or bypass.
    - Route episodes, knowledge, context, significance, config, and every other retained method through capability, schema, confirmation, tracker, Receipt, and Authoritative_State gates.
    - Keep the task proposed if the catalog contract, grant, tracker path, or owner-state read cannot be established.
    - **Depends on:** 6.1, 9.2.
    - _Requirements: 12.2, 12.3, 12.4, 12.7, 12.8_

- [ ] 11. Implement exact Owner_Authorization gates for every Live_Change
  - [ ] 11.1 Implement non-transitive authorization validation and proposed-state blocking
    - Validate action, target main identity, `3tchedCockpitBot`, session, affected files/services, mounts, expected Operation_Files and actual arguments, issue time, and optional expiry against the actual intent.
    - Model legacy removal, pinned-source decoupling, catalog reseal, build activation, deployment, service restart, mount change, and chat cutover as separate actions whose authorization cannot transfer.
    - On absence or mismatch, write proposed/blocked status and perform no live action; never interpret a Denial as authority.
    - **Depends on:** 1.1, 2.1.
    - _Requirements: 11.10, 19.1, 19.2, 19.3, 19.4, 19.5, 19.6, 19.7_

  - [ ] 11.2 Implement authorization-aware command plans for reseal, deployment, restart, and cutover
    - Create complete, reversible command-plan generation that binds each planned command to exactly one validated authorization and expected target/evidence set.
    - Encode the order `authorized source changes -> authorized catalog reseal -> authorized build activation/deployment -> authorized service restart -> authorized cutover` without executing any live command as part of task implementation.
    - Refuse target drift and provide no direct container-runtime, alternate transport, grant change, storage change, or unauthorized mount action.
    - **Depends on:** 10.1.
    - _Requirements: 11.10, 19.1, 19.2, 19.3, 19.4, 19.5, 21.2, 23.1_

- [ ] 12. Remove confirmed legacy chat code behind its dedicated authorization
  - [ ] 12.1 Apply the authorized removal manifest completely
    - After exact legacy-removal authorization and shared-repository ownership are established, remove every manifest item across source, module/build membership, plugin registration, sealed schema source, generated output, services, routes, UI, tests, fixtures, and current documentation.
    - Leave no unreachable remnant or compatibility shim and preserve all inventory items classified as identity/grant/storage/lifecycle, `ctl_plane_chatbot`, threetched service, or unclear.
    - If authorization, ownership, or source evidence is missing, leave removal proposed and make no partial change.
    - **Depends on:** 8.3, 9.2, 10.1.
    - _Requirements: 11.3, 11.5, 11.6, 11.7, 11.10, 13.1, 13.2, 19.1, 19.2, 19.3, 19.4, 23.1, 23.2_

  - [ ] 12.2 Implement the no-remnant and preserved-boundary static verifier
    - Check removed symbols, paths, plugin IDs, service names, registrations, generated code, routes, tests, fixtures, and current documentation for active references.
    - Check that the main identity binding, `ctl_plane_chatbot`, and threetched services remain and that fresh chat depends on no threetched internal type, constant, store, protocol, or source module.
    - Produce evidence records without running the verifier before deployment.
    - **Depends on:** 11.1.
    - _Requirements: 11.5, 11.6, 11.7, 13.1, 13.2, 13.3, 13.8, 21.1_

- [ ] 13. Author complete automated tests without executing them
  - [ ] 13.1 Write property tests for cockpit selection, evidence classification, identity binding, registry, routing, and catalog invalidation
    - Add exactly one fast-check property per design Property 1 through Property 9, each carrying the required feature/property annotation and using only synthetic non-secret generators.
    - Cover exact cockpit matching, evidence non-escalation, catalog-and-grant intersection, strict no-fallback routing, threetched isolation, stale-pin refusal, path confinement, and same-pin schemas.
    - Do not run the tests before authorized deployment.
    - **Depends on:** 2.3, 3.3, 4.2.
    - **Properties 1-9: Cockpit target selection through schema and pin consistency**
    - **Validates: Requirements 1.6-1.8, 2.4, 2.6, 2.8, 2.9, 2.11, 3.2-3.5, 4.4-4.10, 5.3-5.8, 8.10-8.11, 13.4-13.7, 16.1, 16.7, 20.9-20.10, 22.8**

  - [ ] 13.2 Write property tests for tracker equivalence, linkage, finite state read-back, and unknown outcomes
    - Add exactly one fast-check property per design Property 10 through Property 13.
    - Generate missing, duplicate, reordered, and mismatched attempt/terminal/Receipt/state tuples and assert complete only for exact evidence equivalence.
    - Assert reconciliation performs zero subject replays and zero cross-view or alternate-transport calls.
    - Do not run the tests before authorized deployment.
    - **Depends on:** 4.6, 5.3.
    - **Properties 10-13: Completion equivalence through no-replay reconciliation**
    - **Validates: Requirements 2.10, 6.1-6.16, 8.1-8.9, 9.1-9.7, 12.4, 16.8, 20.7**

  - [ ] 13.3 Write property tests for confirmation, denial, credential safety, and authorization
    - Add exactly one fast-check property per design Property 14 through Property 19.
    - Cover concurrent confirmations, sticky denial without unrelated-call mutation, cross-chunk synthetic credential detection, fail-closed checker errors, and exact non-transitive authorization.
    - Ensure shrinking and failure serialization never prints generated secret bytes.
    - Do not run the tests before authorized deployment.
    - **Depends on:** 5.1, 5.2, 5.4, 10.1.
    - **Properties 14-19: Human confirmation through exact live-change authorization**
    - **Validates: Requirements 11.10, 16.1-16.8, 17.1-17.8, 18.1-18.7, 19.1-19.6**

  - [ ] 12.4 Write property tests for lifecycle preservation, ctl_plane_chatbot contract diffing, trace gaps, and status stages
    - Add exactly one fast-check property per design Property 20 through Property 23.
    - Generate one-field preservation mismatches, allowed and forbidden plugin contract diffs, unresolved trace transitions, and independent evidence-stage combinations.
    - Do not run the tests before authorized deployment.
    - **Depends on:** 1.4, 8.2, 9.1.
    - **Properties 20-23: Preservation equality through independent status gates**
    - **Validates: Requirements 8.11-7.12, 10.1-10.11, 12.5-12.6, 12.9, 14.8, 19.7, 21.6, 22.1-22.7**

  - [ ] 12.5 Write focused unit and UI tests for specified examples and evidence labels
    - Add unit cases for binding mismatches, view presence, operation IDs, terminal cardinality, event/outcome mismatch, absent read-back, pin changes, path attacks, credential-bearing denial, unclear inventory items, contract drift, and every preservation field.
    - Add UI tests proving model output, local state, tracker evidence, Receipt, Authoritative_State, unknown, denial, and incomplete status remain visibly distinct.
    - Do not run the tests before authorized deployment.
    - **Depends on:** 6.2, 8.2, 8.3, 9.1, 11.2.
    - _Requirements: 1.7, 1.8, 4.7, 4.8, 5.7, 5.8, 6.12, 8.7, 8.8, 8.9, 11.7, 12.9, 16.7, 17.7, 18.3, 21.1, 22.8_

  - [ ] 12.6 Write post-deployment integration and smoke fixtures without running them
    - Encode the ordered checks for source baseline, cockpit preservation baseline, in-cockpit dual-view describe, tracked identity bind, one granted read per view, retained `ctl_plane_chatbot` read, clean-slate removal, lifecycle snapshots, safe denial, host/listener comparison, and post-cutover preservation.
    - Require Guest_Kernel_Identity, Attempt_Record, exactly one Terminal_Record, Receipt, and matching Authoritative_State for every verification Tool_Call.
    - Keep committed fixtures synthetic and non-secret. At execution, select the real cockpit only from the authorized target and guarded runtime evidence; never write real credentials, grant files, keys, or raw identity records into fixtures.
    - **Depends on:** 8.2, 9.3, 11.2, 12.1, 12.2, 12.3, 12.4, 12.5.
    - _Requirements: 12.7, 12.8, 14.7, 18.6, 20.1, 20.2, 20.3, 20.4, 20.5, 20.6, 20.7, 20.8, 20.9, 20.10, 21.1, 21.2, 21.3, 21.4, 21.5, 21.6, 21.7, 21.8_

- [ ] 13. Prepare and execute separately authorized reseal, deployment, restart, and cutover stages
  - [ ] 13.1 Generate the authorized catalog-reseal input and expected catalog diff
    - After authorized legacy removal and authorized `ctl_plane_chatbot` decoupling source changes are complete, generate reseal inputs showing removed legacy plugins absent and retained `ctl_plane_chatbot` present with an unchanged allowed contract.
    - Bind execution to a separate catalog-reseal Owner_Authorization; when it is absent, leave the command plan proposed and do not reseal.
    - **Depends on:** 9.2, 11.1, 10.2.
    - _Requirements: 11.10, 12.7, 19.1, 19.2, 19.3, 19.4, 21.2_

  - [ ] 13.2 Generate the authorized scoped deployment and restart plan
    - Generate a reversible plan limited to the fresh chat integration and separately authorized pinned changes, with exact target identity, cockpit, services, files, and expected Operation_Files.
    - Keep build activation/deployment and service restart as separate authorizations and separate proposed actions; add no mount, grant, credential, storage, host channel, listener, or container recreation step.
    - **Depends on:** 13.1.
    - _Requirements: 1.5, 10.8, 10.9, 10.10, 11.10, 14.1, 14.2, 14.3, 14.6, 19.1, 19.2, 19.3, 19.4, 23.1, 23.5, 23.7, 23.8_

  - [ ] 13.3 Generate the authorized fresh-chat cutover plan and rollback conditions
    - Generate a plan whose execution requires separate cutover authorization after the reseal, deployment, and restart actions have recorded target-matched execution outcomes. Full cutover/deployment verification is established by section 14, not presumed here.
    - Preserve the main identity, Main_Session, Native_UUID, Grant_Set, Seed, Leaf, Seven_Subvolumes, Mounted_Views, lungs-only host channel, and threetched boundary.
    - Define stop and owner-directed rollback conditions for binding failure, missing tracker/Receipt/state evidence, broken `ctl_plane_chatbot`, broken view, changed preservation snapshot, or any new host surface. A rollback needs its own exact authorization, preserves identity/storage, and must not revive removed legacy chat or bypass a denial. If no compliant rollback exists, stop and preserve the cockpit; do not execute cutover in this planning task.
    - **Depends on:** 13.2.
    - _Requirements: 1.1-1.5, 10.1-10.12, 11.8, 11.9, 11.10, 14.1-14.8, 19.1-19.7, 21.6, 21.7, 21.8_

  - [ ] 13.4 Execute only the separately authorized catalog reseal
    - Require the completed source changes, retained-plugin contract snapshot, exact reseal authorization, accepted input artifacts, and exclusive shared-repository seal/activation ownership.
    - Execute the existing supported reseal procedure from 13.1 without inventing a sealer or copying pinned code; record its action outcome and returned catalog pin, removed-plugin absence, and retained `ctl_plane_chatbot` entry.
    - **Done when:** the authorized action has matching evidence. Otherwise leave this task blocked and stop dependent activation; recording a blocker does not complete reseal or verify clean-slate cutover.
    - **Depends on:** 12.6, 13.3.
    - _Requirements: 11.10, 12.7, 19.1, 19.2, 19.3, 19.4, 21.2, 23.1, 23.2_

  - [ ] 13.5 Activate the approved artifacts and deploy the cockpit integration
    - Use only the artifact-production contract resolved in 1.5 and the exact plan from 13.2. Record artifact identity and source pins; require separate authorization for build activation and deployment, checking each action immediately before execution.
    - Capture the pre-change preservation/channel snapshot through the established tracked read path and use the existing deployment interface. Do not create a container, new channel, mount, grant, credential, or listener.
    - **Done when:** the deployment action succeeds and its target-matched outcome and preservation observations are recorded. Keep the deployed stage unconfirmed pending section 14; failure leaves this task blocked and stops the sequence without alternate execution.
    - **Depends on:** 13.4.
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 11.1, 11.9, 14.1, 14.2, 14.3, 14.6, 19.1, 19.2, 19.3, 19.4, 22.5, 22.6, 23.1_

  - [ ] 13.6 Execute only the named authorized service restart
    - Recheck the restart authorization independently of activation/deployment permission. Run only the supported lifecycle/service operation from the plan, with tracked before/after preservation reads and no mount or storage repair.
    - If the approved plan requires no restart, record an explicit not-required disposition with its source evidence; never invent an unnecessary restart to satisfy the task.
    - **Done when:** the authorized restart outcome and unchanged preservation evidence are recorded, or the source-backed no-restart disposition is recorded. Any required missing evidence blocks cutover.
    - **Depends on:** 13.5.
    - _Requirements: 10.4, 10.5, 10.6, 10.8, 10.9, 10.10, 10.11, 19.1, 19.2, 19.3, 19.4, 19.7_

  - [ ] 13.7 Apply the separately authorized fresh-chat cutover
    - Execute the exact cutover plan only for the preserved main identity, after the prior authorized actions have target-matched outcomes. Route chat only to the fresh interface, retaining `ctl_plane_chatbot` and both views.
    - Record action outcomes and surfaced blockers without calling the cutover verified. Missing binding/evidence or preservation drift stops further changes; use only the separately authorized compliant rollback path, if one exists.
    - **Done when:** the authorized cutover action succeeds and is recorded for post-deployment verification. Otherwise preserve the cockpit and leave this task blocked; a blocker never completes cutover.
    - **Depends on:** 13.6.
    - _Requirements: 11.8, 11.9, 11.10, 19.1, 19.2, 19.3, 19.4, 19.7, 21.6, 21.7, 21.8, 22.6_

- [ ] 15. Run mandatory deferred verification after authorized deployment
  - [ ] 15.1 Check deployment-action records and the explicit verification request
    - Require target-matched records that the authorized reseal, activation/deployment, restart or documented no-restart disposition, and cutover actions were applied in order. Reject missing authorization, target drift, failed action execution, or an unapplied deployment.
    - Require an explicit owner request for the exact post-deployment checks. `deployment-unconfirmed` is the expected entry state, not a prohibition on verification; section 14 establishes the missing verified status.
    - **Done when:** the request, deployed artifact/source identity, action records, and permitted command scope are recorded. No task-list approval is treated as live-change or test authorization.
    - **Depends on:** 13.7.
    - _Requirements: 19.1, 19.2, 19.3, 22.3, 22.4, 22.5, 22.6, 23.5, 23.6, 23.7, 23.8_

  - [ ] 14.2 Run the scoped post-deployment build verification
    - Only on explicit owner request after 14.1 passes, run the required scoped target for `x86_64-unknown-linux-gnu`; run no workspace-wide Cargo command and never overlap Cargo processes.
    - Record exact command, target, authorization/request, exit status, and affected component without promoting tested or deployed stages.
    - **Depends on:** 14.1.
    - _Requirements: 21.1, 22.3, 23.5, 23.7, 23.8_

  - [ ] 15.3 Run scoped property, unit, UI, static, and review checks
    - Only on the explicit request gated in 14.1, run the authored pure checks, no-remnant verifier, credential-class scanner, lint, formatting, type checking, and code review as separate scoped checks. Record exact commands, requests, targets and outcomes.
    - Run at least 100 generated cases for each design property; suppress synthetic credential bytes in shrinking/failure output. Cover invalid input, path escape, duplicate terminal, attribution/outcome mismatch, concurrent confirmations, sticky denial, and evidence-read termination.
    - Check all prohibited product interfaces, copied pinned code, grant/materialization paths, cross-identity credential changes, and forbidden naming aliases. Preserve threetched services regardless of historical names.
    - **Done when:** each requested check has its own evidence and result; failures block the affected completion claim, not unrelated status stages.
    - **Depends on:** 14.2.
    - _Requirements: 3.6, 3.7, 5.5, 11.3, 11.5, 11.6, 12.5, 12.6, 12.9, 13.1, 13.2, 13.3, 15.1, 15.2, 15.3, 15.4, 15.5, 18.6, 18.7, 21.1, 22.4, 23.2, 23.3, 23.6, 23.7, 23.8, 23.9_

  - [ ] 15.4 Verify both views and identity from inside the deployed cockpit
    - Reconfirm accepted source pins and both operation traces; stop affected checks at unresolved transitions. Invoke the existing guest-kernel describe interface inside `3tchedCockpitBot` for both views, recording attributed identity, matching catalog pins, distinct mount sources, and routing assignments.
    - Execute the tracked `identity_sled.get_own_identity` read, one granted read in each view, and a retained `ctl_plane_chatbot` read. Require each subject's attempt, exactly one terminal, Receipt, and post-terminal Authoritative_State with independently attributed supporting-read evidence.
    - **Done when:** all named read/identity checks have matched evidence. A host-only mount or SHM projection proves neither guest binding nor completion; missing evidence leaves that view deployment-unconfirmed.
    - **Depends on:** 14.3.
    - _Requirements: 2.10, 2.11, 3.1, 3.2, 3.3, 8.11, 8.12, 12.7, 12.8, 20.1, 20.2, 20.3, 20.4, 20.5, 20.6, 20.7, 20.8, 20.9, 20.10, 21.3, 21.4, 21.5_

  - [ ] 15.5 Verify clean-slate cutover and preserved host/storage boundaries
    - Exercise the real fresh chat surface bound to the main identity and observe that model text, UI state, tracker evidence, Receipt, and authoritative state remain distinct. Confirm no legacy route remains active and no alternate product transport or MCP path is enabled.
    - Combine the no-remnant search with the active sealed catalog: removed legacy plugins absent, retained `ctl_plane_chatbot` present with the permitted contract. Compare the complete post-cutover preservation snapshot against the pre-change snapshot, including channels/listeners and the unchanged read-only lungs mount.
    - **Done when:** every cutover criterion has evidence; any new host surface, storage/identity difference, broken view, or retained-plugin failure makes cutover incomplete and stops deployment. No automatic repair or resurrection of legacy chat is allowed.
    - **Depends on:** 14.4.
    - _Requirements: 8.12, 11.8, 12.7, 12.8, 14.1, 14.2, 14.3, 14.4, 14.6, 14.7, 14.8, 15.1, 15.2, 15.3, 21.1, 21.2, 21.3, 21.4, 21.5, 21.6, 21.7, 21.8_

  - [ ] 15.6 Exercise authorized lifecycle and safely selected denial scenarios
    - For each separately authorized lifecycle action, use the supported procedure and compare complete tracked before/after snapshots; verify start's `identity_sled.touch_session` call. Record actions not authorized as unperformed, never passed.
    - On the explicit verification request, select a safe ungranted read and observe denial, secret-free blocker creation, session-sticky unavailability, unchanged unrelated pending calls, and zero alternate-view/transport execution. Do not broaden grants to make the scenario pass.
    - **Done when:** each authorized scenario has the required evidence and all withheld scenarios are clearly reported. Denials or failures trigger no subject replay, storage repair, raw runtime command, or bypass.
    - **Depends on:** 14.5.
    - _Requirements: 9.6, 9.7, 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7, 10.8, 10.9, 10.10, 10.11, 10.12, 18.1, 18.2, 17.3, 17.4, 17.5, 17.6, 17.7, 17.8, 19.1, 19.6, 19.7_

  - [ ] 15.7 Write the final evidence-separated Status_Report
    - Report proposed, implemented, built, tested, and deployed independently for each component and task. Publish partial/blocked reports whenever an earlier task stops; this final consolidation never suppresses failure reporting.
    - Cite source files/task IDs, build and test commands/results and explicit owner requests, Guest_Kernel_Identity, tracker evidence, Receipt, Authoritative_State, catalog pin, preservation snapshots, and every open blocker with affected task IDs.
    - Keep any component with incomplete deployment evidence deployment-unconfirmed; distinguish mounted projection, committed-SHM projection, identity binding, and verified Tool_Call completion. Do not mark withheld checks passed.
    - **Depends on:** 14.6.
    - _Requirements: 2.4, 2.10, 2.11, 20.7, 20.8, 22.1, 22.2, 22.3, 22.4, 22.5, 22.6, 22.7, 22.8_

- [ ] 16. Close only the evidenced cockpit scope
  - [ ] 16.1 Reconcile acceptance evidence and open blockers
    - Map all 23 requirements to the implementation tasks and recorded outcomes; require the mandatory checks to pass before declaring their affected work complete.
    - Confirm that this plan changed no subsidiary-spec file and performed no cockpit creation/reimage, unauthorized live action, or unrequested workspace-wide verification.
    - **Done when:** the final report explicitly identifies complete, blocked, unperformed, and deployment-unconfirmed work without claiming that document completion is implementation completion.
    - **Depends on:** 14.7.
    - _Requirements: 1.5, 19.2, 21.7, 21.8, 22.1, 22.6, 22.7, 23.1, 23.7, 23.8, 23.9_

## Notes

- All listed implementation and acceptance-verification tasks are required for their affected completion claims. Deferred execution is not optional coverage; withheld authorization leaves work proposed/unperformed, never implicitly passed.
- Tasks 10.2 and 13.1–13.3 prepare exact plans; tasks 13.4–13.7 execute the separately authorized actions. Section 14 runs only after the deployment actions and an explicit verification request, and then establishes verified deployment/cutover status. It does not require that status in advance.
- An authorization for legacy removal does not authorize pinned-source decoupling, catalog reseal, build activation/deployment, restart, mount change, or cutover. Each action needs its own Owner_Authorization.
- Any missing source trace, capability, mounted view, identity match, tracker record, Receipt, authoritative read, or authorization blocks dependent tasks. A blocked feature is not replaced by a stub, placeholder, dummy value, compatibility shim, partial implementation, alternate transport, direct state access, or cross-view fallback.
- Cockpit product code accesses committed op-dbus SHM state only through the 3tched-fs mounted runtime mirror. A projection read is not Tool_Call completion evidence.
- `/srv/git/odbus-verified`, 3tched-fs, `identity_sled`, the identity-bound container architecture, and op-execution-tracker remain pinned and read-only except for separately authorized complete legacy-chat removal and minimum `ctl_plane_chatbot` decoupling.
- No task creates, clones, reimages, rebuilds, reseeds, repurposes, or duplicates `3tchedCockpitBot`, or renames it other than the owner-directed `CockpitBot3tched` → `3tchedCockpitBot` cutover.

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "5.4"] },
    { "id": 2, "tasks": ["1.4", "8.1"] },
    { "id": 3, "tasks": ["1.5", "8.2"] },
    { "id": 4, "tasks": ["2.1", "8.3"] },
    { "id": 5, "tasks": ["3.1", "9.1", "10.1"] },
    { "id": 6, "tasks": ["3.2", "9.2", "10.2"] },
    { "id": 7, "tasks": ["3.3", "4.1", "11.1"] },
    { "id": 8, "tasks": ["4.2", "11.2", "13.1"] },
    { "id": 9, "tasks": ["4.3", "5.1", "13.2"] },
    { "id": 10, "tasks": ["4.4", "13.3"] },
    { "id": 11, "tasks": ["4.5"] },
    { "id": 12, "tasks": ["4.6"] },
    { "id": 13, "tasks": ["2.2", "5.2"] },
    { "id": 14, "tasks": ["2.3", "5.3", "7.1", "12.3"] },
    { "id": 15, "tasks": ["6.1", "7.2", "12.1", "12.2"] },
    { "id": 16, "tasks": ["6.2", "9.3", "12.4"] },
    { "id": 17, "tasks": ["12.5"] },
    { "id": 18, "tasks": ["12.6"] },
    { "id": 19, "tasks": ["13.4"] },
    { "id": 20, "tasks": ["13.5"] },
    { "id": 21, "tasks": ["13.6"] },
    { "id": 22, "tasks": ["13.7"] },
    { "id": 23, "tasks": ["14.1"] },
    { "id": 24, "tasks": ["14.2"] },
    { "id": 25, "tasks": ["14.3"] },
    { "id": 26, "tasks": ["14.4"] },
    { "id": 27, "tasks": ["14.5"] },
    { "id": 28, "tasks": ["14.6"] },
    { "id": 29, "tasks": ["14.7"] },
    { "id": 30, "tasks": ["15.1"] }
  ]
}
```
