//! Caller-scoped semantic evidence retrieval through the canonical Snowball plugin.

use std::collections::{BTreeSet, HashSet};
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{
    project::ChatMode,
    snowball_plugin::{EvidenceMatch, SearchOwnEventsInput, SearchOwnEventsOutput, SEARCH_OWN_EVENTS_CAPABILITY},
};
use crate::{mutation_engine::MutationEngine, specialist::CognitiveCaller};

async fn scope(engine: &MutationEngine, caller: CognitiveCaller<'_>) -> Result<String> {
    let session = crate::project_dispatch::verified_session(
        engine, caller.actor_id, caller.session_id, caller.session_genesis,
    ).await?;
    let grants: BTreeSet<String> = crate::interceptor::load_capability_grants(caller.actor_id)
        .into_iter().collect();
    ensure!(grants.contains(SEARCH_OWN_EVENTS_CAPABILITY), "own-session evidence search capability is required");
    crate::method_contract_admission::admit_compiled_method(
        "snowball", "search_own_events", Some(SEARCH_OWN_EVENTS_CAPABILITY),
    )?;
    let context = crate::project_dispatch::current_chat_context(engine, &session, &grants).await?;
    ensure!(context.mode == ChatMode::Accountability, "evidence search requires Accountability mode");
    Ok(session)
}

pub(crate) async fn search(
    engine: &MutationEngine,
    caller: CognitiveCaller<'_>,
    input: SearchOwnEventsInput,
) -> Result<SearchOwnEventsOutput> {
    let session = scope(engine, caller).await?;
    ensure!(input.query.len() <= 4000, "evidence query exceeds 4000 UTF-8 bytes");
    let limit = input.limit.unwrap_or(10);
    ensure!((1..=20).contains(&limit), "evidence search limit must be between 1 and 20");
    let (embedding_model, hits) = tokio::time::timeout(
        std::time::Duration::from_secs(45),
        op_cognitive::chain_vectors::search_session_events(
            &input.query, caller.actor_id, &session, limit,
        ),
    ).await.context("evidence search timed out")??;
    ensure!(scope(engine, caller).await? == session, "evidence scope changed during search");
    let mut matches = Vec::new();
    let mut rejected_hits = 0;
    let mut seen = HashSet::new();
    {
        let chain = engine.event_chain.read().await;
        for hit in hits {
            // Never expose index text, arbitrary payload fields, or an event
            // from another identity, even if the index is stale or corrupted.
            let hash = hit.payload.get("event_hash").and_then(|v| v.as_str());
            let event = hash.and_then(|hash| chain.events().iter().find(|e| {
                e.event_hash == *hash && e.actor_id == caller.actor_id
                    && e.session_id.as_deref() == Some(session.as_str())
            }));
            if let Some(event) = event.filter(|_| hit.score.is_finite()) {
                if seen.insert(event.event_hash.clone()) {
                    matches.push(EvidenceMatch {
                        score: hit.score,
                        event: crate::mutation_engine::chain_event_to_record(event),
                    });
                }
            } else {
                rejected_hits += 1;
            }
        }
    }
    ensure!(scope(engine, caller).await? == session, "evidence scope changed before disclosure");
    Ok(SearchOwnEventsOutput { embedding_model, matches, rejected_hits })
}
