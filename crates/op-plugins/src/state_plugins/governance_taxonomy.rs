//! Base governance taxonomy plugin.
//! Defines org structure, roles, capabilities, and data scopes.
//! This is the pluggable layer — organizations load their own taxonomy.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Organization role definition.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OrgRole {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Capabilities this role grants
    pub capabilities: HashSet<String>,
    /// Data scopes this role can access
    pub data_scopes: HashSet<String>,
    /// Can this role delegate to other identities?
    pub can_delegate: bool,
    /// Parent role in hierarchy (if any)
    pub parent_role: Option<String>,
}

/// Capability definition (what operations can be performed).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Capability {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Schema operations this capability grants (e.g., "shell.execute", "state.read")
    pub operations: HashSet<String>,
    /// Is this capability restricted to org scope?
    pub org_scoped: bool,
    /// Requires audit logging?
    pub audit_required: bool,
}

/// Data scope definition (visibility levels).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataScope {
    /// Only visible to the identity itself
    SelfOnly,
    /// Visible within the identity's org/department
    OrgScoped,
    /// Visible org-wide
    OrgWide,
    /// Public
    Public,
}

impl DataScope {
    /// Can identity with scope A see data with scope B?
    pub fn can_see(&self, data_scope: &DataScope) -> bool {
        match (self, data_scope) {
            (_, DataScope::Public) => true,
            (DataScope::Public, _) => false,
            (DataScope::OrgWide, _) => true,
            (DataScope::OrgScoped, DataScope::OrgScoped) => true,
            (DataScope::OrgScoped, DataScope::SelfOnly) => false,
            (DataScope::SelfOnly, DataScope::SelfOnly) => true,
            _ => false,
        }
    }
}

/// Base taxonomy plugin definition.
/// Organizations load and customize this.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GovernanceTaxonomy {
    pub version: u32,
    pub org_name: String,
    pub org_id: String,

    /// Defined roles in this org
    pub roles: HashMap<String, OrgRole>,

    /// Defined capabilities
    pub capabilities: HashMap<String, Capability>,

    /// Root roles (no parent)
    pub root_roles: HashSet<String>,

    /// Data scope for different identity types
    pub default_data_scope: HashMap<String, DataScope>,
}

impl GovernanceTaxonomy {
    /// Create a minimal bootstrapping taxonomy.
    pub fn bootstrap() -> Self {
        let mut roles = HashMap::new();
        let mut capabilities = HashMap::new();

        // Bootstrap role: admin (full access)
        roles.insert(
            "admin".to_string(),
            OrgRole {
                id: "admin".to_string(),
                name: "Administrator".to_string(),
                description: "Full access, can delegate".to_string(),
                capabilities: [
                    "shell.execute",
                    "state.read",
                    "state.write",
                    "policy.read",
                    "policy.write",
                    "governance.manage",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
                data_scopes: [DataScope::OrgWide]
                    .iter()
                    .map(|s| format!("{:?}", s))
                    .collect(),
                can_delegate: true,
                parent_role: None,
            },
        );

        // Bootstrap role: user (limited access)
        roles.insert(
            "user".to_string(),
            OrgRole {
                id: "user".to_string(),
                name: "User".to_string(),
                description: "Limited access, read-only".to_string(),
                capabilities: ["state.read", "identity.self_read"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                data_scopes: [DataScope::SelfOnly]
                    .iter()
                    .map(|s| format!("{:?}", s))
                    .collect(),
                can_delegate: false,
                parent_role: None,
            },
        );

        // Bootstrap capability: shell execution
        capabilities.insert(
            "shell.execute".to_string(),
            Capability {
                id: "shell.execute".to_string(),
                name: "Shell Execution".to_string(),
                description: "Execute shell commands through lungs".to_string(),
                operations: ["lungs.call_shell".to_string()].iter().cloned().collect(),
                org_scoped: false,
                audit_required: true,
            },
        );

        // Bootstrap capability: state read
        capabilities.insert(
            "state.read".to_string(),
            Capability {
                id: "state.read".to_string(),
                name: "State Read".to_string(),
                description: "Read identity state and Cozo sled".to_string(),
                operations: ["lungs.call_read".to_string(), "cozo.query".to_string()]
                    .iter()
                    .cloned()
                    .collect(),
                org_scoped: true,
                audit_required: false,
            },
        );

        // Bootstrap capability: state write
        capabilities.insert(
            "state.write".to_string(),
            Capability {
                id: "state.write".to_string(),
                name: "State Write".to_string(),
                description: "Mutate identity state and Cozo sled".to_string(),
                operations: ["lungs.call_write".to_string(), "cozo.mutate".to_string()]
                    .iter()
                    .cloned()
                    .collect(),
                org_scoped: true,
                audit_required: true,
            },
        );

        GovernanceTaxonomy {
            version: 1,
            org_name: "Default Organization".to_string(),
            org_id: "org-default".to_string(),
            roles,
            capabilities,
            root_roles: ["admin".to_string(), "user".to_string()].iter().cloned().collect(),
            default_data_scope: [
                ("admin".to_string(), DataScope::OrgWide),
                ("user".to_string(), DataScope::SelfOnly),
            ]
            .iter()
            .cloned()
            .collect(),
        }
    }

    /// Check if role has capability
    pub fn role_has_capability(&self, role_id: &str, capability_id: &str) -> bool {
        if let Some(role) = self.roles.get(role_id) {
            role.capabilities.contains(capability_id)
        } else {
            false
        }
    }

    /// Get all capabilities (including inherited from parent role)
    pub fn role_all_capabilities(&self, role_id: &str) -> HashSet<String> {
        let mut caps = HashSet::new();
        let mut current = Some(role_id.to_string());

        while let Some(role_id) = current.take() {
            if let Some(role) = self.roles.get(&role_id) {
                caps.extend(role.capabilities.clone());
                current = role.parent_role.clone();
            }
        }

        caps
    }

    /// Get all data scopes (including inherited from parent role)
    pub fn role_all_data_scopes(&self, role_id: &str) -> HashSet<String> {
        let mut scopes = HashSet::new();
        let mut current = Some(role_id.to_string());

        while let Some(role_id) = current.take() {
            if let Some(role) = self.roles.get(&role_id) {
                scopes.extend(role.data_scopes.clone());
                current = role.parent_role.clone();
            }
        }

        scopes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootstrap_creates_valid_roles() {
        let taxonomy = GovernanceTaxonomy::bootstrap();
        assert!(taxonomy.roles.contains_key("admin"));
        assert!(taxonomy.roles.contains_key("user"));
    }

    #[test]
    fn test_role_has_capability() {
        let taxonomy = GovernanceTaxonomy::bootstrap();
        assert!(taxonomy.role_has_capability("admin", "shell.execute"));
        assert!(!taxonomy.role_has_capability("user", "shell.execute"));
        assert_eq!(taxonomy.role_all_capabilities("user"),
            HashSet::from(["state.read".to_string(), "identity.self_read".to_string()]));
        assert_eq!(taxonomy.role_all_data_scopes("user"),
            HashSet::from(["SelfOnly".to_string()]));
    }

    #[test]
    fn test_data_scope_visibility() {
        assert!(DataScope::OrgWide.can_see(&DataScope::Public));
        assert!(DataScope::OrgWide.can_see(&DataScope::OrgScoped));
        assert!(!DataScope::SelfOnly.can_see(&DataScope::OrgScoped));
        assert!(DataScope::SelfOnly.can_see(&DataScope::SelfOnly));
    }
}
