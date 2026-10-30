# Compliance router data model — proposal v1 (draft, no code yet)

Pipeline (per `docs/DROP-IN-SPEC.md` §2.2): consumer adapter → router →
policy engine → enforcement (identity 3tchedFS call file) → receipt.

## Types

```
Profile   { profile_id, catalog_ref, controls: ControlId[] }   // OSCAL profile, e.g. small-business-portal
Request   { actor, surface, action, tags: Tag[], payload_ref }  // adapter output; actor = mapped principal
Route     { request_ref, controls: ControlId[], candidates: Operation[] }  // tags → controls → operations
Operation { plugin, method, args_schema, required_capability }  // from 3tchedFS operations/<plugin>/<method>/
Decision  { route_ref, verdict: allow | refuse | needs-approval,
            missing_capability?, rule?, approvals?: {have, need} }
Outcome   { decision_ref, operation_id?, event_hash?, outcome, redaction? } // receipt fields, proven today
```

## Tag → control → operation table (initial)

| publish portal article | `pii`, `action:publish` | `etched-pt-1`, `etched-au-1`, `etched-in-1` | `schema_tag_registry/get_policy` → redacted publish → index |
| show my receipts | `audit`, `action:read` | `etched-au-1` | `identity_sled/get_own_identity`, `routing_decision/get_decision` |
| add staff member | `access-control`, `action:write` | `etched-ac-1`, `etched-au-1` | `elevation/authorize_task` (if root: 2 approvals) |
| set 90-day expiry | `retention`, `action:write` | `etched-si-1`, `etched-au-1` | `cron/create_job`, `workflows/start_workflow` |
| anchor release | `integrity`, `approval:root` | `etched-in-1`, `etched-ac-1` | index entry → anchor (2 approvals; anchor needs a sealed method) |
| out-of-bounds op | any | matching control | **refuse**: `{missing_capability, rule}` — first-class result |
| unsealed candidate | any | matching control | **refuse**: `method-not-sealed:<plugin>.<method>` — fail closed |

## Decisions and rendering

- `allow` → enforcement runs the op through the identity mount; `Outcome` renders with the `receipt` component.
- `refuse` → no plugin call; renders with the `refusal` component (missing capability + rule). **Planned.**
- `needs-approval(n of m)` → renders `approval_request`; approvals collected on the same surface; root needs 2 different humans. **Planned** (single-use ticket exists today).
- Redaction applies before render; receipt records it (`redaction_notice`). **Planned.**

## Next step (code, after approval)

Rust crate `routing/` implementing `route(Request, Profile) -> Route` as a pure
function over this table + the OSCAL JSON; no I/O, no grants. Policy and
enforcement stay separate crates. Model (Muse Spark) supplies judgments
(tag suggestions); this table decides.
