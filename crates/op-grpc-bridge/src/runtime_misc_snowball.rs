//! Snowball's current owner model is the append-only session ledger.
//! Query and verification read BridgeCore's durable chain. Semantic scores come
//! only from the existing Voyage/Qdrant owner and are checked against that chain.
//! The still-sealed snapshot/DR/retention contracts are explicitly incompatible;
//! they are not approximated with journal pages or new Btrfs operations.
use std::{collections::{HashMap, HashSet}, time::Duration};

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{
    project::{ChatMode, ResolvedChatContext},
    snowball_plugin::{AuditEventRecord, EvidenceMatch, QueryEventsInput, QueryEventsOutput,
        SearchOwnEventsInput, SearchOwnEventsOutput, VerifyChainInput, VerifyChainOutput},
};
use serde_json::{Value, json};

use crate::{
    core::{BackendStateUpdate, JournalEvent},
    proto::Decision,
    runtime::{BackendContext, BackendOutcome, RuntimeError},
};

pub(super) fn supports(method: &str) -> bool {
    matches!(method,
        "query_events" | "verify_chain" | "search_own_events" |
        "create_snapshot" | "list_snapshots" | "get_snapshot" | "rollback" |
        "get_current_state" | "get_stats" | "set_retention" | "get_projector_stats")
}

pub(super) async fn dispatch(
    ctx: &BackendContext<'_>, method: &str, args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    let result = match method {
        "query_events" => query(ctx, decode(args)?).await?,
        "verify_chain" => verify(ctx, decode(args)?).await?,
        "search_own_events" => search(ctx, decode(args)?).await?,
        "create_snapshot" | "list_snapshots" | "get_snapshot" | "rollback" => {
            return Err(RuntimeError::unsupported(
                "stale Snowball snapshot contract: the session-ledger owner neither creates snapshots nor rolls them back; retire and reseal this declaration",
            ));
        }
        "get_current_state" => return Err(RuntimeError::unsupported(
            "Snowball get_current_state declares a DR state-image result; no session-ledger DR-image owner exists, and a journal page is not that result",
        )),
        "get_stats" | "set_retention" => return Err(RuntimeError::unsupported(
            "Snowball snapshot counts and hourly/daily/weekly/quarterly retention have no session-ledger owner; the sealed contract requires retirement or replacement",
        )),
        "get_projector_stats" => return Err(RuntimeError::unsupported(
            "no end-of-session ledger vectorization owner is installed in the fresh runtime; continuous snapshot-projector counters are not session-finalization statistics",
        )),
        _ => return Err(RuntimeError::unsupported("undeclared Snowball method")),
    };
    super::revalidate(ctx).await?;
    // Results are durable ledger observations, not a replacement Snowball
    // snapshot state tree and not an in-memory audit authority.
    Ok(super::private_result(ctx, result, BackendStateUpdate::Unchanged))
}

fn decode<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, RuntimeError> {
    serde_json::from_value(args.clone()).map_err(|error| RuntimeError::before_effect(error.into()))
}

fn valid_range(from: Option<u64>, to: Option<u64>) -> Result<(u64, u64), RuntimeError> {
    let from = from.unwrap_or(0);
    let to = to.unwrap_or(0);
    if to != 0 && from > to {
        return Err(RuntimeError::before_effect(anyhow::anyhow!("event range is reversed")));
    }
    Ok((from, to))
}

async fn events(ctx: &BackendContext<'_>, from: u64, to: u64) -> Result<Vec<JournalEvent>, RuntimeError> {
    let runtime = ctx.runtime.clone();
    let scope = ctx.caller.private_scope();
    tokio::task::spawn_blocking(move || runtime.core().journal_events(&scope, from, to))
        .await.map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(RuntimeError::backend)
}

async fn query(ctx: &BackendContext<'_>, input: QueryEventsInput) -> Result<Value, RuntimeError> {
    let (from, to) = valid_range(input.from_event_id, input.to_event_id)?;
    let decision = match input.decision.as_deref() {
        None | Some("") => None,
        Some("allow") => Some(Decision::Allow as i32),
        Some("deny") => Some(Decision::Deny as i32),
        Some(_) => return Err(RuntimeError::before_effect(anyhow::anyhow!("decision must be allow or deny"))),
    };
    let plugin = input.plugin_id.as_deref().filter(|plugin| !plugin.is_empty());
    let limit = input.limit.unwrap_or(50).clamp(1, 100) as usize;
    // total_in_chain is the verified caller's session chain, never the global
    // event count or a disclosure of another principal's activity.
    let all = events(ctx, 0, 0).await?;
    let total_in_chain = all.len() as u64;
    let mut matching = all.into_iter().filter(|event|
        (from == 0 || event.event_id >= from) && (to == 0 || event.event_id <= to)
        && plugin.is_none_or(|plugin| event.plugin_id == plugin)
        && decision.is_none_or(|decision| event.decision == decision));
    let mut page = Vec::with_capacity(limit);
    for event in matching.by_ref().take(limit) {
        page.push(record(event).map_err(RuntimeError::backend)?);
    }
    let has_more = matching.next().is_some();
    Ok(serde_json::to_value(QueryEventsOutput { events: page, has_more, total_in_chain })?)
}

async fn verify(ctx: &BackendContext<'_>, input: VerifyChainInput) -> Result<Value, RuntimeError> {
    let (from, to) = valid_range(input.from_event_id, input.to_event_id)?;
    let runtime = ctx.runtime.clone();
    let scope = ctx.caller.private_scope();
    let output = tokio::task::spawn_blocking(move || -> Result<VerifyChainOutput> {
        // Core verifies real MD5 event hashes, predecessor links and durable
        // record binding while walking the interleaved append ledger. The
        // exposed count is scoped; other callers' IDs/errors never escape.
        let verified = runtime.core().verify_journal(from, to)?;
        let own = runtime.core().journal_events(&scope, from, to)?;
        Ok(VerifyChainOutput {
            valid: verified.valid,
            events_verified: own.len() as u64,
            errors: if verified.valid { Vec::new() } else { vec!["durable ledger integrity verification failed".into()] },
        })
    }).await.map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(RuntimeError::backend)?;
    Ok(serde_json::to_value(output)?)
}

async fn accountability(ctx: &BackendContext<'_>) -> Result<(), RuntimeError> {
    super::revalidate(ctx).await?;
    ctx.runtime.require_capability(ctx.caller,
        op_plugins::state_plugins::snowball_plugin::SEARCH_OWN_EVENTS_CAPABILITY).await?;
    let result = ctx.runtime.call_nested(ctx.caller, "project", "get_context", json!({})).await?;
    let selected: ResolvedChatContext = serde_json::from_value(result.result)?;
    if selected.session_id != ctx.caller.session_id || selected.mode != ChatMode::Accountability {
        return Err(RuntimeError::denied("own-session evidence search requires this session's Accountability mode"));
    }
    Ok(())
}

async fn search(ctx: &BackendContext<'_>, input: SearchOwnEventsInput) -> Result<Value, RuntimeError> {
    let limit = input.limit.unwrap_or(10);
    if input.query.trim().is_empty() || input.query.len() > 4000 || !(1..=20).contains(&limit) {
        return Err(RuntimeError::before_effect(anyhow::anyhow!("semantic query requires 1..=4000 bytes and limit 1..=20")));
    }
    accountability(ctx).await?;
    let (embedding_model, hits) = tokio::time::timeout(Duration::from_secs(45),
        op_cognitive::chain_vectors::search_session_events(
            &input.query, &ctx.caller.principal_id, &ctx.caller.session_id, limit,
        ),
    ).await.map_err(|error| RuntimeError::backend(error.into()))?
        .map_err(RuntimeError::backend)?;
    accountability(ctx).await?;
    let canonical: HashMap<_, _> = events(ctx, 0, 0).await?.into_iter()
        .map(|event| (event.event_hash.clone(), event)).collect();
    let mut matches = Vec::with_capacity(hits.len());
    let mut seen = HashSet::with_capacity(hits.len());
    let mut rejected_hits = 0;
    for hit in hits {
        let hash = hit.payload.get("event_hash").and_then(|value| value.as_str()).map(|value| &value[..]);
        let actor = hit.payload.get("actor_id").and_then(|value| value.as_str()).map(|value| &value[..]);
        let session = hit.payload.get("session_id").and_then(|value| value.as_str()).map(|value| &value[..]);
        let event = hash.and_then(|hash| canonical.get(hash)).filter(|event|
            event.actor_id == ctx.caller.principal_id && hit.score.is_finite()
            && actor == Some(ctx.caller.principal_id.as_str())
            && session == Some(ctx.caller.session_id.as_str()));
        match event {
            Some(event) if seen.insert(event.event_hash.clone()) => matches.push(EvidenceMatch {
                score: hit.score, event: record(event.clone()).map_err(RuntimeError::backend)?,
            }),
            Some(_) => {}
            None => rejected_hits += 1,
        }
    }
    super::revalidate(ctx).await?;
    Ok(serde_json::to_value(SearchOwnEventsOutput { embedding_model, matches, rejected_hits })?)
}

fn record(event: JournalEvent) -> Result<AuditEventRecord> {
    let timestamp = chrono::DateTime::<chrono::Utc>::from_timestamp(
        event.timestamp.seconds, u32::try_from(event.timestamp.nanos).context("invalid event timestamp")?,
    ).context("invalid event timestamp")?.to_rfc3339();
    let (path, method_name) = event.target.rsplit_once(':')
        .context("canonical event target does not identify its plugin member")?;
    let root = op_plugins::canonical::plugin_path(&event.plugin_id);
    ensure!((path == root || path.strip_prefix(&root).is_some_and(|suffix| suffix.starts_with('/')))
        && !method_name.is_empty(), "canonical event target/member is invalid");
    let method_name = method_name.to_owned();
    let decision = if event.decision == Decision::Allow as i32 { "allow" }
        else if event.decision == Decision::Deny as i32 { "deny" }
        else { anyhow::bail!("canonical event decision is invalid"); };
    Ok(AuditEventRecord {
        event_id: event.event_id, event_hash: event.event_hash, prev_hash: event.prev_hash,
        timestamp, actor_id: event.actor_id, capability_id: event.capability_id,
        plugin_id: event.plugin_id, method_name, operation_type: event.operation_type,
        target: event.target, tags_touched: event.tags_touched, decision: decision.into(),
        input_patch_hash: event.input_patch_hash, result_effective_hash: event.result_effective_hash,
    })
}
