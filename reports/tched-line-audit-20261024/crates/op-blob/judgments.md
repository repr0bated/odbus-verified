# op-blob declarative JEV receipts

Task: `tched-line-audit-20261024/op-blob`; worker Codex; date 2026-10-24.

## J1 — CLI flag reflection fixture

Successful installed `judge()` call through `functions.eval`, Python; awaited supported API; no model override, routing, source extraction or arbitrary execution. Transcript label: `Judging exact CLI naming occurrences`. Full returned receipt read from `artifact://0:raw:1-1`. One state, four independent typed choice questions. No judge failure or fallback.

Authority supplied: our filesystem/projection uses `3tched` / `3tched-fs`; external ZeroClaw-derived executable/surface uses `threetched`. Preserve existing sealed/contract identifiers absent an explicit migration; no blanket renames.

Source evidence: `crates/op-blob/src/descriptor.rs:871-908`, read within full sequential file coverage `1-230;231-460;461-700;701-928`. Exact evidence submitted:

```rust
871:    #[test]
872:    fn cli_flag_fields_round_trip_with_exact_json_names_and_existing_numbers() {
873:        let mut decl = method_decl_from_schemars_with_output::<SetDeviceInput, AckOutput>(
874:            "configure",
875:            SideEffect::Read,
876:            true,
877:            "threetched.read",
878:            "obs.threetched.configure@v1",
879:        );
880:        decl.args = serde_json::json!({"type":"object","additionalProperties":false,
881:            "properties":{
882:                "--config-dir":{"type":"string"},
883:                "-f":{"type":"boolean"},
884:                "listen_port":{"type":"integer"}
885:            }});
886:        let mut schema = PluginSchema::builder("threetched").build();
887:        schema.methods.insert("configure".into(), decl);
888:        let (bytes, methods) = descriptor_set_for_plugin("threetched", &schema);
889:        let pool = prost_reflect::DescriptorPool::decode(bytes.as_slice()).unwrap();
890:        let input = pool.get_message_by_name(&methods[0].input_full).unwrap();
891:        let flag = input.get_field_by_name("__config_dir").unwrap();
892:        assert_eq!(flag.json_name(), "--config-dir");
893:        assert_eq!(flag.number(), 1);
894:        let short = input.get_field_by_name("_f").unwrap();
895:        assert_eq!(short.json_name(), "-f");
896:        assert_eq!(short.number(), 2);
897:        let existing = input.get_field_by_name("listen_port").unwrap();
898:        assert_eq!(existing.json_name(), "listenPort");
899:        assert_eq!(existing.number(), 3);
900:        let mut json = serde_json::Deserializer::from_str(
901:            r#"{"--config-dir":"/isolated/config","-f":true,"listenPort":443}"#,
902:        );
903:        let message = input.deserialize(&mut json).unwrap();
904:        let output = serde_json::to_value(message).unwrap();
905:        assert_eq!(output["--config-dir"], "/isolated/config");
906:        assert_eq!(output["-f"], true);
907:        assert_eq!(output["listenPort"], "443");
908:    }
```

### Questions and explicit contrary outcomes

- `line877`: "Choose disposition for the tched substring inside threetched.read at line 877 using exact context and authority. It is a CLI flag reflection fixture, not a filesystem-branding statement." Criteria: leave_as_is = already-correct threetched external-surface fixture capability; replace_with_threetched = currently incorrect external branding; replace_with_3tched = our filesystem/projection with incorrect external spelling; insufficient_evidence = context does not establish intent.
- `line878`: "Choose disposition for obs.threetched.configure@v1 at line 878 using exact context and authority." Criteria: leave_as_is = already-qualified external CLI surface fixture taxonomy; replace_with_threetched = incorrect external branding; replace_with_3tched = our filesystem; insufficient_evidence = intent unestablished.
- `line886`: "Choose disposition for PluginSchema::builder(\"threetched\") at line 886 using exact context and authority." Criteria: leave_as_is = already-correct external CLI schema fixture name; replace_with_threetched = external spelling incorrect; replace_with_3tched = actually our filesystem/projection; insufficient_evidence = intent unclear.
- `line888`: "Choose disposition for descriptor_set_for_plugin(\"threetched\", &schema) at line 888 using exact context and authority." Criteria: leave_as_is = correct external CLI fixture plugin identity, consistent with schema; replace_with_threetched = incorrect external spelling; replace_with_3tched = filesystem/projection branding; insufficient_evidence = intent unclear.

### Actual returned receipt

```json
{
  "line877": {"type":"choice","choice":"leave_as_is","confidence":1,"probabilities":{"leave_as_is":1,"replace_with_threetched":0,"replace_with_3tched":0,"insufficient_evidence":0}},
  "line878": {"type":"choice","choice":"leave_as_is","confidence":0.99,"probabilities":{"leave_as_is":0.99,"replace_with_threetched":0.01,"replace_with_3tched":0,"insufficient_evidence":0}},
  "line886": {"type":"choice","choice":"leave_as_is","confidence":0.93,"probabilities":{"leave_as_is":0.95,"replace_with_threetched":0.02,"replace_with_3tched":0.03,"insufficient_evidence":0}},
  "line888": {"type":"choice","choice":"leave_as_is","confidence":0.96,"probabilities":{"leave_as_is":0.97,"replace_with_threetched":0.02,"replace_with_3tched":0.01,"insufficient_evidence":0}}
}
```

Human-readable disposition: preserve all four. The fixture models external CLI arguments and tests exact protobuf JSON flag names/field numbers. `threetched` is already qualified and consistent across capability, taxonomy, schema and descriptor plugin identifiers. None is branding for our filesystem. No source edit or cross-crate migration is warranted. These are static source judgments, not claims that the fixture passes or the external surface is deployed.

## J2 — supplemental claim grounding

Successful `functions.jev_verify` call, model `jev-latest`; no routing. Claim:
“The four threetched occurrences in this op-blob fixture are already-qualified CLI schema/capability/descriptor identifiers, not filesystem branding; leave all four unchanged. This does not establish that the fixture passes or any surface is deployed.”

Evidence supplied: naming authority and `src/descriptor.rs:871-908` context
describing the configure method, exact four identifiers, flag properties and
JSON-name/field-number assertions; source read but not executed; J1 results.
No verbatim quote asserted (`quote` empty).

Actual return: `verdict=supports — low confidence, review`;
`verbatim=0.11`; `misleading=0.7/3`; `confidence=0.37`;
`tokens(in/out)=796/78`.

Because confidence is below0.6, J2 is not decisive evidence. Manual contextual
review uses the already-read exact function: the identifiers are literal
`threetched`, its declared arguments are CLI flags, and neither the function
nor its assertions describe a filesystem mount or product. J1's four explicit
contrary-outcome choices plus that source inspection support preservation.
The empty-quote score is not a fabricated quotation check.
