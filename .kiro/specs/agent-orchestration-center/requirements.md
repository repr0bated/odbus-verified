# Agent Orchestration Center — Requirements

## Status and scope

**Owner-authorized orchestration implementation, 2026-11-19; in progress.** This workstream owns recipes/pieces, native workflow/workstack orchestration, delegation/caller cutover, tracker/core integration and backend docking contracts. Another GPT owns canonical PluginSchema/FUSE/schema delivery. The user owns the KiroWeen UI; no frontend edits or restoration belong here. Source is not build/deployment evidence. All builds/checks are on explicit hold pending owner coordination of four streams: no Cargo/rustc/npm/Vite/typecheck/test/lint/fmt/smoke/bench/API probe. No live credential/grant/container/storage/seal/service mutations. Serial selective task commit/push is authorized through the single parent delivery coordinator only.

The center coordinates TWO SEPARATE native systems: **Workflows**, user-authored object-node graphs, and **Workstacks**, behind-the-scenes task intent/utility orchestration selecting and coordinating agents. A workstack may choose a workflow as one task implementation, but neither system is a mandatory wrapper or synonym for the other. They share the agent registry/pieces, verified execution roles, Runtime admission, core receipts and tracker, not one universal policy scheduler. Reuse useful original 3tched design and safe threetched reasoning concepts without copying raw tool execution.

**Owner correction — mandatory call ingress:** every caller operation uses its identity-bound FUSE `operations/<plugin>/<method>/call` namespace, including workstack/workflow/agent, inference/judgment, graph management and checkpoint/resume operations. The bridge hosts the upgraded FUSE library with TWO SEPARATE mounts, `3tchedFS`/Our and `threetched`/Binary. Runtime is private callback backing/admission, not an alternative same-process caller API. All later references to Runtime calls/admission mean this private post-FUSE checkpoint path, never permission to bypass FUSE or add a raw client/socket.

**Current integration clarifications, 2026-11-19:** deterministic operation-only workflows SHALL NOT require unused inference/Jev groups, grants, routes or model budgets. Optional provider groups are coupled approved data; the actual pinned operation/semantic policy determines which are required. Inference is an external network/cost operation, never a harmless Read merely because state is unchanged. Its actual mounted receiver MUST prove container-owned or explicitly authorized identity-scoped provider/config/key backing; host read-only mirrors cannot execute it. A zero-tool chatbot may submit/read/cancel through its own narrow native admission; final-result readers are not impersonated as the source executor or required to hold provider rights. Final reasoning stays explicitly unverified rather than claiming factual proof.

Private executor/Jev arguments, raw output and their content-derived hashes SHALL NOT enter requestor SHM/projection. Exact content bindings stay in the private native frame/core attempt. A pending `arguments_binding_id` is an opaque non-bearer UUID, never a content hash or authority. Loss of a private original response frame requires `NeedsReview`, not reconstructed receipts, replay or a new inference request. The final getter is its own actual scoped read/checkpoint; original provider completion is not reclassified as agent-final success. See design §4.7 for requested canonical declarations and explicit undelivered owner hooks; no schema/FUSE-owner acknowledgement is assumed.


## Terms

- **Requestor:** verified human/session or chatbot submitting a scoped request.
- **Chatbot:** conversation role with narrow submission/status/cancellation-request permissions, no native tool execution rights and no direct internet access.
- **Agent definition/persona:** reusable behavior, knowledge, examples and declared capability needs; not an identity or container.
- **Execution role:** registered binding identifying which independently authorized executor may perform an agent run's operations.
- **Executor:** actual verified principal, anchored to its existing container identity, authorized under an explicit delegation policy.
- **Helper job container:** job-only system container, not an identity and never a recipient of identity secrets.
- **Operation:** one effect or read admitted by shared Runtime ExecutionAdmission with a canonical operation ID and durable BridgeCore proof.
- **Run/task/attempt:** orchestration work and its bounded scheduling attempts; none replaces a native operation receipt.
- **Workflow:** user-authored, versioned graph of discovered object nodes and typed port connections, managed by WorkflowController.
- **Workstack:** behind-the-scenes task intent/utility policy orchestration, managed by WorkstackController; it can coordinate agent runs directly or choose workflow runs, with separately owned state.
- **IntentSpec / UtilityPolicy:** typed desired outcome/constraints and scoped owner-approved decision policy; Jev supplies atomic semantic judgments, code computes feasible choices and utility.

## R1 — Separate native domain owners, shared execution authority

1. WHEN a native agent is discovered, THE center SHALL derive its definition and operation descriptors from the shared native registry and plugin-owned sealed schema rather than a second agent catalog.
2. WHEN a user-authored workflow is saved or run, THE center SHALL use WorkflowController to own its immutable object-node graph and graph-run lifecycle, reusing retained workflow mechanics without making it the workstack policy owner.
3. WHEN behind-the-scenes tasks are orchestrated, THE center SHALL use WorkstackController to own task intent, utility, agent coordination and its own lifecycle, sharing bounded run/lease primitives, Runtime admission, core receipts and tracker with WorkflowController rather than competing effect authorities.
4. WHERE an old file is unlinked from the retained module graph, THE implementation SHALL NOT restore it solely to satisfy historical architecture descriptions.
5. WHEN evolving orchestration, THE center SHALL preserve useful original native 3tched contracts for verified scope/control, attributed event delivery, typed graph ports/dependencies, scoped variable/context flow and tracker-centered observation, while adapting effect dispatch to retained Runtime and keeping workstack domain ownership separate.
6. WHERE existing intermediate-result or learned-pattern infrastructure is reused, THE center SHALL require scoped proof, matching input/schema/policy and valid read freshness for result reuse, SHALL NOT replace effect admission/receipts with cached output, and SHALL treat learned patterns only as reviewable recipe proposals rather than automatic method or authority promotion.

## R2 — Zero-execution chatbot and explicit delegation

1. WHEN the chatbot submits a request, THE center SHALL require a narrow submit permission and preserve verified requestor, session, project/resource scope, purpose and approved budget.
2. WHILE acting as the chatbot conversation role, THE system SHALL NOT expose or execute arbitrary plugin tools, shell commands, network operations, file operations or provider credentials under that role.
3. WHEN any chatbot user request is accepted, including reasoning-only conversation, THE center SHALL submit it to behind-the-scenes workstack task-intent orchestration and create an attributed agent run under an approved execution role; the chatbot SHALL submit, observe and relay only, without direct answering or the executor's tool grants.
4. WHEN a delegated operation is authorized, THE system SHALL enforce the executor's current exact method capability AND the policy's allowed operation, requestor resource/project scope, duration and budget.
5. WHEN no valid policy/executor binding exists, THE center SHALL return a scoped refusal or approval-required outcome without minting identities, borrowing a service/root caller or broadening grants.
6. WHEN two personas use the same authorized execution role, THE center SHALL permit reuse of the existing executor/container; it SHALL NOT require a new identity or container per persona or run.
7. WHEN inference, retrieval or embedding infrastructure supports a conversation, THE system SHALL perform that work through its independently authorized agent execution role and shared admission, not by secretly giving the chatbot execution rights.
8. WHEN model output recommends an agent, tool or policy change, THE system SHALL treat it as untrusted data, never as a grant or authorization decision.

## R3 — Shared admission, identity and two mounts

1. WHEN any native or external-backed caller operation is invoked from an agent, UI connector, controller, child or judgment worker, THE caller SHALL enter its identity-bound Our/Binary FUSE operation namespace, then the mandatory tracker checkpoint and private Runtime schema/grant/admission/receipt pipeline; direct door/nested/Runtime dispatch SHALL NOT be an alternative ingress.
2. WHEN a call uses an executor identity, THE system SHALL revalidate its anchored container binding and current authority at execution and disclosure boundaries.
3. WHERE FUSE is instantiated in the bridge or elsewhere, THE system SHALL retain TWO SEPARATE mounts: `3tchedFS` (`Surface::Our`) and `threetched` (`Surface::Binary`).
4. WHEN a helper job container is used, THE system SHALL give it only job-scoped inputs and a constrained execution channel; it SHALL NOT receive SID1/IID1/OIA1 material, identity storage, host grant documents or ambient executor credentials.
5. WHEN a sealed operation is missing or denied, THE system SHALL report that boundary and SHALL NOT route around it with a raw tool, subprocess, alternate RPC, MCP or direct database mutation.

## R4 — Durable receipt-linked run state

1. WHEN a run/task changes durable phase, THE center SHALL record that change through the existing BridgeCore ledger owner before publishing its scoped materialization.
2. WHEN an operation is dispatched, THE center SHALL link its admitted operation ID to run ID, task ID, attempt ID, requestor, executor, delegation policy revision and sealed schema hash.
3. WHEN a native operation succeeds, THE center SHALL require its attributed terminal receipt and authoritative observation; an agent success boolean or model assertion SHALL NOT suffice.
4. IF a terminal receipt is missing, a timeout occurs after dispatch, or an owner is lost, THEN THE center SHALL retain the operation as unresolved/outcome-unknown and SHALL NOT automatically replay its effects.
5. WHEN recovery is requested, THE center SHALL reconcile the original operation by its scoped receipt rather than allocating another operation ID for the same uncertain effect.
6. WHEN receipt state and tracker state disagree, THE center SHALL use the core ledger as effect authority and rebuild the tracker projection.

## R5 — Execution tracker integration and privacy

1. WHEN any tool call is admitted or refused, THE center SHALL produce a scoped, redacted lifecycle observation correlated with that call's operation ID; refusals before admission SHALL remain distinguishable from admitted operations.
2. WHILE an execution is nonterminal or unresolved, THE tracker SHALL NOT evict its durable run/task/operation reference to make room for history.
3. WHEN tracker capacity is exhausted, THE center SHALL reject new run admission or page terminal history rather than silently dropping active records.
4. WHEN a request waiter disconnects or is cancelled, THE detached native execution owner SHALL remain responsible for terminal tracking and reconciliation.
5. WHEN broadcasting, listing, logging or projecting execution state, THE tracker SHALL omit raw inputs, outputs, credentials, private prompt/code content and unsafe error strings by default, and SHALL apply current disclosure authorization.
6. WHEN terminal counters/events are derived, THE tracker SHALL count each terminal transition once and SHALL distinguish successful completion, failure-before-effect, denied, cancellation-requested, cancelled, interrupted and outcome-unknown.
7. IF a subscriber misses an event, THEN THE center SHALL recover from a scoped durable cursor/snapshot; an in-memory broadcast SHALL NOT be the event authority.

## R6 — Bounded event-driven scheduling

1. WHEN accepting a plan, THE center SHALL use schema-owned plan contracts and deterministic validation for a finite acyclic graph, unique task IDs, declared typed dependencies, compatible output bindings and all budget limits, including capacity for 64 total expanded tasks/nodes shared per ROOT request.
2. WHILE using the initial policy profile, THE center SHALL permit at most 64 total expanded tasks/nodes per ROOT request, shared across all child runs and both WorkstackController/workstack and WorkflowController/workflow domains with no fresh child or domain allowance; the limits of 64 resident active runs and 2 concurrent reasoning workers SHALL remain shared, and execution effects SHALL remain serialized by unchanged Runtime admission.
3. WHEN a task is claimed, THE center SHALL persist a lease owner, epoch/fencing token and deadline before dispatch and SHALL reject stale-owner completions.
4. WHEN an active task awaits a model, human approval, dependency, timer or child execution, THE controller SHALL release its own submitting/scheduling root Runtime gate before that wait; any actual provider/tool invocation SHALL retain normal Runtime admission and its unchanged gate, with no promise of parallel provider effects.
5. WHEN a worker needs a new child/tool/judgment caller operation, THE controller SHALL persist/defer continuation and end/release the ancestor root before initiating the new identity-bound FUSE call; correlation SHALL NOT inherit authorization or an execution gate across FUSE calls. Only private non-operation callback helpers may share the current gate.
6. WHEN a budget is exhausted, THE center SHALL stop scheduling new work, retain dispatched-operation reconciliation and publish a truthful bounded outcome.
7. WHEN a task completes or a control event arrives, THE scheduler SHALL wake dependent work through bounded events, not polling loops.

## R7 — Cancellation, interruption and shutdown

1. WHEN pause or cancellation is requested, THE center SHALL record the request durably and stop new dependent dispatch at the next safe boundary.
2. WHILE a tool may already have produced effects, THE center SHALL NOT describe cancellation, timeout or worker death as rollback or proof of no effect.
3. WHEN Runtime begins shutdown, THE center SHALL close new submission, retain the existing drain semantics for admitted operations and preserve uncompleted run phases for reconciliation.
4. WHEN the process restarts, THE center SHALL fence old leases and mark nonterminal owners interrupted until original receipts are reconciled; it SHALL NOT auto-resume uncertain effects.
5. WHEN an earlier effect succeeded but a later task failed, THE center SHALL retain each committed child proof and report partial progress without erasing it.

## R8 — Dynamic modular agents and bounded context

1. WHEN assembling a persona, THE center SHALL reuse `PieceLibrary`, `AgentPiece`, `PieceSource`, `TaskContext` and shared agent metadata, while keeping source, version, trust, access scope and content digest attributable.
2. WHEN selecting pieces or executor descriptors, THE center SHALL first filter by verified access, exact sealed capability bindings and delegation policy in deterministic code.
3. WHEN composing a prompt, THE center SHALL enforce one total model-context budget including framing, behavior, knowledge, examples, capabilities, activity, retrieved context, requested persona and reserved output/tool overhead.
4. IF the required trusted minimum cannot fit, THEN THE center SHALL return an explicit budget outcome rather than accepting an oversized first piece or adding an unbudgeted fallback.
5. WHEN memory or markdown contributes text, THE center SHALL treat it as scoped, provenance-bearing data and SHALL NOT let relevance weighting promote it into security policy or permissions.
6. WHEN replacing static persona duplication, THE implementation SHALL preserve useful catalog pieces and workflow recipes while removing obsolete parallel execution/dispatch conventions at cutover.
7. WHEN an authorized owner creates or updates a dynamic agent recipe, THE center SHALL validate its typed input/output contracts, piece references and registered execution-role binding, then publish an immutable version through the existing plugin-owned management surface.
8. WHEN a run is accepted, THE center SHALL pin its exact recipe version, piece digests, schema and policy revisions; an update SHALL NOT silently alter running work.
9. WHEN a recipe is disabled, THE center SHALL reject new runs and require current authorization before any further task dispatch; disabling SHALL NOT erase or undo already dispatched operations.
10. WHEN a recipe version is retired, THE center SHALL retain versions and provenance referenced by active or auditable runs and SHALL reject new pins to the retired version.

## R9 — TypeSafe semantic judgments, deterministic decisions

1. WHEN language understanding is needed to match a task to agents/pieces, THE center SHALL use atomic Jev typed questions over a deterministic, capability-filtered candidate set.
2. WHEN a Choice selects a candidate, THE center SHALL pair it with a companion Noul establishing that any candidate fits and SHALL handle low and uncertain probability explicitly.
3. WHEN more than one candidate can independently fit, THE center SHALL use separate per-candidate Nouls rather than forcing mutually exclusive judgments.
4. WHEN questions share state, THE integration SHALL batch those questions where appropriate and SHALL keep wording, criteria, thresholds, state format and versions together.
5. IF the API fails or returns invalid/missing answers, THEN THE center SHALL return `JudgmentUnavailable` or `NeedsReview` and SHALL NOT silently choose by keywords, random agent, static default or fabricated negative answer.
6. WHEN making an authorization, graph, budget, arithmetic or scheduling decision, THE system SHALL decide in code, not ask Jev to grant permission or run the orchestrator.
7. BEFORE any live API probe, THE operator SHALL explicitly approve its budget and disclosed data; no secret/private code shall be sent under this design work.
8. WHEN selecting models, THE system SHALL retain existing JEV routing; it SHALL NOT add a Cargo-specific model or separate shell/CLI model dispatcher.

## R10 — External threetched contract

1. WHEN using threetched functionality, THE center SHALL call only the installed catalog's discovered and sealed methods with their exact schema and capability, without assuming upstream internal Rust APIs are callable plugin methods.
2. WHEN reusing external reasoning-loop/delegation ideas, THE implementation SHALL adapt them as bounded native task behavior and SHALL NOT start a second autonomous tool loop with ambient credentials.
3. WHEN provenance is reported, THE center SHALL distinguish pinned upstream source, locally staged binary evidence, sealed catalog contract and actual deployed receipts.
4. IF a required external function is not exposed by a sealed method, THEN THE center SHALL report unsupported rather than invent a method, new proto, MCP endpoint or parallel schema directory.
5. WHEN an external reasoning adapter is registered, THE center SHALL require enforceable tool-free operation or an exclusive discovered sealed-operation tool surface whose every attempt re-enters Runtime, including recursive child, reflection, default and fallback paths.
6. IF the installed binary cannot enforce that mediation, THEN THE center SHALL reject its autonomous execution mode as unsupported; a prompt prohibition, empty upstream allowlist or outer Runtime receipt SHALL NOT count as complete tool mediation.
7. WHEN an adapter is enabled, THE center SHALL retain a support declaration pinned to the discovered contract, enforceable mediation mode and provenance; unsupported modes SHALL remain unavailable rather than stubbed.

## R11 — Scoped management, observability and retention

1. WHEN an authorized observer inspects a run, THE center SHALL expose its redacted definition, task dependencies, selected pieces, executor role, progress, budgets and safe receipt references through plugin-owned methods and canonical scoped projections.
2. WHEN another principal/session asks for a private run or operation, THE center SHALL deny disclosure or make it indistinguishable from absence according to the owning method contract.
3. WHEN an owner-approved observer views delegated work, THE center SHALL disclose only the policy-approved requestor-facing summary; the observer SHALL NOT acquire the executor's private method-recovery authority.
4. WHEN terminal history is retained or pruned, THE center SHALL apply the owning retention policy without deleting unresolved effects or breaking required audit references.
5. WHEN session evidence is indexed or vectorized, THE center SHALL preserve the session sled-ledger/Qdrant ownership model and SHALL NOT make tracker caches or Cozo indexes a replacement session authority.

## R12 — Honest design and manual acceptance

1. WHEN reporting this workstream, THE authors SHALL distinguish reviewed source, proposed changes, implemented behavior and deployed behavior.
2. WHEN implementation is reported, THE task list SHALL distinguish source implementation, schema/FUSE-owner handoff dependencies and separately requested manual acceptance; pending checks SHALL NOT be called passing.
3. WHILE the owner-coordinated build hold remains, THE workstream SHALL run no build/check/test/lint/format/benchmark/smoke/API experiment or operational mutation. Selective source commit/push is separately authorized only through the delivery coordinator.

## R13 — Workstacks are distinct from workflows

1. WHEN the center links work to execution, THE system SHALL keep workstack IDs/phases and their domain lifecycle distinct from workflow definitions, run IDs, task IDs and operation IDs.
2. WHEN a workstack or one of its phases requests a workflow or agent run, THE center SHALL retain an explicit reference and receipt relationship; it SHALL NOT rename the workstack as a workflow or move its domain state into the workflow execution store.
3. WHERE the current workstack control owner is absent or unlinked, THE implementation SHALL provide the proposed native WorkstackController and its typed plugin-owned contract; it SHALL NOT revive the old raw gRPC/tool engine or alias workstack state into workflow storage.
4. WHEN workstack state is observed or changed, THE center SHALL preserve its verified owner-defined ordering and control semantics; workflow DAG scheduling SHALL NOT silently override them.
5. WHEN a user designs a workflow, THE system SHALL expose node discovery, preview, validate, create/save, edit/new immutable version, list/get, disable and run/pause/resume/cancel through canonical plugin methods; graph authoring SHALL NOT imply execution authority.
6. WHEN the user builds the KiroWeen workflow editor, THE backend docking contract SHALL provide canonical schema-derived nodes/ports, graph management/run/control, scoped state/events/receipts and approval/input gates through the existing identity door translated to verified container-bound FUSE. The user owns UI implementation. This workstream SHALL NOT edit/restore frontend source, add another renderer/API, or expose browser host filesystem/mounts or identity secrets.
7. WHEN WorkstackController chooses an implementation, THE system SHALL allow either direct agent-task coordination or an explicitly referenced workflow run; it SHALL NOT force every task through a workflow or require every authored workflow to be wrapped in a workstack.

## R14 — Exhaustive specialist coverage and runnable recipes

1. WHEN the modular catalog is cut over, THE center SHALL represent every current registered native persona/specialist ID as a runnable versioned recipe, preserving its expertise, examples, source provenance and discoverable intent ID.
2. WHEN a persona is made runnable, THE system SHALL support reasoning and typed results without requiring shell/process privileges; only explicit execution profiles may direct side effects.
3. WHEN reconciling source inventory, THE implementation SHALL compare exact sets of registry IDs, descriptor IDs, generated markdown frontmatter and historical static declarations, classifying every alias, duplicate, absent artifact and unmatched ID explicitly.
4. WHEN a historical declaration is not a current factory entry, THE implementation SHALL preserve useful expertise as attributed seed content without reactivating its obsolete execution backend or silently claiming a new live alias.
5. WHEN a runtime selector shortlists candidates, THE full catalog SHALL remain addressable; no specialist SHALL disappear merely because it was not selected.
6. WHEN inventorying threetched configuration, THE center SHALL reconcile exact sets of every authorized-visible agent, risk-profile and runtime-profile map key, including disabled, unreachable and unused entries, preserving assignments and states under source-plus-kind-plus-alias identities; each entry SHALL be represented or explicitly unsupported without invented names or execution grants.
7. WHEN completing the cutover, THE implementation SHALL enforce unique stable recipe IDs and exact coverage equality, migrate all retained callers and remove obsolete parallel static persona execution paths rather than retaining permanent compatibility shims.

## R15 — Each eligible discovered object is a workflow node

1. WHEN constructing a workflow, THE center SHALL discover each eligible existing object from the canonical sealed catalog and supported introspection/object graph, and SHALL expose that object as a composable node rather than reducing the workflow to a list of tool names.
2. WHEN a node is defined, THE center SHALL pin its plugin ID, actual object path, schema hash/version, selected operation/read binding and policy revision, and SHALL derive typed input/output ports from that same canonical schema.
3. WHEN nodes are connected, THE center SHALL use explicit source-node/source-port to target-node/target-port edges with validated schema compatibility; missing ports, invalid defaults, cycles, stale pins and absent grants SHALL refuse admission.
4. WHEN a schema declares readable object properties, THE center SHALL read them through canonical committed SHM under current scope/read authorization; a state field SHALL NOT imply a writable mutation port.
5. WHEN a workflow mutates an object, THE center SHALL invoke only a declared admitted operation with verified executor authority and a durable receipt; no direct property/database write or raw D-Bus/tool fallback SHALL substitute.
6. WHEN an agent recipe participates as a node, THE center SHALL bind it to an actual discovered plugin-owned recipe object/record contract and pin its recipe version, while keeping object/node definition distinct from scheduled task, attempt and operation identity.
7. WHERE the real schema/object graph exposes eligible child objects, THE center SHALL support their actual declared bindings; it SHALL NOT invent child objects, services, methods or per-persona identities.
8. WHEN graph preview/discovery is exposed, THE center SHALL derive its typed metadata through the existing workflows PluginSchema and canonical object schemas; it SHALL NOT maintain a second node schema catalog or per-object proto/service.
9. WHEN adapting original `WorkflowNode`, `NodePort`, `NodeConnection` and `WorkflowDefinition` abstractions, THE implementation SHALL retain object-to-node composition and typed graph semantics while replacing their incomplete execution wiring with VerifiedCaller, Runtime and receipt contracts rather than enabling the blocked engine.

## R16 — Workstack task intent and deterministic utility orchestration

1. WHEN a workstack task is accepted, THE controller SHALL persist IntentSpec containing desired outcome, typed success evidence, requestor/project/resource scope, constraints, dependencies, budget and provenance before selecting an agent or workflow implementation.
2. WHEN evaluating alternatives, THE controller SHALL first filter exact capability/policy/resource/dependency/budget feasibility in code, then obtain only needed atomic semantic-fit Jev judgments over the authorized candidates.
3. WHEN ranking feasible alternatives, THE controller SHALL compute utility deterministically from a scoped versioned owner-approved UtilityPolicy with explicit feature meanings, weights, normalization, constraints and tie/uncertainty rules; it SHALL NOT invent production weights or ask a model to grant authority.
4. WHEN recording a choice, THE controller SHALL persist alternatives considered/rejected, feasibility reasons, semantic evidence, exact utility inputs/result, policy revision, chosen recipe/workflow pins, budget allocation and parent/child receipt links.
5. IF intent, required evidence, utility policy, cost/risk estimates or semantic fit is unresolved, THEN THE controller SHALL return NeedsReview or an explicit infeasible/unavailable outcome without silent heuristic fallback, grant expansion or effect dispatch.
6. WHEN a task's outcome or budget changes, THE workstack SHALL re-evaluate only eligible undispatched work under its pinned/explicitly revised policy and SHALL NOT replay uncertain prior effects or rewrite user workflow definitions.
7. WHEN the center exposes status/control, THE system SHALL keep workstack task/intent/utility state and workflow graph/run state distinct while joining them through typed references in one authorized durable management/tracker view.

## R17 — Resumable native approval and structured-input gates

1. WHEN a workflow or workstack needs approval or structured user input, THE owner SHALL commit a scoped pending continuation and end the active call without holding a Runtime gate, worker or process waiting for the answer.
2. WHEN presenting a gate, THE system SHALL bind its safe preview to exact pending plan/arguments hashes, domain/run/node/task/checkpoint revision, verified principal/session/container/delegation and required approver capability.
3. WHEN resuming, THE system SHALL accept exactly one approve, schema-valid JSON response or cancel decision, revalidate all authority/pins and consume the expected checkpoint revision atomically before new admitted work.
4. IF input JSON/schema is invalid, THEN THE checkpoint SHALL remain pending without consumption; concurrent duplicate, stale, changed-plan or revoked-authority resumes SHALL refuse without redispatching prior steps.
5. WHEN a resume locator is used, THE locator SHALL confer no bearer authority; pending gates SHALL be discoverable through authorized canonical scoped state rather than requiring possession of a secret token.
6. WHEN a continuation resumes, THE owner SHALL preserve prior receipts and may yield another gate, but SHALL NOT replay completed or unknown effects.
7. WHEN borrowing OpenClaw Lobster patterns, THE design SHALL retain native object graphs/PluginSchema and core persistence, not install a third DSL/runtime/service/state directory or permit arbitrary shell/env/token execution.

## R18 — Mandatory tracker-mediated appropriateness and authorization checkpoint

1. WHEN any tool call is attempted, THE system SHALL enter one mandatory fail-closed execution-tracker admission coordinator before dispatch, including native, agent, workflow, workstack, recursive, external, reflection, default and fallback calls.
2. WHEN preflighting a call, THE coordinator SHALL validate attributed intent/scope, exact schema/input and configured appropriateness policy, then invoke the existing deterministic Runtime grant/capability/delegation evaluator; missing or denied authority SHALL prevent dispatch.
3. WHEN semantic appropriateness requires understanding request intent or response relevance, THE coordinator SHALL obtain atomic typed Jev judgments and apply a total deterministic policy for match/no-match/uncertainty/error; no model answer SHALL grant rights or default silently to appropriate.
4. IF required pre-dispatch semantic judgment is uncertain, unavailable or negative, THEN THE coordinator SHALL record refusal/NeedsReview without effects; it SHALL NOT bypass the checkpoint or choose a heuristic fallback.
5. WHEN dispatch is admitted, THE coordinator SHALL bind the canonical operation ID and core durable attempt before executing once, then record the committed receipt/outcome before validating output and disclosure.
6. WHEN a tool returns, THE coordinator SHALL validate exact output schema, task/request relevance where required and current disclosure authority before releasing the response; withheld/NeedsReview results SHALL preserve prior effects/receipts and SHALL NOT cause replay.
7. WHEN judgment inference itself requires a model/tool operation, THE system SHALL track/admit it as a schema/policy-typed internal judgment class with deterministic validation and finite evaluation depth, without self-judgment recursion or exemption from tracking/authorization.
8. WHEN exposing Runtime execution APIs, THE implementation SHALL provide no reachable lower raw dispatch path bypassing this checkpoint; tracker and Runtime SHALL share one owner/coordinator without circular calls or parallel grant evaluators.
9. WHEN persisting tracker control decisions, THE system SHALL use existing core ownership/proofs and scoped materializations, not a second effect ledger/database/service; the tracker SHALL NOT mint credentials, expand grants or override Runtime authority.
10. WHEN grounding a request, THE checkpoint SHALL reject invented plugin/object/method/port/capability/argument bindings outside pinned authorized discovery; WHEN grounding a response, THE checkpoint SHALL require attributed receipts, valid domain outcomes and canonical state/version evidence for asserted effects.
11. WHEN an agent final response is released, THE same checkpoint SHALL bind factual/effect claims to authorized source receipt/state references and task-intent evidence; unsupported, missing, contradictory or semantically unresolved claims SHALL be withheld or explicitly unverified/NeedsReview, never fabricated success.
12. WHEN reporting semantic checks, THE system SHALL NOT claim Jev proves effects or guarantees hallucination-free output; model state SHALL be minimal and approved with no secret/private-code disclosure by default, and withheld output SHALL NOT erase committed effects or trigger replay.

## R19 — Mandatory identity-bound FUSE ingress for every caller

1. WHEN any caller initiates an operation, including internal agents/controllers/inference/judgment or checkpoint controls, THE caller SHALL use its correct identity-bound Our/Binary FUSE operation surface before the mandatory tracker/admission pipeline.
2. WHEN the FUSE callback dispatches an operation inside the bridge, THE private backing coordinator SHALL reuse Runtime's authoritative evaluator/core receipts; private helper calls SHALL NOT recursively serialize through FUSE unless initiating another caller operation.
3. WHEN a caller runs in the bridge process, THE system SHALL NOT permit direct Runtime::call, raw registry/tool, socket/RPC or database dispatch as an alternate ingress.
4. WHERE FUSE is instantiated in the bridge or elsewhere, THE system SHALL preserve separate Our and Binary mounts and the verified container identity; absent/denied operation SHALL fail closed, not trigger another transport.
