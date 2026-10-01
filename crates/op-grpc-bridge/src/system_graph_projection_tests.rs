use super::*;
use op_cozo_store::{GraphProvenanceKind, system_graph_snapshot_digest};
use op_state_store::{MethodDecl, SideEffect};

fn fixture(plugin: &str, state: Option<Value>) -> GraphPluginProjection {
    GraphPluginProjection {
        schema: PluginSchema::builder(plugin).build(),
        schema_hash: format!("schema-{plugin}"),
        state,
    }
}
fn project(inputs: &[(&str, Option<Value>)]) -> GraphSnapshot {
    project_system_graph(
        &inputs
            .iter()
            .map(|(name, state)| (name.to_string(), fixture(name, state.clone())))
            .collect(),
        provenance(),
    )
}
fn provenance() -> GraphProvenance {
    GraphProvenance {
        kind: GraphProvenanceKind::ObservedRuntime,
        source_commit: "test-commit".into(),
        release_id: None,
        generated_at: "2026-09-30T00:00:00Z".into(),
        inputs: BTreeMap::new(),
        coverage: Vec::new(),
        limitations: Vec::new(),
    }
}
fn node<'a>(snapshot: &'a GraphSnapshot, kind: &str, label: &str) -> &'a GraphNode {
    snapshot
        .nodes
        .iter()
        .find(|node| node.kind == kind && node.label == label)
        .unwrap()
}
fn has_edge(snapshot: &GraphSnapshot, source: &str, relation: &str, target: &str) -> bool {
    snapshot
        .edges
        .iter()
        .any(|edge| edge.src == source && edge.relation == relation && edge.dst == target)
}

#[test]
fn large_catalog_with_withheld_state_keeps_bounded_explicit_coverage() {
    let plugins = (0..512)
        .map(|number| {
            let name = format!("fixture_{number}");
            (name.clone(), fixture(&name, None))
        })
        .collect();
    let graph = project_system_graph(&plugins, provenance());
    assert_eq!(graph.provenance.limitations.len(), 256);
    assert!(
        graph
            .provenance
            .limitations
            .last()
            .unwrap()
            .contains("inventory is incomplete")
    );
    assert_eq!(
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == "plugin")
            .count(),
        512
    );
    system_graph_snapshot_digest(&graph).unwrap();
}

#[test]
fn schemas_supply_exact_methods_capabilities_and_taxonomy_without_implying_grants() {
    let mut alpha = fixture("alpha", None);
    alpha.schema.dependencies = vec!["beta".into(), "missing".into()];
    alpha.schema.methods.insert(
        "schema-key".into(),
        MethodDecl {
            name: "TransportName".into(),
            args: simd_json::json!({}),
            returns: None,
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("cap.test.alpha.read@v1".into()),
            subid: "obs.test.alpha.read@v1".into(),
        },
    );
    let snapshot = project_system_graph(
        &BTreeMap::from([
            ("alpha".into(), alpha),
            ("beta".into(), fixture("beta", None)),
        ]),
        provenance(),
    );
    let method = node(&snapshot, "method", "schema-key");
    assert_eq!(method.attributes["transport_name"], "TransportName");
    assert_eq!(method.attributes["availability"], "declared_only");
    assert!(has_edge(
        &snapshot,
        "plugin:alpha",
        "depends_on",
        "plugin:beta"
    ));
    assert!(!snapshot.nodes.iter().any(|node| node.label == "missing"));
    assert!(
        snapshot
            .provenance
            .limitations
            .iter()
            .any(|entry| entry.contains("missing unresolved"))
    );
    assert_eq!(
        node(&snapshot, "capability", "cap.test.alpha.read@v1").attributes["grants"],
        "not_projected"
    );
    assert!(snapshot.edges.iter().all(|edge| {
        edge.evidence
            .iter()
            .all(|evidence| evidence.assertion == GraphAssertion::Declared)
    }));
    system_graph_snapshot_digest(&snapshot).unwrap();
}

#[test]
fn incus_typed_effective_devices_and_mail_container_references_are_projected() {
    let snapshot = project(&[
        (
            "incus",
            Some(
                json!({"instances":[{"name":"mail-3tched","status":"Running","type":"container",
            "config":{"environment.AUTH_TOKEN":"private-token"},
            "devices":[{"name":"stale","device":{"type":"disk","source":"/stale","path":"/stale"}}],
            "expanded_devices":[{"name":"persist","device":{"type":"disk","source":"/storage/mail","path":"/var/mail","readonly":false,"pool":"main"}},
                {"name":"socket","device":{"type":"proxy","listen":"unix:/run/mail.sock","connect":"tcp:127.0.0.1:587"}}]}]}),
            ),
        ),
        (
            "mail_server",
            Some(
                json!({"container_name":"mail-3tched","container_status":"Running","domain":"example.test","healthy":true,
            "endpoints":{"imaps":"0.0.0.0:993"},"administration":{"domains":["other.test"],"mailboxes":[{"address":"alice@example.test","maildir":"/private/alice"}],"aliases":[{"address":"bob@example.test"}]}}),
            ),
        ),
    ]);
    let container = node(&snapshot, "container", "mail-3tched");
    assert!(has_edge(
        &snapshot,
        "plugin:mail_server",
        "runs_in",
        &container.id
    ));
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.kind == "filesystem_path" && node.label == "/storage/mail")
    );
    assert!(!snapshot.nodes.iter().any(|node| node.label == "stale"));
    assert_eq!(
        node(&snapshot, "container_device", "persist").attributes["mount_path"],
        "/var/mail"
    );
    assert_eq!(
        node(&snapshot, "mail_domain", "other.test").kind,
        "mail_domain"
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    for secret in [
        "private-token",
        "alice@example",
        "bob@example",
        "/private/alice",
        "environment.AUTH_TOKEN",
    ] {
        assert!(!encoded.contains(secret), "{secret}");
    }
    system_graph_snapshot_digest(&snapshot).unwrap();
}

#[test]
fn current_incus_public_policy_yields_aggregate_counts_without_private_names() {
    use op_plugins::state_plugins::incus::{IncusInstance, IncusPlugin, IncusState};
    use op_state::StatePlugin;
    let root = tempfile::tempdir().unwrap();
    let catalog = root.path().join("catalog");
    let state = root.path().join("state");
    let schema = IncusPlugin::new().schema().unwrap();
    let blob = op_blob::blobify_plugin_schema("incus", schema);
    op_blob::BlobStore::open(&catalog)
        .unwrap()
        .write(&blob)
        .unwrap();
    let instances = [
        ("Running", "container"),
        ("Stopped", "container"),
        ("Frozen", "virtual-machine"),
    ]
    .into_iter()
    .enumerate()
    .map(|(number, (status, kind))| IncusInstance {
        name: format!("private-identity-sentinel-{number}"),
        status: status.into(),
        instance_type: kind.into(),
        config: Some(std::collections::HashMap::from([(
            "environment.SECRET".into(),
            "private-config-sentinel".into(),
        )])),
        ..IncusInstance::default()
    })
    .collect();
    let bytes = serde_json::to_vec(&IncusState { instances }).unwrap();
    op_core::projection_shm::update_projection_in(state.to_str().unwrap(), "incus", |_| {
        Ok(Some(bytes))
    })
    .unwrap();
    let graph =
        crate::system_graph_dispatch::committed_system_graph(&catalog, state.to_str().unwrap())
            .unwrap();
    let summary = &node(&graph, "plugin", "incus").attributes["instance_summary"];
    assert_eq!(summary["total"], 3);
    assert_eq!(
        summary["status_counts"],
        json!({"Frozen":1,"Running":1,"Stopped":1})
    );
    assert_eq!(
        summary["type_counts"],
        json!({"container":2,"virtual-machine":1})
    );
    assert_eq!(summary["named_inventory"], "withheld_by_public_policy");
    assert!(!graph.nodes.iter().any(|node| node.kind == "container"));
    assert!(
        graph
            .provenance
            .limitations
            .iter()
            .any(|entry| entry.contains("withheld by sealed public policy; aggregate counts only"))
    );
    assert!(
        !graph
            .provenance
            .limitations
            .iter()
            .any(|entry| entry.contains("missing or unsafe name"))
    );
    let encoded = serde_json::to_string(&graph).unwrap();
    assert!(
        !encoded.contains("private-identity-sentinel")
            && !encoded.contains("private-config-sentinel")
    );
    assert_eq!(graph.provenance.inputs["shm_generation"], "1");
    system_graph_snapshot_digest(&graph).unwrap();
}

#[test]
fn qdrant_reports_actual_vector_configuration_and_index_health_without_payloads() {
    let snapshot = project(&[(
        "qdrant",
        Some(
            json!({"http_endpoint":"http://qdrant:6333","grpc_endpoint":"http://qdrant:6334", "collections":[{
        "name":"repos_lsp_project_voyage_4_large","status":"green","optimizer_status":"ok","points_count":37,"indexed_vectors_count":31,"segments_count":2,
        "config":{"params":{"vectors":{"size":1024,"distance":"Cosine","on_disk":true}},"hnsw_config":{"m":16,"ef_construct":100},"metadata":{"private":"secret-metadata"}},
        "payload_schema":{"private_email":"secret-schema"},"points":[{"payload":"secret-point"}]}]}),
        ),
    )]);
    let collection = node(
        &snapshot,
        "qdrant_collection",
        "repos_lsp_project_voyage_4_large",
    );
    assert_eq!(collection.attributes["size"], 1024);
    assert_eq!(collection.attributes["distance"], "Cosine");
    assert_eq!(collection.attributes["indexed_vectors_count"], 31);
    assert_eq!(collection.attributes["hnsw"]["m"], 16);
    let encoded = serde_json::to_string(&snapshot).unwrap();
    for secret in [
        "secret-metadata",
        "secret-schema",
        "secret-point",
        "repomix_rag",
    ] {
        assert!(!encoded.contains(secret));
    }
}

#[test]
fn btrfs_keeps_filesystem_scoped_hierarchy_and_does_not_invent_missing_parents() {
    let snapshot = project(&[(
        "btrfs",
        Some(
            json!({"filesystems":{"fs-a":{"path":"/storage","subvolumes":[
        {"id":256,"name":"cache","parent_id":5,"generation":10,"uuid":"sub-a","readonly":false},
        {"id":257,"name":"cache/vector","parent_id":256,"generation":11,"uuid":"sub-b","readonly":true},
        {"id":258,"name":"orphan","parent_id":900,"uuid":"sub-c"}]},
        "fs-b":{"path":"/other","subvolumes":[{"id":256,"name":"memory","parent_id":5,"uuid":"sub-d","readonly":false}]}}}),
        ),
    )]);
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .filter(|node| node.kind == "btrfs_subvolume")
            .count(),
        4
    );
    assert!(has_edge(
        &snapshot,
        "btrfs_subvolume:fs-a:256",
        "contains_subvolume",
        "btrfs_subvolume:fs-a:257"
    ));
    assert!(!snapshot.edges.iter().any(|edge| edge.src.contains("900")));
    assert!(
        snapshot
            .provenance
            .limitations
            .iter()
            .any(|entry| entry.contains("parent 900 unavailable"))
    );
    assert_eq!(
        node(&snapshot, "btrfs_subvolume", "fs-a/257").attributes["readonly"],
        true
    );
    system_graph_snapshot_digest(&snapshot).unwrap();
}

#[test]
fn private_memory_and_reserved_episode_qdrant_defaults_do_not_become_graph_facts() {
    let snapshot = project(&[
        (
            "memory",
            Some(
                json!({"backend":"cozo","isolation":"principal_session","namespaces":[{"name":"private-namespace","entries":"private-content"}]}),
            ),
        ),
        (
            "ctl_plane_chatbot",
            Some(
                json!({"qdrant_collection":"legacy-private-episodes","embedding_plugin":"unobserved-provider",
            "episodes":{"private-episode":{"goal_text":"secret-goal","reasoning_summary":"secret-reasoning"}},"knowledge":{"private-push":{"content":"secret-knowledge"}}}),
            ),
        ),
        ("qdrant", Some(json!({"collections":[]}))),
        (
            "cognitive_mcp",
            Some(
                json!({"bridge_owned":true,"execution_model":"in_process","memory_tool":{"value":"secret-memory"},"code_search":{"query":"secret-query"}}),
            ),
        ),
    ]);
    assert_eq!(
        node(&snapshot, "plugin", "memory").attributes["backend"],
        "cozo"
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    for secret in [
        "private-namespace",
        "private-content",
        "legacy-private-episodes",
        "unobserved-provider",
        "private-episode",
        "secret-goal",
        "secret-reasoning",
        "secret-knowledge",
        "secret-memory",
        "secret-query",
    ] {
        assert!(!encoded.contains(secret), "{secret}");
    }
    assert!(
        !snapshot
            .edges
            .iter()
            .any(|edge| edge.src == "plugin:ctl_plane_chatbot"
                && edge.dst.starts_with("qdrant_collection"))
    );
    let private_cozo = node(&snapshot, "subsystem", "private episode Cozo persistence");
    assert_eq!(private_cozo.attributes["runtime_status"], "unknown");
    assert!(
        snapshot
            .edges
            .iter()
            .filter(|edge| edge.dst == private_cozo.id)
            .all(|edge| edge
                .evidence
                .iter()
                .all(|evidence| evidence.assertion == GraphAssertion::Declared))
    );
}

#[test]
fn embedding_configuration_and_ui_catalog_keep_semantics_but_no_secret_config() {
    let snapshot = project(&[
        (
            "embedding_model",
            Some(
                json!({"provider":"voyage","model_id":"voyage-4-lite","dimensions":1024,"status":"unprobed","endpoint":"https://api.voyageai.com/v1/embeddings","api_key":"secret-key"}),
            ),
        ),
        (
            "json_render",
            Some(
                json!({"packages":[{"package_name":"@json-render/react","version":"0.12.0"}],
            "components":[{"name":"tchedRouterPicker","category":"chat","props_schema":{"example":"secret-prop"}}],"actions":[{"name":"applyModel","mutates_state":true}],"config":{"auth":"secret-config"}}),
            ),
        ),
    ]);
    assert_eq!(
        node(&snapshot, "plugin", "embedding_model").attributes["status"],
        "unprobed"
    );
    assert_eq!(
        node(&snapshot, "ui_component", "tchedRouterPicker").attributes["category"],
        "chat"
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.label == "shared model picker")
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    for secret in ["secret-key", "secret-prop", "secret-config"] {
        assert!(!encoded.contains(secret));
    }
    system_graph_snapshot_digest(&snapshot).unwrap();
}

#[test]
fn missing_and_conflicting_inventories_are_unknown_and_do_not_create_edges() {
    let snapshot = project(&[
        (
            "incus",
            Some(
                json!({"instances":[{"name":"duplicate","devices":[]},{"name":"duplicate","devices":[]}]}),
            ),
        ),
        (
            "mail_server",
            Some(
                json!({"container_name":"duplicate","endpoints":{"imap":"https://user:secret-password@example.test/?token=secret-token"}}),
            ),
        ),
        (
            "btrfs",
            Some(json!({"maximum_subvolumes":5,"deployment_transport":"btrfs_send_receive"})),
        ),
        ("qdrant", None),
    ]);
    assert!(!snapshot.nodes.iter().any(|node| node.kind == "container"
        || node.kind == "btrfs_subvolume"
        || node.kind == "qdrant_collection"));
    assert!(!snapshot.edges.iter().any(|edge| edge.relation == "runs_in"));
    assert_eq!(
        node(&snapshot, "plugin", "qdrant").attributes["state_coverage"],
        "unavailable"
    );
    assert!(
        snapshot
            .provenance
            .limitations
            .iter()
            .any(|entry| entry.contains("filesystem/subvolume inventory unavailable"))
    );
    let encoded = serde_json::to_string(&snapshot).unwrap();
    assert!(!encoded.contains("secret-password"));
    assert!(!encoded.contains("secret-token"));
    assert!(!encoded.contains("maximum_subvolumes"));
}

#[test]
fn projection_is_deterministic_across_inventory_order_and_has_complete_evidence() {
    let first = json!({"instances":[{"name":"b","devices":[]},{"name":"a","devices":[]}]});
    let second = json!({"instances":[{"name":"a","devices":[]},{"name":"b","devices":[]}]});
    let left = project(&[("incus", Some(first))]);
    let right = project(&[("incus", Some(second))]);
    assert_eq!(left, right);
    assert_eq!(
        system_graph_snapshot_digest(&left).unwrap(),
        system_graph_snapshot_digest(&right).unwrap()
    );
    assert!(left.edges.iter().all(|edge| !edge.evidence.is_empty()));
}

#[test]
fn bootstrap_mail_defaults_and_cyclic_subvolumes_are_not_runtime_facts() {
    let snapshot = project(&[
        (
            "mail_server",
            Some(
                json!({"container_name":"mail-default","container_status":"Unknown","healthy":false,"domain":"default.test","endpoints":{"imap":"0.0.0.0:143"}}),
            ),
        ),
        (
            "btrfs",
            Some(
                json!({"filesystems":{"fs":{"path":"/storage","subvolumes":[{"id":256,"parent_id":257},{"id":257,"parent_id":256}]}}}),
            ),
        ),
    ]);
    assert!(
        !snapshot
            .edges
            .iter()
            .any(|edge| edge.src.starts_with("btrfs_subvolume:")
                && edge.relation == "contains_subvolume")
    );
    assert!(
        snapshot
            .provenance
            .limitations
            .iter()
            .any(|entry| entry.contains("cyclic parent hierarchy"))
    );
    assert!(
        snapshot
            .edges
            .iter()
            .filter(|edge| edge.src == "plugin:mail_server")
            .all(|edge| edge
                .evidence
                .iter()
                .all(|evidence| evidence.assertion == GraphAssertion::Declared))
    );
    assert!(
        !node(&snapshot, "plugin", "mail_server")
            .attributes
            .contains_key("healthy")
    );
}
