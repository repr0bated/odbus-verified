# Accountability + CMS Scope — Oct 1 Demo

**Deadline:** 2026-10-01 (YC demo)  
**Model:** Gained ~1 month from Ultra session; show product by Oct 1  
**Focus:** Two parallel stories proving the system works end-to-end

---

## Story 1: Accountability Demo

User intent → Chatbot coordination → Specialist execution → Evidence → Verified outcome

### What It Shows

- **Durable task records** — accepted intent, plan revisions, assignments stored in Cozo
- **Independent evidence** — specialist returns receipts + observed state changes
- **Verified completion** — chatbot reports "done" only when evidence proves it
- **No model-only claims** — saying "done" doesn't make it done; evidence decides

### Minimal Viable Flow

1. **User proposes work** (via chatbot or 3tchedFS): `"verify that identity X can read resource Y"`
2. **Chatbot accepts** — records in Cozo (conversation, proposed task, state)
3. **Specialist** (AI agent, automated service, or human) performs the operation
4. **Specialist returns** evidence:
   - Operation receipt (what was attempted)
   - Observed result (what actually happened)
   - Timestamp + actor identity
5. **Chatbot verifies** — reads evidence from Cozo, checks signature/hash, reports outcome
6. **User sees** — "✅ Verified: identity X can read resource Y at 2026-09-30T11:30:00Z"

### Scope for Oct 1

- [ ] Cozo task schema (task_record, evidence_record tables)
- [ ] Chatbot accepts work through delegation contract
- [ ] One specialist (AI agent) executes and returns receipt
- [ ] Evidence validation (timestamp, actor, operation match)
- [ ] Outcome report (pass/fail with evidence link)
- [ ] Walk-through: user → chatbot → specialist → verified outcome

**NOT for Oct 1:** comprehensive task lifecycle (pause, resume, cancel), multiple specialists, per-user task scoping, audit trail visualization.

---

## Story 2: CMS + Compliance Demo

Content management + NIST 800-53 control binding + audit trail

### What It Shows

- **Compliance from the start** — content creation bound to controls
- **Traceability** — every change linked to a control requirement
- **Verifiable compliance** — you can't mark something done without proving control satisfaction

### Spec Source

**Recovered CMS spec** (from Codex session 2026-08-21):
- `/srv/git/3tched.com/references/recovered/cms-aug21.spec.json`
- 43 UI elements, old catalog format (regenerate in UI-render mode)
- ~8/10 design quality, ~3/10 actual CMS capability

### Minimal CMS for Oct 1

1. **Content types:**
   - Document (title, body, author)
   - Control requirement (NIST ID, description, method reference)
   - Method→control link (which operations satisfy which controls)

2. **UI elements:**
   - Content editor (create/edit/view documents)
   - Control link widget (attach control ID to a method/operation)
   - Audit log view (show who changed what, when)

3. **Backend:**
   - `cms` plugin on project documents (CRUD)
   - Method→control registry (static or Cozo-backed)
   - Chain event recording (every edit writes to chain)

4. **Flow for Oct 1:**
   - Create a document (e.g., "Identity verification procedure")
   - Link it to a NIST control (e.g., AC-2: Account Management)
   - Show the link in the UI and the chain event
   - Verify: control is satisfied by this method, audit trail present

### Scope for Oct 1

- [ ] Recover/regenerate CMS spec in UI-render mode
- [ ] Build document CRUD plugin (minimal)
- [ ] Wire method→control registry (reference table)
- [ ] Chain event recording for edits
- [ ] UI: content editor + control link display
- [ ] Walk-through: create doc → link control → verify chain

**NOT for Oct 1:** rich text editor, media library, publishing workflow, access control per document, multi-level hierarchy.

---

## Integration: Accountability + CMS

The two stories work together:

1. **CMS defines what should happen** — method X satisfies control Y
2. **Accountability proves it happened** — specialist executed X, evidence shows Y satisfied
3. **Chain records both** — content change + execution evidence in one trace

### Example Oct 1 Demo Flow

```
User: "Implement identity verification (AC-2)"
  ↓
Chatbot: "I'll coordinate this. Propose: create procedure doc, link to AC-2"
  ↓
User: "Approved"
  ↓
[CMS creates document, links to AC-2, writes to chain]
  ↓
Chatbot: "Document created. Now executing verification test"
  ↓
[Specialist verifies identity can read resource, returns receipt]
  ↓
[Evidence linked to task and control]
  ↓
Chatbot: "✅ Control AC-2 satisfied by procedure X, verified 2026-09-30T11:30:00Z"
  ↓
User sees: unified trace in chain (content + evidence)
```

---

## Technical Checklist

### Accountability (Required)

- [ ] Cozo tables: `task_record`, `evidence_record`
- [ ] Chatbot plugin: `accept_task`, `record_evidence`, `report_outcome`
- [ ] Specialist integration: delegation contract + receipt format
- [ ] One end-to-end test: propose → execute → verify → report

### CMS (Required)

- [ ] Document CRUD plugin (minimal)
- [ ] Method→control registry (reference + UI display)
- [ ] Chain event integration (edits write to chain)
- [ ] UI: document editor, control link widget, audit log view

### Nice-to-Have (Post-Oct 1)

- [ ] Annascribe independent verification layer
- [ ] Per-user task scoping
- [ ] Multi-specialist coordination
- [ ] Full NIST control hierarchy
- [ ] Rich text editing
- [ ] Publishing workflow

---

## Key Success Criteria

1. **Accountability story**: User sees a task created, executed, and verified with evidence
2. **CMS story**: User creates content, links to a control, sees the chain record
3. **Integration**: Both stories appear in one unified demo walkthrough
4. **Proof**: Demo works on live release (not just dev)

---

## Related

- [[DELEGATION_CONTRACT.md]] — authority model
- `HANDOFF.md` — current state
- `/srv/git/3tched.com/references/recovered/cms-aug21.spec.json` — CMS spec to regenerate
- `crates/op-plugins/src/state_plugins/` — where to add `cms` and related plugins
