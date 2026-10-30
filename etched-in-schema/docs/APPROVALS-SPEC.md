# Approvals spec: two-approval root, policy co-signer, break-glass

> How these methods reach the identity: sealed blob → 3tchedFS operation file → call → receipt. See [`DEV-CYCLE.md`](DEV-CYCLE.md).

Part of the judged **policy** and **rule enforcement** work. Status: **planned**
(nothing here is built yet). Existing platform pieces are marked **existing**.

## 1. Goal

No root-level action runs on one say-so. Routine root actions need the owner
**and** a policy co-signer (the control-plane chatbot); anything outside
written policy is refused unless the owner uses an audited break-glass path.
Every decision — approve, refuse, override — produces a receipt with reasons.

## 2. Actors

| Actor | Identity | Role |
|---|---|---|
| Owner | human principal `<owner-principal>`, session `<owner-session>` (existing, containerless) | requests and approves root actions |
| Co-signer | control-plane chatbot (root identity; today `<subsidiary-principal>`) | approves **by policy only**; never executes |
| Executor | the identity that performs the operation (e.g. the Hackathon subsidiary) | runs the operation after approval, through its mount |
| Later | a designated human second approver | replaces the co-signer for the highest tier |

The root identity holds authority but never executes (owner rule).

## 3. Tiers

| Tier | When | Needs |
|---|---|---|
| **Routine root** | operation is root-level and within written policy | owner approval + co-signer approval |
| **Out of policy** | co-signer finds a policy condition unmet | refused; only break-glass can proceed |
| **Break-glass** | owner overrides a refusal | fresh hardware-key confirmation + cancellable delay + receipt anchored on-chain + second-channel notice |

Non-root operations keep today's behaviour: the bridge's per-call capability
check (**existing**).

## 4. Data model

```text
ApprovalRequest {
  id: uuid                      # request id
  operation: { plugin, method } # e.g. ovsdb_bridge.create_bridge
  arguments_hash: sha256        # of canonical JSON arguments; arguments stored, not free text
  resource: string              # e.g. "bridge:etched0"
  executor: session_id          # identity that will run it
  requested_by: principal_id    # owner
  policy_ref: { profile, control_ids[], version }
  created_at, expires_at        # short TTL (e.g. 10 min)
}

Approval {
  request_id, approver_principal, kind: owner|cosigner|human,
  decision: approve|refuse, reasons[], judgments{}?, receipt: operation_id/event_hash
}

BreakGlass {
  request_id, hardware_key_assertion, delay_s, cancelled: bool,
  anchor: { chain_id, tx | attestation_uid }, notice_channel
}
```

The request is **structured**: the co-signer never sees free-text
justification from the requester.

## 5. Co-signer decision (judgments from the model, decision in code)

The co-signer answers atomic questions about the structured request (the
chatbot via Meta's model, or Jev nouls); **code** decides from fixed
thresholds (yes ≥ 0.7, no ≤ 0.3, otherwise unsettled).

| Judgment (noul) | Meaning |
|---|---|
| `in_declared_boundary` | Is `resource` inside the executor's declared boundary in `policy_ref`? |
| `operation_permitted_by_control` | Does a control in `policy_ref` permit this operation on this resource? |
| `executor_matches_request` | Is the executor the identity the policy assigns to this kind of operation? |
| `destructive` | Would this operation delete or overwrite existing state? |
| `affects_shared_infrastructure` | Does it touch infrastructure other identities depend on (e.g. `ovsbr0`)? |

Truth table (in code):

| in_boundary | permitted | executor_ok | shared_infra | destructive | Decision |
|---|---|---|---|---|---|
| yes | yes | yes | no | any | **approve** |
| yes | yes | yes | yes | any | **refuse** (break-glass only) |
| no or unsettled | — | — | — | — | **refuse** |
| — | no or unsettled | — | — | — | **refuse** |
| — | — | no or unsettled | — | — | **refuse** |

`destructive` does not change the decision; it is recorded and raises the
break-glass delay. Exact-match facts (does `resource` equal an allow-listed
value, is the request expired) are checked in code, not asked of the model.

## 6. Flow

```text
owner request (UI / team chat / mount)
  → build ApprovalRequest (structured; arguments hashed and stored)
  → owner approval (signed by owner session)
  → co-signer: judgments → code decision → Approval + receipt
      approve → issue a single-use execution ticket (existing elevation.authorize_task style, 60 s)
      refuse  → show the reasons; offer break-glass
  → executor runs the operation through its 3tchedFS call file with the ticket;
    the bridge checks the exact capability (existing) → operation receipt
  → result + all receipts shown to the owner
```

Break-glass: owner chooses override → hardware-key assertion (e.g.
FIDO2/WebAuthn) → delay (cancellable from any owner device) → notice on a
second channel → anchor the BreakGlass record on-chain → issue the ticket →
execute as above.

## 7. Enforcement points

- A root-tier operation without a valid ticket (two approvals or break-glass)
  is refused before execution, with the reason.
- Tickets are single-use, bound to `request_id`, `arguments_hash`, `executor`
  and a short TTL; a mismatch is refused.
- The co-signer cannot approve its own requests and holds no execution grants.
- All receipts are append-only; refusals are receipted too.

## 8. Acceptance criteria

1. A routine root action within policy runs only after owner + co-signer
   approval; both approvals and the operation have receipts.
2. A request on a resource outside the boundary (e.g. `bridge:ovsbr0`) is
   refused by the co-signer with the failed judgment named.
3. Free-text added to a request does not change the co-signer's decision
   (test: same structured request ± persuasive text → same decision).
4. Break-glass requires the hardware-key step and the delay, can be cancelled
   during the delay, and leaves an anchored record.
5. Reusing a ticket, changing arguments after approval, or executing after
   expiry is refused.

## 9. Integration with existing 3tched

| Need | Existing piece |
|---|---|
| per-call capability check | bridge admission |
| single-use ticket shape | `elevation.authorize_task` (60 s TTL), `list_protected_actions` |
| receipts | MutationEngine operation ids and chain events |
| identities | owner human session; chatbot service identity; subsidiary identity |
| model judgments | Meta (`META_API_KEY`), TypeSafe Jev (`TYPESAFE_API_KEY`) |

## 10. Open questions

- Hardware key: FIDO2/WebAuthn via the web UI, or a separate device app?
- Where the approval store lives: a new sealed plugin (preferred, receipted) or
  this repo's service with receipts written through the identity.
- Delay length per tier, and the second notification channel.
