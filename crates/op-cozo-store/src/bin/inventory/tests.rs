use super::*;
use std::os::unix::fs::symlink;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "bin",
        "sv/bridge",
        "etc/opdbus",
        "etc/mail",
        "usr/share/op-dbus/graphs",
    ] {
        fs::create_dir_all(dir.path().join(name)).unwrap();
    }
    fs::write(
        dir.path().join("MANIFEST"),
        "commit: 123456789abcdef\ngolden-build: fixture\n",
    )
    .unwrap();
    fs::write(dir.path().join("bin/bridge"), b"binary").unwrap();
    fs::write(dir.path().join("sv/bridge/run"),"#!/bin/sh\n# wait_dep ignored\nwait_dep dbus\nexport PASSWORD='private-unretained'\nexec /usr/local/bin/bridge --config /etc/mail/main.cf\n").unwrap();
    fs::write(
        dir.path().join("etc/mail/main.cf"),
        "password = private-unretained\nkey_file = /etc/op-dbus/secrets/mail-key\n",
    )
    .unwrap();
    fs::write(dir.path().join("etc/opdbus/enabled-services"), "bridge\n").unwrap();
    fs::write(dir.path().join("etc/opdbus/retired-services"), "old\n").unwrap();
    dir
}
#[test]
fn dotted_retired_services_keep_declarations_and_reject_traversal_names() {
    let dir = fixture();
    let manifest = dir.path().join("etc/opdbus/retired-services");
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    let names = [
        "netmaker-op-grpc-adapters.retired-netns",
        "nm-ovs-nic.retired",
    ];
    fs::write(&manifest, format!("{}\n{}\n", names[0], names[1])).unwrap();
    let graph = prepared(dir.path(), "release:test", "2026-09-30T00:00:00Z", &output).unwrap();
    for (index, name) in names.iter().enumerate() {
        let id = format!("service:{name}");
        assert!(graph.nodes.iter().any(|node| node.id == id));
        let declaration = graph
            .edges
            .iter()
            .find(|edge| edge.src == id && edge.relation == "declared_retired")
            .unwrap();
        assert_eq!(declaration.dst, "file:etc/opdbus/retired-services");
        assert_eq!(
            declaration.evidence[0].source,
            "etc/opdbus/retired-services"
        );
        assert_eq!(declaration.evidence[0].locator, format!("L{}", index + 1));
    }
    for name in [
        ".",
        "..",
        "../service",
        "service/../old",
        "/old",
        "old\\service",
        ".old",
        "old.",
    ] {
        fs::write(&manifest, format!("{name}\n")).unwrap();
        let error =
            prepared(dir.path(), "release:test", "2026-09-30T00:00:00Z", &output).unwrap_err();
        assert!(
            error.to_string().contains("invalid staged service name"),
            "{name}"
        );
    }
}
#[test]
fn deterministic_inventory_has_real_artifacts_and_line_evidence_without_values() {
    let dir = fixture();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    let snapshot = prepared(
        dir.path(),
        "release:fixture",
        "2026-09-30T00:00:00Z",
        &output,
    )
    .unwrap();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    assert!(!String::from_utf8_lossy(&encoded).contains("private-unretained"));
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|n| n.id == "file:etc/mail/main.cf"
                && n.attributes["sha256"].as_str().unwrap().len() == 64)
    );
    let dep = snapshot
        .edges
        .iter()
        .find(|e| e.relation == "depends_on")
        .unwrap();
    assert_eq!(dep.dst, "service:dbus");
    assert_eq!(dep.evidence[0].locator, "L3");
    assert!(
        snapshot
            .edges
            .iter()
            .any(|e| e.relation == "runs" && e.dst == "file:bin/bridge")
    );
    fs::write(&output, b"ignored graph output").unwrap();
    fs::write(
        dir.path().join("MANIFEST"),
        "commit: 123456789abcdef\ngolden-build: fixture\nsystem-graph-sha256: abcdef\n",
    )
    .unwrap();
    let second = prepared(
        dir.path(),
        "release:fixture",
        "2026-09-30T00:00:00Z",
        &output,
    )
    .unwrap();
    // MANIFEST size is excluded too, since the later graph receipt grows it.
    let mut first = serde_json::to_value(snapshot).unwrap();
    let mut second = serde_json::to_value(second).unwrap();
    for value in [&mut first, &mut second] {
        for node in value["nodes"].as_array_mut().unwrap() {
            if node["id"] == "file:MANIFEST" {
                node["attributes"].as_object_mut().unwrap().remove("size");
            }
        }
    }
    assert_eq!(first, second);
}
#[test]
fn external_symlinks_are_not_followed_and_protected_files_are_not_read() {
    let dir = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private"), "private-unretained").unwrap();
    symlink(outside.path(), dir.path().join("outside")).unwrap();
    fs::create_dir_all(dir.path().join("etc/op-dbus/secrets")).unwrap();
    fs::write(
        dir.path().join("etc/op-dbus/secrets/key"),
        "private-unretained",
    )
    .unwrap();
    let graph = prepared(
        dir.path(),
        "release:test",
        "2026-09-30T00:00:00Z",
        &outside.path().join("graph.json"),
    )
    .unwrap();
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.id == "file:outside" && n.attributes["target_status"] == "not_followed")
    );
    assert!(!graph.nodes.iter().any(|n| n.id == "file:outside/private"));
    let node = graph
        .nodes
        .iter()
        .find(|n| n.id == "file:etc/op-dbus/secrets/key")
        .unwrap();
    assert!(!node.attributes.contains_key("sha256"));
    assert!(
        !serde_json::to_string(&graph)
            .unwrap()
            .contains("private-unretained")
    );
}
#[test]
fn output_rejects_unsafe_path_and_keeps_previous_on_failure() {
    let dir = fixture();
    assert!(
        prepared(
            dir.path(),
            "release:test",
            "2026-09-30T00:00:00Z",
            &dir.path().join("etc/result.json")
        )
        .is_err()
    );
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    fs::write(&output, "previous").unwrap();
    assert!(atomic_write(&output, &vec![0; MAX_JSON + 1]).is_err());
    assert_eq!(fs::read_to_string(&output).unwrap(), "previous");
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), dir.path().join("link")).unwrap();
    assert!(atomic_write(&dir.path().join("link/out"), b"{} ").is_err());
}
#[test]
fn source_conversion_merges_external_requirements_and_rejects_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    let graph = json!({"nodes":[{"id":"meta:crate-graph","label":"meta","props":{"commit":"123456789abcdef"}},{"id":"ext:serde","label":"ext_crate","props":{"req":"^1"}},{"id":"ext:serde","label":"ext_crate","props":{"req":"^2"}}],"edges":[]});
    fs::write(&input, serde_json::to_vec(&graph).unwrap()).unwrap();
    let converted = source_graph(&input, "source:workspace", "2026-09-30T00:00:00Z").unwrap();
    let serde = converted
        .nodes
        .iter()
        .find(|n| n.id == "ext:serde")
        .unwrap();
    assert_eq!(serde.attributes["requirements"], json!(["^1", "^2"]));
    let graph = json!({"nodes":[{"id":"crate:a","label":"crate","props":{"path":"a"}},{"id":"crate:a","label":"crate","props":{"path":"b"}}],"edges":[]});
    fs::write(&input, serde_json::to_vec(&graph).unwrap()).unwrap();
    assert!(source_graph(&input, "source:workspace", "2026-09-30T00:00:00Z").is_err());
}
#[test]
fn static_paths_skip_dynamic_expansion_and_uris() {
    assert_eq!(
        literal_paths(
            "TOKEN=secret https://name:password@host /etc/config ${X:-/etc/default} /etc/$X /etc/../outside"
        ),
        BTreeSet::from(["/etc/config".into(), "/etc/default".into()])
    );
}

#[test]
fn cli_emits_valid_typed_snapshot_with_merged_repeated_evidence() {
    let dir = fixture();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    fs::write(
        dir.path().join("sv/bridge/run"),
        "wait_dep dbus\nwait_dep dbus\nexec /usr/local/bin/bridge\nexec /usr/local/bin/bridge\n",
    )
    .unwrap();
    run(vec![
        "--prepared".into(),
        dir.path().to_str().unwrap().into(),
        "--output".into(),
        output.to_str().unwrap().into(),
        "--namespace".into(),
        "release:fixture".into(),
        "--generated-at".into(),
        "2026-09-30T00:00:00Z".into(),
    ])
    .unwrap();
    let bytes = fs::read(&output).unwrap();
    let graph: GraphSnapshot = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        graph
            .edges
            .iter()
            .filter(|e| e.relation == "depends_on")
            .count(),
        1
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .find(|e| e.relation == "depends_on")
            .unwrap()
            .evidence[0]
            .locator,
        "L1,L2"
    );
    op_cozo_store::system_graph_snapshot_digest(&graph).unwrap();
    run(vec![
        "--prepared".into(),
        dir.path().to_str().unwrap().into(),
        "--output".into(),
        output.to_str().unwrap().into(),
        "--namespace".into(),
        "release:fixture".into(),
        "--generated-at".into(),
        "2026-09-30T00:00:00Z".into(),
    ])
    .unwrap();
    assert_eq!(bytes, fs::read(&output).unwrap());
}

#[test]
fn prepared_catalog_requires_actual_manifest_pinned_blob() {
    let dir = fixture();
    let path = dir.path().join("var/lib/opdbus/plugin-blobs");
    fs::create_dir_all(&path).unwrap();
    let hash = "a".repeat(64);
    fs::write(
        path.join(".manifest.json"),
        serde_json::to_vec(&json!({"plugins":{"mail":hash}})).unwrap(),
    )
    .unwrap();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    assert!(prepared(dir.path(), "release:test", "2026-09-30T00:00:00Z", &output).is_err());
    fs::write(path.join("mail.aaaaaaaaaaaaaaaa.blob"), b"sealed fixture").unwrap();
    let graph = prepared(dir.path(), "release:test", "2026-09-30T00:00:00Z", &output).unwrap();
    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.src == "plugin:mail" && e.relation == "sealed_in")
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.id == "plugin:mail" && n.attributes["live_presence_verified"] == false)
    );
}
#[test]
fn source_cli_preserves_distinct_edge_occurrences_in_one_valid_edge() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    let output = dir.path().join("output.json");
    let graph = json!({"nodes":[{"id":"meta:crate-graph","label":"meta","props":{"commit":"123456789abcdef"}},{"id":"crate:a","label":"crate","props":{}},{"id":"ext:b","label":"ext_crate","props":{"req":"^1"}}],"edges":[{"src":"crate:a","rel":"uses_ext","dst":"ext:b","props":{"kind":"normal"}},{"src":"crate:a","rel":"uses_ext","dst":"ext:b","props":{"kind":"dev"}}]});
    fs::write(&input, serde_json::to_vec(&graph).unwrap()).unwrap();
    run(vec![
        "--source-graph".into(),
        input.to_str().unwrap().into(),
        "--output".into(),
        output.to_str().unwrap().into(),
        "--namespace".into(),
        "source:workspace".into(),
        "--generated-at".into(),
        "2026-09-30T00:00:00Z".into(),
    ])
    .unwrap();
    let converted: GraphSnapshot = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert_eq!(converted.edges.len(), 1);
    assert_eq!(
        converted.edges[0].attributes["occurrences"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    op_cozo_store::system_graph_snapshot_digest(&converted).unwrap();
}

#[test]
fn exec_arguments_do_not_become_executables() {
    assert_eq!(
        literal_exec_program("exec /usr/local/bin/bridge --helper /usr/bin/sv"),
        Some("/usr/local/bin/bridge")
    );
    assert_eq!(
        literal_exec_program(
            "exec numactl --localalloc /usr/local/bin/bridge --helper /usr/bin/sv"
        ),
        Some("/usr/local/bin/bridge")
    );
    assert_eq!(
        literal_exec_program("exec env SECRET=/usr/bin/sv /usr/local/bin/bridge"),
        None
    );
}

#[test]
fn source_receipt_exposes_dirty_and_unknown_source_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("source.json");
    for value in [Some(true), None] {
        let mut props = json!({"commit":"123456789abcdef"});
        if let Some(dirty) = value {
            props["worktree_dirty"] = json!(dirty);
        }
        fs::write(&input,serde_json::to_vec(&json!({"nodes":[{"id":"meta:crate-graph","label":"meta","props":props}],"edges":[]})).unwrap()).unwrap();
        let graph = source_graph(&input, "source:workspace", "2026-09-30T00:00:00Z").unwrap();
        assert_eq!(
            graph.provenance.inputs["source_worktree_dirty"],
            if value.is_some() { "true" } else { "unknown" }
        );
        assert!(
            graph
                .provenance
                .limitations
                .iter()
                .any(|s| s.contains(if value.is_some() {
                    "uncommitted"
                } else {
                    "unknown"
                }))
        );
    }
}
