# Declarative JEV receipts — op-blob

2026-10-24; one installed `await judge(state, questions)` request via eval, 12 choice questions. No model override. No source-search program. Raw returned dictionary was read completely from `artifact://0:raw:1-1` after the display truncated its long line. Confidence is the returned confidence, not inferred from winning probability. This judges fixture semantics, not runtime availability or whole-crate absence.

## Verbatim state

`source`: `crates/op-blob/src/descriptor.rs:871-908`

`authority`: Audit rubric: tched_router is retired; threetched is external binary/plugin surface; 3tchedFS/tched_fs is filesystem; op-llm owns direct generation. This context is an unexecuted local descriptor-construction unit test, not an operation invocation. Active/deployed receiver availability has not been checked.

`context`:

```text
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

## Verbatim choice criteria

All questions have `type: choice`. Axis selects the following criteria object:

```json
{
  "role": {
    "routing_dispatch": "Routing control plane, HTTP :8084 or router runtime client",
    "model_selection_calling": "Model/provider selection or generation dispatch",
    "filesystem_projection": "3tchedFS/lungs/SHM filesystem projection",
    "contract_identifier": "Package/import or external binary/plugin fixture and its contract identifiers",
    "ordinary_word": "English substring unrelated to system naming"
  },
  "integrity": {
    "dead_end": "Attempts retired tched_router/dead :8084 invocation or demonstrated missing receiver",
    "alive": "Demonstrated active provider or operational FUSE CLI/plugin invocation",
    "not_applicable": "Contract/package/test fixture or ordinary word, no receiver invoked"
  },
  "action": {
    "repoint_to_op_llm": "Direct model/provider generation must use op-llm",
    "repoint_to_threetched_fuse": "External binary invocation must use identity-bound threetched FUSE CLI",
    "delete_dead_end": "Retired dispatch/facade/obsolete dead-end test",
    "rename_to_3tched": "Misnamed filesystem reference",
    "rename_to_threetched": "Misnamed external binary reference",
    "leave_as_is": "Correct contract/package/threetched fixture or ordinary word; no demonstrated reroute needed"
  }
}
```

## Verbatim question instructions

```text
L877_role: Classify the single tched occurrence in 'threetched.read' at descriptor.rs:877 for role. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L877_integrity: Classify the single tched occurrence in 'threetched.read' at descriptor.rs:877 for integrity. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L877_action: Classify the single tched occurrence in 'threetched.read' at descriptor.rs:877 for action. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L878_role: Classify the single tched occurrence in 'obs.threetched.configure@v1' at descriptor.rs:878 for role. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L878_integrity: Classify the single tched occurrence in 'obs.threetched.configure@v1' at descriptor.rs:878 for integrity. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L878_action: Classify the single tched occurrence in 'obs.threetched.configure@v1' at descriptor.rs:878 for action. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L886_role: Classify the single tched occurrence in 'threetched' at descriptor.rs:886 for role. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L886_integrity: Classify the single tched occurrence in 'threetched' at descriptor.rs:886 for integrity. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L886_action: Classify the single tched occurrence in 'threetched' at descriptor.rs:886 for action. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L888_role: Classify the single tched occurrence in 'threetched' at descriptor.rs:888 for role. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L888_integrity: Classify the single tched occurrence in 'threetched' at descriptor.rs:888 for integrity. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
L888_action: Classify the single tched occurrence in 'threetched' at descriptor.rs:888 for action. Use exact surrounding test context and audit authority; distinguish descriptor synthesis from invocation. Choose only an evidence-supported category.
```

## Returned answers

JSON transcription of the returned dictionary, preserving all values:

```json
{
  "L877_role": {"type":"choice","choice":"contract_identifier","confidence":1,"probabilities":{"routing_dispatch":0,"model_selection_calling":0,"filesystem_projection":0,"contract_identifier":1,"ordinary_word":0}},
  "L877_integrity": {"type":"choice","choice":"not_applicable","confidence":1,"probabilities":{"dead_end":0,"alive":0,"not_applicable":1}},
  "L877_action": {"type":"choice","choice":"leave_as_is","confidence":0.96,"probabilities":{"repoint_to_op_llm":0,"repoint_to_threetched_fuse":0,"delete_dead_end":0.01,"rename_to_3tched":0,"rename_to_threetched":0.02,"leave_as_is":0.97}},
  "L878_role": {"type":"choice","choice":"contract_identifier","confidence":1,"probabilities":{"routing_dispatch":0,"model_selection_calling":0,"filesystem_projection":0,"contract_identifier":1,"ordinary_word":0}},
  "L878_integrity": {"type":"choice","choice":"not_applicable","confidence":1,"probabilities":{"dead_end":0,"alive":0,"not_applicable":1}},
  "L878_action": {"type":"choice","choice":"leave_as_is","confidence":0.98,"probabilities":{"repoint_to_op_llm":0,"repoint_to_threetched_fuse":0,"delete_dead_end":0.01,"rename_to_3tched":0,"rename_to_threetched":0.01,"leave_as_is":0.98}},
  "L886_role": {"type":"choice","choice":"contract_identifier","confidence":1,"probabilities":{"routing_dispatch":0,"model_selection_calling":0,"filesystem_projection":0,"contract_identifier":1,"ordinary_word":0}},
  "L886_integrity": {"type":"choice","choice":"not_applicable","confidence":1,"probabilities":{"dead_end":0,"alive":0,"not_applicable":1}},
  "L886_action": {"type":"choice","choice":"leave_as_is","confidence":0.98,"probabilities":{"repoint_to_op_llm":0,"repoint_to_threetched_fuse":0,"delete_dead_end":0.01,"rename_to_3tched":0,"rename_to_threetched":0.01,"leave_as_is":0.98}},
  "L888_role": {"type":"choice","choice":"contract_identifier","confidence":1,"probabilities":{"routing_dispatch":0,"model_selection_calling":0,"filesystem_projection":0,"contract_identifier":1,"ordinary_word":0}},
  "L888_integrity": {"type":"choice","choice":"not_applicable","confidence":1,"probabilities":{"dead_end":0,"alive":0,"not_applicable":1}},
  "L888_action": {"type":"choice","choice":"leave_as_is","confidence":0.98,"probabilities":{"repoint_to_op_llm":0,"repoint_to_threetched_fuse":0,"delete_dead_end":0.01,"rename_to_3tched":0,"rename_to_threetched":0.01,"leave_as_is":0.98}}
}
```

## Auditor disposition

Accept all four role/integrity/action judgments. The source constructs reflection descriptors and parses JSON; it does not attempt operation dispatch. All four spellings correctly identify the external-surface fixture, not the filesystem. No uncertainty below 0.6, no reroute, no deletion or rename. Full-reader coverage, not this selected context alone, supplies the four-occurrence total.
