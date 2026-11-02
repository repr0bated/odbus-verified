# Etched in Schema

**An identity that acts — and can prove what it did.**

Etched in Schema is a self-contained 3tched identity — the living embodiment
of a business subsidiary — that turns a business goal into deployed,
working infrastructure. Every action it takes is authorised against its
capabilities, bounded by compliance rules, recorded as a tamper-evident
receipt, and anchored on a blockchain so anyone can verify *who did what,
under what authority, and what actually happened* without trusting the agent
or its operator.

Built for the Meta Model API hackathon (Startup Track — judged on a working
product) by Jeremy Hobson, founder of 3tched.

> **The method is the blob — the identity acts only through 3tchedFS.**
> Every method is a plugin schema sealed into a blob. The identity's own
> 3tchedFS mount projects each sealed method as a typed file
> (`operations/<plugin>/<method>/call`); the identity writes arguments there,
> the bridge admits it by kernel identity, checks the exact capability, commits
> and returns a receipt. No MCP, no direct API calls, no `ovs-*` commands.
> Every new method built here follows: **logic in this repo → plugin
> registration → sealed blob → operation file in the mount → call → receipt.**
> The UI is derived from the same blobs: screens from sealed schemas, actions
> call the same sealed methods and show their receipts. Each plugin is a
> **schema feed** (schema + committed state, streamed); state has one writer
> (MutationEngine) and is archived as receipts, Btrfs snowball snapshots and
> vectors.
> See [`docs/DEV-CYCLE.md`](docs/DEV-CYCLE.md).

## Why

AI agents can reason about goals, but they rarely have a durable identity,
verifiable permissions, accountable execution, or the ability to build and
operate real systems. On-chain artifacts have the mirror-image problem: a
contract address or a content hash proves something exists, not that it came
from an accountable actor following permitted steps. That missing link
between artifacts and accountable, authorised actors is what keeps businesses
from trusting both autonomous agents and Web3.

Etched in Schema closes that gap from both sides: every on-chain artifact
points to the identity and authorised operation behind it, and every
operation points to the artifacts it produced, with the history anchored
on-chain.

## Goals — the new, judged product

The identity platform (3tched) existed before the hackathon and is used only
through its interfaces. The **new development, built and judged here**, is:

1. **Compliance routing** — a business's requirements become an OSCAL profile;
   OSCAL tags route each request and each requirement to the components and
   plugin operations that implement it.
2. **Rule enforcement** — boundary rules compile into the capability grants,
   budgets and network policy that are enforced on every call, with a clear
   refusal (and the exact missing capability) when a request is out of bounds.
3. **Policy** — authoring, versioning and evaluating the rules: what each
   identity may do, which actions need human approval (two approvals for root),
   and what happens to data (e.g. PII redaction, retention).
4. **Drop-in consumer app** — the existing UI generator repurposed: one spec
   renders on the web and in tools people already use, starting with **team chat** — an open-source
   Matrix client (Element) first, reaching other chat tools through Matrix
   bridges — and every action is governed by 1–3. Spec:
   [docs/DROP-IN-SPEC.md](docs/DROP-IN-SPEC.md).
   A user asks in team chat; the request is routed, checked against policy,
   executed through the identity or refused, and the receipt comes back in the
   thread.

The web UI is optional, only if time allows.

**Golden path** (a working product, end to end, three times in a row before
anything else): a user asks the agent in team chat to do something for a business
→ compliance routing maps it to the required controls and operations →
policy is evaluated → allowed operations run through the identity and return
receipts; a disallowed one is refused with the reason → the thread shows the
result, the receipts, and any redaction applied.

Out of the critical path: the avatar/embodiment, moving identities between
hosts, client acquisition, and on-chain anchoring (kept only if it fits once
the golden path works).

## What is new vs. what already exists

Everything in **this repository** is developed during the hackathon. It is
built *on* 3tched, which already exists and is used only through its
published interfaces (see [docs/3TCHED-RESOURCES.md](docs/3TCHED-RESOURCES.md)).

| New in this repo (judged) | Existing 3tched resource it uses |
|---|---|
| OSCAL catalog of 3tched capabilities, tag taxonomy, profiles, component definitions | `oscal_subid_registry` (subid discovery), `schema_tag_registry` (tag metadata, owner-scoped policy) |
| Compliance router: request / profile → controls → plugin operations | sealed plugin catalog, identity operation files (3tchedFS) |
| Rule enforcement: OSCAL rules → capability grants, budgets, OpenFlow policy; refusals with reasons | bridge per-call capability check, `openflow` / `ovsdb_bridge`, router budgets |
| Policy engine and policy documents (approvals, redaction, retention) | `schema_tag_registry` policies, `routing_decision`, `elevation` |
| Drop-in consumer app: team chat first (Matrix/Element) | chat path to Meta's model, identity receipts |
| Business declarations (starting with a Florida LLC) | — |

Pre-existing and **not** part of the judged work: the identity container and
sled, kernel-peer admission, 3tchedFS, the bridge and MutationEngine, receipts,
the router, and the 3tched.com UI.

## Architecture at a glance

```
          internet only                      local filesystem only
  Meta model API  ◄──┐                 ┌──► 3tchedFS mount (lungs) ──► 3tched bridge
  blockchain      ◄──┤   IDENTITY      │      kernel-peer identity, capability check,
                     └── CONTAINER  ───┘      MutationEngine commit, receipt
                          (the twin)
                               │
            OSCAL profile ─► tag router ─► plugin operations (incus, openflow, …)
                               │
                  receipts ─► Web3 index ─► Merkle root anchored on-chain
```

- The identity has **no network path to 3tched**; it reaches 3tched only
  through its own FUSE mount, where every operation is a typed file.
- Its only network is the internet: the Meta model API and the blockchain.
- Subsidiaries map to Linux users; the root is the 3tched control plane.
  See [docs/SUBSIDIARY-SLED.md](docs/SUBSIDIARY-SLED.md).
- How this compares to SPIFFE, BeyondCorp and DIDs, and what it adds:
  [docs/PRIOR-ART.md](docs/PRIOR-ART.md).

## Repository layout

| Path | Contents |
|---|---|
| `schema/` | JSON Schemas: Web3 index entry, business declarations |
| `oscal/` | OSCAL catalog, profiles, component definitions (Compliance Trestle workspace) |
| `routing/` | tag router: profile → components → operations |
| `enforcement/` | boundary compiler: OSCAL rules → grants, budgets, OpenFlow policy |
| `consumer/` | drop-in consumer apps (team chat first) |
| `cms/` | the CMS and its control behaviours (background) |
| `web3/` | index log, Merkle anchoring |
| `ui/` | Svelte components for the 3tched.com app |
| `START-HERE.md`, `CONTEXT.md` | open in omp; CONTEXT.md bundles every doc (rebuild with `tools/build-context.sh`) |
| `docs/` | 3tched resources, subsidiary/sled model, prior art and positioning, how to win |
| `SPARK-BRIEF.md` | current state, rules, open issues, progress log |

## Working rules

- Read `SPARK-BRIEF.md` before starting; log progress at its end (dated).
- Mark claims as **proven** (demonstrated, with evidence) or **planned**.
- Schemas here are drafts; promoted ones become sealed 3tched plugins (Rust).
- UI: Svelte/SvelteKit + json-render's Svelte renderer only.
- In pitches say "implements and evidences these controls", never
  "compliant" or "certified".
