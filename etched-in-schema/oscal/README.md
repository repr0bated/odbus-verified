# oscal

OSCAL foundation (Compliance Trestle workspace, OSCAL 1.2.1). **Planned**
(draft for the hackathon; validated with `trestle validate`, not yet wired
to the router).

- `catalogs/etched-catalog/catalog.json` — 5 controls covering the golden
  path: `etched-pt-1` PII redaction, `etched-au-1` audit, `etched-ac-1`
  access control, `etched-si-1` retention, `etched-in-1` integrity. Each
  control names candidate 3tched plugin operations (`tched-plugin` /
  `tched-operation` props) and its evidence (`evidence-kind` + assessment
  part). Proven vs planned per control in `implementation-status` props.
- `TAG-TAXONOMY.md` — tag → control → operations routing tags v1.
- `profiles/small-business-portal/profile.json` — one small regulated
  business (Fort Lauderdale client portal); includes all 5 controls, adds
  the two-person rule tailoring on `etched-ac-1`.
