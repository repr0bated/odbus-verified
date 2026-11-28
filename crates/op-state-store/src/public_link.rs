//! The link between a record on the public chain and 3tched's hash chain: every
//! id that names the same record, tied together inside one time bubble.
//!
//! ```text
//! public chain   chain id, EtchedRecord address, record id, root, tx hash,
//!                block number and hash, block time
//! hash chain     event id, event hash and time of the event that took the record in
//! vector         the ledger point id the vectorizer gives that event
//! private entry  keccak256 of the entry (full record and salts) the event holds
//! ```
//!
//! The record starts on the public chain, so the bubble opens at the public
//! block's time and closes `seconds` later; the hash-chain event must fall
//! inside it. `skew` lets the host clock run a little behind the block's clock.
//! A link outside its bubble is not a link.
//!
//! The vector id is derived, never looked up: [`ledger_point_id`] is the one
//! definition (op-cognitive re-exports it for its session-ledger upserts), and
//! subsidiary's `ui/src/lib/web3/link.ts` computes the same UUIDv5. Anyone
//! holding the event hash can name the vector, and a search hit on that vector
//! leads back to the event and from there to the public record.

use crate::public_record::{canonical_json, entry_hash, RecordEntry, RecordError};
use serde::{Deserialize, Serialize};

pub const LINK_VERSION: &str = "subsidiary.link/1";
pub const DEFAULT_BUBBLE_SECONDS: i64 = 300;
pub const DEFAULT_SKEW_SECONDS: i64 = 5;

/// Namespace for session-ledger points: keyed by event hash, so a retried
/// end-of-session vectorization overwrites in place. Fixed forever.
pub const LEDGER_POINT_NAMESPACE: uuid::Uuid =
    uuid::Uuid::from_u128(0x3c0b_91e4_5a27_4d6f_8b12_e6f4_09d3_a751);

/// The vector point an event's embedding is keyed under.
pub fn ledger_point_id(event_hash: &str) -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &LEDGER_POINT_NAMESPACE,
        format!("ledger:{event_hash}").as_bytes(),
    )
}

/// Where and when the record was published, read from the public chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublicRef {
    pub chain_id: u64,
    pub contract: String,
    pub record_id: u64,
    pub root: String,
    pub tx_hash: String,
    pub block_number: u64,
    pub block_hash: String,
    /// Unix seconds of the block.
    pub block_time: i64,
}

/// The hash-chain event that took the record in, as `snowball.query_events`
/// returns it. Other fields of the event are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedEventInput {
    pub event_id: u64,
    pub event_hash: String,
    /// RFC 3339.
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bubble {
    pub opens_at: i64,
    pub closes_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkedEvent {
    pub event_id: u64,
    pub event_hash: String,
    pub time: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LinkedVector {
    pub ledger_point_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Link {
    pub version: String,
    pub bubble: Bubble,
    pub public: PublicRef,
    pub hashchain: LinkedEvent,
    pub vector: LinkedVector,
    pub entry_hash: String,
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("event {0} has no readable timestamp")]
    BadTimestamp(u64),
    #[error("bubble seconds must be a positive integer")]
    BadSeconds,
    #[error("skew must be a whole number of seconds")]
    BadSkew,
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error("{}", .0.join("; "))]
    Problems(Vec<String>),
}

/// Unix seconds of the event, rounded down.
pub fn event_seconds(event: &LinkedEventInput) -> Result<i64, LinkError> {
    chrono::DateTime::parse_from_rfc3339(&event.timestamp)
        .map(|t| t.timestamp())
        .map_err(|_| LinkError::BadTimestamp(event.event_id))
}

/// Tie the ids together. Fails if the event falls outside the bubble or the
/// entry's root is not the public root.
pub fn build_link(
    public: &PublicRef,
    event: &LinkedEventInput,
    entry: &RecordEntry,
    seconds: Option<i64>,
    skew: Option<i64>,
) -> Result<Link, LinkError> {
    let seconds = seconds.unwrap_or(DEFAULT_BUBBLE_SECONDS);
    let skew = skew.unwrap_or(DEFAULT_SKEW_SECONDS);
    if seconds <= 0 {
        return Err(LinkError::BadSeconds);
    }
    if skew < 0 {
        return Err(LinkError::BadSkew);
    }
    let link = Link {
        version: LINK_VERSION.to_owned(),
        bubble: Bubble {
            opens_at: public.block_time - skew,
            closes_at: public.block_time + seconds,
        },
        public: public.clone(),
        hashchain: LinkedEvent {
            event_id: event.event_id,
            event_hash: event.event_hash.clone(),
            time: event_seconds(event)?,
        },
        vector: LinkedVector {
            ledger_point_id: ledger_point_id(&event.event_hash).to_string(),
        },
        entry_hash: entry_hash(entry)?,
    };
    let problems = check_link(&link, Some(public), Some(event), Some(entry));
    if problems.is_empty() {
        Ok(link)
    } else {
        Err(LinkError::Problems(problems))
    }
}

/// Check a link against what each side says now: the public ref read from the
/// chain, the event read from the hash chain, the entry the event holds. Pass
/// what you have. Returns the problems found (the same words as link.ts).
pub fn check_link(
    link: &Link,
    public: Option<&PublicRef>,
    event: Option<&LinkedEventInput>,
    entry: Option<&RecordEntry>,
) -> Vec<String> {
    if link.version != LINK_VERSION {
        return vec![format!("unknown link version {}", link.version)];
    }
    let mut problems = Vec::new();
    let inside = |t: i64| link.bubble.opens_at <= t && t <= link.bubble.closes_at;
    if !inside(link.public.block_time) {
        problems.push("public block time is outside the bubble".to_owned());
    }
    if !inside(link.hashchain.time) {
        problems.push("hash-chain event time is outside the bubble".to_owned());
    }
    if link.vector.ledger_point_id != ledger_point_id(&link.hashchain.event_hash).to_string() {
        problems.push("vector id is not the ledger point id of the event hash".to_owned());
    }
    if let Some(p) = public {
        if p != &link.public {
            problems.push("public chain says something else".to_owned());
        }
    }
    if let Some(e) = event {
        if e.event_id != link.hashchain.event_id || e.event_hash != link.hashchain.event_hash {
            problems.push("hash-chain event id or hash differs".to_owned());
        } else {
            match event_seconds(e) {
                Ok(t) if t == link.hashchain.time => {}
                Ok(_) => problems.push("hash-chain event time differs".to_owned()),
                Err(err) => problems.push(err.to_string()),
            }
        }
    }
    if let Some(entry) = entry {
        match entry_hash(entry) {
            Ok(h) if h == link.entry_hash => {}
            Ok(_) => problems.push("entry differs from the one linked".to_owned()),
            Err(err) => problems.push(err.to_string()),
        }
        if entry.root != link.public.root {
            problems.push("entry root differs from the public root".to_owned());
        }
    }
    problems
}

/// The canonical text of a link, as the CLI prints it and the hash chain stores it.
pub fn link_text(link: &Link) -> Result<String, RecordError> {
    canonical_json(&serde_json::to_value(link).expect("link serializes"))
}

#[cfg(test)]
#[path = "public_link_tests.rs"]
mod tests;
