use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "bin",
        "sv/bridge",
        "usr/share/op-dbus/etc/opdbus",
        "usr/share/op-dbus/etc/op-dbus",
        "usr/share/op-dbus/assets",
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
    fs::set_permissions(dir.path().join("bin/bridge"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        dir.path().join("sv/bridge/run"),
        "#!/bin/sh\n# op-dbus-release: application\nwait_dep() {\n sv start \"$1\" >/dev/null\n until sv check \"$1\"; do sleep 1; done\n}\nwait_dep dbus\nuntil sv check observed; do sleep 1; done\nsv -w 5 up helper\nexport PASSWORD='private-unretained'\nCONFIG_ROOT=/var/lib/bridge\nBOOTSTRAP=/usr/share/op-dbus/etc/op-dbus/bootstrap.toml\ninstall -m 0644 \"$BOOTSTRAP\" \"$CONFIG_ROOT/config.toml\"\nexec /usr/local/libexec/op-dbus/bin/bridge --config /usr/share/op-dbus/etc/op-dbus/main.conf\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("usr/share/op-dbus/etc/op-dbus/main.conf"),
        "password = private-unretained\nkey_file = /etc/op-dbus/secrets/mail-key\ninclude = current.conf\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("usr/share/op-dbus/etc/op-dbus/nested.conf"),
        "assets = /usr/share/op-dbus/assets\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("usr/share/op-dbus/etc/op-dbus/bootstrap.toml"),
        "directory = \"shared/data\"\n",
    )
    .unwrap();
    symlink(
        "nested.conf",
        dir.path().join("usr/share/op-dbus/etc/op-dbus/current.conf"),
    )
    .unwrap();
    fs::write(dir.path().join("usr/share/op-dbus/assets/data"), b"asset").unwrap();
    fs::write(
        dir.path().join("usr/share/op-dbus/etc/opdbus/enabled-services"),
        "bridge\ndbus\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("usr/share/op-dbus/etc/opdbus/retired-services"),
        "old\n",
    )
    .unwrap();
    dir
}
#[test]
fn dotted_retired_services_keep_declarations_and_reject_traversal_names() {
    let dir = fixture();
    let manifest = dir.path().join("usr/share/op-dbus/etc/opdbus/retired-services");
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
        assert_eq!(declaration.dst, "file:usr/share/op-dbus/etc/opdbus/retired-services");
        assert_eq!(
            declaration.evidence[0].source,
            "usr/share/op-dbus/etc/opdbus/retired-services"
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
    let provenance = "usr/share/op-dbus/etc/op-dbus/source-record.json";
    fs::write(dir.path().join(provenance), serde_json::to_vec(&json!({
        "repository": "https://example.org/skills",
        "commit": "a".repeat(40),
        "source_path": "upstream/skill",
        "files": [{"path": "main.conf", "source_path": "upstream/skill/main.conf", "sha256": "b".repeat(64)}]
    })).unwrap()).unwrap();
    let snapshot = prepared(
        dir.path(),
        "release:fixture",
        "2026-09-30T00:00:00Z",
        &output,
    )
    .unwrap();
    let encoded = serde_json::to_vec(&snapshot).unwrap();
    let record = snapshot.nodes.iter().find(|node| node.id == format!("file:{provenance}")).unwrap();
    assert_eq!(record.attributes["category"], "provenance");
    assert!(!snapshot.nodes.iter().any(|node| node.id.contains("/upstream/skill")));
    assert!(!String::from_utf8_lossy(&encoded).contains("private-unretained"));
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|n| n.id == "file:usr/share/op-dbus/etc/op-dbus/main.conf"
                && n.attributes["sha256"].as_str().unwrap().len() == 64)
    );
    let dep = snapshot
        .edges
        .iter()
        .find(|e| e.relation == "depends_on" && e.dst == "service:dbus")
        .unwrap();
    assert_eq!(dep.dst, "service:dbus");
    assert_eq!(dep.evidence[0].locator, "L7");
    assert!(
        snapshot
            .edges
            .iter()
            .any(|e| e.relation == "runs" && e.dst == "file:bin/bridge")
    );
    let service = snapshot.nodes.iter().find(|n| n.id == "service:bridge").unwrap();
    assert_eq!(service.attributes["release_owned"], true);
    assert_eq!(service.attributes["deps"], json!(["dbus", "helper", "observed"]));
    assert_eq!(service.attributes["starts"], json!(["dbus", "helper"]));
    let base = snapshot.nodes.iter().find(|n| n.id == "service:dbus").unwrap();
    assert_eq!(base.attributes["release_owned"], false);
    let asset = snapshot.nodes.iter().find(|n| n.id == "file:usr/share/op-dbus/assets/data").unwrap();
    assert_eq!(asset.attributes["consumers"], json!(["bridge"]));
    assert_eq!(asset.attributes["release_target"], "/usr/share/op-dbus");
    let metadata = fs::symlink_metadata(dir.path().join("usr/share/op-dbus/assets/data")).unwrap();
    assert_eq!(asset.attributes["uid"], metadata.uid());
    assert_eq!(asset.attributes["gid"], metadata.gid());
    let link = snapshot.nodes.iter().find(|n| n.id == "file:usr/share/op-dbus/etc/op-dbus/current.conf").unwrap();
    assert_eq!(link.attributes["target"], "nested.conf");
    assert_eq!(link.attributes["consumers"], json!(["bridge"]));
    assert!(snapshot.nodes.iter().any(|node| node.id == "reference:/var/lib/bridge/shared/data"));
    let target = snapshot.nodes.iter().find(|n| n.id == "target:/usr/local/libexec/op-dbus/bin").unwrap();
    assert_eq!(target.attributes["source"], "bin");
    assert_eq!(target.attributes["noexec"], false);
    let data_target = snapshot.nodes.iter().find(|n| n.id == "target:/usr/share/op-dbus").unwrap();
    assert_eq!(data_target.attributes["noexec"], true);
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
    // A provenance-looking file with a runtime include is not exempt.
    let mut mixed: Value = serde_json::from_slice(&fs::read(dir.path().join(provenance)).unwrap()).unwrap();
    mixed["include"] = json!("/usr/share/op-dbus/etc/op-dbus/missing.conf");
    fs::write(dir.path().join(provenance), serde_json::to_vec_pretty(&mixed).unwrap()).unwrap();
    assert!(prepared(dir.path(), "release:test", "2026-10-18T00:00:00Z", &output).is_err());
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
    assert_eq!(
        graph.nodes.iter().find(|n| n.id == "file:outside").unwrap().attributes["target"],
        outside.path().to_str().unwrap()
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
fn dynamic_restart_references_fail_without_retaining_secret_values() {
    let dir = fixture();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    for command in [
        "exec \"$UNKNOWN_EXECUTABLE\"",
        "VALUE=$(printf data) \"$UNKNOWN_EXECUTABLE\"",
        "sv start \"$UNKNOWN_SERVICE\"",
        "exec /usr/local/libexec/op-dbus/bin/bridge --config \"$UNKNOWN_CONFIG\"",
        ". \"$UNKNOWN_INCLUDE\"",
        "exec /usr/local/libexec/op-dbus/bin/bridge --config \"$(unknown_config)\"",
        "exec /usr/local/libexec/op-dbus/bin/bridge --config=\"$UNKNOWN_CONFIG\"",
        "exec /usr/local/libexec/op-dbus/bin/bridge --config \"`unknown_config`\"",
        "KNOWN=/usr/local/libexec/op-dbus/bin/bridge\nKNOWN=$(unknown_program)\nexec \"$KNOWN\"",
        "KNOWN=/usr/local/libexec/op-dbus/bin/bridge true\nexec \"$KNOWN\"",
        "exec /usr/local/libexec/op-dbus/bin/bridge --config \"$(dirname /etc/first; dirname /etc/second)\"",
        "exec /usr/local/libexec/op-dbus/bin/missing",
    ] {
        fs::write(
            dir.path().join("sv/bridge/run"),
            format!("#!/bin/sh\n# op-dbus-release: application\n{command}\n"),
        )
        .unwrap();
        assert!(prepared(dir.path(), "release:test", "2026-09-30T00:00:00Z", &output).is_err());
    }
}

#[test]
fn cli_emits_valid_typed_snapshot_with_merged_repeated_evidence() {
    let dir = fixture();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    fs::write(
        dir.path().join("sv/bridge/run"),
        "# op-dbus-release: application\nwait_dep dbus\nwait_dep dbus\nexec /usr/local/libexec/op-dbus/bin/bridge\nexec /usr/local/libexec/op-dbus/bin/bridge\n",
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
        "L2,L3"
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
    let path = dir.path().join("usr/share/op-dbus/plugin-blobs");
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
    let dir = fixture();
    fs::write(dir.path().join("bin/wrapper"), r#"#!/bin/sh
SCRIPT_PATH=$(readlink -f -- "$0")
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd -P)
manifest=/usr/share/op-dbus/etc/op-dbus/main.conf
value=$(wc -c <"$manifest")
: "${BUS_ADDRESS:=unix:path=/run/opdbus/session-bus.sock}"
SOCKET_PATH="${BUS_ADDRESS#unix:path=}"
install -d "$(dirname "$SOCKET_PATH")"
printf 'status: %s\n' \
    "$UNKNOWN_STATUS"
exec "$SCRIPT_DIR/bridge" --config "$manifest"
"#).unwrap();
    fs::write(dir.path().join("sv/bridge/run"),
        "#!/bin/sh\n# op-dbus-release: application\nexec /usr/local/libexec/op-dbus/bin/wrapper\n").unwrap();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    let graph = prepared(dir.path(), "release:test", "2026-10-18T00:00:00Z", &output).unwrap();
    for path in ["file:bin/bridge", "file:usr/share/op-dbus/etc/op-dbus/main.conf", "reference:/run/opdbus/session-bus.sock"] {
        assert!(graph.edges.iter().any(|edge| edge.src == "file:bin/wrapper" && edge.dst == path), "{path}");
    }
    assert!(!graph.nodes.iter().any(|node| node.id.ends_with(')')));
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
    }
}

#[test]
fn staged_secret_helper_resolves_function_paths_without_executing_it() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("libexec/3tched")).unwrap();
    fs::write(
        dir.path().join("libexec/3tched/emqx-prepare"),
        include_str!("../../../../../deploy/runit/libexec-3tched/emqx-prepare"),
    ).unwrap();
    fs::create_dir_all(dir.path().join("usr/share/op-dbus/emqx-etc")).unwrap();
    fs::write(dir.path().join("sv/bridge/run"),
        "#!/bin/sh\n# op-dbus-release: application\n/usr/local/libexec/op-dbus/helpers/emqx-prepare\nexec /usr/local/libexec/op-dbus/bin/bridge\n").unwrap();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    let graph = prepared(dir.path(), "release:test", "2026-10-17T00:00:00Z", &output).unwrap();
    for name in ["emqx-dashboard-password", "emqx-mqtt-password", "emqx-api-key", "emqx-api-secret", "emqx-node-cookie"] {
        assert!(graph.edges.iter().any(|edge|
            edge.src == "file:libexec/3tched/emqx-prepare"
                && edge.dst == format!("reference:/etc/opdbus/secrets/{name}")));
    }
    let helper = graph.nodes.iter().find(|node| node.id == "file:libexec/3tched/emqx-prepare").unwrap();
    assert_eq!(helper.attributes["consumers"], json!(["bridge"]));
}

#[test]
fn helper_bindings_converge_before_completeness_diagnostics() {
    let dir = fixture();
    fs::create_dir_all(dir.path().join("libexec/3tched")).unwrap();
    fs::write(dir.path().join("sv/bridge/run"),
        "#!/bin/sh\n# op-dbus-release: application\nexec /usr/local/libexec/op-dbus/helpers/z-helper\n").unwrap();
    fs::write(dir.path().join("libexec/3tched/z-helper"),
        "#!/bin/sh\nexec /usr/local/libexec/op-dbus/helpers/a-helper /usr/share/op-dbus/etc/op-dbus/main.conf /var/lib/late\n").unwrap();
    fs::write(dir.path().join("libexec/3tched/a-helper"),
        "#!/bin/sh\ninstall -m 0644 /usr/share/op-dbus/etc/op-dbus/bootstrap.toml \"$2/config.toml\"\nexec /usr/local/libexec/op-dbus/bin/bridge --config \"$1\"\n").unwrap();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    let graph = prepared(dir.path(), "release:test", "2026-10-23T00:00:00Z", &output).unwrap();
    let config = graph.nodes.iter().find(|node|
        node.id == "file:usr/share/op-dbus/etc/op-dbus/main.conf").unwrap();
    assert_eq!(config.attributes["consumers"], json!(["bridge"]));
    assert!(graph.edges.iter().any(|edge| edge.src == "file:libexec/3tched/a-helper"
        && edge.dst == config.id));
    assert!(graph.nodes.iter().any(|node| node.id == "reference:/var/lib/late/shared/data"));
    assert!(!graph.nodes.iter().any(|node|
        node.id == "reference:/usr/share/op-dbus/etc/op-dbus/shared/data"));
    // A stable graph still rejects an argument that no captured caller binds.
    fs::write(dir.path().join("libexec/3tched/z-helper"),
        "#!/bin/sh\nexec /usr/local/libexec/op-dbus/helpers/a-helper\n").unwrap();
    let error = prepared(dir.path(), "release:test", "2026-10-23T00:00:00Z", &output).unwrap_err();
    assert!(error.to_string().contains("unresolved dynamic executable/config path"));
}

#[test]
fn expanding_helper_bindings_fail_closed_at_pass_bound() {
    let dir = fixture();
    fs::write(dir.path().join("bin/helper"), b"captured below").unwrap();
    let mut base = Builder::new();
    base.walk(dir.path(), Path::new(""), 0).unwrap();
    base.release_targets().unwrap();
    let mut inputs = ScanInputs::default();
    inputs.push("sv/bridge/run",
        "exec /usr/local/libexec/op-dbus/bin/helper /var/lib/root".into(), true).unwrap();
    inputs.push("bin/helper",
        "exec /usr/local/libexec/op-dbus/bin/helper \"$1/nested\"".into(), true).unwrap();
    let error = scan_to_fixed_point(base, &inputs, 3).err().expect("must not publish a partial graph");
    assert!(error.to_string().contains("did not converge within 3 passes"));
}

#[test]
fn function_paths_are_scoped_and_unknown_calls_still_block() {
    let dir = fixture();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    fs::write(dir.path().join("sv/bridge/run"), r#"#!/bin/sh
# op-dbus-release: application
CONFIG=/usr/share/op-dbus/etc/op-dbus/main.conf
launch() {
    config_path=$1
    exec /usr/local/libexec/op-dbus/bin/bridge --config "$config_path"
}
report() {
    printf '%s %s\n' "$1" "$2"
}
launch "$CONFIG"
report "providers.models.$MODEL.model" "${TIMEOUT:-90}"
"#).unwrap();
    let graph = prepared(dir.path(), "release:test", "2026-10-18T00:00:00Z", &output).unwrap();
    assert!(graph.edges.iter().any(|edge| edge.src == "service:bridge"
        && edge.dst == "file:usr/share/op-dbus/etc/op-dbus/main.conf"));
    for script in [
        "launch() {\n config_path=$1\n exec /usr/local/libexec/op-dbus/bin/bridge --config \"$config_path\"\n}\nlaunch /usr/share/op-dbus/etc/op-dbus/main.conf\nlaunch \"$UNKNOWN_CONFIG\"\n",
        "launch() {\n config_path=$1\n}\nlaunch /usr/share/op-dbus/etc/op-dbus/main.conf\nexec \"$config_path\"\n",
        "launch() {\n config_path=$1\n}\nlaunch /usr/share/op-dbus/etc/op-dbus/missing.conf\n",
        "launch() {\n config_path=$1\n}\nlaunch /usr/share/op-dbus/etc/op-dbus/main.conf\nlaunch\n",
        "launch() {\n config_path=$1\n}\nlaunch /usr/share/op-dbus/etc/op-dbus/main.conf\nlaunch relative-unresolved.conf\n",
        "launch() {\n exec \"$1\"\n}\nlaunch \"$UNKNOWN_PROGRAM\"\n",
        "launch() {\n . \"$1\"\n}\nlaunch \"$UNKNOWN_INCLUDE\"\n",
    ] {
        fs::write(dir.path().join("sv/bridge/run"),
            format!("#!/bin/sh\n# op-dbus-release: application\n{script}")).unwrap();
        assert!(prepared(dir.path(), "release:test", "2026-10-17T00:00:00Z", &output).is_err());
    }
}

#[test]
fn catalog_only_transition_keeps_staging_owner_and_cached_consumers() {
    let dir = fixture();
    let catalog = dir.path().join(CATALOG_DIR);
    fs::create_dir_all(&catalog).unwrap();
    fs::create_dir_all(dir.path().join("sv/rundirs")).unwrap();
    fs::create_dir_all(dir.path().join("libexec/3tched")).unwrap();
    fs::write(dir.path().join("bin/opblob"), b"opblob fixture artifact").unwrap();
    fs::write(dir.path().join("libexec/3tched/opdbus-rundirs-up"),
        include_str!("../../../../../deploy/runit/libexec-3tched/opdbus-rundirs-up")).unwrap();
    fs::write(dir.path().join("sv/rundirs/run"),
        include_str!("../../../../../deploy/runit/opdbus-rundirs/run")).unwrap();
    fs::write(dir.path().join("sv/bridge/run"),
        "#!/bin/sh\n# op-dbus-release: application\n# op-dbus-consumes: /usr/share/op-dbus/plugin-blobs\nsv start rundirs\nexec /usr/local/libexec/op-dbus/bin/bridge\n").unwrap();
    let output = dir.path().join(format!("{GRAPH_DIR}/release.json"));
    let mut previous_hash = None;
    for generation in ["a", "b"] {
        fs::write(catalog.join(".manifest.json"),
            serde_json::to_vec(&json!({"plugins":{"mail":generation.repeat(64)}})).unwrap()).unwrap();
        let blob = format!("mail.{}.blob", generation.repeat(16));
        fs::write(catalog.join(&blob), format!("sealed catalog generation {generation}")).unwrap();
        let graph = prepared(dir.path(), "release:test", "2026-10-17T00:00:00Z", &output).unwrap();
        for path in [format!("{CATALOG_DIR}/.manifest.json"), format!("{CATALOG_DIR}/{blob}")] {
            let node = graph.nodes.iter().find(|node| node.id == format!("file:{path}")).unwrap();
            assert_eq!(node.attributes["consumers"], json!(["bridge", "rundirs"]));
        }
        let bridge = graph.nodes.iter().find(|node| node.id == "service:bridge").unwrap();
        assert_eq!(bridge.attributes["deps"], json!(["rundirs"]));
        assert_eq!(bridge.attributes["starts"], json!(["rundirs"]));
        let manifest = graph.nodes.iter().find(|node| node.id == format!("file:{CATALOG_DIR}/.manifest.json")).unwrap();
        if let Some(before) = previous_hash.replace(manifest.attributes["sha256"].clone()) {
            assert_ne!(before, manifest.attributes["sha256"]);
        }
        fs::remove_file(catalog.join(blob)).unwrap();
    }
    fs::remove_file(catalog.join(".manifest.json")).unwrap();
    fs::remove_dir(catalog).unwrap();
    assert!(prepared(dir.path(), "release:test", "2026-10-17T00:00:00Z", &output).is_err());
}
