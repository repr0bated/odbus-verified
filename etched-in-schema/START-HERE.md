# START HERE — Etched in Schema (open this in omp)

You are building **Etched in Schema** for the Meta Model API hackathon,
**Startup Track — judged on a working product**. Builder: Jeremy Hobson
(3tched). Use Meta's models (Muse Spark) for the product's AI steps.


> **The method is the blob — the identity acts only through 3tchedFS.**
> Every method is a plugin schema sealed into a blob. The identity's own
> 3tchedFS mount projects each sealed method as a typed file
> (`operations/<plugin>/<method>/call`); the identity writes arguments there,
> the bridge admits it by kernel identity, checks the exact capability, commits
> and returns a receipt. No MCP, no direct API calls, no `ovs-*` commands.
> Every new method built here follows: **logic in this repo → plugin
> registration → sealed blob → operation file in the mount → call → receipt.**
> See [`docs/DEV-CYCLE.md`](docs/DEV-CYCLE.md).

## Load everything

Read **`CONTEXT.md`** in full before doing anything (it is generated and
local-only: run `tools/build-context.sh` if it is missing). Real host
addresses and identity IDs are in the git-ignored `local/IDENTIFIERS.md`; the
public docs use placeholders such as `<door-ip>` and `<subsidiary-session>`.
Never commit real addresses, IDs or keys — this repository is public. It bundles every project
document in priority order (scope first). If you need a single source, the
originals are:

1. `README.md` — goals, golden path, new vs. existing
2. `docs/DEV-CYCLE.md` — **read first after the README: blob → mount → identity**
3. `docs/DROP-IN-SPEC.md` — the drop-in to build (generator + team chat + generated backends)
3. `docs/RUNTIME.md` — where things run (develop as `jeremy`, run as `3tched`), calling the platform, supervision, network, models, secrets
4. `SPARK-BRIEF.md` — state, rules, open issues, progress log
5. `docs/3TCHED-RESOURCES.md` — the platform you call, and how (call-file protocol)
6. `docs/SUBSIDIARY-SLED.md` — identity, root, subsidiaries, approvals
7. `docs/APPROVALS-SPEC.md` — two-approval root, policy co-signer, break-glass
8. `docs/PRIOR-ART.md` — positioning (SPIFFE, BeyondCorp, DIDs)
9. `docs/WINNING.md` — priorities for the demo
10. `review/out/findings.md` — Jev's scored review of the strategy
11. `schema/*.json` — draft schemas

## Scope — what is judged (new work only)

The identity platform (3tched) existed before the hackathon; use it only
through its interfaces. Build, in this repository:

1. **Compliance routing** — request / business profile → OSCAL controls → plugin operations.
2. **Rule enforcement** — rules compiled into grants, budgets, network policy; refusals name the missing capability.
3. **Policy** — approvals (two humans for root), redaction, retention; versioned and evaluated in code.
4. **Drop-in** — the existing UI generator repurposed: one spec renders on the web and in team chat (Matrix/Element first); every action governed by 1–3; generated backends (the model writes definitions, fixed code runs them).

UI beyond the drop-in only if time allows.

## Golden path (must pass 3 times in a row before anything else)

A user asks in team chat for something a business needs → compliance routing
maps it to controls and operations → policy is evaluated → allowed operations
run through the identity and return receipts; a disallowed one is refused with
the reason → the thread shows result, receipts and any redaction.

## Hard rules

- Do not rebuild what exists: the UI generator (catalog, spec validation,
  generation, schema rendering) is proven — reuse it.
- The model generates **definitions and specs, never backend code**; fixed,
  tested code executes them. Judgments from the model, decisions in code.
- Never edit 3tched's database directly, never bypass a refusal, never call
  plugins from an adapter; adapters hold no grants.
- Mark every claim **proven** (with evidence) or **planned**.
- **No stubs presented as built.** The pre-commit hook
  (`tools/check-no-stubs.sh`) refuses unfinished-work markers in code and docs
  (the list is in the script), and refuses a book page in the main sections
  whose folder holds only a README — list unbuilt work under `# Planned` in
  `book/src/SUMMARY.md`. Each Rust crate denies Clippy's unfinished-code lints
  (see `approvals/Cargo.toml`). Identifier placeholders such as `<door-ip>` are
  allowed and required.
- Do not name vendors in product copy (say "team chat", not a brand).
- Commit small and often, and **push often** — the public history is the
  evidence the work was done during the hackathon. Never rewrite history or
  force-push. A pre-commit check (`tools/check-public.sh`, enabled with
  `git config core.hooksPath .githooks`) refuses real addresses, identity IDs
  and anything key-shaped; fix the content with placeholders, never bypass it
  with `--no-verify`. Log progress, dated, at the end of `SPARK-BRIEF.md`.

## First task

Create the OSCAL foundation in `oscal/` with `trestle`: a catalog of the 3tched
capabilities the golden path needs (start with the CMS controls in
`SPARK-BRIEF.md`: PII redaction, audit, access control, retention, integrity),
a tag taxonomy, and one profile for a small regulated business. Then propose
the compliance-router data model in `routing/` before writing code.
