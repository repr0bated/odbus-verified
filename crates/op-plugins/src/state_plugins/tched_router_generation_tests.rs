use super::*;
use op_cozo_store::CozoGraphShuttle;

fn owner(principal: &str, session: &str) -> GenerationOwner {
    GenerationOwner::new(principal.into(), session.into()).unwrap()
}
fn input() -> StartGenerationInput {
    StartGenerationInput {
        agent_alias: "fixture-agent_alias".into(),
        prompt: "PRIVATE_PROMPT_MUST_NOT_PERSIST".into(),
    }
}
fn service() -> GenerationMethods {
    GenerationMethods::new(Arc::new(RuntimeDocuments::new(
        CozoGraphShuttle::new_in_memory().unwrap(),
    )))
}

#[tokio::test]
async fn owner_scope_siblings_and_job_revisions_survive_transitions() {
    let service = service();
    let owner = owner("principal-a", "session-a");
    let hash = "a".repeat(64);
    service
        .create(&owner, &input(), "one", &hash)
        .await
        .unwrap();
    service
        .create(&owner, &input(), "two", &hash)
        .await
        .unwrap();
    service.running(&owner, "one").await.unwrap();
    let state = service
        .append(&owner, "one", "first\n".into())
        .await
        .unwrap();
    assert_eq!(state.generation_jobs["two"].revision, 1);
    assert_eq!(state.generation_jobs["one"].output, "first\n");
    let (a, b) = tokio::join!(
        service.append(&owner, "one", "second".into()),
        service.running(&owner, "two")
    );
    a.unwrap();
    b.unwrap();
    let state = service
        .finish(&owner, "one", GenerationStatus::Completed, None)
        .await
        .unwrap();
    assert_eq!(state.generation_jobs["one"].output, "first\nsecond");
    assert_eq!(
        state.generation_jobs["two"].status,
        GenerationStatus::Running
    );
    assert!(service
        .recover(&super::GenerationOwner::new("principal-b".into(), "session-a".into()).unwrap())
        .await
        .unwrap()
        .is_none());
    assert!(service
        .recover(&super::GenerationOwner::new("principal-a".into(), "session-b".into()).unwrap())
        .await
        .unwrap()
        .is_none());
    assert!(!serde_json::to_string(&state)
        .unwrap()
        .contains(&input().prompt));
    let raw = service
        .documents
        .read(&owner.document_id())
        .await
        .unwrap()
        .unwrap();
    assert!(!serde_json::to_string(&raw.document)
        .unwrap()
        .contains(&input().prompt));
}

#[tokio::test]
async fn stale_cancel_active_delete_and_bounds_leave_the_durable_row_unchanged() {
    let service = service();
    let owner = owner("a", "b");
    let hash = "b".repeat(64);
    let created = service
        .create(&owner, &input(), "job", &hash)
        .await
        .unwrap();
    assert!(service.request_cancel(&owner, "job", 2).await.is_err());
    assert!(service.delete(&owner, "job", 1).await.is_err());
    let current = service
        .documents
        .read(&owner.document_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.revision, created.generation_revision);
    service.running(&owner, "job").await.unwrap();
    assert!(service
        .append(&owner, "job", "x".repeat(MAX_CHUNK_BYTES + 1))
        .await
        .is_err());
    let cancelled = service.request_cancel(&owner, "job", 2).await.unwrap();
    assert_eq!(
        cancelled.generation_jobs["job"].status,
        GenerationStatus::CancelRequested
    );
    assert!(service.append(&owner, "job", "late".into()).await.is_err());
    let done = service
        .finish(&owner, "job", GenerationStatus::Cancelled, None)
        .await
        .unwrap();
    let deleted = service
        .delete(&owner, "job", done.generation_jobs["job"].revision)
        .await
        .unwrap();
    assert!(deleted.generation_jobs.is_empty());
    assert_eq!(
        GenerationState::from_plugin_state(&deleted.plugin_state().unwrap()).unwrap(),
        deleted
    );
}

#[tokio::test]
async fn reopen_recovery_interrupts_unfinished_work_without_replaying_prompt() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("db");
    let owner = owner("a", "b");
    {
        let service = GenerationMethods::new(Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_persistent_private(path.clone()).unwrap(),
        )));
        service
            .create(&owner, &input(), "job", &"a".repeat(64))
            .await
            .unwrap();
        service.running(&owner, "job").await.unwrap();
        service
            .append(&owner, "job", "last committed text".into())
            .await
            .unwrap();
    }
    let service = GenerationMethods::new(Arc::new(RuntimeDocuments::new(
        CozoGraphShuttle::new_persistent_private(path).unwrap(),
    )));
    let recovered = service.recover(&owner).await.unwrap().unwrap();
    assert_eq!(
        recovered.generation_jobs["job"].status,
        GenerationStatus::Interrupted
    );
    assert_eq!(
        recovered.generation_jobs["job"].failure,
        Some(GenerationFailure::ProcessInterrupted)
    );
    assert_eq!(
        recovered.generation_jobs["job"].output,
        "last committed text"
    );
    assert_eq!(service.recover(&owner).await.unwrap().unwrap(), recovered);
}

#[tokio::test]
async fn forged_owner_and_corrupt_retained_rows_fail_closed() {
    let service = service();
    let owner = owner("a", "b");
    service
        .create(&owner, &input(), "job", &"a".repeat(64))
        .await
        .unwrap();
    service
        .documents
        .update(&owner.document_id(), |prior| {
            let mut value = prior.unwrap();
            value["session"] = serde_json::json!("other");
            Ok(value)
        })
        .await
        .unwrap();
    assert!(service.recover(&owner).await.is_err());
    assert!(service.running(&owner, "job").await.is_err());
}

#[test]
fn selectors_are_reused_but_storage_domains_and_inputs_stay_separate() {
    let a = owner("ab", "c");
    let b = owner("a", "bc");
    assert_ne!(a.selector(), b.selector());
    assert!(a.document_id().starts_with("tched_generation:v1:"));
    for agent_alias in ["", "../another", "different/session", "bad name"] {
        assert!(StartGenerationInput {
            agent_alias: agent_alias.into(),
            prompt: "hello".into()
        }
        .validate()
        .is_err());
    }
    assert!(serde_json::from_value::<StartGenerationInput>(
        serde_json::json!({"agent_alias":"a","prompt":"x","session_id":"victim"})
    )
    .is_err());
}

#[tokio::test]
async fn escaped_output_aggregate_budget_fits_document_and_rejects_before_append() {
    let service = service();
    let owner = owner("owner", "session");
    let state = service
        .create(&owner, &input(), "base", &"a".repeat(64))
        .await
        .unwrap();
    let mut jobs = BTreeMap::new();
    for number in 0..4 {
        let mut job = state.generation_jobs["base"].clone();
        job.job_id = format!("full-{number}");
        job.status = GenerationStatus::Completed;
        job.output = "\u{0000}".repeat(MAX_OUTPUT_BYTES);
        jobs.insert(job.job_id.clone(), job);
    }
    let mut next = state.generation_jobs["base"].clone();
    next.job_id = "next".into();
    next.status = GenerationStatus::Running;
    jobs.insert(next.job_id.clone(), next);
    let snapshot = GenerationState {
        generation_revision: 10,
        generation_jobs: jobs,
    };
    validate_jobs(&snapshot.generation_jobs).unwrap();
    assert!(serde_json::to_vec(&snapshot).unwrap().len() < MAX_DOCUMENT_BYTES);
    let stored = StoredGenerations {
        format_version: 1,
        principal: owner.principal.clone(),
        session: owner.session.clone(),
        jobs: snapshot.generation_jobs.clone(),
    };
    assert!(serde_json::to_vec(&stored).unwrap().len() < MAX_DOCUMENT_BYTES);
    assert!(snapshot.preflight_append("next", "x").is_err());
    // The same invariant is enforced by the actual durable transition. Seed an
    // exact bounded fixture row and prove a rejected append creates no revision.
    let row = service
        .documents
        .update_with_result(&owner.document_id(), move |_| {
            Ok((serde_json::to_value(stored)?, ()))
        })
        .await
        .unwrap()
        .0;
    assert!(service.append(&owner, "next", "x".into()).await.is_err());
    assert_eq!(
        service
            .documents
            .read(&owner.document_id())
            .await
            .unwrap()
            .unwrap()
            .revision,
        row.revision
    );
}

#[tokio::test]
async fn completion_cannot_replace_the_committed_incremental_text() {
    let service = service();
    let owner = owner("a", "b");
    service
        .create(&owner, &input(), "job", &"a".repeat(64))
        .await
        .unwrap();
    service.running(&owner, "job").await.unwrap();
    let state = service
        .append(&owner, "job", "actual".into())
        .await
        .unwrap();
    assert!(service
        .complete(&owner, "job", "fabricated".into(), None, None)
        .await
        .is_err());
    assert_eq!(
        service
            .documents
            .read(&owner.document_id())
            .await
            .unwrap()
            .unwrap()
            .revision,
        state.generation_revision
    );
    assert_eq!(
        service
            .complete(&owner, "job", "actual".into(), None, None)
            .await
            .unwrap()
            .generation_jobs["job"]
            .status,
        GenerationStatus::Completed
    );
}

#[tokio::test]
async fn bounded_fragment_batch_is_one_revision_with_original_order_and_count() {
    let service = service();
    let owner = owner("batch-owner", "batch-session");
    service
        .create(&owner, &input(), "job", &"a".repeat(64))
        .await
        .unwrap();
    service
        .create(&owner, &input(), "sibling", &"a".repeat(64))
        .await
        .unwrap();
    let prior = service.running(&owner, "job").await.unwrap();
    let fragments = vec!["first".into(), "\n".into(), "λ".into()];
    prior.preflight_append_batch("job", &fragments).unwrap();
    let committed = service
        .append_batch(&owner, "job", fragments)
        .await
        .unwrap();
    assert_eq!(committed.generation_revision, prior.generation_revision + 1);
    assert_eq!(
        committed.generation_jobs["job"].revision,
        prior.generation_jobs["job"].revision + 1
    );
    assert_eq!(committed.generation_jobs["job"].committed_chunks, 3);
    assert_eq!(committed.generation_jobs["job"].output, "first\nλ");
    assert_eq!(
        committed.generation_jobs["sibling"],
        prior.generation_jobs["sibling"]
    );
    let finished = service
        .complete(&owner, "job", "first\nλ".into(), None, None)
        .await
        .unwrap();
    assert_eq!(finished.generation_jobs["job"].committed_chunks, 3);
}

#[tokio::test]
async fn invalid_batches_and_cancelled_jobs_leave_the_durable_revision_unchanged() {
    let service = service();
    let owner = owner("batch-owner", "batch-session");
    service
        .create(&owner, &input(), "job", &"a".repeat(64))
        .await
        .unwrap();
    let prior = service.running(&owner, "job").await.unwrap();
    for fragments in [
        vec![],
        vec![String::new()],
        vec!["x".repeat(MAX_CHUNK_BYTES + 1)],
        vec!["x".repeat(MAX_CHUNK_BYTES), "y".into()],
        vec!["x".into(); MAX_BATCH_FRAGMENTS + 1],
    ] {
        assert!(prior.preflight_append_batch("job", &fragments).is_err());
        assert!(service
            .append_batch(&owner, "job", fragments)
            .await
            .is_err());
    }
    assert_eq!(
        service
            .documents
            .read(&owner.document_id())
            .await
            .unwrap()
            .unwrap()
            .revision,
        prior.generation_revision
    );
    let mut frame_limited = prior.clone();
    frame_limited
        .generation_jobs
        .get_mut("job")
        .unwrap()
        .committed_chunks = MAX_FRAMES - 1;
    assert!(frame_limited
        .preflight_append_batch("job", &["one".into(), "two".into()])
        .is_err());
    let cancelled = service
        .request_cancel(&owner, "job", prior.generation_jobs["job"].revision)
        .await
        .unwrap();
    assert!(service
        .append_batch(&owner, "job", vec!["queued-before-cancel".into()])
        .await
        .is_err());
    assert_eq!(
        service
            .documents
            .read(&owner.document_id())
            .await
            .unwrap()
            .unwrap()
            .revision,
        cancelled.generation_revision
    );
}
