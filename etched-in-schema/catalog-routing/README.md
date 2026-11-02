# catalog-routing

Pure request routing over nested catalogs: `routing/DATA-MODEL.md` as code.
No I/O, no network.

- `catalogs/compliance` — control tags → OSCAL controls → candidate plugin
  operations (`oscal/TAG-TAXONOMY.md`; `oscal/` JSON is authoritative).
- `catalogs/ui` — component support per renderer target
  (`docs/DROP-IN-SPEC.md` §2.1; the sealed `json_render` blob is authoritative).
- `catalogs/models` — provider/model selection over a caller-supplied registry
  (the live `chat_router` catalog is authoritative; this crate only matches).
- `decide` — allow / refuse (naming the missing capability) / needs-approval
  (root: two humans, mirroring the `approvals/` crate).
