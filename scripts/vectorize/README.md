# Vectorization scripts

One-off Python scripts (not part of the Rust workspace) that seed Qdrant
collections + a Cozo compliance graph, written this session because the
real Rust MCP tools (`refresh_blob_vectors`/`search_blob_vectors` in
`crates/op-cognitive-mcp`) couldn't be compiled/deployed here yet (see
`.kiro/specs/unified-blob-catalog-mcp/`).

## Setup

Both scripts need a Python venv with a few packages not in system Python
(PEP 668 blocks `pip install` outside a venv on this box):

```bash
python3 -m venv venv
venv/bin/pip install pycozo cozo_embedded pylspclient \
    tree-sitter tree-sitter-rust tree-sitter-go tree-sitter-c tree-sitter-cpp \
    tree-sitter-python tree-sitter-typescript tree-sitter-javascript
```

Both require `VOYAGE_API_KEY` and `VOYAGE_API_KEY_LITE` in the environment
(see `~/.bash_secrets`) — they rotate between the two Voyage accounts,
switching once a credential is within a 1.75M-token safety margin of its
real 2M/model free-tier cap.

Qdrant endpoint is hardcoded to `http://10.200.0.2:6333` (the `qdrant`
container's exposed port on the `svc0` bridge) — no intermediary.

## `vectorize_compliance.py`

Chunks and embeds the compliance bundle (`/home/admin/voyage-vectorize-bundle/`)
into two Qdrant collections (`compliance_official`, `compliance_general`)
plus a Cozo graph (`/home/admin/compliance-cozo/db` — Datalog relations:
`controls`, `frameworks`, `belongs_to`, `maps_to`, `derived_from`) so
structured cross-framework mappings are queryable as a graph, not just
semantically. See module docstring for the chunking strategy per source
shape (OSCAL `<control>` XML extraction vs. JSON array-of-objects
explosion vs. header/window fallback).

```bash
venv/bin/python3 vectorize_compliance.py all   # both files
venv/bin/python3 vectorize_compliance.py oscal  # just oscal-specs.md
venv/bin/python3 vectorize_compliance.py frameworks  # just compliance-specs.md
```

Idempotent and resumable: point ids are deterministic (UUID v5), and the
script checks Qdrant for already-written groups before spending any
Voyage tokens re-embedding them.

## `vectorize_code.py`

Tree-sitter AST-aware chunking (not naive line windows) for the LSP-tier
code repos cloned into `/home/admin/git/repos-bulk/`. One chunk per
function/struct/class/trait/impl/enum, paired with its preceding doc
comment (or, for Python, its docstring). Writes to the `code_lsp` Qdrant
collection.

```bash
venv/bin/python3 vectorize_code.py /home/admin/git/repos-bulk/<repo-dir> <repo-name>
```

`rust-lang/rust` is deliberately NOT run through this script — too large/
submodule-heavy for a flat per-file AST walk (a prior attempt "took
forever and never finished"). That one needs a real `rust-analyzer` LSP
session instead (not yet built).

## Known gap

Both scripts' sanity-check queries (and the Rust `search_blob_vectors`
tool) currently embed the *query* with the plain `voyage-4` endpoint
while the *stored* vectors were embedded with `voyage-context-4`
(contextualized). Same 1024 dimensions, technically a different model/
vector space. Results have been good in practice, but this should be
fixed to use `voyage-context-4`'s query mode consistently before relying
on this for anything precision-sensitive.
