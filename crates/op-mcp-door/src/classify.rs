//! Capability classes (design "Classes"; Requirement 7).
//!
//! The class sits on top of each operation's sealed `required_capability`.
//! It is derived deterministically from the mounted metadata, so it can be
//! regenerated after every reseal without touching the blobs.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Public,
    Read,
    Write,
    Invoke,
    Exec,
    Shell,
    Network,
    Storage,
    Admin,
    Secret,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Public => "public",
            Class::Read => "read",
            Class::Write => "write",
            Class::Invoke => "invoke",
            Class::Exec => "exec",
            Class::Shell => "shell",
            Class::Network => "network",
            Class::Storage => "storage",
            Class::Admin => "admin",
            Class::Secret => "secret",
        }
    }

    /// Classes that need a per-call elevation ticket (Requirement 10.1).
    pub fn needs_ticket(self) -> bool {
        matches!(
            self,
            Class::Shell | Class::Exec | Class::Admin | Class::Secret
        )
    }

    /// Classes a toolset-scoped grant may cover (Requirement 9.6).
    pub fn toolset_grantable(self) -> bool {
        matches!(self, Class::Public | Class::Read | Class::Invoke)
    }

    /// Classes that may never appear in `core` or a default selection.
    pub fn never_default(self) -> bool {
        matches!(self, Class::Shell | Class::Exec)
    }
}

/// Tools that may become `public` if they also pass every other check.
pub const PUBLIC_ALLOW_LIST: &[(&str, &str)] =
    &[("cognitive", "schema_read"), ("cognitive", "oscal_subids")];

const COMMAND_ARGS: &[&str] = &["command", "cmd", "script", "shell_command", "argv"];
const SECRET_PLUGINS: &[&str] = &["keypair", "adc", "gcloud_adc"];
const SECRET_SUBSTRINGS: &[&str] = &[
    "auth_token",
    "refresh_auth",
    "setup_auth",
    "reauth",
    "save_auth",
    "credential",
    "secret",
    "password",
    "private_key",
];
const ADMIN_PLUGINS: &[&str] = &[
    "login1",
    "users",
    "human_principal",
    "identity_sled",
    "keyring",
    "elevation",
];
const EXEC_PLUGINS: &[&str] = &["incus", "oci", "cron", "persona", "workflows", "lxd"];
const NETWORK_PLUGINS: &[&str] = &[
    "net",
    "rtnetlink",
    "ovsdb_bridge",
    "rovs_commands",
    "wireguard",
    "wg_opdbus",
    "wgcf",
    "xray",
    "dnsresolver",
    "privacy_routes",
    "routing_decision",
    "proxy_server",
    "tunnel",
];
const STORAGE_PLUGINS: &[&str] = &["btrfs", "incus", "lxd", "fuse", "oci"];
const INVOKE_PLUGINS: &[&str] = &["cognitive", "notebooklm"];
const INVOKE_METHODS: &[&str] = &[
    "sequential_thinking",
    "context_manager",
    "specialist",
    "gemini_query",
    "rust_pro",
    "python_pro",
    "typescript_pro",
    "code_review_orchestrator",
    "notebook_query",
    "notebook_query_start",
    "cross_notebook_query",
    "workflow_query",
];
const AGENT_INVOKE_VERBS: &[&str] = &[
    "_advise",
    "_analyze",
    "_consult",
    "_explain",
    "_help",
    "_recommend",
    "_review",
];
const AGENT_EXEC_VERBS: &[&str] = &["_exec", "_run_workflow", "_validate"];

/// Inputs the classifier looks at.
pub struct Facts<'a> {
    pub plugin: &'a str,
    pub method: &'a str,
    pub mutation: bool,
    pub idempotent: bool,
    /// Top-level argument names from `input.schema.json`.
    pub arg_names: &'a BTreeSet<String>,
}

/// Classify with the built-in public allow-list.
pub fn classify(facts: &Facts<'_>) -> Class {
    classify_with_allow_list(facts, PUBLIC_ALLOW_LIST)
}

/// Classify with an explicit public allow-list. The hard deny-list still wins.
pub fn classify_with_allow_list(facts: &Facts<'_>, allow: &[(&str, &str)]) -> Class {
    let base = base_class(facts);
    let allowed = allow
        .iter()
        .any(|(plugin, method)| *plugin == facts.plugin && *method == facts.method);
    if allowed && !facts.mutation && facts.idempotent && !public_forbidden(facts, base) {
        Class::Public
    } else {
        base
    }
}

/// The hard deny-list (Requirement 7.3). True means "never public".
pub fn public_forbidden(facts: &Facts<'_>, base: Class) -> bool {
    base != Class::Read
        || facts.mutation
        || facts.method.starts_with("memory_")
        || facts.plugin == "memory"
        || facts.plugin == "notebooklm"
        || has_command_arg(facts.arg_names)
}

pub fn has_command_arg(arg_names: &BTreeSet<String>) -> bool {
    arg_names
        .iter()
        .any(|name| COMMAND_ARGS.contains(&name.as_str()))
}

fn tokens(method: &str) -> impl Iterator<Item = &str> {
    method.split(['_', '-', '.']).filter(|t| !t.is_empty())
}

fn base_class(f: &Facts<'_>) -> Class {
    let method = f.method;
    let has_token = |set: &[&str]| tokens(method).any(|t| set.contains(&t));
    if SECRET_PLUGINS.contains(&f.plugin)
        || SECRET_SUBSTRINGS.iter().any(|s| method.contains(s))
        || has_token(&["adc"])
    {
        return Class::Secret;
    }
    if has_command_arg(f.arg_names) || has_token(&["bash", "shell"]) {
        return Class::Shell;
    }
    if (f.mutation && ADMIN_PLUGINS.contains(&f.plugin))
        || has_token(&["reboot", "poweroff", "shutdown", "halt"])
        || method.starts_with("kill_")
        || method.contains("grant")
        || method.contains("revoke")
        || method.contains("register_key")
        || (f.plugin == "lxd" && method.starts_with("auth"))
    {
        return Class::Admin;
    }
    let agent_exec =
        method.starts_with("agent_") && AGENT_EXEC_VERBS.iter().any(|v| method.ends_with(v));
    if agent_exec
        || method == "workflow_run"
        || (f.plugin == "cozo" && method == "run_query")
        || has_token(&["exec", "execute", "spawn"])
        || (EXEC_PLUGINS.contains(&f.plugin) && has_token(&["run", "kill"]))
    {
        return Class::Exec;
    }
    if f.mutation && (NETWORK_PLUGINS.contains(&f.plugin) || f.plugin.starts_with("openflow")) {
        return Class::Network;
    }
    if f.mutation && STORAGE_PLUGINS.contains(&f.plugin) {
        return Class::Storage;
    }
    let agent_invoke =
        method.starts_with("agent_") && AGENT_INVOKE_VERBS.iter().any(|v| method.ends_with(v));
    if agent_invoke || (INVOKE_PLUGINS.contains(&f.plugin) && INVOKE_METHODS.contains(&method)) {
        return Class::Invoke;
    }
    if f.mutation {
        Class::Write
    } else {
        Class::Read
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class_of(plugin: &str, method: &str, mutation: bool, args: &[&str]) -> Class {
        let names: BTreeSet<String> = args.iter().map(|a| (*a).to_owned()).collect();
        classify(&Facts {
            plugin,
            method,
            mutation,
            idempotent: !mutation,
            arg_names: &names,
        })
    }

    #[test]
    fn inventory_examples_land_in_the_audit_classes() {
        assert_eq!(
            class_of(
                "cognitive",
                "agent_bash_pro_exec",
                true,
                &["command", "cwd", "timeout"]
            ),
            Class::Shell
        );
        assert_eq!(
            class_of("incus", "exec_instance", true, &["name"]),
            Class::Exec
        );
        assert_eq!(class_of("oci", "run_container", true, &[]), Class::Exec);
        assert_eq!(class_of("oci", "kill", true, &[]), Class::Exec);
        assert_eq!(class_of("cron", "run_job", true, &[]), Class::Exec);
        assert_eq!(
            class_of("cozo", "run_query", true, &["script"]),
            Class::Shell
        );
        assert_eq!(class_of("cozo", "run_query", true, &["query"]), Class::Exec);
        assert_eq!(class_of("persona", "execute", true, &[]), Class::Exec);
        assert_eq!(
            class_of("cognitive", "workflow_run", true, &[]),
            Class::Exec
        );
        assert_eq!(
            class_of("cognitive", "agent_rust_pro_validate", false, &[]),
            Class::Exec
        );
        assert_eq!(class_of("login1", "reboot", true, &[]), Class::Admin);
        assert_eq!(class_of("login1", "kill_session", true, &[]), Class::Admin);
        assert_eq!(
            class_of("human_principal", "register_key", true, &[]),
            Class::Admin
        );
        assert_eq!(
            class_of("notebooklm", "save_auth_tokens", true, &[]),
            Class::Secret
        );
        assert_eq!(
            class_of("notebooklm", "refresh_auth", true, &[]),
            Class::Secret
        );
        assert_eq!(class_of("keypair", "get_public", false, &[]), Class::Secret);
        assert_eq!(class_of("wireguard", "set_peer", true, &[]), Class::Network);
        assert_eq!(
            class_of("openflow_obfuscation", "apply", true, &[]),
            Class::Network
        );
        assert_eq!(class_of("wireguard", "get_status", false, &[]), Class::Read);
        assert_eq!(
            class_of("btrfs", "create_subvolume", true, &[]),
            Class::Storage
        );
        assert_eq!(
            class_of("cognitive", "sequential_thinking", false, &[]),
            Class::Invoke
        );
        assert_eq!(
            class_of("cognitive", "agent_rust_pro_advise", false, &[]),
            Class::Invoke
        );
        assert_eq!(
            class_of("cognitive", "memory_store", true, &[]),
            Class::Write
        );
        assert_eq!(
            class_of("cognitive", "memory_recall", false, &[]),
            Class::Read
        );
        assert_eq!(
            class_of("cognitive", "code_search", false, &[]),
            Class::Read
        );
    }

    #[test]
    fn allow_list_makes_schema_tools_public() {
        assert_eq!(
            class_of("cognitive", "schema_read", false, &["plugin"]),
            Class::Public
        );
        assert_eq!(
            class_of("cognitive", "oscal_subids", false, &[]),
            Class::Public
        );
        // Same name in another plugin is not on the allow-list.
        assert_eq!(class_of("other", "schema_read", false, &[]), Class::Read);
    }

    #[test]
    fn hard_deny_list_beats_the_allow_list() {
        let names = BTreeSet::new();
        let command: BTreeSet<String> = ["command".to_owned()].into();
        let allow_everything: &[(&str, &str)] = &[
            ("cognitive", "memory_recall"),
            ("memory", "get_entry"),
            ("notebooklm", "notebook_list"),
            ("cognitive", "schema_read"),
            ("cognitive", "code_index"),
            ("cognitive", "sequential_thinking"),
        ];
        let check = |plugin: &str, method: &str, mutation: bool, args: &BTreeSet<String>| {
            classify_with_allow_list(
                &Facts {
                    plugin,
                    method,
                    mutation,
                    idempotent: true,
                    arg_names: args,
                },
                allow_everything,
            )
        };
        assert_eq!(
            check("cognitive", "memory_recall", false, &names),
            Class::Read
        );
        assert_eq!(check("memory", "get_entry", false, &names), Class::Read);
        assert_eq!(
            check("notebooklm", "notebook_list", false, &names),
            Class::Read
        );
        assert_eq!(
            check("cognitive", "schema_read", false, &command),
            Class::Shell
        );
        assert_eq!(check("cognitive", "code_index", true, &names), Class::Write);
        assert_eq!(
            check("cognitive", "sequential_thinking", false, &names),
            Class::Invoke
        );
        assert_eq!(
            check("cognitive", "schema_read", false, &names),
            Class::Public
        );
    }

    #[test]
    fn non_idempotent_reads_are_never_public() {
        let names = BTreeSet::new();
        let class = classify(&Facts {
            plugin: "cognitive",
            method: "schema_read",
            mutation: false,
            idempotent: false,
            arg_names: &names,
        });
        assert_eq!(class, Class::Read);
    }

    #[test]
    fn ticket_and_toolset_rules() {
        for class in [Class::Shell, Class::Exec, Class::Admin, Class::Secret] {
            assert!(class.needs_ticket());
            assert!(!class.toolset_grantable());
        }
        for class in [Class::Public, Class::Read, Class::Invoke] {
            assert!(!class.needs_ticket());
            assert!(class.toolset_grantable());
        }
        assert!(!Class::Write.toolset_grantable());
        assert!(Class::Shell.never_default() && Class::Exec.never_default());
    }
}
