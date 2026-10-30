# Declarative Jev judgments — op-chat-responder

Task: `tched-routing-audit-20261024/op-chat-responder`; auditor: Codex crate worker; date: 2026-10-24.

Two actual installed `judge(state, questions)` requests. No completion-model substitute. J1: 50 typed choices (16 occurrences × 3, F1 action, U1 contract uncertainty). J2: 2 supplemental choices. Below are the exact submitted state, questions, returned choices, probabilities and confidence. Source summaries explicitly identify themselves; other contexts preserve submitted source transcription. All paths in state are relative to `crates/op-chat-responder/`.

## Interpretation limits

Confidence <0.6 is not decisive. J1 weak answers: O03_role0.35; O04_integrity0.37; O10_role0.56; O12_role0.36; O13_role0.59. They remain unresolved taxonomy/contract uncertainties, not accepted classifications. O13 alive0.86 means static active-plugin/mounted-path alignment under the assignment premise, NOT proved runtime method support. U1=unverified_external_contract1 confirms the limit. Negative warnings are not executable calls. F1 is one separate deduplicated missing-generation path, not one dead end per warning.

## J1 exact request state

```json
{
  "task": "tched-routing-audit-20261024/op-chat-responder",
  "authority": "Report only. tched_router is retired/no schema; threetched is active external plugin accessed only through identity-bound FUSE; 3tchedFS is our filesystem, tched-fs package/tched_fs import and ../threetched-fs retained path are correct contracts. op-llm owns direct model generation. Static evidence only: no runtime, sealed declaration or external receiver verification.",
  "contexts": {
    "Cargo.toml:11-19": "[dependencies]\nanyhow = \"1\"\nclap = { version = \"4\", features = [\"derive\", \"env\"] }\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\ntched-fs = { path = \"../threetched-fs\" }\ntracing = \"0.1\"\ntracing-subscriber = { version = \"0.3\", features = [\"env-filter\"] }\nuuid = { version = \"1\", features = [\"v4\"] }",
    "src/main.rs:1-34": "//! Guest oneshot chat responder for OP-DBUS.\n//!\n//! The chatbot guest unit launches exactly one designated turn:\n//! `op-chat-responder --turn-id %i`. This binary opens the guest's mounted\n//! views, reads scoped human input, observes the control-plane selection,\n//! runs the **our-stack** generate hook, and commits via `chat_response`\n//! (`respond_to_user` or `cannot_perform`).\n//!\n//! ZeroClaw/`threetched` is foundation only: config observation may use\n//! `threetched.config.get`. Chat generate must **not** call\n//! `threetched.stateless-generate` or any threetched chat/generate CLI.\n//!\n//! Identity is the live guest kernel binding — not a CLI principal, host MQTT\n//! wake, or D-Bus `ProcessPendingTurn` relay (those paths have no receiver).\n\nuse anyhow::{Context, Result, bail, ensure};\nuse clap::Parser;\nuse serde::Deserialize;\nuse serde_json::{Value, json};\nuse std::path::Path;\nuse tched_fs::invocation::Deadline;\nuse tched_fs::mounted::{GUEST_MOUNT_PATHS, MountedNamespace, declared_mount_index};\nuse tched_fs::namespace::MountIdentity;\nuse tracing::{info, warn};\n\nconst CONTROL_PLANE_AGENT: &str = \"control_plane\";\nconst MODEL_PROVIDER_PATH: &str = \"agents.control_plane.model_provider\";\n\n/// Cross-crate CallMethod the combiner must seal, grant, and guest-mount so\n/// this worker can produce a model reply without threetched chat/generate CLI.\n/// Dispatch target: `op_llm::chat::ChatManager` (our stack), not\n/// `threetched.stateless-generate` and not `tched_router.Chat`.\nconst OUR_GENERATE_CALLMETHOD: &str =\n    \"PluginService.CallMethod{plugin_id=\\\"large_language_model\\\",method=\\\"stream_generate\\\"}\";",
    "src/main.rs:150-198": "fn config_get(views: &GuestViews, path: &str, turn_id: &str) -> Result<Value> {\n    let result = views.call_response(\n        \"threetched\",\n        \"config.get\",\n        json!({\n            \"arguments\": [path],\n            \"--json\": true,\n        }),\n        turn_id,\n        Deadline::ordinary(),\n    )?;\n    let stdout = process_receipt_stdout(&result)?;\n    let parsed: ConfigJson =\n        serde_json::from_str(stdout).context(\"config.get --json stdout is not {{path,value}}\")?;\n    ensure!(\n        parsed.path == path,\n        \"config.get returned path {} for requested {path}\",\n        parsed.path\n    );\n    Ok(parsed.value)\n}\n\nfn observe_selection(views: &GuestViews, turn_id: &str) -> Result<(String, String)> {\n    let provider_ref = config_get(views, MODEL_PROVIDER_PATH, turn_id)?;\n    let provider_ref = provider_ref\n        .as_str()\n        .context(\"agents.control_plane.model_provider is not a string\")?\n        .trim();\n    ensure!(\n        !provider_ref.is_empty() && !provider_ref.contains('\\0'),\n        \"control-plane model_provider is empty or invalid\"\n    );\n    let (family, alias) = provider_ref\n        .split_once('.')\n        .context(\"model_provider must be family.alias\")?;\n    ensure!(\n        !family.is_empty() && !alias.is_empty() && !alias.contains('.'),\n        \"model_provider family.alias is malformed\"\n    );\n    let model_path = format!(\"providers.models.{family}.{alias}.model\");\n    let model = config_get(views, &model_path, turn_id)?;\n    let model = model\n        .as_str()\n        .context(\"selected model path is not a string\")?\n        .trim()\n        .to_owned();\n    ensure!(!model.is_empty() && !model.contains('\\0'), \"selected model is empty\");\n    Ok((family.to_owned(), model))\n}",
    "src/main.rs:200-222": "/// Our-stack generate hook for a designated response turn.\n///\n/// When combiner has sealed/granted/guest-mounted [`OUR_GENERATE_CALLMETHOD`]\n/// and retargeted it to `op_llm` (not threetched generate CLI), replace the\n/// stub body with a mounted `large_language_model.stream_generate` call that\n/// returns a done frame. Until then this is an explicit non-ZC stub.\nfn our_stack_generate(\n    _views: &GuestViews,\n    turn_id: &str,\n    prompt: &str,\n    provider: &str,\n    model: &str,\n    response_mode: &str,\n) -> Result<DoneFrame> {\n    let _ = (prompt, CONTROL_PLANE_AGENT);\n    bail!(\n        \"our-stack generate unavailable for turn {turn_id} \\\n         (mode={response_mode}, provider={provider}, model={model}): \\\n         combiner must wire guest-mounted {OUR_GENERATE_CALLMETHOD} \\\n         → op_llm::chat::ChatManager; do not use threetched.stateless-generate \\\n         or any threetched chat/generate CLI\"\n    )\n}",
    "src/main.rs:80-123": "GuestViews::open opens both GUEST_MOUNT_PATHS using MountedNamespace::open_for_mode and MountIdentity::GuestKernel. for_plugin resolves declared_mount_index from contains_plugin for both mounts. call_response obtains mount.method_decl(plugin, method), invokes call_for_response(plugin, method, &decl, arguments, turn_id, deadline), and extracts receipt.result. [Source summary, not verbatim.]",
    "src/main.rs:308-351": "process_turn observes provider/model using observe_selection, then calls our_stack_generate with generation.prompt and response_mode. Ok(frame) checks type done, exact provider/model match and bounded nonempty response before commit_response. Err(error) builds automatic generation deferred reason with OUR_GENERATE_CALLMETHOD and calls refuse. refuse at265-278 invokes chat_response.cannot_perform through call_response. [Source summary, not verbatim.]"
  },
  "occurrences": [
    [
      "O01",
      "Cargo.toml",
      16,
      1,
      "tched-fs",
      "Cargo.toml:11-19"
    ],
    [
      "O02",
      "Cargo.toml",
      16,
      2,
      "../threetched-fs",
      "Cargo.toml:11-19"
    ],
    [
      "O03",
      "src/main.rs",
      9,
      1,
      "threetched",
      "src/main.rs:1-34"
    ],
    [
      "O04",
      "src/main.rs",
      10,
      1,
      "threetched.config.get",
      "src/main.rs:1-34"
    ],
    [
      "O05",
      "src/main.rs",
      11,
      1,
      "threetched.stateless-generate",
      "src/main.rs:1-34"
    ],
    [
      "O06",
      "src/main.rs",
      11,
      2,
      "threetched",
      "src/main.rs:1-34"
    ],
    [
      "O07",
      "src/main.rs",
      21,
      1,
      "tched_fs",
      "src/main.rs:1-34"
    ],
    [
      "O08",
      "src/main.rs",
      22,
      1,
      "tched_fs",
      "src/main.rs:1-34"
    ],
    [
      "O09",
      "src/main.rs",
      23,
      1,
      "tched_fs",
      "src/main.rs:1-34"
    ],
    [
      "O10",
      "src/main.rs",
      30,
      1,
      "threetched",
      "src/main.rs:1-34"
    ],
    [
      "O11",
      "src/main.rs",
      32,
      1,
      "threetched.stateless-generate",
      "src/main.rs:1-34"
    ],
    [
      "O12",
      "src/main.rs",
      32,
      2,
      "tched_router.Chat",
      "src/main.rs:1-34"
    ],
    [
      "O13",
      "src/main.rs",
      152,
      1,
      "threetched",
      "src/main.rs:150-198"
    ],
    [
      "O14",
      "src/main.rs",
      203,
      1,
      "threetched",
      "src/main.rs:200-222"
    ],
    [
      "O15",
      "src/main.rs",
      219,
      1,
      "threetched.stateless-generate",
      "src/main.rs:200-222"
    ],
    [
      "O16",
      "src/main.rs",
      220,
      1,
      "threetched",
      "src/main.rs:200-222"
    ]
  ]
}
```

## J1 exact questions

```json
{
  "O01_role": {
    "type": "choice",
    "instructions": "For occurrence O01: Cargo.toml:16, token tched-fs, ordinal 1 on its line. Use context Cargo.toml:11-19 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O01_integrity": {
    "type": "choice",
    "instructions": "For occurrence O01: Cargo.toml:16, token tched-fs, ordinal 1 on its line. Use context Cargo.toml:11-19 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O01_action": {
    "type": "choice",
    "instructions": "For occurrence O01: Cargo.toml:16, token tched-fs, ordinal 1 on its line. Use context Cargo.toml:11-19 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O02_role": {
    "type": "choice",
    "instructions": "For occurrence O02: Cargo.toml:16, token ../threetched-fs, ordinal 2 on its line. Use context Cargo.toml:11-19 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O02_integrity": {
    "type": "choice",
    "instructions": "For occurrence O02: Cargo.toml:16, token ../threetched-fs, ordinal 2 on its line. Use context Cargo.toml:11-19 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O02_action": {
    "type": "choice",
    "instructions": "For occurrence O02: Cargo.toml:16, token ../threetched-fs, ordinal 2 on its line. Use context Cargo.toml:11-19 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O03_role": {
    "type": "choice",
    "instructions": "For occurrence O03: src/main.rs:9, token threetched, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O03_integrity": {
    "type": "choice",
    "instructions": "For occurrence O03: src/main.rs:9, token threetched, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O03_action": {
    "type": "choice",
    "instructions": "For occurrence O03: src/main.rs:9, token threetched, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O04_role": {
    "type": "choice",
    "instructions": "For occurrence O04: src/main.rs:10, token threetched.config.get, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O04_integrity": {
    "type": "choice",
    "instructions": "For occurrence O04: src/main.rs:10, token threetched.config.get, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O04_action": {
    "type": "choice",
    "instructions": "For occurrence O04: src/main.rs:10, token threetched.config.get, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O05_role": {
    "type": "choice",
    "instructions": "For occurrence O05: src/main.rs:11, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O05_integrity": {
    "type": "choice",
    "instructions": "For occurrence O05: src/main.rs:11, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O05_action": {
    "type": "choice",
    "instructions": "For occurrence O05: src/main.rs:11, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O06_role": {
    "type": "choice",
    "instructions": "For occurrence O06: src/main.rs:11, token threetched, ordinal 2 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O06_integrity": {
    "type": "choice",
    "instructions": "For occurrence O06: src/main.rs:11, token threetched, ordinal 2 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O06_action": {
    "type": "choice",
    "instructions": "For occurrence O06: src/main.rs:11, token threetched, ordinal 2 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O07_role": {
    "type": "choice",
    "instructions": "For occurrence O07: src/main.rs:21, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O07_integrity": {
    "type": "choice",
    "instructions": "For occurrence O07: src/main.rs:21, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O07_action": {
    "type": "choice",
    "instructions": "For occurrence O07: src/main.rs:21, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O08_role": {
    "type": "choice",
    "instructions": "For occurrence O08: src/main.rs:22, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O08_integrity": {
    "type": "choice",
    "instructions": "For occurrence O08: src/main.rs:22, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O08_action": {
    "type": "choice",
    "instructions": "For occurrence O08: src/main.rs:22, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O09_role": {
    "type": "choice",
    "instructions": "For occurrence O09: src/main.rs:23, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O09_integrity": {
    "type": "choice",
    "instructions": "For occurrence O09: src/main.rs:23, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O09_action": {
    "type": "choice",
    "instructions": "For occurrence O09: src/main.rs:23, token tched_fs, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O10_role": {
    "type": "choice",
    "instructions": "For occurrence O10: src/main.rs:30, token threetched, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O10_integrity": {
    "type": "choice",
    "instructions": "For occurrence O10: src/main.rs:30, token threetched, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O10_action": {
    "type": "choice",
    "instructions": "For occurrence O10: src/main.rs:30, token threetched, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O11_role": {
    "type": "choice",
    "instructions": "For occurrence O11: src/main.rs:32, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O11_integrity": {
    "type": "choice",
    "instructions": "For occurrence O11: src/main.rs:32, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O11_action": {
    "type": "choice",
    "instructions": "For occurrence O11: src/main.rs:32, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O12_role": {
    "type": "choice",
    "instructions": "For occurrence O12: src/main.rs:32, token tched_router.Chat, ordinal 2 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O12_integrity": {
    "type": "choice",
    "instructions": "For occurrence O12: src/main.rs:32, token tched_router.Chat, ordinal 2 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O12_action": {
    "type": "choice",
    "instructions": "For occurrence O12: src/main.rs:32, token tched_router.Chat, ordinal 2 on its line. Use context src/main.rs:1-34 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O13_role": {
    "type": "choice",
    "instructions": "For occurrence O13: src/main.rs:152, token threetched, ordinal 1 on its line. Use context src/main.rs:150-198 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O13_integrity": {
    "type": "choice",
    "instructions": "For occurrence O13: src/main.rs:152, token threetched, ordinal 1 on its line. Use context src/main.rs:150-198 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O13_action": {
    "type": "choice",
    "instructions": "For occurrence O13: src/main.rs:152, token threetched, ordinal 1 on its line. Use context src/main.rs:150-198 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O14_role": {
    "type": "choice",
    "instructions": "For occurrence O14: src/main.rs:203, token threetched, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O14_integrity": {
    "type": "choice",
    "instructions": "For occurrence O14: src/main.rs:203, token threetched, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O14_action": {
    "type": "choice",
    "instructions": "For occurrence O14: src/main.rs:203, token threetched, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O15_role": {
    "type": "choice",
    "instructions": "For occurrence O15: src/main.rs:219, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O15_integrity": {
    "type": "choice",
    "instructions": "For occurrence O15: src/main.rs:219, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O15_action": {
    "type": "choice",
    "instructions": "For occurrence O15: src/main.rs:219, token threetched.stateless-generate, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "O16_role": {
    "type": "choice",
    "instructions": "For occurrence O16: src/main.rs:220, token threetched, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Classify role and functional intent.",
    "criteria": {
      "routing_dispatch": "Routing control plane/runtime dispatch",
      "model_selection_calling": "Model selection/config observation or generation intent, including explicit prohibited alternatives",
      "filesystem_projection": "Filesystem/mount/SHM projection reference",
      "contract_identifier": "Package/import/retained directory/external binary identifier, no operation intent",
      "ordinary_word": "Ordinary English substring"
    }
  },
  "O16_integrity": {
    "type": "choice",
    "instructions": "For occurrence O16: src/main.rs:220, token threetched, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Classify integrity of THIS occurrence, distinguishing a negative comment/error warning from an executed call. Active plugin alignment is not runtime verification.",
    "criteria": {
      "dead_end": "Occurrence executes or proposes an unimplemented/retired operation; do not count a negative prohibition as a call",
      "alive": "Actual mounted call to active plugin per assignment, static alignment only, not proof of method/deployment availability",
      "not_applicable": "Package/import/identifier or negative documentary prohibition; not an executed call"
    }
  },
  "O16_action": {
    "type": "choice",
    "instructions": "For occurrence O16: src/main.rs:220, token threetched, ordinal 1 on its line. Use context src/main.rs:200-222 plus supplied whole call-path context. Choose required action for THIS occurrence. Treat enclosing generation stub deficiency separately from correctly spelled negative prohibitions.",
    "criteria": {
      "repoint_to_op_llm": "Generation currently targets wrong/missing route and requires op-llm",
      "repoint_to_threetched_fuse": "External binary call requires identity-bound FUSE migration",
      "delete_dead_end": "Obsolete executable dispatch/facade/test must be deleted",
      "rename_to_3tched": "Filesystem spelling incorrect",
      "rename_to_threetched": "External binary spelling incorrect",
      "leave_as_is": "Correct identifier/import/mounted external config observation or accurate prohibition; no change to token"
    }
  },
  "F1": {
    "type": "choice",
    "instructions": "Evaluate our_stack_generate (200-222) with process_turn308-351. What action best addresses the actual generation path, independent of the correctly negative threetched mentions? No changes authorized in this audit.",
    "criteria": {
      "repoint_to_op_llm": "Unconditional bail means no model generation receiver is invoked; implement identity-bound mounted our-stack model generation to op-llm",
      "repoint_to_threetched_fuse": "Invoke external foundation generation instead",
      "leave_as_is": "Working model generation is already implemented"
    }
  },
  "U1": {
    "type": "choice",
    "instructions": "Does assigned source alone prove threetched.config.get supports arguments+[--json] and emits {path,value} at runtime?",
    "criteria": {
      "unverified_external_contract": "Mounted dispatch is shown but receiver schema/implementation/live availability absent from assigned evidence; preserve uncertainty",
      "proven_contract": "Assigned source proves sealed receiver and runtime output",
      "proven_dead_end": "Assigned source demonstrates config.get cannot exist"
    }
  }
}
```

## J1 exact returned answers

```json
{
  "O01_role": {
    "type": "choice",
    "choice": "contract_identifier",
    "confidence": 0.96,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0,
      "filesystem_projection": 0.03,
      "contract_identifier": 0.97,
      "ordinary_word": 0
    }
  },
  "O01_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0,
      "alive": 0.01,
      "not_applicable": 0.99
    }
  },
  "O01_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0.01,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O02_role": {
    "type": "choice",
    "choice": "contract_identifier",
    "confidence": 0.92,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0,
      "filesystem_projection": 0.06,
      "contract_identifier": 0.94,
      "ordinary_word": 0
    }
  },
  "O02_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.94,
    "probabilities": {
      "dead_end": 0,
      "alive": 0.04,
      "not_applicable": 0.96
    }
  },
  "O02_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0.01,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O03_role": {
    "type": "choice",
    "choice": "contract_identifier",
    "confidence": 0.35,
    "probabilities": {
      "routing_dispatch": 0.01,
      "model_selection_calling": 0.24,
      "filesystem_projection": 0.05,
      "contract_identifier": 0.48,
      "ordinary_word": 0.22
    }
  },
  "O03_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.94,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0.03,
      "not_applicable": 0.96
    }
  },
  "O03_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.95,
    "probabilities": {
      "repoint_to_op_llm": 0.01,
      "repoint_to_threetched_fuse": 0.01,
      "delete_dead_end": 0,
      "rename_to_3tched": 0.01,
      "rename_to_threetched": 0.01,
      "leave_as_is": 0.96
    }
  },
  "O04_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.97,
    "probabilities": {
      "routing_dispatch": 0.01,
      "model_selection_calling": 0.98,
      "filesystem_projection": 0,
      "contract_identifier": 0.01,
      "ordinary_word": 0
    }
  },
  "O04_integrity": {
    "type": "choice",
    "choice": "alive",
    "confidence": 0.37,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0.58,
      "not_applicable": 0.41
    }
  },
  "O04_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.98,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0.02,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.98
    }
  },
  "O05_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.99,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0.99,
      "filesystem_projection": 0,
      "contract_identifier": 0.01,
      "ordinary_word": 0
    }
  },
  "O05_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0,
      "not_applicable": 0.99
    }
  },
  "O05_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 1,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 1
    }
  },
  "O06_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.66,
    "probabilities": {
      "routing_dispatch": 0.02,
      "model_selection_calling": 0.73,
      "filesystem_projection": 0,
      "contract_identifier": 0.21,
      "ordinary_word": 0.04
    }
  },
  "O06_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0,
      "not_applicable": 0.99
    }
  },
  "O06_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0.01,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O07_role": {
    "type": "choice",
    "choice": "contract_identifier",
    "confidence": 0.91,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0,
      "filesystem_projection": 0.07,
      "contract_identifier": 0.93,
      "ordinary_word": 0
    }
  },
  "O07_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0,
      "alive": 0.01,
      "not_applicable": 0.99
    }
  },
  "O07_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 1,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 1
    }
  },
  "O08_role": {
    "type": "choice",
    "choice": "contract_identifier",
    "confidence": 0.85,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0,
      "filesystem_projection": 0.12,
      "contract_identifier": 0.88,
      "ordinary_word": 0
    }
  },
  "O08_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0,
      "alive": 0.01,
      "not_applicable": 0.99
    }
  },
  "O08_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0.01,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O09_role": {
    "type": "choice",
    "choice": "contract_identifier",
    "confidence": 0.85,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0,
      "filesystem_projection": 0.12,
      "contract_identifier": 0.88,
      "ordinary_word": 0
    }
  },
  "O09_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 1,
    "probabilities": {
      "dead_end": 0,
      "alive": 0,
      "not_applicable": 1
    }
  },
  "O09_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 1,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 1
    }
  },
  "O10_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.56,
    "probabilities": {
      "routing_dispatch": 0.04,
      "model_selection_calling": 0.65,
      "filesystem_projection": 0.01,
      "contract_identifier": 0.26,
      "ordinary_word": 0.04
    }
  },
  "O10_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.95,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0.02,
      "not_applicable": 0.97
    }
  },
  "O10_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.98,
    "probabilities": {
      "repoint_to_op_llm": 0.01,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0.01,
      "leave_as_is": 0.98
    }
  },
  "O11_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.96,
    "probabilities": {
      "routing_dispatch": 0.01,
      "model_selection_calling": 0.97,
      "filesystem_projection": 0,
      "contract_identifier": 0.02,
      "ordinary_word": 0
    }
  },
  "O11_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 1,
    "probabilities": {
      "dead_end": 0,
      "alive": 0,
      "not_applicable": 1
    }
  },
  "O11_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0.01,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O12_role": {
    "type": "choice",
    "choice": "routing_dispatch",
    "confidence": 0.36,
    "probabilities": {
      "routing_dispatch": 0.49,
      "model_selection_calling": 0.39,
      "filesystem_projection": 0,
      "contract_identifier": 0.12,
      "ordinary_word": 0
    }
  },
  "O12_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0,
      "not_applicable": 0.99
    }
  },
  "O12_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0.01,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O13_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.59,
    "probabilities": {
      "routing_dispatch": 0.14,
      "model_selection_calling": 0.67,
      "filesystem_projection": 0.01,
      "contract_identifier": 0.18,
      "ordinary_word": 0
    }
  },
  "O13_integrity": {
    "type": "choice",
    "choice": "alive",
    "confidence": 0.86,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0.91,
      "not_applicable": 0.08
    }
  },
  "O13_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0,
      "repoint_to_threetched_fuse": 0.01,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O14_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.76,
    "probabilities": {
      "routing_dispatch": 0.01,
      "model_selection_calling": 0.81,
      "filesystem_projection": 0,
      "contract_identifier": 0.1,
      "ordinary_word": 0.08
    }
  },
  "O14_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0,
      "not_applicable": 0.99
    }
  },
  "O14_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.98,
    "probabilities": {
      "repoint_to_op_llm": 0.02,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.98
    }
  },
  "O15_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.99,
    "probabilities": {
      "routing_dispatch": 0,
      "model_selection_calling": 0.99,
      "filesystem_projection": 0,
      "contract_identifier": 0.01,
      "ordinary_word": 0
    }
  },
  "O15_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 1,
    "probabilities": {
      "dead_end": 0,
      "alive": 0,
      "not_applicable": 1
    }
  },
  "O15_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0.01,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "O16_role": {
    "type": "choice",
    "choice": "model_selection_calling",
    "confidence": 0.76,
    "probabilities": {
      "routing_dispatch": 0.01,
      "model_selection_calling": 0.81,
      "filesystem_projection": 0,
      "contract_identifier": 0.15,
      "ordinary_word": 0.02
    }
  },
  "O16_integrity": {
    "type": "choice",
    "choice": "not_applicable",
    "confidence": 0.98,
    "probabilities": {
      "dead_end": 0.01,
      "alive": 0,
      "not_applicable": 0.99
    }
  },
  "O16_action": {
    "type": "choice",
    "choice": "leave_as_is",
    "confidence": 0.99,
    "probabilities": {
      "repoint_to_op_llm": 0.01,
      "repoint_to_threetched_fuse": 0,
      "delete_dead_end": 0,
      "rename_to_3tched": 0,
      "rename_to_threetched": 0,
      "leave_as_is": 0.99
    }
  },
  "F1": {
    "type": "choice",
    "choice": "repoint_to_op_llm",
    "confidence": 0.97,
    "probabilities": {
      "repoint_to_op_llm": 0.98,
      "repoint_to_threetched_fuse": 0,
      "leave_as_is": 0.02
    }
  },
  "U1": {
    "type": "choice",
    "choice": "unverified_external_contract",
    "confidence": 1,
    "probabilities": {
      "unverified_external_contract": 1,
      "proven_contract": 0,
      "proven_dead_end": 0
    }
  }
}
```

## J2 exact request

Same complete J1 state above; questions:

```json
{
  "F1_integrity": {
    "type": "choice",
    "instructions": "From src/main.rs200-222 and308-351, classify the actual automatic generation path (not negative threetched warnings). our_stack_generate always bails; process_turn refuses on that error. Does this implement model generation?",
    "criteria": {
      "dead_end": "Generation has no invoked receiver: unconditional bail; refusal is a different operation",
      "alive": "Model generation is actually dispatched",
      "not_applicable": "No generation path is intended"
    }
  },
  "retired_call": {
    "type": "choice",
    "instructions": "For all sixteen occurrences and the fully read source, does tched_router.Chat at32 actually invoke the retired plugin?",
    "criteria": {
      "negative_reference_only": "The sole retired plugin mention is a not-comment; no callsite targets it",
      "executed_retired_call": "Source invokes the retired plugin",
      "insufficient": "Cannot distinguish from supplied source"
    }
  }
}
```

## J2 exact returned answers

```json
{
  "F1_integrity": {
    "type": "choice",
    "choice": "dead_end",
    "confidence": 1,
    "probabilities": {
      "dead_end": 1,
      "alive": 0,
      "not_applicable": 0
    }
  },
  "retired_call": {
    "type": "choice",
    "choice": "negative_reference_only",
    "confidence": 1,
    "probabilities": {
      "negative_reference_only": 1,
      "executed_retired_call": 0,
      "insufficient": 0
    }
  }
}
```
