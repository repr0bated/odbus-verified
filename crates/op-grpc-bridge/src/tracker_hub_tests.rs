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
    assert_eq!(
        files_containing(&live, ".complete_operation_with_updates("),
        ["runtime.rs"]
    );
    assert_eq!(files_containing(&live, "call_owned("), ["runtime.rs"]);
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
