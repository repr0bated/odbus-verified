# Group security levels and capability boundaries

2026-10-12T08:55Z · OD-119 · Codex `/root/learning_runtime` · design for review, no grant rollout.
Source checkout: `34ff9a502d953bb7b1520d530eed849b507e266a` plus shared uncommitted work.

Integration owner: Ultra's existing OD-06 schema-tagged-routing work, with Luna
as a delegated classification/audit worker. This matrix is a proposed extension
of that existing OSCAL/routing policy chain, not a separate classification
registry or permission framework. Current field assessment, policy publication
and shadow-gate implementation must be reused. The canonical target-dispatch
consumer and reviewed L1-L4 restrictions remain integration work.

## Controlling interpretation

The owner's correction supersedes the earlier ascending-responsibility proposal: **ROOT is the existing sole control-plane chatbot; AGENT-L1 is the highest delegated security tier; AGENT-L4 is the most restricted.** The agent levels describe permitted authority, not job titles. Coordinator, evaluator and analyst responsibilities can run within any appropriate assigned tier. ROOT orchestrates and delegates, never directly executes changes. No new ROOT identity is proposed.

The existing ROOT chatbot must be running and must supply the UI generator's orchestration and identity; generated operations go to authorized delegates. This is the owner's requirement, not verified running state. Preserve the human requester's attribution and verified backend identity binding; neither a frontend claim nor a newly minted ROOT may substitute for them.

Keep three independent decisions explicit:

- **Authority:** verified principal, explicitly assigned groups/direct exceptions, exact sealed capability and method, and a task ceiling. A tier permits consideration of an operation; it does not grant it.
- **Resource scope:** project/session, document or other resource IDs, authoritative tags, permitted fields and purpose. Tags select policy; model-supplied labels cannot grant authority or establish trusted provenance.
- **Autonomy:** which already authorized steps may run without another prompt, bounded by existing approval scope, time, cost and cancellation. More autonomy never increases capabilities or resource access.

## Proposed responsibilities using existing names

Examples below are **observed method declarations**, not assignments or proof that any group can invoke them. Read each capability together with its listed scope and the coarse-capability gaps below. There is no implicit L1→L2→L3→L4 inheritance, no nested group, and no wildcard grant.

| Existing group | Security responsibility / illustrative permitted authority | Actual declared method → capability example | Required scope and approval |
| --- | --- | --- | --- |
| `$ROOT` | Existing singleton chatbot, including UI-generator orchestration; selects constrained delegates and reviews evidence; no direct execution of changes. | [`cognitive_mcp.schema_read`](/run/mount/3tchedFS/plugins/cognitive_mcp/operations/schema_read/metadata.json) → `cognitive_mcp.read` supports discovery. No dedicated constrained-delegation method was identified in the seven inspected declarations. | Preserve human requester and delegated actor; pin task/resources/allowed operations/budget. Discovery is not permission to call every method exposed. Do not substitute `workflow_run` for a proven delegation-only boundary. |
| `$AGENT-L1` | Highest delegated trust: may coordinate approved multi-step changes and scoped recovery, within an explicit ceiling; cannot grant authority or approve itself. | `workflows.query_workflows` → `workflows.read`; `workflows.cancel_workflow` → `workflows.write` illustrates bounded intervention, not blanket workflow control. | Named workflow/execution, approved recovery action and stop conditions. Delegate only within existing authority; independently authorize each executing principal. No group/grant editing or automatic governance powers. |
| `$AGENT-L2` | Bounded executor: may carry out specified mutations with known targets and postconditions. | `project.put_document` → `project.write`; `workflows.start_workflow` → `workflows.write`. | Named project/collection/document or workflow, exact allowed inputs, current ACL and approval scope; verify receipt/result. Start/cancel rights cannot currently be separated by `workflows.write` alone. |
| `$AGENT-L3` | Preparation tier: may draft changes and perform isolated checks, without applying live changes or promoting its own work. | `json_render.validate_spec` → `json_render.read`; `json_render.save_candidate` → `json_render.gallery` is an optional scoped artifact write. | Isolated/spec-only validation; owned candidate and session. Save-only gallery authority is missing from this coarse capability; withhold it unless an additional enforced restriction supplies that boundary. |
| `$AGENT-L4` | Most restricted: scoped observation and analysis; no live mutation, publication or policy change. | `cognitive_mcp.schema_read` → `cognitive_mcp.read`; `agent_config.get_config` / `list_tools` → `agent.read`. | Approved plugin/data visibility and bounded retrieval. Audit all other methods sharing a capability before assigning it. `project.read` and `json_render.gallery` are unsuitable as read-only guarantees by name alone. |
| `$ADMIN` | Human routine administration within assigned projects/resources; does not automatically own system governance. | `project.invite` / `revoke_member` / `select_agent` → `project.admin`. | Existing project-admin ACL, exact target session/agent, invitation role and lifetime. Approval follows existing scope; unrelated projects and global policies remain excluded. |
| `$SUPERADMIN` | Human governance and sensitive approval; separation from a proposing agent remains mandatory. | `json_render.promote_candidate` → `json_render.promote`; `elevation.authorize_task` → `elevation.authorize`. | Review exact candidate/change; protected call needs its own grant plus a one-use ticket for the same principal/session/method/arguments. A human's ticket is not transferable to an agent. Use the existing secure authorization surface; never put credentials in model context. |
| `$EXTERNAL-MCP` | Relationship/access channel for approved external clients, potentially vendor/partner use; not an execution tier. | `json_render.get_component_schema` → `json_render.read` is a possible individually authorized discovery example. | Authenticated external principal, explicit method/data/task scope. MCP transport grants no additional authority; preserve intentional human/machine binding paths. This example is not an existing EXTERNAL-MCP grant. |
| `$VENDOR` | Relationship for a supplier's explicitly assigned resources; no automatic admin or executor status. | `project.list_documents` → `project.read` is a possible assigned-project example. | Project membership plus document visibility; capability also admits `accept_invite`, so it does not by itself enforce read-only vendor access. Partner is not a separate current group. |
| `$GUEST` | Minimal expressly shared access; no implicit public data, execution or invitations. | `json_render.list_components` → `json_render.read` is a possible approved catalog example. | Only approved catalog visibility; evaluate all capability-sharing methods first. Guest membership is never inferred from an unknown principal. |

An L4 evaluator can inspect an authorized result without deployment rights; an L3 validation worker can check the ROOT UI generator's proposed interface without executing its controls. The same worker role may receive a separately approved L2 task when execution is needed. The level is not self-selected, and changing it never substitutes for explicit capabilities, resource checks or approval.

## Proposed two-person protection for exceptional SUPERADMIN actions

The owner's intended boundary is broad SUPERADMIN authority with independent human approval for exercising that privileged change authority. Examples include privileged membership/grant expansion, enforcement or audit-retention policy changes, and destructive recovery or system-wide security changes. This example list is not an exhaustive approved policy. Routine actions authorized within an ADMIN scope follow that scope's policy; SUPERADMIN membership does not bypass any gate. Both approvers must be distinct verified human principals; bind their approvals to the same canonical operation hash, resource scope, expiry and policy/schema versions. Changed requests invalidate approvals. No chatbot vote, self-approval or two sessions of the same person satisfies the requirement. Record decisions in the immutable ledger; approval never permits rewriting audit history.

The proposed UI presents a timed approval request, similar to an authenticator prompt: the second human reviews the exact action and scope, authenticates, then authorizes that one operation. Authentication proof and operation authorization remain distinct. Use a configurable deadline; expiry or any bound-request change invalidates the approval. Consume it once at execution after rechecking both humans' current authority. No generic reusable OTP grant and no ROOT approval satisfy this flow.

This is proposed policy, not implemented quorum. The current owner is solo: **do not activate it until a second authorized human and a reviewed recovery path exist**. Existing exact-task elevation is single-party protection and provides no evidence of two-person approval.

## Observed declarations and coarse authority gaps

Read-only discovery used `/run/mount/3tchedFS/plugins/<plugin>/schema.json` and its `operations/<method>/metadata.json` projection. No operation call was written. Observed versions: cognitive_mcp **4.1.0**, project **1.1.0**, workflows **2.0.0**, agent_config **2.0.0**, json_render **3.0.0**, snowball **2.0.0**, elevation **1.0.1**. These are this mounted namespace's declarations, not an assertion about another release or a successful invocation.

- `project.read` covers read methods **and** `accept_invite` (mutation); `chat.context` covers `get_context`/`resolve_context` **and** `select_context` (mutation).
- `json_render.gallery` covers list/get **and** save/delete; `json_render.promote` covers promote/retire. Candidate ownership and promotion are separate checks, not implied by the proposed tier.
- `workflows.write` covers start/pause/resume/cancel; `agent.invoke` covers register/reset/update configuration. No start-only, cancel-only or harmless-config-only capability was found in these declarations.
- `snowball.read` includes `query_events` and `get_current_state`; do not equate it with own-session audit visibility. `snowball.invoke` combines snapshot/rollback/retention changes; `rollback` explicitly restores the recovery image, not live provider state. It is not a generic L1 recovery grant.
- `routing`, `schema_tag_registry` and `tag_router` schema paths were absent in this namespace. Source drafts cannot supply current sealed routing-method examples or prove tag enforcement.

Projected object hashes (read as published, not independently recomputed):

| Plugin | `object.hash` |
| --- | --- |
| cognitive_mcp | `46384ebeacf133ea9e0ad646ceba1f2a85f26c9b23155f1c02cdc8599d7a9546` |
| project | `c9368f96f92c24c6e0b595394d6e39647185446ccf2f74a7eda8f065baba4fb7` |
| workflows | `24d041d5c43bb041d3e4c89c3af3f00fc1ef19c9e47de96360abf3d3fa82ad0e` |
| agent_config | `9b20818a111d2417e4f01e821978247ab3ab908f15ae2ae61782ca9bb7ca77ce` |
| json_render | `76d51b6b0465a0706eae670921857266e94f73f0f35eb8285e0de6697b3e0bbd` |
| snowball | `45e94b734b0aaf0a458426dddeb36563ace85c722cbb471c0da4493fb30e0f63` |
| elevation | `ba9a0b09773a3850c646b72dc85a1670b51ae59fc068b69fa9f88b96855cb7ae` |

## Existing enforcement and limits — source review only

1. **Groups are unions, not security ceilings.** [interceptor.rs:1706](../crates/op-grpc-bridge/src/interceptor.rs#L1706) declares exactly ten names; [line 1837](../crates/op-grpc-bridge/src/interceptor.rs#L1837) unions direct and explicitly named-group capabilities. The current shared-validator change rejects malformed documents before expansion, but no tier ordering, singleton ROOT count, resource restriction or L1–L4 ceiling is imposed here. Multiple group membership and direct exceptions can widen the union. A proposed tier ceiling must remain an additional deterministic admission requirement, not a claim that these groups already enforce it.
2. **ROOT's full non-execution boundary remains unproven.** [mcp_policy.rs:254](../crates/op-grpc-bridge/src/mcp_policy.rs#L254) denies the singleton's `dbus_call` and selected direct-tool prefixes. This narrow guard does not demonstrate that every mutation path is prohibited; the group name alone supplies no such guarantee.
3. **Project limits have real but bounded connections.** [project_dispatch.rs:257](../crates/op-grpc-bridge/src/project_dispatch.rs#L257) rejects a creation ceiling exceeding the creator's grants; [project.rs:154](../crates/op-plugins/src/state_plugins/project.rs#L154) intersects ceiling and caller grants. [agent_catalog_dispatch.rs:43](../crates/op-grpc-bridge/src/agent_catalog_dispatch.rs#L43) checks allowed agents and effective capabilities for selected-project catalog-agent calls. Project dispatch also rechecks identity, exact grant and ACL ([lines 194 and 310](../crates/op-grpc-bridge/src/project_dispatch.rs#L194)). This is not evidence that every generic plugin invocation inherits that project ceiling or enforces document-tag redaction.
4. **Elevation is exact-call protection, not universal approval.** [task_elevation.rs:103](../crates/op-grpc-bridge/src/task_elevation.rs#L103) consumes a ticket matching principal/session/plugin/method/arguments; [mutation_engine.rs:3721](../crates/op-grpc-bridge/src/mutation_engine.rs#L3721) connects consumption before audited dispatch. The mounted elevation declaration lists router configuration/migration and catalog promotion/retirement protection. It does not establish approval gates for every workflow or resource mutation. Existing human tickets cannot authorize a different delegated actor.
5. **OSCAL/compliance coverage is partial.** [mutation_engine.rs:3634](../crates/op-grpc-bridge/src/mutation_engine.rs#L3634) applies Cozo deny rules if the cognitive store is available. [op-cozo-store/lib.rs:722](../crates/op-cozo-store/src/lib.rs#L722) matches plugin/operation deny rules; absent matches allow, and query errors also allow at line 767. OSCAL identifiers alone do not establish authorization, complete regulatory coverage or fail-closed enforcement for mandatory policy. Live policy rows and end-to-end rejection remain unverified.

## Grant normalization proposal — no changes applied

Tracked defaults ([capability-grants.json](../deploy/security/capability-grants.json)) currently give ROOT 48 capabilities and SUPERADMIN 209; ADMIN, L1–L4, EXTERNAL-MCP, VENDOR and GUEST each contain the same single system-graph read capability. They do not implement this matrix. The two listed principals duplicate their respective ROOT/SUPERADMIN group capabilities directly. This is repository policy evidence, not a fresh live-membership inventory.

For a separately approved migration: capture the authoritative current document/version and rollback copy; compute each principal's **effective set = direct capabilities ∪ explicitly assigned group capabilities**; identify which duplicate direct entries are accidental and which are intentional exceptions. Remove only approved redundancy, validate the complete document, and require exact before/after effective-set parity for every principal before publication. Keep intentional exceptions documented: they deliberately survive later group revocation. A mismatch, concurrent policy change or unclassified exception stops the migration.

Use the existing publication owner ([op-grants-materializer.rs:16](../crates/op-grpc-bridge/src/bin/op-grants-materializer.rs#L16)), preserve credentials and memberships, and keep rollback to the exact previous document. At the owner's execution window, verify expansion, revocation and unaffected principals, then record policy version and evidence. No direct SHM edit, new grant writer, alternate schema or live entitlement change is proposed by this report.

Loaded guidance: the nine mandatory backend skills were already read in this agent session. This pass read declarations and source only; it did not execute tests/builds, call protected methods, change grants, seal blobs, deploy services or prove live enforcement.

Conceptual reference for roles and separation of duties: [NIST RBAC FAQ](https://csrc.nist.gov/projects/role-based-access-control/faqs). This report's particular tiers and approval choices are project proposals, not requirements attributed to NIST.
