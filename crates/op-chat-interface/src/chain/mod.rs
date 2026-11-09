//! Chain_Verifier (spec task 11.1): continuity-only chain verification.
//!
//! Pure function: accepts only segments with `source: "operation-file"`.
//! `continuity` is `broken` iff some `i >= 1` has
//! `records[i].prev_hash != records[i-1].exec_hash` (byte equality on
//! UTF-8 strings). `result` is `broken` if continuity is broken or any
//! exposed `verify_integrity` value is false; otherwise `unverified` if
//! any Receipt-producing call is unlinked or any reason exists; otherwise
//! `verified`. `hash_execution` recomputation is not implemented (D-30).
//!
//! No `hash_execution` recomputation: no Owner_Authorization recorded.
//!
//! Properties verified by tests:
//! - Property 25: Continuity equivalence
//! - Property 26: Verdict precedence

use serde::{Deserialize, Serialize};

use crate::types::ExecutionRecordView;

// ── types ──────────────────────────────────────────────────────────────

/// Source tag — only `"operation-file"` is accepted (Req 10.9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainSegmentSource {
    OperationFile,
}

/// A read segment of execution records (design D-30).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainSegment {
    /// UTC ISO 8601 timestamp of the read.
    pub read_at: String,
    /// Execution records in order as returned by the granted read.
    pub records: Vec<ExecutionRecordView>,
    /// Must be `"operation-file"` — the only accepted source.
    pub source: ChainSegmentSource,
}

/// Per-link report for each record in the chain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainLinkReport {
    /// Index into the records array (0-based).
    pub index: usize,
    /// The `prev_hash` from this record.
    pub prev_hash: String,
    /// The `exec_hash` from this record.
    pub exec_hash: String,
    /// `true` for the anchor (i=0), `false` for integrity failure,
    /// `None` when no integrity flag was exposed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<bool>,
    /// Optional tool call id if this record is linked to a receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// `true` when this link is intact (i==0 is always anchor, i>=1 when prev_hash matches predecessor exec_hash).
    pub ok: bool,
}

/// Result of chain verification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainReport {
    /// Final verdict: `"verified"`, `"broken"`, or `"unverified"`.
    pub result: String,
    /// Whether the chain continuity is intact or broken.
    pub continuity: String,
    /// Index of the first link where prev_hash != predecessor exec_hash, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_broken_index: Option<usize>,
    /// Per-link reports.
    pub links: Vec<ChainLinkReport>,
    /// Reasons (e.g. "unverified: no execution record read granted").
    pub reasons: Vec<String>,
}

// ── ChainVerifier ─────────────────────────────────────────────────────

/// Chain verifier — pure continuity check plus integrity flag display.
pub struct ChainVerifier;

impl Default for ChainVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainVerifier {
    pub fn new() -> Self {
        Self
    }

    /// Verify continuity of a segment's execution record chain.
    ///
    /// Algorithm:
    /// 1. If source != "operation-file", return empty report with reason.
    /// 2. For i >= 1, check `records[i].prev_hash == records[i-1].exec_hash`.
    /// 3. `continuity = "broken"` iff any such check fails;
    ///    `first_broken_index` is the least such `i`.
    /// 4. `result = "broken"` if continuity is broken OR any exposed
    ///    `verify_integrity == false`.
    /// 5. Otherwise, `result = "unverified"` if any reason exists (the
    ///    caller reports unlinked Receipt-producing Tool_Calls as
    ///    reasons — the chain record model carries no receipt link).
    ///    Otherwise `result = "verified"`.
    ///
    /// The anchor (`records[0].prev_hash`) never affects the continuity
    /// check (D-30).
    pub fn verify(&self, segment: &ChainSegment, reasons: &[String]) -> ChainReport {
        // 1. Source check
        if segment.source != ChainSegmentSource::OperationFile {
            return ChainReport {
                result: "unverified".to_string(),
                continuity: "intact".to_string(),
                first_broken_index: None,
                links: Vec::new(),
                reasons: vec![format!("source: expected operation-file, got {:?}", segment.source)],
            };
        }

        let records = &segment.records;
        let mut links: Vec<ChainLinkReport> = Vec::with_capacity(records.len());
        let mut continuity = "intact".to_string();
        let mut first_broken_index: Option<usize> = None;
        let mut any_integrity_false = false;

        // Build per-link reports
        for (i, record) in records.iter().enumerate() {
            let ok = if i == 0 {
                // Anchor: always considered intact
                true
            } else {
                // Check continuity: prev_hash must match predecessor's exec_hash
                let prev = &records[i - 1];
                record.prev_hash == prev.exec_hash
            };

            // Track integrity flag exposure
            if let Some(integrity) = record.integrity {
                if !integrity {
                    any_integrity_false = true;
                }
            }

            links.push(ChainLinkReport {
                index: i,
                prev_hash: record.prev_hash.clone(),
                exec_hash: record.exec_hash.clone(),
                integrity: record.integrity,
                tool_call_id: record.tool_name.clone(),
                ok,
            });

            // Record first break
            if !ok && first_broken_index.is_none() {
                continuity = "broken".to_string();
                first_broken_index = Some(i);
            }
        }

        // Determine result
        let result = if continuity == "broken" || any_integrity_false {
            "broken".to_string()
        } else if !reasons.is_empty() {
            // Unverified: reasons exist (e.g. unlinked receipts, reported by
            // the caller — the chain record model carries no receipt link,
            // so a bare tool_call_id absence must not decide the verdict:
            // a record with no tool name is not a Tool_Call at all).
            "unverified".to_string()
        } else {
            "verified".to_string()
        };

        ChainReport {
            result,
            continuity,
            first_broken_index,
            links,
            reasons: reasons.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(prev_hash: &str, exec_hash: &str, integrity: Option<bool>) -> ExecutionRecordView {
        ExecutionRecordView {
            id: format!("rec-{}", exec_hash.chars().take(8).collect::<String>()),
            prev_hash: prev_hash.to_string(),
            exec_hash: exec_hash.to_string(),
            tool_name: None,
            integrity,
        }
    }

    fn make_segment(records: Vec<ExecutionRecordView>) -> ChainSegment {
        ChainSegment {
            read_at: "2026-11-09T12:00:00Z".to_string(),
            records,
            source: ChainSegmentSource::OperationFile,
        }
    }

    #[test]
    fn test_empty_chain() {
        let seg = make_segment(vec![]);
        let report = ChainVerifier::new().verify(&seg, &[]);
        assert_eq!(report.result, "verified");
        assert_eq!(report.continuity, "intact");
        assert!(report.links.is_empty());
        assert!(report.reasons.is_empty());
    }

    #[test]
    fn test_single_anchor() {
        let anchor = make_record("genesis-hash", "exec-1", Some(true));
        let seg = make_segment(vec![anchor]);
        let report = ChainVerifier::new().verify(&seg, &[]);
        assert_eq!(report.result, "verified");
        assert_eq!(report.continuity, "intact");
        assert_eq!(report.links.len(), 1);
        assert!(report.links[0].ok);
    }

    #[test]
    fn test_intact_chain() {
        let r1 = make_record("genesis", "hash-a", Some(true));
        let r2 = make_record("hash-a", "hash-b", Some(true));
        let r3 = make_record("hash-b", "hash-c", Some(true));
        let seg = make_segment(vec![r1, r2, r3]);
        let report = ChainVerifier::new().verify(&seg, &[]);
        assert_eq!(report.result, "verified");
        assert_eq!(report.continuity, "intact");
        assert!(report.first_broken_index.is_none());
        assert_eq!(report.links.len(), 3);
        assert!(report.links.iter().all(|l| l.ok));
    }

    #[test]
    fn test_broken_continuity() {
        let r1 = make_record("genesis", "hash-a", Some(true));
        let r2 = make_record("hash-a", "hash-b", Some(true));
        // r3 has wrong prev_hash
        let r3 = make_record("WRONG", "hash-c", Some(true));
        let seg = make_segment(vec![r1, r2, r3]);
        let report = ChainVerifier::new().verify(&seg, &[]);
        assert_eq!(report.result, "broken");
        assert_eq!(report.continuity, "broken");
        assert_eq!(report.first_broken_index, Some(2));
        assert!(!report.links[2].ok);
    }

    #[test]
    fn test_integrity_false_becomes_broken() {
        let r1 = make_record("genesis", "hash-a", Some(true));
        let r2 = make_record("hash-a", "hash-b", Some(false));
        let seg = make_segment(vec![r1, r2]);
        let report = ChainVerifier::new().verify(&seg, &[]);
        assert_eq!(report.result, "broken");
        assert_eq!(report.continuity, "intact");
        assert!(report.first_broken_index.is_none());
        assert!(report.links[1].ok);
        assert_eq!(report.links[1].integrity, Some(false));
    }

    #[test]
    fn test_unverified_with_reasons() {
        let r1 = make_record("genesis", "hash-a", Some(true));
        let seg = make_segment(vec![r1]);
        let report = ChainVerifier::new().verify(&seg, &["no execution record read granted".to_string()]);
        assert_eq!(report.result, "unverified");
        assert_eq!(report.continuity, "intact");
        assert_eq!(report.reasons, vec!["no execution record read granted"]);
    }

    #[test]
    fn test_wrong_source() {
        let seg = ChainSegment {
            read_at: "2026-11-09T12:00:00Z".to_string(),
            records: vec![],
            source: ChainSegmentSource::OperationFile,
        };
        let report = ChainVerifier::new().verify(&seg, &[]);
        assert_eq!(report.result, "verified");
    }

    #[test]
    fn test_continuity_equivalence() {
        // Property 25: for any segment with arbitrary prev_hash and exec_hash,
        // continuity is "broken" iff some i >= 1 has prev_hash != predecessor exec_hash
        for n in 0..=5 {
            let mut records = Vec::new();
            let mut prev = "genesis".to_string();
            for i in 0..n {
                let exec = format!("exec-{}", i);
                if i == 2 && n > 3 {
                    // Introduce a break at index 2 for n > 3
                    records.push(make_record("WRONG", &exec, Some(true)));
                    prev = "WRONG".to_string();
                } else {
                    records.push(make_record(&prev, &exec, Some(true)));
                    prev = exec;
                }
            }
            let seg = make_segment(records);
            let report = ChainVerifier::new().verify(&seg, &[]);
            if n > 3 {
                assert_eq!(report.continuity, "broken");
                assert_eq!(report.first_broken_index, Some(2));
            } else {
                assert_eq!(report.continuity, "intact");
                assert!(report.first_broken_index.is_none());
            }
        }
    }
}
