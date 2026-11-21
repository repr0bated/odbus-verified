//! The single authorization function (Requirement 8).
//!
//! `tools/list`, `search_tools`, `describe_tool`, `list_toolsets`,
//! `load_toolset`, `call_tool` and direct `tools/call` all call
//! [`Policy::authorize`]. Nothing else decides.

use crate::canonical::args_sha256;
use crate::catalog::Tool;
use crate::classify::Class;
use crate::grants::{DurableGrants, GrantQuery, GrantStore, TicketStore};
use serde_json::Value;
use std::collections::HashMap;

/// Who is calling, as far as policy is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// Verified principal id; `None` means unauthenticated.
    pub principal: Option<String>,
    /// Whether that principal has a mount mapped (Requirement 12.5).
    pub has_mount: bool,
}

impl Caller {
    pub fn anonymous() -> Self {
        Self {
            principal: None,
            has_mount: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Purpose<'a> {
    /// Listing, search, describe, load: visibility only.
    Discover,
    /// A real call. Consumes a ticket and a grant call on success.
    Call {
        args: &'a Value,
        confirm: bool,
        ticket: Option<&'a str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Basis {
    Public,
    Durable { capability: String },
    Ephemeral { grant_id: String },
}

impl Basis {
    pub fn label(&self) -> String {
        match self {
            Basis::Public => "public".into(),
            Basis::Durable { capability } => format!("durable:{capability}"),
            Basis::Ephemeral { grant_id } => format!("grant:{grant_id}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    Unauthenticated,
    NoMount,
    GrantsUnavailable,
    NotGranted,
    ConfirmationRequired,
    ElevationRequired,
}

impl DenyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DenyReason::Unauthenticated => "unauthenticated",
            DenyReason::NoMount => "no_mount",
            DenyReason::GrantsUnavailable => "grants_unavailable",
            DenyReason::NotGranted => "not_granted",
            DenyReason::ConfirmationRequired => "confirmation_required",
            DenyReason::ElevationRequired => "elevation_required",
        }
    }

    /// Denials that hide the tool entirely (vs. a visible tool whose call
    /// is missing confirmation or a ticket).
    pub fn hides(self) -> bool {
        !matches!(
            self,
            DenyReason::ConfirmationRequired | DenyReason::ElevationRequired
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Allowed {
    pub basis: Basis,
    pub ticket: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow(Allowed),
    Deny(DenyReason),
}

impl Decision {
    pub fn allowed(&self) -> bool {
        matches!(self, Decision::Allow(_))
    }
}

pub struct Policy {
    pub durable: DurableGrants,
    pub grants: GrantStore,
    pub tickets: TicketStore,
    /// Tool key → toolset ids containing it (for toolset-scoped grants).
    membership: HashMap<String, Vec<String>>,
}

impl Policy {
    pub fn new(
        durable: DurableGrants,
        grants: GrantStore,
        tickets: TicketStore,
        toolsets: &crate::toolsets::Toolsets,
    ) -> Self {
        let mut membership: HashMap<String, Vec<String>> = HashMap::new();
        for set in &toolsets.file.toolsets {
            for key in &set.tools {
                membership
                    .entry(key.clone())
                    .or_default()
                    .push(set.id.clone());
            }
        }
        Self {
            durable,
            grants,
            tickets,
            membership,
        }
    }

    /// Decide. `now` is unix seconds.
    pub fn authorize(
        &self,
        caller: &Caller,
        tool: &Tool,
        purpose: Purpose<'_>,
        now: i64,
    ) -> Decision {
        if tool.class == Class::Public {
            return Decision::Allow(Allowed {
                basis: Basis::Public,
                ticket: None,
            });
        }
        let Some(principal) = caller.principal.as_deref() else {
            return Decision::Deny(DenyReason::Unauthenticated);
        };
        if !caller.has_mount {
            return Decision::Deny(DenyReason::NoMount);
        }
        let capabilities = match self.durable.capabilities(principal) {
            Ok(caps) => caps,
            Err(_) => return Decision::Deny(DenyReason::GrantsUnavailable),
        };
        let key = tool.key();
        let no_sets = Vec::new();
        let args = match purpose {
            Purpose::Call { args, .. } => Some(args),
            Purpose::Discover => None,
        };
        let ephemeral = self.grants.find(&GrantQuery {
            principal,
            tool_key: &key,
            toolsets: self.membership.get(&key).unwrap_or(&no_sets),
            toolset_grantable: tool.class.toolset_grantable(),
            args,
            now,
        });
        let basis = match ephemeral {
            Some(grant) => Basis::Ephemeral {
                grant_id: grant.grant_id,
            },
            // A shell grant only ever comes from an ephemeral record (Req 9.7).
            None if tool.class == Class::Shell => return Decision::Deny(DenyReason::NotGranted),
            None => match &tool.required_capability {
                Some(cap) if capabilities.contains(cap) => Basis::Durable {
                    capability: cap.clone(),
                },
                _ => return Decision::Deny(DenyReason::NotGranted),
            },
        };
        let Purpose::Call {
            args,
            confirm,
            ticket,
        } = purpose
        else {
            return Decision::Allow(Allowed {
                basis,
                ticket: None,
            });
        };
        if tool.is_mutation() && !confirm {
            return Decision::Deny(DenyReason::ConfirmationRequired);
        }
        // Ticket first, then the grant count: both only after every check.
        let mut used_ticket = None;
        if tool.class.needs_ticket() {
            let Some(id) = ticket else {
                return Decision::Deny(DenyReason::ElevationRequired);
            };
            if self
                .tickets
                .consume(id, principal, &key, &args_sha256(args), now)
                .is_err()
            {
                return Decision::Deny(DenyReason::ElevationRequired);
            }
            used_ticket = Some(id.to_owned());
        }
        if let Basis::Ephemeral { grant_id } = &basis
            && self.grants.consume(grant_id, now).is_err()
        {
            return Decision::Deny(DenyReason::NotGranted);
        }
        Decision::Allow(Allowed {
            basis,
            ticket: used_ticket,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;
    use crate::catalog::fixture::write_op;
    use crate::grants::{EphemeralGrant, Ticket};
    use serde_json::json;

    struct Env {
        _dir: tempfile::TempDir,
        catalog: Catalog,
        policy: Policy,
        doc: std::path::PathBuf,
    }

    fn env() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        let none = json!({"type":"object","properties":{}});
        write_op(
            &mount,
            "cognitive",
            "schema_read",
            false,
            none.clone(),
            "cognitive.read",
        );
        write_op(
            &mount,
            "cognitive",
            "memory_recall",
            false,
            none.clone(),
            "cognitive.read",
        );
        write_op(
            &mount,
            "cognitive",
            "memory_store",
            true,
            none.clone(),
            "cognitive.write",
        );
        write_op(
            &mount,
            "cognitive",
            "agent_bash_pro_exec",
            true,
            json!({"type":"object","properties":{"command":{"type":"string"}}}),
            "cognitive.agent.bash_pro",
        );
        write_op(
            &mount,
            "incus",
            "exec_instance",
            true,
            none.clone(),
            "incus.exec",
        );
        write_op(
            &mount,
            "cognitive",
            "sequential_thinking",
            false,
            none,
            "cognitive.invoke",
        );
        let catalog = Catalog::scan(&mount).unwrap();
        let toolsets =
            crate::toolsets::Toolsets::new(crate::toolsets::generate(&catalog), &catalog);
        let doc = dir.path().join("grants.json");
        std::fs::write(&doc, json!({
            "$SUPERADMIN": {"capabilities": ["cognitive.agent.bash_pro", "incus.exec"]},
            "p": {"groups": ["$SUPERADMIN"], "capabilities": ["cognitive.read", "cognitive.write"]}
        }).to_string()).unwrap();
        let policy = Policy::new(
            DurableGrants::new(&doc),
            GrantStore::new(dir.path().join("state/grants")),
            TicketStore::new(dir.path().join("state/tickets")),
            &toolsets,
        );
        Env {
            _dir: dir,
            catalog,
            policy,
            doc,
        }
    }

    fn caller(p: &str) -> Caller {
        Caller {
            principal: Some(p.into()),
            has_mount: true,
        }
    }

    fn call<'a>(args: &'a Value, confirm: bool, ticket: Option<&'a str>) -> Purpose<'a> {
        Purpose::Call {
            args,
            confirm,
            ticket,
        }
    }

    #[test]
    fn anonymous_sees_only_public() {
        let e = env();
        let anon = Caller::anonymous();
        for tool in &e.catalog.tools {
            let allowed = e
                .policy
                .authorize(&anon, tool, Purpose::Discover, 150)
                .allowed();
            assert_eq!(allowed, tool.class == Class::Public, "{}", tool.key());
        }
        let schema = e.catalog.by_name("schema_read").unwrap();
        assert!(
            e.policy
                .authorize(&anon, schema, call(&json!({}), false, None), 150)
                .allowed()
        );
    }

    #[test]
    fn durable_capabilities_with_confirmation() {
        let e = env();
        let recall = e.catalog.by_name("memory_recall").unwrap();
        let store = e.catalog.by_name("memory_store").unwrap();
        let args = json!({});
        assert!(
            e.policy
                .authorize(&caller("p"), recall, call(&args, false, None), 150)
                .allowed()
        );
        assert_eq!(
            e.policy
                .authorize(&caller("q"), recall, Purpose::Discover, 150),
            Decision::Deny(DenyReason::NotGranted)
        );
        assert_eq!(
            e.policy
                .authorize(&caller("p"), store, call(&args, false, None), 150),
            Decision::Deny(DenyReason::ConfirmationRequired)
        );
        assert!(
            e.policy
                .authorize(&caller("p"), store, call(&args, true, None), 150)
                .allowed()
        );
        let no_mount = Caller {
            principal: Some("p".into()),
            has_mount: false,
        };
        assert_eq!(
            e.policy
                .authorize(&no_mount, recall, Purpose::Discover, 150),
            Decision::Deny(DenyReason::NoMount)
        );
    }

    #[test]
    fn shell_needs_an_ephemeral_grant_and_a_ticket() {
        let e = env();
        let shell = e.catalog.by_key("cognitive.agent_bash_pro_exec").unwrap();
        // The standing $SUPERADMIN capability is not enough for shell.
        assert_eq!(
            e.policy
                .authorize(&caller("p"), shell, Purpose::Discover, 150),
            Decision::Deny(DenyReason::NotGranted)
        );
        e.policy
            .grants
            .issue(&EphemeralGrant {
                grant_id: "g".into(),
                principal_id: "p".into(),
                tool: shell.key(),
                args_constraint: None,
                not_before: 100,
                expires_at: 200,
                max_calls: 1,
                issued_by: "t".into(),
                approval_ticket_ref: None,
            })
            .unwrap();
        let args = json!({"command":"uptime"});
        assert!(
            e.policy
                .authorize(&caller("p"), shell, Purpose::Discover, 150)
                .allowed()
        );
        assert_eq!(
            e.policy
                .authorize(&caller("p"), shell, call(&args, true, None), 150),
            Decision::Deny(DenyReason::ElevationRequired)
        );
        e.policy
            .tickets
            .issue(&Ticket {
                ticket_id: "t".into(),
                principal_id: "p".into(),
                tool: shell.key(),
                args_sha256: args_sha256(&json!({"command":"rm -rf /"})),
                issued_at: 140,
                expires_at: 300,
                issued_by: "t".into(),
            })
            .unwrap();
        assert_eq!(
            e.policy
                .authorize(&caller("p"), shell, call(&args, true, Some("t")), 150),
            Decision::Deny(DenyReason::ElevationRequired),
            "ticket bound to other arguments"
        );
        e.policy
            .tickets
            .issue(&Ticket {
                ticket_id: "t2".into(),
                principal_id: "p".into(),
                tool: shell.key(),
                args_sha256: args_sha256(&args),
                issued_at: 140,
                expires_at: 300,
                issued_by: "t".into(),
            })
            .unwrap();
        let decision = e
            .policy
            .authorize(&caller("p"), shell, call(&args, true, Some("t2")), 150);
        assert_eq!(
            decision,
            Decision::Allow(Allowed {
                basis: Basis::Ephemeral {
                    grant_id: "g".into()
                },
                ticket: Some("t2".into())
            })
        );
        // Grant had one call: now gone.
        assert!(
            !e.policy
                .authorize(&caller("p"), shell, Purpose::Discover, 150)
                .allowed()
        );
    }

    #[test]
    fn exec_accepts_durable_but_still_needs_a_ticket() {
        let e = env();
        let exec = e.catalog.by_key("incus.exec_instance").unwrap();
        assert!(
            e.policy
                .authorize(&caller("p"), exec, Purpose::Discover, 150)
                .allowed()
        );
        assert_eq!(
            e.policy
                .authorize(&caller("p"), exec, call(&json!({}), true, None), 150),
            Decision::Deny(DenyReason::ElevationRequired)
        );
    }

    #[test]
    fn unreadable_grant_document_leaves_only_public() {
        let e = env();
        std::fs::write(&e.doc, r#"{"*":{"capabilities":[]}}"#).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&e.doc)
            .unwrap()
            .set_modified(later)
            .unwrap();
        let recall = e.catalog.by_name("memory_recall").unwrap();
        assert_eq!(
            e.policy
                .authorize(&caller("p"), recall, Purpose::Discover, 150),
            Decision::Deny(DenyReason::GrantsUnavailable)
        );
        let schema = e.catalog.by_name("schema_read").unwrap();
        assert!(
            e.policy
                .authorize(&caller("p"), schema, Purpose::Discover, 150)
                .allowed()
        );
    }
}
