//! Chart facts: positions, members, roles. All placeholder data in tests.

use serde::{Deserialize, Serialize};

/// A person or service identity holding a position.
/// `key` is the stable member identifier (e.g. a WireGuard pubkey).
/// `attributes` are clearance/compartment markers the member holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub key: String,
    pub attributes: Vec<String>,
}

/// A named bundle of capabilities, gated by required attributes.
/// `capabilities` are exact sealed capability strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub capabilities: Vec<String>,
    pub requires_attributes: Vec<String>,
}

/// One node: explicit roles, explicit members. `reports_to` names the
/// parent position id, or `None` for the chart root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub id: String,
    pub reports_to: Option<String>,
    pub roles: Vec<String>,
    pub members: Vec<Member>,
}

/// The whole chart: positions plus the role catalog they reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgChart {
    pub id: String,
    pub positions: Vec<Position>,
    pub roles: Vec<Role>,
}

impl OrgChart {
    pub fn role(&self, id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_lookup() {
        let chart = OrgChart {
            id: "t".into(),
            positions: vec![],
            roles: vec![Role {
                id: "r".into(),
                capabilities: vec!["c".into()],
                requires_attributes: vec![],
            }],
        };
        assert!(chart.role("r").is_some());
        assert!(chart.role("nope").is_none());
    }
}
