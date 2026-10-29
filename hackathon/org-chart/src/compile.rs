//! The compiler: chart facts in, effective capabilities out.
//!
//! Rules: explicit roles only (no inheritance); unknown roles refuse the
//! position; missing attributes skip with reason; reporting cycles and
//! depth beyond [`MAX_DEPTH`] refuse the whole chart.

use crate::model::OrgChart;
use std::collections::{BTreeMap, BTreeSet};

/// Maximum reporting depth. Deeper charts refuse as malformed.
pub const MAX_DEPTH: usize = 32;

/// Maximum positions at any one reporting level. Bounds generated-object
/// fan-out per level (departments, managers, whatever the chart holds):
/// injected bulk positions refuse the whole chart. Tunable per deployment.
pub const MAX_POSITIONS_PER_LEVEL: usize = 256;

/// One derived grant, with provenance for the "why do I hold this" view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub member_key: String,
    pub capability: String,
    pub via_role: String,
    pub via_position: String,
}

/// A grant withheld, with its reason. Never silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    pub member_key: String,
    pub capability: String,
    pub via_role: String,
    pub via_position: String,
    pub reason: String,
}

/// Compiler output: grants plus skips, pinned to the sealed catalog hash
/// the compile verified against. Empty chart compiles to empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompileOutput {
    pub grants: Vec<Grant>,
    pub skips: Vec<Skip>,
    /// Hex of the sealed catalog (`opblob` catalog hash) at compile time,
    /// or `None` for an unpinned compile. `verify_seal` enforces the pin.
    pub sealed_catalog_hash: Option<String>,
}

/// Whole-chart refusal: the chart itself is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChartError {
    UnknownRole { position: String, role: String },
    Cycle { position: String },
    TooDeep { position: String },
    TooManyAtLevel { level: usize, count: usize },
    DuplicatePosition { position: String },
}

/// Seal verification failure: recompile against the live catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SealError {
    Unpinned,
    Mismatch { expected: String, live: String },
}

/// Refuse unless the output's pinned hash equals the live sealed catalog
/// hash. Detects catalog substitution or drift between compile and use.
/// Unpinned outputs always refuse: pin or recompile.
pub fn verify_seal(output: &CompileOutput, live_hash: &str) -> Result<(), SealError> {
    match &output.sealed_catalog_hash {
        None => Err(SealError::Unpinned),
        Some(expected) if expected == live_hash => Ok(()),
        Some(expected) => Err(SealError::Mismatch {
            expected: expected.clone(),
            live: live_hash.to_string(),
        }),
    }
}

fn check_structure(chart: &OrgChart) -> Result<(), ChartError> {
    let mut seen = BTreeSet::new();
    for p in &chart.positions {
        if !seen.insert(p.id.clone()) {
            return Err(ChartError::DuplicatePosition {
                position: p.id.clone(),
            });
        }
    }
    let ids: BTreeSet<&str> = chart.positions.iter().map(|p| p.id.as_str()).collect();
    let mut levels: BTreeMap<usize, usize> = BTreeMap::new();
    for p in &chart.positions {
        for role in &p.roles {
            if chart.role(role).is_none() {
                return Err(ChartError::UnknownRole {
                    position: p.id.clone(),
                    role: role.clone(),
                });
            }
        }
        // Walk the reporting chain: missing parents are roots; cycles and
        // over-depth chains refuse. Chain length is the position's level.
        let mut chain = BTreeSet::new();
        chain.insert(p.id.as_str());
        let mut current = p.reports_to.as_deref();
        let mut depth = 1;
        while let Some(id) = current {
            if !chain.insert(id) {
                return Err(ChartError::Cycle {
                    position: p.id.clone(),
                });
            }
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(ChartError::TooDeep {
                    position: p.id.clone(),
                });
            }
            current = chart
                .positions
                .iter()
                .find(|q| q.id == id)
                .and_then(|q| q.reports_to.as_deref());
            if !ids.contains(id) {
                break;
            }
        }
        *levels.entry(depth).or_insert(0) += 1;
    }
    for (level, count) in &levels {
        if *count > MAX_POSITIONS_PER_LEVEL {
            return Err(ChartError::TooManyAtLevel {
                level: *level,
                count: *count,
            });
        }
    }
    Ok(())
}
 
 /// Compile `(chart, ())` to effective grants. The profile argument is
 /// reserved: role capabilities already name sealed caps, so no profile
 /// filtering happens here — controls govern use at `decide()` time.
/// Unpinned: `sealed_catalog_hash` is `None`; `verify_seal` refuses it.
/// Prefer [`compile_pinned`] with the live sealed catalog hash.
 pub fn compile(chart: &OrgChart) -> Result<CompileOutput, ChartError> {
    compile_pinned(chart, None)
}

/// Compile pinned to a sealed catalog hash (`opblob` catalog hash hex).
/// `verify_seal` later refuses use under any other catalog.
pub fn compile_pinned(
    chart: &OrgChart,
    sealed_catalog_hash: Option<&str>,
) -> Result<CompileOutput, ChartError> {
    check_structure(chart)?;
    let mut out = CompileOutput {
        sealed_catalog_hash: sealed_catalog_hash.map(str::to_string),
        ..Default::default()
    };
    let mut granted: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    for position in &chart.positions {
        for role_id in &position.roles {
            let role = chart.role(role_id).expect("checked above");
            for member in &position.members {
                let missing: Vec<&String> = role
                    .requires_attributes
                    .iter()
                    .filter(|a| !member.attributes.contains(a))
                    .collect();
                for cap in &role.capabilities {
                    if missing.is_empty() {
                        // First writer wins deterministically (position order);
                        // provenance records where it came from.
                        granted
                            .entry((member.key.clone(), cap.clone()))
                            .or_insert((role.id.clone(), position.id.clone()));
                    } else {
                        out.skips.push(Skip {
                            member_key: member.key.clone(),
                            capability: cap.clone(),
                            via_role: role.id.clone(),
                            via_position: position.id.clone(),
                            reason: format!(
                                "missing attributes: {}",
                                missing
                                    .iter()
                                    .map(|s| s.as_str())
                                    .collect::<Vec<_>>()
                                    .join(",")
                            ),
                        });
                    }
                }
            }
        }
    }
    for ((member_key, capability), (via_role, via_position)) in granted {
        out.grants.push(Grant {
            member_key,
            capability,
            via_role,
            via_position,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Member, OrgChart, Position, Role};

    fn member(key: &str, attrs: &[&str]) -> Member {
        Member {
            key: key.into(),
            attributes: attrs.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn role(id: &str, caps: &[&str], attrs: &[&str]) -> Role {
        Role {
            id: id.into(),
            capabilities: caps.iter().map(|s| s.to_string()).collect(),
            requires_attributes: attrs.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn level_bound_refuses_bulk_injection() {
        let mut positions = Vec::new();
        for i in 0..=super::MAX_POSITIONS_PER_LEVEL {
            positions.push(Position {
                id: format!("bulk-{i}"),
                reports_to: None,
                roles: vec![],
                members: vec![],
            });
        }
        let chart = OrgChart {
            id: "bulk".into(),
            positions,
            roles: vec![],
        };
        assert_eq!(
            compile(&chart),
            Err(ChartError::TooManyAtLevel {
                level: 1,
                count: super::MAX_POSITIONS_PER_LEVEL + 1,
            })
        );
    }

    #[test]
    fn pin_carries_hash_and_verify_enforces_it() {
        use super::{compile_pinned, verify_seal, SealError};
        let chart = OrgChart {
            id: "pin".into(),
            positions: vec![],
            roles: vec![],
        };
        let pinned = compile_pinned(&chart, Some("abc123")).unwrap();
        assert_eq!(pinned.sealed_catalog_hash.as_deref(), Some("abc123"));
        assert!(verify_seal(&pinned, "abc123").is_ok());
        assert_eq!(
            verify_seal(&pinned, "deadbeef"),
            Err(SealError::Mismatch {
                expected: "abc123".into(),
                live: "deadbeef".into(),
            })
        );
        let unpinned = compile(&chart).unwrap();
        assert_eq!(verify_seal(&unpinned, "abc123"), Err(SealError::Unpinned));
    }

    #[test]
    fn empty_chart_compiles_empty() {
        let out = compile(&OrgChart {
            id: "empty".into(),
            positions: vec![],
            roles: vec![],
        })
        .unwrap();
        assert!(out.grants.is_empty() && out.skips.is_empty());
    }

    #[test]
    fn unknown_role_refuses_position() {
        let chart = OrgChart {
            id: "bad".into(),
            positions: vec![Position {
                id: "p".into(),
                reports_to: None,
                roles: vec!["ghost".into()],
                members: vec![member("k", &[])],
            }],
            roles: vec![],
        };
        assert_eq!(
            compile(&chart),
            Err(ChartError::UnknownRole {
                position: "p".into(),
                role: "ghost".into(),
            })
        );
    }

    #[test]
    fn cycle_refuses_chart() {
        let mk = |id: &str, parent: &str| Position {
            id: id.into(),
            reports_to: Some(parent.into()),
            roles: vec![],
            members: vec![],
        };
        let chart = OrgChart {
            id: "cycle".into(),
            positions: vec![mk("a", "b"), mk("b", "a")],
            roles: vec![],
        };
        assert!(matches!(compile(&chart), Err(ChartError::Cycle { .. })));
    }

    #[test]
    fn missing_attributes_skip_with_reason() {
        let chart = OrgChart {
            id: "gate".into(),
            positions: vec![Position {
                id: "p".into(),
                reports_to: None,
                roles: vec!["r".into()],
                members: vec![member("k", &[])],
            }],
            roles: vec![role("r", &["cap.x"], &["compartment-a"])],
        };
        let out = compile(&chart).unwrap();
        assert!(out.grants.is_empty());
        assert_eq!(out.skips.len(), 1);
        assert!(out.skips[0].reason.contains("compartment-a"));
    }

    #[test]
    fn duplicate_position_refuses() {
        let mk = || Position {
            id: "p".into(),
            reports_to: None,
            roles: vec![],
            members: vec![],
        };
        let chart = OrgChart {
            id: "dup".into(),
            positions: vec![mk(), mk()],
            roles: vec![],
        };
        assert!(matches!(
            compile(&chart),
            Err(ChartError::DuplicatePosition { .. })
        ));
    }
}
