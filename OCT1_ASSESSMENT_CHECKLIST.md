# Oct 1 Assessment Checklist

**Run after golden build completes.** Evaluate whether we're on track for Oct 1 demo.

---

## Golden Build Validation

- [ ] Build completed without errors
- [ ] Seal catalog hash recorded
- [ ] `opblob persist` succeeded (catalog retained across reboot)
- [ ] `build-golden.sh` completed (snapshot, send/receive, symlink updated)
- [ ] `/opt/op-dbus/current` points to new release
- [ ] No service restart errors in build-golden output

---

## Release Boot & Service Health

- [ ] Release boots cleanly (no kernel panics, no mount failures)
- [ ] Run: `sudo sv check op-grpc-bridge op-web emqx`
- [ ] All four show: `run` (services up)
- [ ] No hung processes or OOM kills in dmesg

---

## Chatbot Identity & Lungs

- [ ] Chatbot container is up and has an identity
- [ ] Run: `op-lungs-check` (or equivalent) — **PASS**
- [ ] Lungs socket exists and is readable: `/run/opdbus/lungs.sock`
- [ ] Chatbot can read its own identity from 3tchedFS mount

**Critical:** This is the foundation for everything else.

---

## Demo Story 1: Accountability

- [ ] Cozo tables exist: `task_record`, `evidence_record` (or equivalent)
- [ ] Can create a task record through chatbot
- [ ] Can record evidence (receipt + observed state)
- [ ] Can verify completion (read evidence, check timestamp + actor)
- [ ] Walk-through works: propose → execute → verify → report outcome

**Success:** User sees a task created, executed, and verified with evidence in chain.

---

## Demo Story 2: CMS

- [ ] CMS plugin deployed and sealed (appears in `list_plugins`)
- [ ] Can create a document through UI or API
- [ ] Can link document to a method/control (method→control registry works)
- [ ] Chain records the edit (check chain events for document create)
- [ ] UI shows the link and audit trail

**Success:** User creates content, links to a control, sees the trace in chain.

---

## Integration: Accountability + CMS

- [ ] Both stories are live in same release
- [ ] One walkthrough shows both (create doc → verify evidence → see control satisfied)
- [ ] Chain events show both content + evidence
- [ ] Demo path is repeatable and works from a WireGuard peer or host

---

## Blockers & Gaps

- [ ] List any features/pieces still missing for the demo
- [ ] Mark as "blocking Oct 1" or "nice-to-have post-Oct 1"
- [ ] Estimate time to fix each blocker

---

## Critical Path Items (Must Have Oct 1)

- [ ] Chatbot identity working
- [ ] Accountability task + evidence cycle working
- [ ] CMS CRUD + control linking working
- [ ] Both stories in one release
- [ ] Demo walkthrough is clean and repeatable

---

## Defer to Post-Oct 1

These are valuable but not blocking:

- [ ] Annascribe compliance verification layer
- [ ] Multi-specialist coordination
- [ ] Full NIST control hierarchy (draft is enough)
- [ ] Per-user task scoping
- [ ] Comprehensive UI polish

---

## Next Steps (If Green)

1. Record demo walkthrough (screen capture of: create task → execute → verify → create doc → link control → show chain)
2. Prepare talking points for YC demo
3. Test on live release from WireGuard peer (browser path)
4. Have owner review and approve demo story

---

## Next Steps (If Red)

For each failing item:
1. Is it fixable before Oct 1? (Estimate hours)
2. Can we defer it and still have a demo story?
3. Does it block other critical items?

Record findings in `SIGNALS.md` with blocker status.
