# NVIDIA Inception — Narrative Plan

> The story that got us here: background → evolution → the realizations that shaped the system.
> Goal of this doc: make a reviewer believe the architecture is principled, not accidental — and
> that the founder is the reason. Fill the `TODO(jeremy)` blocks; the rest is scaffolding + the
> lightbulb moments captured from the design itself.

---

## 0. One-paragraph thesis (write last, lead with it)
The single sentence a reviewer should remember. Draft:
> "We preserve the user's real origin and hide the *transport* instead of the user — and every
> layer of the system falls out of that one decision."
TODO(jeremy): sharpen to your voice; add the product/AI hook (what it lets customers *do*).

---

## 1. Background — why a self-taught systems thinker
Frame the no-formal-CS background as the asset it is: you reason from invariants, not from
frameworks you were handed.
- TODO(jeremy): where you started, what you were doing before, what pulled you into this.
- TODO(jeremy): how you actually learned (projects, failures, the first system you built).
- The honest framing to keep: *"I design from principles and correct when a piece contradicts them"*
  — which is the rare skill, and is demonstrable, not a claim.

## 2. The problem that started it
What real need forced the first version. Privacy + identity + AI access, but in your words.
- TODO(jeremy): the original pain. Who was it for? What broke / was missing in existing tools?
- TODO(jeremy): the first crude version and why it wasn't enough.

## 3. The evolution (versions as a story, not a changelog)
Show movement — each stage solved the previous stage's contradiction. Suggested beats:
- Early: port-based / veth / fragile plumbing → what hurt about it.
- Middle: the move toward D-Bus-as-schema, plugins, projection.
- Now: socket-only fabric, identity sled, Gemma as the routing brain.
- TODO(jeremy): real dates/milestones, what each migration cost, what you learned.

## 4. The lightbulb moments (the heart of the doc)
Each reframed as a *principle*, with the moment that produced it. These are real, from the design:

1. **"Preserve the origin, hide the transport."**
   The inversion of conventional VPN thinking. Most designs mask the user behind a shared exit and
   then can't hold anyone accountable. Keeping the real origin is what lets the identity sled etch a
   real peer and the injected header gate on it. *This is the load-bearing insight — everything
   composes because of it.*
   TODO(jeremy): the moment you realized masking would break your own accountability model.

2. **"The plugin IS the schema — no exceptions."**
   Everything projects to `/org/opdbus/v1/plugins/<name>`; no plugin means no schema means no object.
   Uniformity is what makes the tree, the UI rendering, and tool discovery all free.
   TODO(jeremy): when you decided to make this absolute.

3. **"Everything is a unix socket."**
   Kill the veth pairs and exposed ports; nicless containers, xray routes by SNI into sockets. The
   only irreducible ports are the ones with no SNI to route on (inbound SMTP-25, WireGuard's UDP).
   TODO(jeremy): what failure made you go all-socket.

4. **"One model, three jobs."**
   Gemma as the single routing brain — subid classification + OpenFlow tag routing + subdomain
   resolution, all from the sled/schema state. Collapses three static subsystems into one generated
   artifact.
   TODO(jeremy): when you saw these were the same problem.

5. **fwmark / REALITY — unobservable, not anonymous.**
   fwmark keeps WireGuard off WARP so the origin survives; REALITY makes the ingress
   indistinguishable from ordinary HTTPS to a real site. Unblockable transport + hard identity
   gating, without anonymity. (The realization that these are *different* goals.)
   TODO(jeremy): the moment the obfuscation-vs-anonymity distinction clicked.

## 5. The architecture, as consequences (not a parts list)
State the few invariants, then show each layer falling out of them. One clean diagram:
```
WireGuard (direct, real-origin transport; fwmark keeps it off WARP)
   └─ unix sockets + xray SNI routing  (no ports)
        └─ nicless containers (incus proxy: in-ns port → host .sock)
REALITY ingress = looks like HTTPS to a borrowed site (no VPN fingerprint)
Identity sled (SHM) → injected header = the only access gate
Gemma → subid + tag + subdomain, generated from sled/schema state
```
TODO(jeremy): confirm/redraw to taste; keep it to ONE figure.

## 6. Why NVIDIA / GPU / AI (the Inception lane)
Make the AI the product and the privacy fabric the enabler.
- Gemma routing brain, cognitive-mcp + RAG, embeddings/vector store — the GPU-justifying workload.
- TODO(jeremy): the AI product a customer actually buys; where GPUs are non-optional.
- TODO(jeremy): traction / users / regulatory angle (compliance/OSCAL target) if applicable.

---

### Writing order (do it in this sequence, not top-to-bottom)
1. §4 lightbulb moments (you have these — they're the easiest and the core).
2. §3 evolution (string the moments into a timeline).
3. §1–2 background/problem (sets up §3).
4. §5 architecture-as-consequences (tighten what we mapped this session).
5. §6 AI/GPU lane.
6. §0 thesis last, once the rest exists.

### What only you can supply
Every `TODO(jeremy)` — dates, personal history, the felt moment behind each principle, the product
and traction. I can draft prose around each once you give me the raw facts, or interview you section
by section.
