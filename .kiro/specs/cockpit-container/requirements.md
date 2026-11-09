# Requirements Document

## Introduction

This specification covers the existing main 3tched control-plane cockpit, its
main sealed identity, session, storage, grants, lifecycle, and integration with
the fresh identity-neutral chat interface specified in
`/srv/git/odbus-verified/.kiro/specs/identity-chat-interface/` (the canonical
copy; owner, 2026-11-09). The existing cockpit target name is
`3tchedCockpitBot` (owner rename from `CockpitBot3tched`, 2026-11-09). This specification preserves that cockpit in place and does
not create another cockpit.

The main 3tched identity remains authoritative for the cockpit. The cockpit
uses the existing identity-bound container architecture, op-dbus,
`identity_sled`, 3tched-fs, the sealed catalog, the op-execution-tracker, and
the external host lifecycle interfaces as pinned dependencies. Implementation
may read those dependencies and use their existing interfaces. Implementation
may not copy or modify a pinned dependency except for (a) the clean-slate removal
of legacy chat code and the required decoupling of `ctl_plane_chatbot`, under
`/srv/git/kiroween/.kiro/steering/chat-clean-slate.md` and a specific Owner_Authorization,
and (b) gating schema and state visibility in 3tched-fs and op-grpc-bridge so a
caller sees only what it holds grants for (identity-chat-interface `design.md`
D-48, task 24), under the owner's 2026-11-09 permission to change
`/srv/git/odbus-verified`. Deploying either remains a Live_Change.

Chat server placement (owner, 2026-11-09: "vps"): the Chat_Interface server
(the Rust Chat_Route in `crates/op-chat-interface`) for the 3tched_Identity runs
on the VPS host, not inside the Cockpit. It reaches op-dbus only through the
Mounted_Views, and its identity is the one `identity_sled.get_own_identity`
returns through them: the container's identity lives in the mounted
`identity_sled` (owner, 2026-11-09), with no separate binding. The browser
reaches it on the host (identity-chat-interface Requirement 32.8). The Cockpit
gains no listener, host channel or network surface from the chat
(Requirement 14).

The cockpit uses both mounted filesystem views at the same time:

- `/run/mounts/3tchedFS` exposes operations that run on 3tched code.
- `/run/mounts/threetched` exposes operations that run on the ZeroClaw-derived
  executable `/usr/bin/threetched`.

Both views use the same 3tched-fs engine, sealed catalog, and identity
enforcement. The view for an operation is selected by what runs the operation.
A denial, error, or unknown outcome does not authorize routing the operation
through the other view.

3tched-fs is the runtime mirror of committed op-dbus shared-memory state for
both Mounted_Views. The 3tched-fs daemon presents current committed op-dbus SHM
state through FUSE. Cockpit processes use the Mounted_Views and never open SHM
directly. An observed mount, directory entry, or schema proves that the
corresponding mounted projection exists, but that observation alone does not
prove correct Guest_Kernel_Identity binding or completion of a Tool_Call. Every
Tool_Call, including every Read_Tool_Call, uses Operation_File dispatch and the
op-execution-tracker and requires an Attempt_Record, exactly one
Terminal_Record, a Receipt, and matching Authoritative_State before the
Tool_Call is reported complete. A successful filesystem write or direct state
observation cannot replace Tracker_Evidence.

Container creation outside the preservation and lifecycle of
`3tchedCockpitBot` is outside this specification and belongs to
`/srv/git/kiroween/.kiro/specs/cockpit-lxd-standalone/`.

Every requirement is subject to the workspace security and naming rules. Where
this document differs from a workspace steering rule, the steering rule wins.

## Glossary

- **3tched_Identity**: The existing main sealed 3tched identity that owns the
  control-plane cockpit, its session, grants, credentials, and storage.
- **Main_Session**: The existing session
  `b0c777e3-0955-cd8b-3d01-ca12998cae28` of the 3tched_Identity.
- **Cockpit**: The existing identity-scoped control-plane container named
  `3tchedCockpitBot` (renamed from `CockpitBot3tched`; owner, 2026-11-09).
- **Native_UUID**: The existing container UUID assigned to the Cockpit by the
  owning container runtime.
- **Sealed_Identity**: The authoritative identity state held by op-dbus and
  `identity_sled`.
- **Identity_Binding**: The binding among the 3tched_Identity, Main_Session,
  Cockpit, Native_UUID, grants, credentials, and storage.
- **SID1**: The sealed identity credential envelope. SID1 bytes are
  Credential_Material.
- **Credential_Material**: SID1 bytes, OIA1 bytes, private keys, provider
  credentials, session credentials, and any usable secret derived from those
  values.
- **Grant_Set**: The capabilities already assigned to the 3tched_Identity.
- **Seed**: The immutable, read-only shared Btrfs base image used by the
  identity-bound container architecture.
- **Leaf**: The identity-specific writable Btrfs device already assigned to the
  Cockpit.
- **Seven_Subvolumes**: The Btrfs sibling subvolumes `@`, `@agent_memory`,
  `@agent_rules`, `@agent_shared`, `@agent_skills`, `@agent_soul`, and
  `@agent_user`.
- **Lifecycle_Operation**: Login, start, stop, restart, logout, recovery, or
  failed-start cleanup of the Cockpit.
- **3tched_View**: The mounted view `/run/mounts/3tchedFS`, containing only
  operations that run on 3tched code.
- **Threetched_View**: The mounted view `/run/mounts/threetched`, containing
  only operations run by `/usr/bin/threetched`.
- **Mounted_Views**: The 3tched_View and Threetched_View used together.
- **Matching_View**: The 3tched_View for an operation that runs on 3tched code,
  or the Threetched_View for an operation run by `/usr/bin/threetched`.
- **3tched-fs**: The 3tched filesystem daemon and engine that provides both
  Mounted_Views with sealed catalog and identity enforcement and presents
  current Committed_SHM_State through FUSE.
- **Committed_SHM_State**: State committed in op-dbus shared memory and
  presented by 3tched-fs through both Mounted_Views.
- **SHM_Runtime_Mirror**: The FUSE projection through which 3tched-fs presents
  current Committed_SHM_State to cockpit processes. Cockpit processes use the
  SHM_Runtime_Mirror instead of opening op-dbus shared memory directly.
- **Sealed_Catalog**: The identity-enforced catalog that declares the plugins,
  methods, schemas, capabilities, side effects, hash, and generation exposed
  through the Mounted_Views.
- **Catalog_Pin**: The Sealed_Catalog hash and generation observed for an
  operation.
- **Guest_Kernel_Identity**: The caller identity and session attributed by the
  guest-kernel mounted-view path, without a caller-supplied SID1 path or
  session override.
- **Lungs_Socket**: `/run/opdbus/lungs/lungs.sock`, exposed to the Cockpit as
  the existing lungs channel used by 3tched-fs. The Lungs_Socket is the
  Cockpit's only host channel.
- **Operation_File**: A mounted path of the form
  `operations/<plugin>/<method>/call` in a Matching_View.
- **PluginV1_Path**: The existing operation protocol path that processes an
  Operation_File write through PluginV1, lungs, backend dispatch,
  op-execution-tracker, and the returned Receipt.
- **Tool_Call**: One requested invocation of one plugin method with one input,
  one `subid`, and one `operation_id`.
- **Read_Tool_Call**: A Tool_Call whose Sealed_Catalog method declaration has a
  recognized read-only side effect.
- **Mutation_Tool_Call**: A Tool_Call whose Sealed_Catalog method declaration
  has a mutation side effect, no side-effect declaration, or an unrecognized
  side-effect declaration.
- **op-execution-tracker**: The existing authoritative attempt, terminal
  outcome, and hash-chain recording path for op-dbus operations.
- **Attempt_Record**: The op-execution-tracker record proving that a Tool_Call
  entered backend execution, attributed to an `operation_id`, caller identity,
  caller session, plugin, method, `subid`, Catalog_Pin, and hash-chain position.
- **Terminal_Record**: The single op-execution-tracker record that closes an
  `operation_id` with an outcome and hash-chain linkage.
- **Tracker_Evidence**: The Attempt_Record and exactly one matching
  Terminal_Record for one Tool_Call.
- **Hash_Chain_Linkage**: The record hash and preceding-record hash fields that
  link an op-execution-tracker record into the existing execution chain.
- **Receipt**: The completed result returned through the Operation_File and
  linked to Tracker_Evidence for the same `operation_id`.
- **Authoritative_State**: State read back from the owning op-dbus plugin
  through a granted operation in the Matching_View after a Tool_Call.
- **Chat_Interface**: The fresh identity-neutral application specified in
  `/srv/git/odbus-verified/.kiro/specs/identity-chat-interface/`: a Rust
  Chat_Route in `crates/op-chat-interface` running on the VPS host, and a
  TypeScript/Svelte browser Chat_UI and Canvas.
- **Chatbot_Glue**: The new 3tched code that binds the Chat_Interface to the
  Cockpit's existing identity and Mounted_Views.
- **ctl_plane_chatbot**: The existing 3tched control-plane plugin that remains
  installed and is integrated into the Chat_Interface through Operation_Files.
- **Legacy_Chat_Inventory**: The record of existing chat-related source,
  registration, services, schemas, routes, tests, fixtures, documentation, and
  callers, classified before removal or preservation.
- **Denial**: A permission, capability, policy, identity, catalog, view, or
  operation refusal.
- **Unknown_Outcome**: A Tool_Call outcome for which no valid matching Receipt
  and Tracker_Evidence establish the terminal result.
- **Blocker_Record**: A redacted record of a Denial, Unknown_Outcome, missing
  dependency, attribution mismatch, or invariant failure.
- **Owner_Authorization**: An explicit, recorded authorization from the owner
  for one named Live_Change and target.
- **Live_Change**: A change to a live identity, grant, credential, storage,
  mount, service, deployment, container, catalog, or runtime process.
- **Baseline_Record**: The source-trace record for pinned interfaces and their
  callers.
- **Runtime_Record**: The read-only record of observed mounts, identities,
  catalogs, tracker evidence, receipts, state, services, and container state.
- **Status_Report**: A record that reports proposed, implemented, built,
  tested, and deployed status separately.
- **Implementer**: The agent or engineer carrying out this specification.
- **Repository**: `/srv/git/odbus-verified`, the repository that contains
  this main cockpit specification and its cockpit-specific implementation and records.
  The Rust Chat_Route of the Chat_Interface is `crates/op-chat-interface` in
  this Repository; only its browser Chat_UI and Canvas are TypeScript/Svelte
  (owner, 2026-11-09).

## Requirements

### Requirement 1: Preserve the existing cockpit and main identity

**User Story:** As the owner, I want the existing main control-plane cockpit and
identity preserved, so that chat integration cannot replace or repurpose the
control plane.

#### Acceptance Criteria

1. THE Implementer SHALL preserve the Cockpit name as `3tchedCockpitBot` (after the owner-authorized rename from `CockpitBot3tched`).
2. THE Implementer SHALL preserve the Main_Session as `b0c777e3-0955-cd8b-3d01-ca12998cae28`.
3. THE Implementer SHALL preserve the Cockpit Native_UUID.
4. THE Implementer SHALL preserve the Identity_Binding.
5. THE Implementer SHALL perform no create, clone, unauthorized rename, reimage, rebuild, reseed, repurpose, or duplicate-container action for the Cockpit.
6. IF an action targets a container other than the Cockpit, THEN THE Implementer SHALL classify the action as outside this specification.
7. IF an observed container named `3tchedCockpitBot` carries a session other than the Main_Session, THEN THE Implementer SHALL stop the dependent work and create a Blocker_Record.
8. IF an observed container carrying the Main_Session has a native name other than `3tchedCockpitBot`, THEN THE Implementer SHALL stop the dependent work and create a Blocker_Record.

### Requirement 2: Establish source, projection, identity, and completion evidence

**User Story:** As the owner, I want the cockpit's existing contracts, mounted
projections, identity binding, and Tool_Call outcomes re-verified, so that each
evidence type supports only the claim that the evidence establishes.

#### Acceptance Criteria

1. THE Baseline_Record SHALL identify the pinned commit, source path, symbol signature, file digest, and current callers for each pinned interface used by this specification.
2. THE Baseline_Record SHALL cover `identity_sled`, the identity-bound container lifecycle, 3tched-fs, PluginV1, lungs dispatch, backend plugin dispatch, op-execution-tracker, Receipt creation, Sealed_Catalog enforcement, and Guest_Kernel_Identity enforcement.
3. THE Implementer SHALL perform source discovery with read-only operations.
4. THE Runtime_Record SHALL classify each finding as observed runtime evidence, source definition only, or not found.
5. THE Runtime_Record SHALL record each Mounted_Views mount path, kernel mount source, owning process, Sealed_Catalog hash, Sealed_Catalog generation, and observed Guest_Kernel_Identity.
6. WHEN a mount, directory entry, or schema is observed in the 3tched_View or Threetched_View, THE Runtime_Record SHALL record the corresponding mounted projection as present.
7. WHEN a state value is observed through the 3tched_View or Threetched_View, THE Runtime_Record SHALL classify the value as current Committed_SHM_State presented by 3tched-fs.
8. THE Runtime_Record SHALL accept no mounted-projection observation by itself as proof of correct Guest_Kernel_Identity binding or Tool_Call completion.
9. THE Runtime_Record SHALL accept no successful filesystem write or direct state observation as a substitute for Tracker_Evidence.
10. WHEN completion of a Tool_Call is reported, THE Runtime_Record SHALL include matching Guest_Kernel_Identity, an Attempt_Record, exactly one Terminal_Record, a Receipt, and Authoritative_State.
11. IF any evidence named in criterion 10 is absent or mismatched, THEN THE Runtime_Record SHALL classify the Tool_Call as incomplete.
12. THE Implementer SHALL perform runtime discovery with read-only operations.

### Requirement 3: Preserve sealed identity and grants

**User Story:** As the owner, I want the cockpit to use only the existing main
sealed identity and grants, so that chat integration cannot broaden authority.

#### Acceptance Criteria

1. WHEN a Chat_Interface session starts for the 3tched_Identity, THE Chatbot_Glue SHALL obtain the Sealed_Identity through `identity_sled.get_own_identity` in the 3tched_View the Chat_Route uses on the VPS; the container's identity lives in the mounted `identity_sled`.
2. WHEN `identity_sled.get_own_identity` returns, THE Chatbot_Glue SHALL require the returned session to equal the Main_Session.
3. WHEN `identity_sled.get_own_identity` returns, THE Chatbot_Glue SHALL require the returned identity to equal the 3tched_Identity recorded for the Cockpit.
4. IF the returned identity or session differs from the expected Identity_Binding, THEN THE Chatbot_Glue SHALL refuse to start the Chat_Interface session and create a Blocker_Record.
5. THE Chat_Interface SHALL derive available tools from the Sealed_Catalog and Grant_Set of the 3tched_Identity.
6. THE Chat_Interface SHALL contain no path that creates, broadens, materializes, overrides, or changes a grant.
7. THE Chat_Interface SHALL contain no path that changes a credential of an identity other than the 3tched_Identity.
8. WHEN a credential operation for the 3tched_Identity is requested, THE Chat_Interface SHALL apply the Sealed_Catalog capability, validation, confirmation, tracker, Receipt, and Authoritative_State requirements of this specification.
9. THE Chatbot_Glue SHALL use Guest_Kernel_Identity for every Operation_File call.
10. THE Chatbot_Glue SHALL supply no SID1 file path or caller-selected lungs session value.

### Requirement 4: Preserve both mounted views and strict routing

**User Story:** As the cockpit operator, I want every operation routed through
the view matching the code that runs the operation, so that 3tched and
threetched remain isolated across upgrades and failures.

#### Acceptance Criteria

1. THE Cockpit SHALL mount the 3tched_View and the Threetched_View at the same time.
2. THE Mounted_Views SHALL use the same Sealed_Catalog hash and generation.
3. THE Mounted_Views SHALL use distinct kernel mount sources.
4. THE Chatbot_Glue SHALL route every operation that runs on 3tched code through an Operation_File in the 3tched_View.
5. THE Chatbot_Glue SHALL route every operation run by `/usr/bin/threetched` through an Operation_File in the Threetched_View.
6. THE Chatbot_Glue SHALL derive each plugin, method, argument schema, result schema, Required_Capability, side effect, Catalog_Pin, and Matching_View from the mounted Sealed_Catalog.
7. IF an operation appears only in a view other than the Matching_View, THEN THE Chatbot_Glue SHALL refuse the operation and create a Blocker_Record.
8. IF an operation appears in both views, THEN THE Chatbot_Glue SHALL refuse the operation and create a Blocker_Record.
9. IF the Sealed_Catalog hash or generation changes before a Tool_Call executes, THEN THE Chatbot_Glue SHALL refuse the Tool_Call until the tool registry is re-derived from both views.
10. IF a Tool_Call receives a Denial, error, or Unknown_Outcome in the Matching_View, THEN THE Chatbot_Glue SHALL make no attempt for that Tool_Call through the other view.
11. THE Repository SHALL use `threetched` as the interface name for operations run by `/usr/bin/threetched`.
12. IF the Threetched_View exposes its service only through a name forbidden by `/srv/git/kiroween/.kiro/steering/naming-3tched-threetched.md`, THEN THE Implementer SHALL record the naming defect and leave the affected operation uncalled.
13. THE Cockpit SHALL permit process access to Committed_SHM_State only through the Mounted_Views.

### Requirement 5: Use operation files as the only product call interface

**User Story:** As the owner, I want cockpit actions to use existing operation
files only, so that identity, grants, schemas, tracking, and receipts stay on
the enforced path.

#### Acceptance Criteria

1. THE Chatbot_Glue SHALL invoke every Tool_Call by writing the validated input to one Operation_File in the Matching_View.
2. THE Chatbot_Glue SHALL read the Tool_Call result and Receipt from the same Operation_File path used for the write.
3. THE Chatbot_Glue SHALL validate each Tool_Call input against the argument schema from the same Catalog_Pin used to select the Operation_File.
4. THE Chatbot_Glue SHALL validate each Tool_Call result against the result schema from the same Catalog_Pin used to select the Operation_File.
5. THE Chat_Interface SHALL contain no direct op-dbus, D-Bus, backend store, SHM open, SHM read, SHM write, raw container-runtime, or alternate operation transport.
6. THE Chat_Interface SHALL implement no operation fallback outside the Matching_View.
7. IF an Operation_File is absent, unreadable, schema-invalid, or outside the expected mounted root, THEN THE Chatbot_Glue SHALL refuse the Tool_Call and create a Blocker_Record.
8. IF path normalization places an Operation_File outside the expected Matching_View root, THEN THE Chatbot_Glue SHALL refuse the Tool_Call and create a Blocker_Record.

### Requirement 6: Route every tool call through op-execution-tracker

**User Story:** As the owner, I want every read and mutation recorded by the
existing execution tracker, so that no cockpit action can occur without
identity attribution and hash-chain evidence.

#### Acceptance Criteria

1. THE Tool_Executor SHALL route every Read_Tool_Call and every Mutation_Tool_Call through the existing op-execution-tracker path.
2. THE Tool_Executor SHALL apply criterion 1 to Tool_Calls in the 3tched_View and the Threetched_View.
3. THE op-execution-tracker SHALL create an Attempt_Record for every Tool_Call before backend execution produces a terminal outcome.
4. THE Attempt_Record SHALL identify the Tool_Call by `operation_id`, caller identity, caller session, plugin, method, `subid`, Catalog_Pin, and Hash_Chain_Linkage.
5. THE op-execution-tracker SHALL create exactly one Terminal_Record for each Tool_Call `operation_id`.
6. THE Terminal_Record SHALL identify the Tool_Call by `operation_id`, caller identity, caller session, plugin, method, `subid`, Catalog_Pin, outcome, and Hash_Chain_Linkage.
7. WHEN a Receipt is returned, THE Receipt SHALL link to the Attempt_Record and Terminal_Record of the same `operation_id`.
8. WHEN Tracker_Evidence is evaluated, THE evaluator SHALL require exact equality among the Tool_Call, Attempt_Record, Terminal_Record, and Receipt for caller identity, caller session, plugin, method, `subid`, Catalog_Pin, and `operation_id`.
9. WHEN Tracker_Evidence is evaluated, THE evaluator SHALL require the Terminal_Record outcome to equal the Receipt outcome.
10. WHEN Tracker_Evidence is evaluated, THE evaluator SHALL require valid Hash_Chain_Linkage for the Attempt_Record and Terminal_Record.
11. IF an Attempt_Record is missing, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete.
12. IF a Terminal_Record is missing or more than one Terminal_Record matches the `operation_id`, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete.
13. IF attribution differs among the Tool_Call, Attempt_Record, Terminal_Record, or Receipt, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete and create a Blocker_Record.
14. IF Hash_Chain_Linkage is missing or invalid, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete and create a Blocker_Record.
15. IF Tracker_Evidence is incomplete or mismatched, THEN THE Tool_Executor SHALL make no retry through another view, transport, identity, direct backend call, SHM write, container-runtime command, or local substitute.
16. THE Tool_Executor SHALL accept no local log, UI state, filesystem-write success, process exit status, or model output as a substitute for Tracker_Evidence.

### Requirement 7: Trace the complete tracked operation path

**User Story:** As the owner, I want the complete operation path traced in
source, so that tracker enforcement and Receipt linkage are established from
implementation rather than inferred from mounted files.

#### Acceptance Criteria

1. THE Baseline_Record SHALL trace an Operation_File write from the 3tched-fs write handler into the PluginV1_Path.
2. THE Baseline_Record SHALL trace the PluginV1_Path through the lungs dispatch entry.
3. THE Baseline_Record SHALL trace the lungs dispatch entry into backend plugin dispatch.
4. THE Baseline_Record SHALL trace backend plugin dispatch into op-execution-tracker attempt recording.
5. THE Baseline_Record SHALL trace backend completion into op-execution-tracker terminal recording.
6. THE Baseline_Record SHALL trace the Terminal_Record into Receipt construction.
7. THE Baseline_Record SHALL trace the Receipt from backend dispatch to the Operation_File read result.
8. THE Baseline_Record SHALL record each trace step with repository pin, file path, one-based line number, symbol, caller, and callee.
9. THE Baseline_Record SHALL identify the source location that preserves `operation_id`, caller identity, caller session, plugin, method, `subid`, Catalog_Pin, outcome, and Hash_Chain_Linkage at each applicable trace step.
10. THE Baseline_Record SHALL perform the trace separately for a 3tched operation and a threetched operation.
11. IF a trace step cannot be established from pinned source, THEN THE Implementer SHALL create a Blocker_Record naming the last established symbol and the unresolved transition.
12. WHILE a trace step for a view remains unresolved, THE Implementer SHALL report operations through that view as deployment-unconfirmed.
13. THE Implementer SHALL perform source tracing with read-only operations.

### Requirement 8: Require tracker, receipt, and authoritative state for completion

**User Story:** As the owner, I want completion based on the execution tracker,
Receipt, and owning state, so that transport success cannot be mistaken for an
applied operation.

#### Acceptance Criteria

1. WHEN any Tool_Call, including a Read_Tool_Call, is reported complete, THE Tool_Executor SHALL provide the matching Attempt_Record.
2. WHEN any Tool_Call, including a Read_Tool_Call, is reported complete, THE Tool_Executor SHALL provide exactly one matching Terminal_Record.
3. WHEN any Tool_Call, including a Read_Tool_Call, is reported complete, THE Tool_Executor SHALL provide the matching Receipt from the Operation_File.
4. WHEN a Tool_Call is reported complete, THE Tool_Executor SHALL provide Authoritative_State read after the Terminal_Record through a granted Read_Tool_Call in the Matching_View.
5. WHEN a Tool_Call is reported complete, THE Authoritative_State SHALL reflect the outcome recorded by the Terminal_Record and Receipt.
6. IF the Attempt_Record is absent or mismatched, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete.
7. IF the Terminal_Record is absent, duplicated, or mismatched, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete.
8. IF the Receipt is absent, invalid, or mismatched, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete.
9. IF Authoritative_State is absent or does not reflect the recorded outcome, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete.
10. WHEN a state value is observed through the SHM_Runtime_Mirror, THE Tool_Executor SHALL recognize the value as current Committed_SHM_State at the time of observation.
11. THE Tool_Executor SHALL accept no direct state observation, successful filesystem write, local log, Chat_Interface state, filesystem result, process result, or model output as a substitute for Tracker_Evidence or Receipt.
12. WHEN the Chat_Interface displays a Tool_Call result, THE Chat_Interface SHALL distinguish model output, local UI state, Tracker_Evidence, Receipt, and Authoritative_State.

### Requirement 9: Handle unknown outcomes without bypass

**User Story:** As the owner, I want unknown outcomes reconciled through
existing evidence, so that uncertainty cannot cause duplicate or bypassed
actions.

#### Acceptance Criteria

1. IF a Tool_Call returns no valid Receipt or Terminal_Record, THEN THE Tool_Executor SHALL classify the Tool_Call as Unknown_Outcome.
2. WHEN a Tool_Call enters Unknown_Outcome, THE Tool_Executor SHALL read available op-execution-tracker records for the same `operation_id` through a granted Operation_File.
3. WHEN a Tool_Call enters Unknown_Outcome, THE Tool_Executor SHALL read Authoritative_State through a granted Operation_File in the Matching_View.
4. IF reconciliation finds valid Tracker_Evidence, a matching Receipt, and matching Authoritative_State, THEN THE Tool_Executor SHALL classify the Tool_Call according to the Terminal_Record outcome.
5. IF reconciliation leaves Tracker_Evidence, Receipt, or Authoritative_State absent or mismatched, THEN THE Tool_Executor SHALL report the Tool_Call as incomplete and create a Blocker_Record.
6. IF reconciliation leaves a Tool_Call incomplete, THEN THE Tool_Executor SHALL send no repeat execution for that Tool_Call.
7. THE Tool_Executor SHALL perform no Unknown_Outcome reconciliation through another view, transport, identity, direct backend read, SHM write, container-runtime command, or local cache.

### Requirement 10: Preserve storage and lifecycle invariants

**User Story:** As the owner, I want cockpit lifecycle actions to preserve its
existing storage, so that routine operation cannot alter identity-bound Btrfs
membership or layout.

#### Acceptance Criteria

1. THE Implementer SHALL preserve the Seed as read-only and unchanged.
2. THE Implementer SHALL preserve the existing Leaf identity and Btrfs device membership.
3. THE Implementer SHALL preserve exactly the Seven_Subvolumes.
4. WHEN a Lifecycle_Operation starts, THE lifecycle procedure SHALL record the Seed identity, Leaf identity, Btrfs member set, Seven_Subvolumes set, Identity_Binding, and Native_UUID through granted reads.
5. WHEN a Lifecycle_Operation ends, THE lifecycle procedure SHALL read the values from criterion 4 again through granted reads.
6. WHEN a Lifecycle_Operation is reported complete, THE lifecycle procedure SHALL require the before and after values from criterion 4 to be equal.
7. WHEN the Cockpit starts, THE lifecycle procedure SHALL use the existing `identity_sled.touch_session` Operation_File in the 3tched_View.
8. THE lifecycle procedure SHALL add no Btrfs member during a Lifecycle_Operation.
9. THE lifecycle procedure SHALL remove no Btrfs member during a Lifecycle_Operation.
10. THE lifecycle procedure SHALL create, delete, rename, or replace no subvolume during a Lifecycle_Operation.
11. IF a storage or Identity_Binding invariant differs after a Lifecycle_Operation, THEN THE lifecycle procedure SHALL report the Lifecycle_Operation as incomplete and create a Blocker_Record.
12. THE Repository SHALL contain no cockpit procedure that reimages, reseeds, duplicates, or recreates the Cockpit.

### Requirement 11: Integrate the fresh chat interface without legacy chat code

**User Story:** As the owner, I want the existing cockpit to use only the fresh
chat interface, so that old chat implementations do not remain on an active
path.

#### Acceptance Criteria

1. THE Chatbot_Glue SHALL integrate the Chat_Interface specified in `/srv/git/odbus-verified/.kiro/specs/identity-chat-interface/` with the existing Cockpit's identity.
2. THE Chatbot_Glue SHALL bind the Chat_Interface to the 3tched_Identity returned by `identity_sled.get_own_identity` through the 3tched_View the Chat_Route uses on the VPS, where the container's identity lives in the mounted `identity_sled`.
3. THE Chat_Interface SHALL import, copy, wrap, call, or adapt no legacy chat implementation.
4. WHEN the Implementer encounters chat-related code, THE Implementer SHALL add the path, symbol, pinned commit, callers, and classification to the Legacy_Chat_Inventory.
5. WHEN a Legacy_Chat_Inventory item is classified as legacy chat code, THE Implementer SHALL remove its source, registration, schema entry, generated code, service, route, user interface, test, fixture, and current documentation.
6. WHEN a legacy chat removal is completed, THE Implementer SHALL leave no compatibility shim or unreachable remnant.
7. IF a chat-related item is an identity, grant, credential, storage, lifecycle, receipt, tracker, or threetched-service binding, THEN THE Implementer SHALL preserve the item and record the preservation classification.
8. WHEN the fresh Chat_Interface is deployed to the Cockpit, THE Cockpit SHALL route chat only through the fresh Chat_Interface.
9. IF the fresh Chat_Interface cannot bind to the Identity_Binding through the Mounted_Views, THEN THE Implementer SHALL preserve the Cockpit and create a Blocker_Record.
10. THE Implementer SHALL treat deployment, cutover, legacy removal, catalog resealing, and service restart as Live_Changes.

### Requirement 12: Retain and decouple ctl_plane_chatbot

**User Story:** As the owner, I want the existing control-plane plugin retained
and exposed through the fresh chat interface, so that control-plane functions
remain available without depending on threetched internals.

#### Acceptance Criteria

1. THE Implementer SHALL retain the `ctl_plane_chatbot` plugin.
2. THE Chat_Interface SHALL discover `ctl_plane_chatbot` methods from the Sealed_Catalog in the 3tched_View.
3. THE Chat_Interface SHALL invoke each `ctl_plane_chatbot` method through its Operation_File in the 3tched_View.
4. THE Chat_Interface SHALL apply capability, schema-validation, mutation-confirmation, tracker, Receipt, and Authoritative_State requirements to every `ctl_plane_chatbot` Tool_Call.
5. THE Implementer SHALL remove `ctl_plane_chatbot` source dependencies on chat-router types, size limits, and plugin identifiers.
6. THE Implementer SHALL preserve every `ctl_plane_chatbot` method and record field not coupled to the removed dependency.
7. WHEN `ctl_plane_chatbot` decoupling is reported deployed, THE Implementer SHALL provide a Sealed_Catalog entry for the retained plugin.
8. WHEN `ctl_plane_chatbot` decoupling is reported deployed, THE Implementer SHALL provide one granted read Tool_Call with valid Tracker_Evidence, Receipt, and Authoritative_State.
9. IF decoupling changes a `ctl_plane_chatbot` schema contract beyond removal of the legacy coupling, THEN THE Implementer SHALL stop the change and create a Blocker_Record.

### Requirement 13: Preserve the threetched service boundary

**User Story:** As the owner, I want threetched-backed services kept separate
from fresh 3tched chat code, so that an executable upgrade affects only the
threetched view.

#### Acceptance Criteria

1. THE Implementer SHALL preserve services run by `/usr/bin/threetched` as threetched services.
2. THE Implementer SHALL exclude threetched services from legacy chat removal solely because a current or historical name contains `chat`.
3. THE Chat_Interface SHALL depend on no internal type, constant, store, protocol, or source module of a threetched service.
4. THE Chat_Interface SHALL discover threetched methods only from the Threetched_View Sealed_Catalog.
5. THE Chat_Interface SHALL call threetched methods only through Operation_Files in the Threetched_View.
6. IF a threetched method is absent from the Threetched_View, THEN THE Chat_Interface SHALL mark the method unavailable and use no alternate path.
7. IF a threetched upgrade changes only the Threetched_View schemas, THEN THE Chat_Interface SHALL leave 3tched_View tool definitions unchanged.
8. THE Legacy_Chat_Inventory SHALL record defective historical names of threetched services without using those names as new interface names.

### Requirement 14: Keep lungs as the only host channel

**User Story:** As the owner, I want the cockpit's host boundary unchanged, so
that fresh chat integration adds no host channel or inbound network surface.

#### Acceptance Criteria

1. THE Cockpit SHALL use the Lungs_Socket as its only host channel.
2. THE Chat_Interface SHALL add no host socket, proxy device, network interface, shared host directory, or host API to the Cockpit.
3. THE Chat_Interface SHALL add no listening TCP socket, listening UDP socket, or inbound network door to the Cockpit.
4. THE Chat_Interface SHALL initiate no direct connection from the Cockpit to a host address, Door_8090, D-Bus endpoint, container-runtime endpoint, or backend store.
5. THE Chat_Interface SHALL use the Mounted_Views for every product operation.
6. THE Cockpit SHALL preserve the existing read-only lungs directory mount and guest socket location.
7. WHEN deployment is verified, THE Runtime_Record SHALL compare the Cockpit's host channels and listening sockets before and after deployment.
8. IF deployment adds a host channel or listening socket, THEN THE Implementer SHALL report deployment as incomplete and create a Blocker_Record.
9. THE Chat_Route serving the 3tched_Identity SHALL run on the VPS host and not inside the Cockpit (owner, 2026-11-09); its one listener (identity-chat-interface Requirement 32.8) is on the host and SHALL NOT count as a Cockpit listener under criteria 3 and 8.

### Requirement 15: Exclude product MCP

**User Story:** As the owner, I want no MCP component in the cockpit product, so
that the existing operation-file control plane remains the only control plane.

#### Acceptance Criteria

1. THE Chat_Interface product code SHALL contain no MCP server.
2. THE Chat_Interface product code SHALL contain no MCP client.
3. THE Chat_Interface product code SHALL contain no MCP transport.
4. THE Chat_Interface product dependencies SHALL contain no MCP integration used by the product.
5. IF a package exposes optional MCP functionality, THEN THE Chat_Interface SHALL leave that functionality disabled and unimported.
6. THE Chat_Interface SHALL use Operation_Files as its only tool execution interface.

### Requirement 16: Validate mutations and require confirmation

**User Story:** As the cockpit operator, I want every mutation validated and
confirmed, so that model output cannot change control-plane state by itself.

#### Acceptance Criteria

1. WHEN a Tool_Call targets a Mutation_Tool_Call, THE Chat_Interface SHALL validate the input against the Sealed_Catalog argument schema before requesting confirmation.
2. WHEN a Mutation_Tool_Call input is valid, THE Chat_Interface SHALL hold the validated input in server-side state bound to the Tool_Call `operation_id`, 3tched_Identity, Main_Session, plugin, method, `subid`, and Catalog_Pin.
3. THE Chat_Interface SHALL execute a Mutation_Tool_Call only after an explicit operator confirmation matches the held Tool_Call values.
4. THE Chat_Interface SHALL accept no model text, tool result, message text, local UI state, or filesystem state as operator confirmation.
5. WHEN a confirmation is accepted, THE Chat_Interface SHALL consume the confirmation for exactly one Tool_Call.
6. IF a confirmation value differs from the held Tool_Call values, THEN THE Chat_Interface SHALL reject the confirmation and leave the Tool_Call unexecuted.
7. IF the Catalog_Pin changes after confirmation and before execution, THEN THE Chat_Interface SHALL discard the confirmation and leave the Tool_Call unexecuted.
8. WHEN a confirmed Mutation_Tool_Call executes, THE Tool_Executor SHALL apply Requirements 6 through 9 without exception.

### Requirement 17: Handle denials as permission boundaries

**User Story:** As the owner, I want each denial recorded and respected, so that
the cockpit cannot bypass grants or identity policy.

#### Acceptance Criteria

1. WHEN a Denial occurs, THE Implementer SHALL create a Blocker_Record containing the operation, caller identity, caller session, plugin, method, `subid`, `operation_id`, Catalog_Pin, missing capability or policy, redacted error, and timestamp.
2. WHEN a Denial occurs, THE Tool_Executor SHALL stop the Tool_Call.
3. WHEN a Denial occurs, THE Chat_Interface SHALL mark the denied tool unavailable for the remainder of the chat session.
4. WHEN a Denial occurs, THE Chat_Interface SHALL preserve every unrelated pending Tool_Call unchanged.
5. IF a Tool_Call receives a Denial, THEN THE Tool_Executor SHALL make no further call for that Tool_Call through another Operation_File, view, transport, identity, direct store, raw container-runtime command, or local substitute.
6. IF a denied tool is requested again in the same chat session, THEN THE Chat_Interface SHALL reject the request without an Operation_File write.
7. IF a Denial error contains Credential_Material, THEN THE Blocker_Record SHALL replace the Credential_Material with a redaction marker.
8. THE Blocker_Record SHALL contain no digest computed from Credential_Material.

### Requirement 18: Protect credentials and identity attribution

**User Story:** As the owner, I want credential material kept out of chat,
records, and logs, so that the cockpit's sealed identity remains confidential.

#### Acceptance Criteria

1. THE Chat_Interface SHALL exclude Credential_Material from model context, logs, errors, telemetry, UI output, persisted chat state, Tracker_Evidence projections, Runtime_Records, and Blocker_Records.
2. THE Chat_Interface SHALL hold no SID1 bytes in browser storage, URLs, rendered content, or service-worker caches.
3. WHEN Credential_Material is detected in an outbound item, THE Chat_Interface SHALL block the whole item before any byte reaches the destination.
4. WHEN Credential_Material is detected, THE Chat_Interface SHALL record only the credential class, blocked channel, 3tched_Identity display name, and timestamp.
5. IF a credential check fails or times out, THEN THE Chat_Interface SHALL withhold the checked item.
6. THE Repository SHALL contain no Credential_Material.
7. THE Implementer SHALL provide a repository check that reports credential-class findings without reporting matched values.
8. THE Chat_Interface SHALL derive caller attribution only from Guest_Kernel_Identity and `identity_sled.get_own_identity` evidence.

### Requirement 19: Authorize every live change

**User Story:** As the owner, I want each live change separately authorized, so
that requirements work and source changes cannot silently alter the cockpit.

#### Acceptance Criteria

1. WHEN a task would perform a Live_Change, THE Implementer SHALL require an Owner_Authorization naming the action, target identity, target Cockpit, affected files or services, and expected Operation_Files.
2. IF no matching Owner_Authorization exists, THEN THE Implementer SHALL record the planned action and leave the Live_Change unperformed.
3. IF the actual target, identity, session, service, mount, operation, or argument differs from the Owner_Authorization, THEN THE Implementer SHALL stop before acting and ask the owner to authorize the difference.
4. THE Implementer SHALL treat legacy chat removal, pinned-source modification, catalog resealing, build activation, deployment, service restart, mount change, and chat cutover as separate Live_Changes.
5. THE Implementer SHALL limit work without Owner_Authorization to read-only discovery and reversible source changes in the Repository.
6. THE Implementer SHALL treat a Denial as no authorization to change grants, identities, credentials, storage, or routes.
7. THE Implementer SHALL preserve the Cockpit while a Live_Change remains unauthorized or blocked.

### Requirement 20: Verify mounted-view deployment from inside the cockpit

**User Story:** As the owner, I want both mounted views verified under the
cockpit's guest kernel, so that mounted projection existence and correct
identity binding are established separately.

#### Acceptance Criteria

1. WHEN Mounted_Views deployment is verified, THE Implementer SHALL run the existing guest-kernel describe interface from inside the Cockpit for each view.
2. WHEN Mounted_Views deployment is verified, THE Implementer SHALL record the Guest_Kernel_Identity attributed to each view.
3. WHEN Mounted_Views deployment is verified, THE Implementer SHALL require both views to report the same Sealed_Catalog hash and generation.
4. WHEN Mounted_Views deployment is verified, THE Implementer SHALL require the views to report distinct kernel mount sources.
5. WHEN the 3tched_View is verified, THE Implementer SHALL perform one granted Read_Tool_Call for a 3tched operation.
6. WHEN the Threetched_View is verified, THE Implementer SHALL perform one granted Read_Tool_Call for a threetched operation.
7. WHEN a view read is reported verified, THE Implementer SHALL provide valid Tracker_Evidence, Receipt, and Authoritative_State for the read.
8. IF a view lacks Guest_Kernel_Identity, valid Tracker_Evidence, a Receipt, or Authoritative_State, THEN THE Implementer SHALL report that view as deployment-unconfirmed.
9. IF a view exposes an operation assigned to the other View_Kind, THEN THE Implementer SHALL report a routing defect and create a Blocker_Record.
10. THE Implementer SHALL accept no host-only mount observation or SHM_Runtime_Mirror read by itself as proof of Guest_Kernel_Identity binding or Tool_Call completion.

### Requirement 21: Verify clean-slate cutover and remaining functionality

**User Story:** As the owner, I want the fresh chat cutover and legacy removal
verified together, so that the existing cockpit remains functional with no old
chat path.

#### Acceptance Criteria

1. WHEN legacy chat removal is reported complete, THE Implementer SHALL provide a repository search showing no active reference to each removed symbol, path, plugin identifier, service name, route, schema entry, test, fixture, or current documentation entry.
2. WHEN catalog resealing is reported complete, THE Implementer SHALL provide the Sealed_Catalog hash and generation showing each removed chat plugin absent.
3. WHEN cutover is reported complete, THE Implementer SHALL provide the `identity_sled.get_own_identity` Tracker_Evidence, Receipt, and Authoritative_State from the Cockpit's 3tched_View.
4. WHEN cutover is reported complete, THE Implementer SHALL provide one granted `ctl_plane_chatbot` read with Tracker_Evidence, Receipt, and Authoritative_State.
5. WHEN cutover is reported complete, THE Implementer SHALL provide one granted threetched read with Tracker_Evidence, Receipt, and Authoritative_State from the Threetched_View.
6. WHEN cutover is reported complete, THE Implementer SHALL show that the Cockpit name, Native_UUID, Main_Session, Identity_Binding, Grant_Set, Seed, Leaf, Seven_Subvolumes, and host-channel inventory equal their pre-cutover values.
7. IF any criterion in this requirement lacks evidence, THEN THE Implementer SHALL report cutover as incomplete.
8. IF removal or cutover breaks the Cockpit, `ctl_plane_chatbot`, or either Mounted_Views view, THEN THE Implementer SHALL stop deployment and create a Blocker_Record.

### Requirement 22: Report implementation status by evidence stage

**User Story:** As the owner, I want status separated by evidence stage, so that
mounted projection existence, guest-kernel identity binding, and Tool_Call
completion are reported as separate claims.

#### Acceptance Criteria

1. THE Status_Report SHALL state proposed, implemented, built, tested, and deployed status separately for each component and task.
2. WHEN a component is reported implemented, THE Status_Report SHALL cite the source files and task identifiers that implement it.
3. WHEN a component is reported built, THE Status_Report SHALL cite the scoped build command, target, exit status, and Owner_Authorization or owner request.
4. WHEN a component is reported tested, THE Status_Report SHALL cite the explicit owner request, test command, and exit status.
5. WHEN a component is reported deployed, THE Status_Report SHALL cite Guest_Kernel_Identity, Tracker_Evidence, Receipt, and Authoritative_State evidence.
6. IF deployed evidence is incomplete, THEN THE Status_Report SHALL report the component as deployment-unconfirmed or incomplete.
7. THE Status_Report SHALL list every open Blocker_Record and the dependent tasks held by that blocker.
8. THE Status_Report SHALL distinguish mounted-projection existence, current Committed_SHM_State observations, Guest_Kernel_Identity binding, and verified Tool_Call completion evidence.

### Requirement 23: Keep changes scoped and dependencies pinned

**User Story:** As the owner, I want the cockpit work narrowly scoped, so that
fresh chat integration does not change unrelated control-plane components.

#### Acceptance Criteria

1. THE Implementer SHALL make the smallest source change that satisfies an authorized task.
2. THE Implementer SHALL leave pinned identity, storage, grant, lifecycle, tracker, 3tched-fs, and container architecture unchanged outside the clean-slate exception and the D-48 visibility-gating exception named in the Introduction.
3. THE Implementer SHALL copy no pinned dependency source into the Repository.
4. THE Implementer SHALL record each pinned dependency path and commit in the Baseline_Record.
5. WHEN a scoped build is explicitly requested, THE Implementer SHALL build only the required target for `x86_64-unknown-linux-gnu`.
6. THE Implementer SHALL run tests, linting, formatting checks, type checks, and smoke fixtures only on explicit owner request after deployment.
7. THE Implementer SHALL run no workspace-wide Cargo command without an explicit workspace request.
8. WHILE one Cargo process is running, THE Implementer SHALL start no other Cargo process.
9. THE Implementer SHALL modify no file owned by `/srv/git/kiroween/.kiro/specs/cockpit-lxd-standalone/` while implementing this specification.
