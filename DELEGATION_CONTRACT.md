# Delegation Contract & Chatbot Authority

**At release, the chatbot owns the control-plane conversation and delegation lifecycle.** It must understand your intent, make recommendations, propose plans, and hand authorized work to approved agents.

To make that authority real:

## Chatbot Requirements

- **A stable identity.** Its authority belongs to the chatbot's container and identity, independent of which model supplies its reasoning.
- **A durable task record.** Accepted intent, plan revisions, approvals, assignments, cancellations, and outcomes must live in authoritative state. Conversation memory provides context; it does not grant permission.
- **An enforced delegation contract.** Each assignment identifies the approved agent, exact operation, permitted resources, limits, expiry, and required evidence. Agents cannot widen that scope or delegate onward without explicit authorization.
- **Narrow local interfaces.** Through its 3tchedFS lungs, the chatbot can propose work, request authorized delegation, observe progress, and read results. Its release identity receives no direct internet access, shell, arbitrary filesystem access, or generic execution capability.
- **Independent completion evidence.** Agents return receipts and observed results. The chatbot reports completion from that evidence; saying "done" cannot itself make a task complete.
- **Permissions enforced outside the model.** Your instructions and configured policy determine what can proceed. The chatbot cannot approve its own privilege expansion or add agents to the approved registry.

## Why Delegation Itself Needs Permission Checks

A chatbot without a shell could otherwise acquire equivalent power through an unrestricted worker. Delegation authority must be gated by policy.

## The Intended Relationship

```
You establish intent and authorization
        ↓
Chatbot maintains the accepted task
        ↓
Policy admits a scoped delegation
        ↓
Approved agent executes
        ↓
Receipts update authoritative state
        ↓
Chatbot explains the outcome
```

**The chatbot is authoritative for coordination and the accepted task.** Your authority, runtime permissions, and execution evidence remain independently enforced.

The release must establish these boundaries in identities, schemas, and grants—not just in system prompts.

---

**Canonical reference:** read this before designing chatbot capabilities, delegation mechanisms, or grant policies.
