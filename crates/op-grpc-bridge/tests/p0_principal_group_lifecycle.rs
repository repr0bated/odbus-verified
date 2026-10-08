//! Separate acceptance coverage for the current principal grant-group implementation.
//!
//! The ordinary test creates a private fixture and runs only the ignored helper in
//! its own child process. OP_GRANTS_PATH is supplied to that child with Command::env;
//! this executable never changes its parent's environment or touches live grants.
#![cfg(target_os = "linux")]

use op_grpc_bridge::interceptor::{load_capability_grants, validate_grant_document};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const CHILD_MARKER: &str = "OP_P0_PRINCIPAL_GROUP_LIFECYCLE_CHILD";
const FIXTURE_PREFIX: &str = "p0-principal-group-lifecycle-";
const PRINCIPAL: &str = "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff";
const OWN: &str = "cognitive_mcp.read";
const INHERITED: &str = "cap.data.cozo.system-graph.read@v1";
const CHANGED: &str = "cap.fixture.changed.read@v1";

fn private_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
}

#[test]
fn principal_group_changes_reclaim_descriptors_in_an_isolated_child() {
    let directory = tempfile::Builder::new()
        .prefix(FIXTURE_PREFIX)
        .tempdir()
        .unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = directory.path().canonicalize().unwrap();
    let grants = root.join("grants.json");
    let marker = format!("p0-private-fixture-parent-{}", std::process::id());
    private_file(&grants, b"{}");
    private_file(&root.join(".p0-child"), marker.as_bytes());

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "principal_group_lifecycle_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("OP_GRANTS_PATH", &grants)
        .env(CHILD_MARKER, &marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "isolated group lifecycle child exceeded 20s: {} {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "isolated group lifecycle child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("512 principal group lifecycle cycles passed"),
        "child did not execute the lifecycle assertions"
    );
    print!("{}", String::from_utf8_lossy(&output.stdout));
}

fn checked_fixture_path() -> std::path::PathBuf {
    let marker = std::env::var(CHILD_MARKER)
        .expect("ignored helper must run only through the explicit private child harness");
    assert!(marker.starts_with("p0-private-fixture-parent-"));
    let path = std::path::PathBuf::from(std::env::var_os("OP_GRANTS_PATH").unwrap());
    assert!(path.is_absolute());
    assert_eq!(path.file_name().unwrap(), "grants.json");
    let root = path.parent().unwrap();
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(FIXTURE_PREFIX)
    );
    assert_eq!(root.canonicalize().unwrap(), root);
    assert_eq!(
        root.parent().unwrap(),
        std::env::temp_dir().canonicalize().unwrap()
    );
    let marker_path = root.join(".p0-child");
    let process_uid = fs::metadata("/proc/self").unwrap().uid();
    for (candidate, directory) in [
        (root, true),
        (path.as_path(), false),
        (marker_path.as_path(), false),
    ] {
        let metadata = fs::symlink_metadata(candidate).unwrap();
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(metadata.uid(), process_uid);
        assert_eq!(metadata.is_dir(), directory);
        if !directory {
            assert!(metadata.is_file());
        }
        assert_eq!(metadata.mode() & 0o077, 0);
    }
    assert_eq!(fs::read_to_string(root.join(".p0-child")).unwrap(), marker);
    path
}

fn open_descriptor_count() -> usize {
    // The iterator's own descriptor is included consistently, then dropped here.
    fs::read_dir("/proc/self/fd").unwrap().count()
}

fn replace_fixture(path: &Path, document: &Value, expected: &[&str], valid: bool) {
    let bytes = serde_json::to_vec(document).unwrap();
    assert_eq!(validate_grant_document(&bytes).is_ok(), valid);
    fs::write(path, bytes).unwrap();
    let expected: HashSet<String> = expected.iter().map(|item| (*item).to_owned()).collect();
    for _ in 0..4 {
        assert_eq!(load_capability_grants(PRINCIPAL), expected);
        assert!(load_capability_grants("$GUEST").is_empty());
    }
}

/// Never selected by the normal test suite. The parent above supplies the exact
/// private directory, ownership checks, marker and OP_GRANTS_PATH in a fresh process.
#[test]
#[ignore = "helper for the explicitly isolated parent test; refuses absent private fixture marker"]
fn principal_group_lifecycle_child() {
    let path = checked_fixture_path();
    let baseline = open_descriptor_count();
    for cycle in 0..512 {
        // Add principal membership and inherit the group's capability.
        let mut document = json!({
            "$GUEST": {"capabilities": [INHERITED]},
            (PRINCIPAL): {"groups": ["$GUEST"], "capabilities": [OWN]}
        });
        replace_fixture(&path, &document, &[OWN, INHERITED], true);

        // Removing membership must remove inherited authority on the next call.
        document[PRINCIPAL]["groups"] = json!([]);
        replace_fixture(&path, &document, &[OWN], true);

        // Readding membership observes the group's changed capability immediately.
        document[PRINCIPAL]["groups"] = json!(["$GUEST"]);
        document["$GUEST"]["capabilities"] = json!([CHANGED]);
        replace_fixture(&path, &document, &[OWN, CHANGED], true);

        // A member loses inherited authority when the group becomes empty.
        document["$GUEST"]["capabilities"] = json!([]);
        replace_fixture(&path, &document, &[OWN], true);

        // Invalid membership fails closed even though the principal has direct grants.
        document[PRINCIPAL]["groups"] = json!(["$NOPE"]);
        replace_fixture(&path, &document, &[], false);

        assert_eq!(
            open_descriptor_count(),
            baseline,
            "progressive descriptor retention after cycle {cycle}"
        );
    }
    replace_fixture(
        &path,
        &json!({
            "$NOPE": {"capabilities": [INHERITED]},
            (PRINCIPAL): {"capabilities": [OWN]}
        }),
        &[],
        false,
    );
    let final_fds = open_descriptor_count();
    assert_eq!(final_fds, baseline);
    println!(
        "512 principal group lifecycle cycles passed fd_baseline={baseline} fd_final={final_fds} cycles=512 lookups=20488"
    );
}
