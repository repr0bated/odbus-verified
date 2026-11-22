//! Owner rules (2026-11-22), enforced on the source so a bypass fails the test
//! suite:
//!
//! * The door is the sole ingress; every tool/method call from any origin
//!   (door CallMethod/Mutate/SetProperty, FUSE, lungs, nested agent/workflow/
//!   chat calls, the exchange point) enters `Runtime::call`.
//! * The execution tracker is the sole gate before real egress: a plugin
//!   effect needs a core operation (`begin_operation*`) and a `BackendContext`,
//!   which exist only in `Runtime::call_owned`, after the tracker commits
//!   `Running` (`checkpoint_transition` -> `checkpoint_record`).
//! * The sled ledger is written only by the tracker's ReceiptCompleted commit,
//!   as one action (`checkpoint_record` -> `runtime_sled_ledger::append_from_tracker`).

use std::path::PathBuf;

fn sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut stack = vec![dir];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                if name.ends_with("_tests.rs") || name == "tracker_hub_tests.rs" {
                    continue;
                }
                out.push((name, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    out
}

/// Modules actually compiled into the bridge: `lib.rs` `#[path]`/`mod`
/// declarations plus one level of nested `#[path]` modules.
fn live(sources: &[(String, String)]) -> Vec<(String, String)> {
    let lib = &sources.iter().find(|(name, _)| name == "lib.rs").unwrap().1;
    let mut names: Vec<String> = vec!["lib.rs".into()];
    let declared = |text: &str, names: &mut Vec<String>| {
        for line in text.lines().map(str::trim) {
            if let Some(rest) = line.strip_prefix("#[path = \"") {
                if let Some(file) = rest.split('"').next() {
                    names.push(file.rsplit('/').next().unwrap().to_owned());
                }
            } else if let Some(rest) = line
                .strip_prefix("pub mod ")
                .or_else(|| line.strip_prefix("pub(crate) mod "))
                .or_else(|| line.strip_prefix("mod "))
            {
                if let Some(module) = rest.strip_suffix(';') {
                    names.push(format!("{module}.rs"));
                }
            }
        }
    };
    declared(lib, &mut names);
    let first: Vec<String> = names.clone();
    for name in first {
        if let Some((_, text)) = sources.iter().find(|(file, _)| *file == name) {
            declared(text, &mut names);
        }
    }
    sources
        .iter()
        .filter(|(name, _)| names.contains(name))
        .cloned()
        .collect()
}

fn files_containing(sources: &[(String, String)], needle: &str) -> Vec<String> {
    let mut files: Vec<String> = sources
        .iter()
        .filter(|(_, text)| text.contains(needle))
        .map(|(name, _)| name.clone())
        .collect();
    files.sort();
    files
}

#[test]
fn core_operations_begin_and_complete_only_in_the_tracked_call() {
    let live = live(&sources());
    assert_eq!(
        files_containing(&live, ".begin_operation_attributed("),
        ["runtime.rs"]
    );
    // core_operations.rs: the core's own `complete_operation` wrapper, which
    // nothing outside the core calls (asserted below).
    assert_eq!(
        files_containing(&live, ".complete_operation_with_updates("),
        ["core_operations.rs", "runtime.rs"]
    );
    assert!(files_containing(&live, ".complete_operation(").is_empty());
    assert!(files_containing(&live, ".begin_operation(").is_empty());
    // decoy_network.rs: the separate op-decoy-network process admitting a call
    // the bridge already tracked (runtime_network -> decoy_network::dispatch).
    assert_eq!(
        files_containing(&live, "call_owned("),
        ["decoy_network.rs", "runtime.rs"]
    );
}

#[test]
fn backend_contexts_are_built_only_inside_an_admitted_call() {
    // runtime_chat_conversations derives the asking session's context from an
    // already-admitted ctx (same operation); every other literal is a bypass.
    let live = live(&sources());
    assert_eq!(
        files_containing(&live, "BackendContext {"),
        ["runtime.rs", "runtime_chat_conversations.rs"]
    );
}

#[test]
fn tracker_running_commit_precedes_backend_dispatch() {
    let runtime =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime.rs"))
            .unwrap();
    let owned = runtime.find("async fn call_owned(").expect("call_owned");
    let body = &runtime[owned..];
    let running = body
        .find("ExecutionStatus::Running")
        .expect("Running commit");
    let dispatch = body.find("self.dispatch(&ctx").expect("backend dispatch");
    assert!(
        running < dispatch,
        "tracker Running commit must precede backend egress"
    );
}

#[test]
fn sled_ledger_is_written_only_from_the_tracker_commit() {
    let live = live(&sources());
    assert_eq!(
        files_containing(&live, "append_from_tracker("),
        ["runtime_checkpoint.rs", "runtime_sled_ledger.rs"]
    );
    assert_eq!(
        files_containing(&live, "runtime_sled_ledger::entry_for("),
        ["runtime.rs"]
    );
    assert_eq!(
        files_containing(&live, "\"current.jsonl\""),
        ["runtime_sled_ledger.rs"]
    );
    let checkpoint = &live
        .iter()
        .find(|(name, _)| name == "runtime_checkpoint.rs")
        .unwrap()
        .1;
    let commit = checkpoint
        .find("apply_committed(record.clone())")
        .expect("tracker commit");
    let append = checkpoint
        .find("append_from_tracker(")
        .expect("sled append");
    assert!(
        commit < append,
        "sled append must follow the tracker commit"
    );
}

/// Every live site of a real-egress primitive, audited 2026-11-22. A new site
/// fails here until it is routed through `Runtime::call` (or classified).
///
/// * Inside a tracked call (`BackendContext`, after the `Running` commit):
///   chat_router_*, model_registry_dispatch, runtime_agent_provider (provider
///   HTTP), runtime_platform (emqx plugin), proxy_methods (forwarder configured
///   by a tracked call), runtime_lungs (lungs socket), nlm_cli (via
///   runtime_cognitive), responder_wake + runtime_chat_conversations (wake in
///   submit_turn), runtime_misc_cozo (generic cozo), publish_backend_state
///   callers (state projections of tracked operations; never the sled).
/// * runtime_mqtt_wake: content-free wake for a commit the tracker already
///   made; the tracker's own checkpoints emit these notices, so tracking a
///   wake would recurse. runtime_mqtt_auth: kick on revoked authority.
/// * mqtt_doorbell_relay, tls_door: byte transport on the door ingress.
/// * Accepted untracked, recorded in the spec: compliance_http (the
///   /compliance/api proxy, behind the verified door identity since the owner
///   decision of 2026-11-22) and chat_link (ChatLink worker handoff, 25.3).
#[test]
fn real_egress_sites_are_the_audited_set() {
    let live = live(&sources());
    let audited: &[(&str, &[&str])] = &[
        (
            "dispatch_emqx_method(",
            &[
                "responder_wake.rs",
                "runtime_mqtt_auth.rs",
                "runtime_mqtt_wake.rs",
                "runtime_platform.rs",
            ],
        ),
        (
            "publish_responder_wake(",
            &["responder_wake.rs", "runtime_chat_conversations.rs"],
        ),
        (
            "publish_backend_state(",
            &[
                "runtime.rs",
                "runtime_chat.rs",
                "runtime_chat_conversations.rs",
                "runtime_chat_generations.rs",
                "runtime_creation.rs",
                "runtime_network.rs",
            ],
        ),
        ("Command::new(", &["chat_link.rs", "nlm_cli.rs"]),
        (
            "reqwest::",
            &[
                "chat_router_agent_route.rs",
                "chat_router_catalog_binding.rs",
                "chat_router_generation.rs",
                "chat_router_runtime.rs",
                "compliance_http.rs",
                "model_registry_dispatch.rs",
                "runtime_agent_provider.rs",
            ],
        ),
        (
            "TcpStream::connect",
            &["mqtt_doorbell_relay.rs", "proxy_methods.rs", "tls_door.rs"],
        ),
        ("UnixStream::connect", &["runtime_lungs.rs"]),
        ("dispatch_cozo_method(", &["runtime_misc_cozo.rs"]),
    ];
    for (needle, expected) in audited {
        assert_eq!(&files_containing(&live, needle), expected, "{needle}");
    }
}

#[test]
fn generic_cozo_refuses_personal_relations_before_opening_the_store() {
    let cozo = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/runtime_misc_cozo.rs"),
    )
    .unwrap();
    let dispatch = cozo
        .find("pub(super) async fn dispatch(")
        .expect("dispatch");
    let body = &cozo[dispatch..];
    let guard = body
        .find("refuse_personal_relations(method, args)")
        .expect("personal relation guard");
    let open = body.find(".cozo_shuttle()").expect("store open");
    let native = body.find("dispatch_cozo_method(").expect("native dispatch");
    assert!(guard < open && guard < native);
}

fn source(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join(name),
    )
    .unwrap()
}

/// Owner rule 2026-11-22: personal memory lives inside `identity-<session>`
/// but apart from the sled. The sled ledger and its finalize/vectorization
/// read only `ledger/` (non-recursively); the personal store is `memory/cozo`;
/// nothing else in the live bridge names or walks an identity volume.
#[test]
fn personal_store_is_outside_every_sled_reader() {
    let ledger = source("runtime_sled_ledger.rs");
    let cognitive = source("runtime_cognitive.rs");
    assert!(ledger.contains("let ledger = sled.join(\"ledger\");"));
    assert!(!ledger.contains("\"memory\""));
    assert_eq!(ledger.matches("read_dir(").count(), 1);
    assert!(ledger.contains("std::fs::read_dir(&ledger)?"));
    assert!(cognitive.contains("let memory = volume.join(\"memory\");"));
    assert!(!cognitive.contains("join(\"ledger\")"));
    let live = live(&sources());
    assert_eq!(
        files_containing(&live, "default_identity-"),
        [
            "runtime_cognitive.rs",
            "runtime_identity_owner.rs",
            "runtime_sled_ledger.rs"
        ]
    );
}

#[test]
fn compliance_api_is_mounted_only_behind_the_door_identity() {
    let server = source("server.rs");
    assert!(server.contains("crate::compliance_http::authenticated_compliance_api_proxy("));
    assert!(!server.contains("compliance_http::compliance_api_proxy"));
}
