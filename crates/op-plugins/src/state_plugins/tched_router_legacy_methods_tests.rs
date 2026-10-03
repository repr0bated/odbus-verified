use super::*;
use crate::state_plugins::common::llm_projection::{ModelRoute, Provider};
use op_cozo_store::CozoGraphShuttle;

fn provider(id: &str) -> Provider {
    Provider {
        id: id.into(),
        ..Default::default()
    }
}

fn route(hint: &str, provider: &str, model: &str) -> ModelRoute {
    ModelRoute {
        hint: hint.into(),
        provider: provider.into(),
        model: model.into(),
        available: true,
        status: "available".into(),
        ..Default::default()
    }
}

fn observed() -> ObservedTchedRouterState {
    ObservedTchedRouterState {
        status: "ready".into(),
        selected_provider: "p1".into(),
        selected_model: "m1".into(),
        providers: vec![provider("p1"), provider("p2")],
        model_routes: vec![route("fast", "p1", "m1"), route("reason", "p2", "m2")],
    }
}

fn methods() -> TchedRouterLegacyMethods {
    TchedRouterLegacyMethods::new(Arc::new(RuntimeDocuments::new(
        CozoGraphShuttle::new_in_memory().unwrap(),
    )))
}

#[tokio::test]
async fn runtime_reads_and_state_use_the_same_observation() {
    let methods = methods();
    let view = methods
        .observe("GetModelRoutes", &json!({}), &observed())
        .await
        .unwrap();
    assert_eq!(
        view.output["model_routes"],
        view.represented_state["model_routes"]
    );
    let resolved = methods
        .observe("ResolveRoute", &json!({"hint":"reason"}), &observed())
        .await
        .unwrap();
    assert_eq!(
        resolved.output["route"],
        serde_json::to_value(&observed().model_routes[1]).unwrap()
    );
    let by_model = methods
        .observe("ResolveRoute", &json!({"hint":"m1"}), &observed())
        .await
        .unwrap();
    assert_eq!(by_model.output["route"]["hint"], "fast");
}

#[tokio::test]
async fn assignments_preserve_siblings_and_are_durable_receipts() {
    let methods = methods();
    let (first, _) = methods
        .assign(ModelRole::OvsRouting, "m1", &observed())
        .await
        .unwrap();
    assert_eq!(first.document_revision, 1);
    assert!(first.changed);
    let (second, state) = methods
        .assign(ModelRole::Obfuscation, "m2", &observed())
        .await
        .unwrap();
    assert_eq!(second.document_revision, 2);
    assert_eq!(second.model_assignments.ovs_routing.as_deref(), Some("m1"));
    assert_eq!(second.model_assignments.obfuscation.as_deref(), Some("m2"));
    assert_eq!(
        state["model_assignments"],
        serde_json::to_value(&second.model_assignments).unwrap()
    );
    let (same, _) = methods
        .assign(ModelRole::Obfuscation, "m2", &observed())
        .await
        .unwrap();
    assert_eq!(same.document_revision, 3);
    assert!(!same.changed);
}

#[tokio::test]
async fn every_declared_role_updates_only_its_owned_field() {
    let methods = methods();
    for (role, model) in [
        (ModelRole::OvsRouting, "m1"),
        (ModelRole::Obfuscation, "m2"),
        (ModelRole::Vectorization, "m1"),
        (ModelRole::QdrantRetrieval, "m2"),
        (ModelRole::CozoRetrieval, "m1"),
    ] {
        let (receipt, _) = methods.assign(role, model, &observed()).await.unwrap();
        assert_eq!(receipt.role, role.field());
        assert_eq!(receipt.model_id, model);
    }
    let (_, all) = methods.assignments().await.unwrap();
    assert_eq!(all.ovs_routing.as_deref(), Some("m1"));
    assert_eq!(all.obfuscation.as_deref(), Some("m2"));
    assert_eq!(all.vectorization.as_deref(), Some("m1"));
    assert_eq!(all.qdrant_retrieval.as_deref(), Some("m2"));
    assert_eq!(all.cozo_retrieval.as_deref(), Some("m1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_role_writes_preserve_siblings() {
    let methods = Arc::new(methods());
    let mut writes = tokio::task::JoinSet::new();
    for (role, model) in [
        (ModelRole::OvsRouting, "m1"),
        (ModelRole::Obfuscation, "m2"),
        (ModelRole::Vectorization, "m1"),
        (ModelRole::QdrantRetrieval, "m2"),
        (ModelRole::CozoRetrieval, "m1"),
    ] {
        let methods = methods.clone();
        writes.spawn(async move { methods.assign(role, model, &observed()).await });
    }
    while let Some(write) = writes.join_next().await {
        write.unwrap().unwrap();
    }
    let (_, all) = methods.assignments().await.unwrap();
    assert!(all.ovs_routing.is_some());
    assert!(all.obfuscation.is_some());
    assert!(all.vectorization.is_some());
    assert!(all.qdrant_retrieval.is_some());
    assert!(all.cozo_retrieval.is_some());
}

#[tokio::test]
async fn malformed_or_ambiguous_inputs_fail_before_durable_write() {
    let methods = methods();
    assert!(
        methods
            .assign(ModelRole::OvsRouting, "missing", &observed())
            .await
            .is_err()
    );
    assert!(
        methods
            .observe(
                "ResolveRoute",
                &json!({"hint":"fast","extra":1}),
                &observed()
            )
            .await
            .is_err()
    );
    let mut ambiguous = observed();
    ambiguous.model_routes.push(route("other", "p2", "m1"));
    assert!(
        methods
            .assign(ModelRole::OvsRouting, "m1", &ambiguous)
            .await
            .is_err()
    );
    assert_eq!(methods.assignments().await.unwrap().0, 0);
}

#[tokio::test]
async fn partial_assignment_document_does_not_invent_defaults() {
    let methods = methods();
    let (_, assignments) = methods.assignments().await.unwrap();
    assert_eq!(serde_json::to_value(assignments).unwrap(), json!({}));
    let (_, state) = methods
        .assign(ModelRole::Vectorization, "m2", &observed())
        .await
        .unwrap();
    assert_eq!(state["model_assignments"], json!({"vectorization":"m2"}));
}

#[tokio::test]
async fn assignments_survive_durable_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cozo");
    {
        let methods = TchedRouterLegacyMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_persistent(path.clone()).unwrap(),
        )));
        methods
            .assign(ModelRole::QdrantRetrieval, "m2", &observed())
            .await
            .unwrap();
    }
    let reopened = TchedRouterLegacyMethods::new(Arc::new(RuntimeDocuments::new(
        CozoGraphShuttle::new_persistent(path).unwrap(),
    )));
    let (revision, assignments) = reopened.assignments().await.unwrap();
    assert_eq!(revision, 1);
    assert_eq!(assignments.qdrant_retrieval.as_deref(), Some("m2"));
}

#[tokio::test]
async fn duplicate_hint_is_rejected_instead_of_first_match_wins() {
    let methods = methods();
    let mut ambiguous = observed();
    ambiguous.model_routes.push(route("fast", "p2", "m2"));
    let error = methods
        .observe("ResolveRoute", &json!({"hint":"fast"}), &ambiguous)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
}

#[tokio::test]
async fn learned_hint_routes_keep_hint_and_model_resolution_exact() {
    let methods = methods();
    let mut learned = observed();
    learned.model_routes = vec![
        route("runtime", "p1", "m1"),
        route("fast", "p1", "m1"),
        route("runtime", "p2", "m2"),
    ];
    let by_hint = methods
        .observe("ResolveRoute", &json!({"hint":"fast"}), &learned)
        .await
        .unwrap();
    assert_eq!(by_hint.output["route"]["hint"], "fast");
    assert_eq!(by_hint.output["route"]["model"], "m1");
    let by_model = methods
        .observe("ResolveRoute", &json!({"hint":"m1"}), &learned)
        .await
        .unwrap();
    assert_eq!(by_model.output["route"]["provider"], "p1");
    assert_eq!(by_model.output["route"]["hint"], "runtime");
}
