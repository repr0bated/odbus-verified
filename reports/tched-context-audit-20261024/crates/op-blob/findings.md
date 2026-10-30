# Findings — op-blob fresh tched context audit

## F1 — threetched values are test-fixture identifiers, not routing claims

**Evidence:** `crates/op-blob/src/descriptor.rs:871-908` defines `cli_flag_fields_round_trip_with_exact_json_names_and_existing_numbers`. It uses `threetched` in a capability, subid, schema ID, and descriptor synthesis ID to exercise CLI field-name conversion, protobuf/JSON names, field numbering, and round-trip behavior. The test does not call a method or publish a catalog entry.

**JEV:** J1–J4 each chose `retain_contextual_reference`, confidence 0.98 (see `judgments.md`).

**Disposition:** Preserve all four values. They are locally coherent fixture data and are not evidence of current route availability, permissions, or deployment. No op-blob source correction is supported.

## Explicit limits

This was a source audit, not runtime verification. The external method receiver, grants, sealed catalog, mounted views, and live deployment were not queried. Generic discrepancies outside tched-specific context are not expanded into this task.