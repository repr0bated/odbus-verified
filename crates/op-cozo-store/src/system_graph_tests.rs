use super::*;

fn node(id: &str) -> GraphNode {
    GraphNode {
        id: id.into(),
        kind: "service".into(),
        label: id.into(),
        attributes: BTreeMap::new(),
    }
}
fn edge(src: &str, dst: &str) -> GraphEdge {
    GraphEdge {
        src: src.into(),
        relation: "depends_on".into(),
        dst: dst.into(),
        evidence: vec![GraphEvidence {
            source: "fixture".into(),
            locator: "manifest:1".into(),
            assertion: GraphAssertion::Declared,
        }],
        attributes: BTreeMap::new(),
    }
}
fn snapshot(namespace: &str) -> GraphSnapshot {
    GraphSnapshot {
        format_version: 1,
        namespace: namespace.into(),
        provenance: GraphProvenance {
            kind: GraphProvenanceKind::Source,
            source_commit: "test-commit".into(),
            release_id: None,
            generated_at: "2026-09-30T00:00:00Z".into(),
            inputs: BTreeMap::from([("fixture".into(), "sha256:test".into())]),
            coverage: vec!["services".into()],
            limitations: vec!["declared only".into()],
        },
        nodes: vec![node("a"), node("b"), node("c")],
        edges: vec![edge("a", "b"), edge("b", "c")],
    }
}
fn nodes(namespace: &str) -> GraphNodeQuery {
    GraphNodeQuery {
        namespace: namespace.into(),
        revision: None,
        after_id: None,
        kind: None,
        limit: 100,
    }
}
fn edges(namespace: &str) -> GraphEdgeQuery {
    GraphEdgeQuery {
        namespace: namespace.into(),
        revision: None,
        after: None,
        node_id: None,
        direction: GraphDirection::Both,
        relation: None,
        limit: 100,
    }
}
fn traversal(namespace: &str, start: &str, direction: GraphDirection) -> GraphTraversalQuery {
    GraphTraversalQuery {
        namespace: namespace.into(),
        revision: None,
        start: start.into(),
        direction,
        relations: vec!["depends_on".into()],
        max_depth: 32,
        max_nodes: 100,
        max_edges: 100,
    }
}

#[test]
fn canonical_digest_is_order_independent_and_tracks_evidence() {
    let original = snapshot("source");
    let mut shuffled = original.clone();
    shuffled.nodes.reverse();
    shuffled.edges.reverse();
    assert_eq!(
        system_graph_snapshot_digest(&original).unwrap(),
        system_graph_snapshot_digest(&shuffled).unwrap()
    );
    shuffled.edges[0].evidence[0].assertion = GraphAssertion::Observed;
    assert_ne!(
        system_graph_snapshot_digest(&original).unwrap(),
        system_graph_snapshot_digest(&shuffled).unwrap()
    );
}
#[test]
fn replacement_removes_stale_rows_preserves_namespaces_and_legacy_graph() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    store
        .store_node("a", "private", serde_json::json!({"owner":"identity"}))
        .unwrap();
    store
        .store_edge("a", "private_relation", "b", None)
        .unwrap();
    let first = store.replace_system_graph(&snapshot("source"), 0).unwrap();
    store.replace_system_graph(&snapshot("release"), 0).unwrap();
    let mut next = snapshot("source");
    next.nodes.truncate(1);
    next.edges.clear();
    let second = store.replace_system_graph(&next, first.revision).unwrap();
    assert_eq!(second.revision, 2);
    assert_eq!(second.node_count, 1);
    assert_eq!(second.edge_count, 0);
    assert_eq!(
        store
            .query_system_graph_nodes(&nodes("source"))
            .unwrap()
            .nodes
            .len(),
        1
    );
    assert!(
        store
            .query_system_graph_edges(&edges("source"))
            .unwrap()
            .edges
            .is_empty()
    );
    assert_eq!(
        store
            .query_system_graph_nodes(&nodes("release"))
            .unwrap()
            .nodes
            .len(),
        3
    );
    assert!(
        !store
            .query_node("a")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !store
            .query_edges_from("a")
            .unwrap()
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn cas_conflict_and_provenance_change_leave_previous_snapshot_intact() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let first = store.replace_system_graph(&snapshot("source"), 0).unwrap();
    let mut next = snapshot("source");
    next.nodes.push(node("d"));
    assert!(
        store
            .replace_system_graph(&next, 0)
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
    next.provenance.kind = GraphProvenanceKind::ObservedRuntime;
    assert!(
        store
            .replace_system_graph(&next, 1)
            .unwrap_err()
            .to_string()
            .contains("provenance")
    );
    assert_eq!(store.system_graph_status("source").unwrap().unwrap(), first);
}
#[test]
fn failed_native_write_rolls_back_deletions_and_partial_inserts() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let first = store
        .replace_system_graph(&snapshot("test:rollback-before-commit"), 0)
        .unwrap();
    let mut next = snapshot("test:rollback-before-commit");
    next.nodes.truncate(1);
    next.edges.clear();
    assert!(store.replace_system_graph(&next, 1).is_err());
    assert_eq!(
        store.system_graph_status(&next.namespace).unwrap().unwrap(),
        first
    );
    assert_eq!(
        store
            .query_system_graph_nodes(&nodes(&next.namespace))
            .unwrap()
            .nodes
            .len(),
        3
    );
    assert_eq!(
        store
            .query_system_graph_edges(&edges(&next.namespace))
            .unwrap()
            .edges
            .len(),
        2
    );
}
#[test]
fn malformed_snapshot_never_changes_state() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let first = store.replace_system_graph(&snapshot("source"), 0).unwrap();
    let mut cases = vec![];
    let mut bad = snapshot("source");
    bad.nodes.push(node("a"));
    cases.push(bad);
    let mut bad = snapshot("source");
    bad.edges[0].dst = "missing".into();
    cases.push(bad);
    let mut bad = snapshot("source");
    bad.edges.push(bad.edges[0].clone());
    cases.push(bad);
    let mut bad = snapshot("source");
    bad.edges[0].evidence.clear();
    cases.push(bad);
    let mut bad = snapshot("source");
    bad.provenance.generated_at = "not-time".into();
    cases.push(bad);
    let mut bad = snapshot("source");
    bad.nodes[0]
        .attributes
        .insert("payload".into(), Value::String("a".repeat(MAX_PROPERTIES)));
    cases.push(bad);
    let mut bad = snapshot("source");
    bad.format_version = 2;
    cases.push(bad);
    for bad in cases {
        assert!(store.replace_system_graph(&bad, 1).is_err());
        assert_eq!(store.system_graph_status("source").unwrap().unwrap(), first);
    }
}
#[test]
fn bounded_revision_paging_has_no_skips_and_stale_cursors_fail() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    store.replace_system_graph(&snapshot("source"), 0).unwrap();
    let mut query = nodes("source");
    query.limit = 1;
    let page = store.query_system_graph_nodes(&query).unwrap();
    assert_eq!(page.nodes[0].id, "a");
    query.after_id = page.next_after_id;
    assert!(store.query_system_graph_nodes(&query).is_err());
    query.revision = Some(page.snapshot.revision);
    let page = store.query_system_graph_nodes(&query).unwrap();
    assert_eq!(page.nodes[0].id, "b");
    query.after_id = page.next_after_id;
    let page = store.query_system_graph_nodes(&query).unwrap();
    assert_eq!(page.nodes[0].id, "c");
    assert!(page.next_after_id.is_none());
    let mut edge_query = edges("source");
    edge_query.limit = 1;
    let page = store.query_system_graph_edges(&edge_query).unwrap();
    assert_eq!(page.edges[0].src, "a");
    edge_query.after = page.next_after;
    edge_query.revision = Some(1);
    let page = store.query_system_graph_edges(&edge_query).unwrap();
    assert_eq!(page.edges[0].src, "b");
    assert!(page.next_after.is_none());
    store.replace_system_graph(&snapshot("source"), 1).unwrap();
    assert!(
        store
            .query_system_graph_nodes(&query)
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
}
#[test]
fn fixed_parameter_queries_treat_script_text_as_data_and_do_not_mutate() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let mut data = snapshot("source");
    data.nodes.push(node("'; :rm graph_node { id }; '"));
    store.replace_system_graph(&data, 0).unwrap();
    let mut query = nodes("source");
    query.kind = Some("'; :rm graph_node { id }; '".into());
    assert!(
        store
            .query_system_graph_nodes(&query)
            .unwrap()
            .nodes
            .is_empty()
    );
    let mut query = edges("source");
    query.node_id = Some("'; :rm graph_node { id }; '".into());
    assert!(
        store
            .query_system_graph_edges(&query)
            .unwrap()
            .edges
            .is_empty()
    );
    assert_eq!(
        store
            .system_graph_status("source")
            .unwrap()
            .unwrap()
            .revision,
        1
    );
    assert_eq!(
        store
            .query_system_graph_nodes(&nodes("source"))
            .unwrap()
            .nodes
            .len(),
        4
    );
}
#[test]
fn traversal_deduplicates_cycles_and_resolves_reverse_impact() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let mut data = snapshot("source");
    data.edges.push(edge("c", "a"));
    store.replace_system_graph(&data, 0).unwrap();
    let graph = store
        .traverse_system_graph(&traversal("source", "a", GraphDirection::Outgoing))
        .unwrap();
    assert_eq!(graph.nodes.len(), 3);
    assert_eq!(graph.edges.len(), 3);
    assert!(!graph.truncated);
    let graph = store
        .traverse_system_graph(&traversal("source", "c", GraphDirection::Incoming))
        .unwrap();
    assert_eq!(
        graph.nodes[1],
        GraphReachedNode {
            id: "b".into(),
            depth: 1
        }
    );
    assert_eq!(graph.nodes.len(), 3);
    let mut query = traversal("source", "a", GraphDirection::Both);
    query.relations = vec!["other".into()];
    assert_eq!(store.traverse_system_graph(&query).unwrap().nodes.len(), 1);
}
#[test]
fn traversal_limits_report_truncation_and_invalid_requests_fail_closed() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    store.replace_system_graph(&snapshot("source"), 0).unwrap();
    let mut query = traversal("source", "a", GraphDirection::Outgoing);
    query.max_depth = 1;
    let graph = store.traverse_system_graph(&query).unwrap();
    assert_eq!(graph.nodes.len(), 2);
    assert!(graph.truncation_reasons.contains(&"depth_limit".into()));
    query.max_depth = 32;
    query.max_nodes = 1;
    let graph = store.traverse_system_graph(&query).unwrap();
    assert_eq!(graph.nodes.len(), 1);
    assert!(graph.truncation_reasons.contains(&"node_limit".into()));
    query.max_nodes = 100;
    query.max_edges = 1;
    let graph = store.traverse_system_graph(&query).unwrap();
    assert!(graph.truncated);
    assert_eq!(graph.edges.len(), 1);
    query.max_depth = 33;
    assert!(store.traverse_system_graph(&query).is_err());
    let mut query = nodes("source");
    query.limit = 1001;
    assert!(store.query_system_graph_nodes(&query).is_err());
}
#[test]
fn durable_receipt_and_graph_survive_reopen() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("graph");
    let receipt = {
        let store = CozoGraphShuttle::new_persistent(path.clone()).unwrap();
        store.replace_system_graph(&snapshot("source"), 0).unwrap()
    };
    let mut reopened = None;
    for _ in 0..100 {
        match CozoGraphShuttle::new_persistent(path.clone()) {
            Ok(store) => {
                reopened = Some(store);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let store = reopened.expect("RocksDB close completes");
    assert_eq!(
        store.system_graph_status("source").unwrap().unwrap(),
        receipt
    );
    assert_eq!(
        store
            .query_system_graph_edges(&edges("source"))
            .unwrap()
            .edges
            .len(),
        2
    );
}

#[test]
fn namespace_discovery_is_bounded_and_ordered() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    store.replace_system_graph(&snapshot("z"), 0).unwrap();
    store.replace_system_graph(&snapshot("a"), 0).unwrap();
    let page = store
        .list_system_graphs(&GraphNamespaceQuery {
            after_namespace: None,
            limit: 1,
        })
        .unwrap();
    assert_eq!(page.snapshots[0].namespace, "a");
    assert_eq!(page.next_after_namespace, Some("a".into()));
    let page = store
        .list_system_graphs(&GraphNamespaceQuery {
            after_namespace: page.next_after_namespace,
            limit: 1,
        })
        .unwrap();
    assert_eq!(page.snapshots[0].namespace, "z");
    assert!(page.next_after_namespace.is_none());
}
#[test]
fn encoded_page_and_traversal_byte_limits_are_enforced() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let mut data = snapshot("wide");
    data.nodes.clear();
    data.edges.clear();
    for index in 0..201 {
        let mut n = node(&format!("n{index:03}"));
        n.attributes
            .insert("payload".into(), Value::String("a".repeat(12_000)));
        data.nodes.push(n);
        if index > 0 {
            let mut e = edge("n000", &format!("n{index:03}"));
            e.attributes
                .insert("payload".into(), Value::String("b".repeat(12_000)));
            data.edges.push(e);
        }
    }
    store.replace_system_graph(&data, 0).unwrap();
    let mut nq = nodes("wide");
    nq.limit = 1000;
    let page = store.query_system_graph_nodes(&nq).unwrap();
    assert!(page.next_after_id.is_some());
    assert!(encode(&page).unwrap().len() <= MAX_SYSTEM_GRAPH_RESULT_BYTES);
    nq.after_id = page.next_after_id;
    nq.revision = Some(page.snapshot.revision);
    let next = store.query_system_graph_nodes(&nq).unwrap();
    assert_eq!(page.nodes.len() + next.nodes.len(), 201);
    assert!(next.next_after_id.is_none());
    let mut eq = edges("wide");
    eq.limit = 1000;
    let page = store.query_system_graph_edges(&eq).unwrap();
    assert!(page.next_after.is_some());
    assert!(encode(&page).unwrap().len() <= MAX_SYSTEM_GRAPH_RESULT_BYTES);
    let mut tq = traversal("wide", "n000", GraphDirection::Outgoing);
    tq.max_nodes = 1000;
    tq.max_edges = 5000;
    let result = store.traverse_system_graph(&tq).unwrap();
    assert!(result.truncation_reasons.contains(&"byte_limit".into()));
    assert!(encode(&result).unwrap().len() <= MAX_SYSTEM_GRAPH_RESULT_BYTES);
}
#[test]
fn concurrent_cas_writers_have_one_winner() {
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    let left = store.clone();
    let right = store.clone();
    let a = std::thread::spawn(move || left.replace_system_graph(&snapshot("source"), 0));
    let b = std::thread::spawn(move || right.replace_system_graph(&snapshot("source"), 0));
    let success = [a.join().unwrap(), b.join().unwrap()]
        .into_iter()
        .filter(Result::is_ok)
        .count();
    assert_eq!(success, 1);
    assert_eq!(
        store
            .system_graph_status("source")
            .unwrap()
            .unwrap()
            .revision,
        1
    );
}
