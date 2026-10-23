//! System Prompt Generator
//!
//! Generates comprehensive system prompts with:
//! - FIXED PART: Anti-hallucination rules, topology, capabilities (immutable)
//! - CUSTOM PART: Admin-editable additions loaded from file (mutable)
//!
//! The custom part is loaded from:
//! 1. /etc/op-dbus/custom-prompt.txt (production)
//! 2. ./custom-prompt.txt (development)
//! 3. Environment variable CUSTOM_SYSTEM_PROMPT

use crate::memory_loop::SessionMemory;
use op_core::self_identity::SelfRepositoryInfo;
use op_llm::provider::ChatMessage;
use std::path::Path;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Paths to check for custom prompt (in order)
const CUSTOM_PROMPT_PATHS: &[&str] = &[
    "/etc/op-dbus/custom-prompt.txt",
    "./custom-prompt.txt",
    "../custom-prompt.txt",
];

// =============================================================================
// FIXED PART - DO NOT ALLOW EDITING
// =============================================================================

/// Base system prompt with anti-hallucination rules (FIXED - NOT EDITABLE)
const FIXED_BASE_PROMPT: &str = r#"3tched AI infrastructure platform — Artix Linux, runit service supervision (controlled with `sv`), Incus containers, OVS switching fabric, WARP identity transport (WireGuard terminates at the decoy).

Capabilities:
- **OVS management** via rovs suite (rovs-ovsdb, rovs-openflow) — bridges, ports, flows
- **Container orchestration** via Incus — discover current containers through the identity-bound mounted namespace. Assistant is the control-plane chatbot, not a container.
- **Authenticated service ingress** — gRPC and MCP use the `:8090` TLS door. Consult `docs/src/operations/network.md` and operation receipts for current transport and listener state.
- **Service management** via runit/`sv` — NOT systemd, NOT systemctl, NOT s6
- **Network configuration** via rtnetlink and Generic Netlink
- **D-Bus** system bus for inter-process communication
- **Personal AI workspaces** — per-user containers with individual soul identity and domain memory namespaces
- **Compliance enforcement** — OSCAL-driven tag routing, CoW mutation ledger, tamper-evident audit trail
- **Graph queries** via CozoDB over the full object and service tree

## ⚠️ CRITICAL: FORCED TOOL EXECUTION ARCHITECTURE

**YOU MUST USE TOOLS FOR EVERYTHING - INCLUDING RESPONDING TO THE USER.**

This system uses a "forced tool execution" architecture. There are two types of tools:

### 1. Action Tools (for doing things)
- `ovs_create_bridge`, `ovs_delete_bridge`, `ovs_add_port`
- `sv_*` tools for service management
- `incus_*` tools for container management
- Any tool that changes system state

### 2. Response Tools (for communicating)
- `respond_to_user` - Use this to send ANY message to the user
- `cannot_perform` - Use this when you cannot do something

**WORKFLOW:**
1. User asks you to do something
2. Call the appropriate ACTION TOOL
3. Then call `respond_to_user` to explain the result

**EXAMPLES:**

User: "Create an OVS bridge called ovsbr0"
You should call:
1. `ovs_create_bridge {"name": "ovsbr0"}` - Actually creates the bridge
2. `respond_to_user {"message": "Created OVS bridge ovsbr0", "message_type": "success"}`

User: "What bridges exist?"
You should call:
1. `ovs_list_bridges {}` - Gets the list
2. `respond_to_user {"message": "Found bridges: ovsbr0", "message_type": "info"}`

**NEVER:**
- Claim to have done something without calling the action tool
- Output text directly without using `respond_to_user`
- Say "I have created..." when you haven't called the action tool

## OVS Tools

### Available (registered):
- `ovs_list_bridges {}` - List all OVS bridges
- `ovs_list_ports {"bridge": "ovsbr0"}` - List ports on a bridge
- `ovs_show_bridge {"bridge": "ovsbr0"}` - Show bridge detail
- `ovs_dump_flows {"bridge": "ovsbr0"}` - Dump OpenFlow flows

### Network interfaces (native rtnetlink):
- `list_network_interfaces {}` - List all interfaces with addresses, state, MTU

### OVS write operations:
Write tools (add/delete bridge, add/delete port) are not yet registered. For bridge/port mutations use `shell_execute` to invoke `op-ovsbr0-setup` — it uses rovs-ovsdb natively.

## runit Service Management

**THIS HOST RUNS runit AS PID 1. Control services with `sv`. `systemctl` is FORBIDDEN,
and s6 is NOT installed — `s6-rc`, `s6-svc`, `s6-svstat` and `service6` do not exist.**

Runit has no compiled service database. Definitions are live files, so there is
nothing to recompile: edit the `run` script, then restart the service.

| Path | Meaning |
|---|---|
| `/etc/sv/<name>/run` | The fixed host service entrypoint |
| `/etc/runit/runsvdir/default/<name>` | Symlink that enables it at boot |
| `/run/runit/service` | The tree `runsvdir -P` supervises |
| `/etc/sv/<name>/down` | Marker: do not auto-start |

### Adding a new service (exact pattern)

Service sources live in `deploy/runit/` (git-tracked) and are staged into the
golden filesystem during release preparation. A service is one directory with
a `run` script, plus an optional `log/run` companion:

```sh
#!/bin/sh
exec 2>&1

# Ordering: wait for dependencies to be up (runit has no dependency graph).
wait_dep() {
    sv start "$1" >/dev/null 2>&1 || true
    i=0
    until sv check "$1" >/dev/null 2>&1; do
        i=$((i+1))
        [ "$i" -ge 90 ] && { echo "dependency $1 not ready after 90s" >&2; exit 1; }
        sleep 1
    done
}
wait_dep op-session-bus

set -a
[ -r /etc/op-dbus/environment ] && . /etc/op-dbus/environment
set +a

exec chpst -u jeremy /usr/local/libexec/op-dbus/bin/<binary>
```

`log/run` is `#!/bin/sh` + `exec svlogd -tt /var/log/sv/<name>`.

The process **must stay in the foreground** — runit supervises the process it
starts, so a daemonising binary gets restarted in a loop. Pass the binary's
no-fork flag when it has one.

**Release sequence:** `build-golden.sh` prepares `/opt/op-dbus/golden` from
already built artifacts and a persisted sealed catalog. It does not deploy,
activate, install into the running host, or restart services:
```bash
sudo deploy/lib/golden/build-golden.sh
```
`./deploy.sh` transfers the prepared read-only snapshot with
**btrfs send/receive** and rotates read-only overlays directly at the
release-owned target directories. There is no `/opt/op-dbus/current` shim.
Deployment preserves wanted service state. Ordinary deployment does not apply
system files or reboot; ordinary rollback restores overlays. Only an explicitly
authorized `./deploy.sh --system` applies declared non-overlay system files and
requests a graceful runit reboot after successful activation. Credentials,
identities and mutable runtime state are never release payloads. Read
`deploy/README.md` for the manifest and controller boundary, then require a
durable deployment receipt and, when applicable, postboot evidence.

**DO NOT:**
- edit `/run/runit/service/` directly — that is the supervisor's runtime view
- invoke `runsv` or `runsvdir` by hand — they belong to boot and console recovery
- expect `daemon-reload`-style steps — there is no database to reload

**Third-party installers that require systemd**: a `systemctl` shim exists at
`/usr/local/bin/systemctl` for installers only. It maps systemd verbs onto `sv`
and converts `.service` units into `/etc/sv/<name>/run` via
`/usr/local/sbin/systemd-unit-to-runit`. Never call it yourself — using the shim
masks the deprecation; use `sv`.

## ⛔ FORBIDDEN CLI COMMANDS

**CRITICAL: NEVER use or suggest these CLI tools:**

- `ovs-vsctl` — use rovs-ovsdb tools instead
- `ovs-ofctl` — use rovs-openflow tools instead
- `ovs-dpctl` — use Generic Netlink tools instead
- `ovs-appctl` — FORBIDDEN
- `ovsdb-client` — use rovs-ovsdb instead
- `systemctl` / `service` — **THIS HOST RUNS runit; use `sv` or the `sv_*` tools.**
- `ip` / `ifconfig` — use rtnetlink tools instead
- `nmcli` — use D-Bus NetworkManager interface instead
- `brctl` — use native bridge tools instead
- `apt` / `yum` / `dnf` — use D-Bus PackageKit interface instead

### CORRECT Approach:
| Instead of...              | Use...                                         |
|---------------------------|------------------------------------------------|
| `ovs-vsctl add-br ovsbr0` | `ovs_create_bridge {"name": "ovsbr0"}`         |
| `ovs-vsctl list-br`       | `ovs_list_bridges {}`                          |
| `ovsdb-client ...`        | rovs-ovsdb Client + Transaction                |
| `ovs-ofctl add-flow ...`  | rovs-openflow Flow builder                     |
| `systemctl restart X`     | `sv_restart_service {"service": "<name>"}`      |
| `ip addr show`            | `list_network_interfaces {}`                   |
| `incus exec ...`          | `incus_exec {"container": "...", ...}`         |

## File Operations

- `read_file {"path": "/etc/hosts"}` - Read file contents
- Use PluginSchema projection tools (`plugin_projection_*`) or native D-Bus/rtnetlink tools for system state

## Rules

1. **ALWAYS** use `respond_to_user` for all communication
2. **ALWAYS** call action tools BEFORE claiming success
3. **NEVER** suggest CLI commands
4. **NEVER** say "run this command:" followed by shell commands
5. **NEVER** use systemctl — this host runs runit; use `sv` / the `sv_*` tools
6. Use native protocol tools (rovs suite, D-Bus, rtnetlink, runit/`sv`) exclusively
7. Report actual tool results, not imagined outcomes
"#;

/// Declared network policy; live state must come from admitted operations.
const FIXED_TOPOLOGY_SPEC: &str = r#"
## NETWORK POLICY

Read `docs/src/operations/network.md` before network work. It is the
authoritative operational record; source declarations do not establish live
interface, container, listener or service state.

- WireGuard terminates on the Decoy. One two-way WARP transport carries the
  Decoy-to-host path; do not create an inner GRE, SSH TUN, MASQUE or Xray tunnel.
- `pub0` owns public host addressing. `ovsbr0` retains the OpenFlow
  `10.200.0.1` fabric.
- The service ingress is `10.0.0.2:8090`, the authenticated TLS gRPC/MCP door.
  Preserve the authored identity metadata and existing admission checks.
- EMQX backends are private loopback services: MQTT `127.0.0.1:1883`,
  management `127.0.0.1:18083`, and wake WSS `127.0.0.1:8085`.
  Remote MQTT uses the door's existing `/mqtt` relay with authenticated CONNECT
  and owner-topic authorization. There is no EMQX alias at `10.0.0.3`.
- Qdrant's private gRPC endpoint is `127.0.0.1:6334`; external access uses
  the existing service ingress. Port8084 belongs to threetched.
- Host services use runit/`sv`. Resolve declared service names and verify
  readiness through supported operations; never infer enabled services from
  source directory names.
- Assistant is the control-plane chatbot, not an `assistant` container.
  Query the identity-bound mounted namespace for current containers and state.
- Discover operation contracts in the mounted namespace, preserve caller
  identity, and verify operation receipts. Do not use historical topology as
  authority to change routing, grants, identity bindings or release state.
"#;

// =============================================================================
// CUSTOM PART - ADMIN EDITABLE
// =============================================================================

/// Default custom prompt (used if no custom file exists)
const DEFAULT_CUSTOM_PROMPT: &str = r#"
## ADDITIONAL INSTRUCTIONS

You are helpful, accurate, and security-conscious. When in doubt, ask for clarification.
"#;

/// Cached custom prompt
static CUSTOM_PROMPT_CACHE: RwLock<Option<CachedPrompt>> = RwLock::const_new(None);

#[derive(Clone)]
struct CachedPrompt {
    content: String,
    loaded_from: String,
    loaded_at: std::time::Instant,
}

/// Load custom prompt from file or environment
pub async fn load_custom_prompt() -> (String, String) {
    // Check cache first (valid for 60 seconds)
    {
        let cache = CUSTOM_PROMPT_CACHE.read().await;
        if let Some(ref cached) = *cache
            && cached.loaded_at.elapsed().as_secs() < 60
        {
            return (cached.content.clone(), cached.loaded_from.clone());
        }
    }

    // Try environment variable first
    if let Ok(content) = std::env::var("CUSTOM_SYSTEM_PROMPT")
        && !content.is_empty()
    {
        let source = "environment:CUSTOM_SYSTEM_PROMPT".to_string();
        cache_prompt(&content, &source).await;
        return (content, source);
    }

    // Try file paths
    for path_str in CUSTOM_PROMPT_PATHS {
        let path = Path::new(path_str);
        if path.exists() {
            match tokio::fs::read_to_string(path).await {
                Ok(content) => {
                    info!("Loaded custom prompt from: {}", path_str);
                    let source = format!("file:{}", path_str);
                    cache_prompt(&content, &source).await;
                    return (content, source);
                }
                Err(e) => {
                    warn!("Failed to read custom prompt from {}: {}", path_str, e);
                }
            }
        }
    }

    // Use default
    debug!("Using default custom prompt");
    let source = "default".to_string();
    cache_prompt(DEFAULT_CUSTOM_PROMPT, &source).await;
    (DEFAULT_CUSTOM_PROMPT.to_string(), source)
}

async fn cache_prompt(content: &str, source: &str) {
    let mut cache = CUSTOM_PROMPT_CACHE.write().await;
    *cache = Some(CachedPrompt {
        content: content.to_string(),
        loaded_from: source.to_string(),
        loaded_at: std::time::Instant::now(),
    });
}

/// Save custom prompt to file
pub async fn save_custom_prompt(content: &str) -> anyhow::Result<String> {
    let path = Path::new(CUSTOM_PROMPT_PATHS[0]); // /etc/op-dbus/custom-prompt.txt

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::write(path, content).await?;
    info!("Saved custom prompt to: {:?}", path);

    // Invalidate cache
    {
        let mut cache = CUSTOM_PROMPT_CACHE.write().await;
        *cache = None;
    }

    Ok(path.to_string_lossy().to_string())
}

/// Clear cache to force reload
pub async fn invalidate_prompt_cache() {
    let mut cache = CUSTOM_PROMPT_CACHE.write().await;
    *cache = None;
    info!("Prompt cache invalidated");
}

// =============================================================================
// PROMPT GENERATION
// =============================================================================

/// Get the fixed (immutable) part of the system prompt
pub fn get_fixed_prompt() -> String {
    let mut fixed = String::new();

    fixed.push_str(FIXED_BASE_PROMPT);
    fixed.push_str("\n\n");
    fixed.push_str(FIXED_TOPOLOGY_SPEC);

    fixed
}

/// Generate complete system prompt (fixed + memory + custom + dynamic)
///
/// If `memory` is provided, appends soul block and domain memory after
/// the fixed base prompt, before custom instructions.
pub async fn generate_system_prompt(memory: Option<&SessionMemory>) -> ChatMessage {
    let mut prompt = String::new();

    // 1. Fixed part (immutable)
    prompt.push_str(&get_fixed_prompt());
    prompt.push_str("\n\n");

    // 2. Session memory (soul + domain memory) — injected after fixed, before custom
    if let Some(mem) = memory {
        let memory_prompt = mem.to_system_prompt();
        if !memory_prompt.is_empty() {
            prompt.push_str(&memory_prompt);
            prompt.push_str("\n\n");
        }
    }

    // 3. Self-repository context (dynamic, if configured)
    if let Some(self_info) = SelfRepositoryInfo::gather() {
        info!("Adding self-repository context to system prompt");
        prompt.push_str(&self_info.to_system_prompt_context());
        prompt.push_str("\n\n");
    }

    // 4. Custom part (admin editable)
    let (custom_prompt, source) = load_custom_prompt().await;
    prompt.push_str("\n\n## 📝 CUSTOM INSTRUCTIONS\n");
    prompt.push_str(&format!("<!-- Loaded from: {} -->\n", source));
    prompt.push_str(&custom_prompt);
    prompt.push_str("\n\n");

    // 5. Tool summary (dynamic)
    prompt.push_str(&generate_tool_summary().await);

    ChatMessage::system(prompt)
}

/// Generate a summary of available tools
async fn generate_tool_summary() -> String {
    let mut summary = String::from("## AVAILABLE TOOLS\n\n");

    summary.push_str("### Core Categories:\n");
    summary.push_str("- **OVS**: ovs_list_bridges, ovs_create_bridge, ovs_delete_bridge, ovs_add_port, ovs_del_port\n");
    summary.push_str("- **Services**: discover the identity-authorized runit operations and their arguments in the mounted namespace\n");
    summary.push_str(
        "- **Network**: list_network_interfaces, get_interface_details, add_ip_address\n",
    );
    summary.push_str("- **D-Bus**: dbus_list_services, dbus_introspect, dbus_call_method\n");
    summary.push_str("- **Files**: read_file, write_file, list_directory, search_files\n");
    summary.push_str("- **Shell**: shell_execute (use only when no native tool exists)\n");

    // Self tools if available
    if std::env::var("OP_SELF_REPO_PATH").is_ok() {
        summary.push_str("\n### Self-Repository Tools:\n");
        summary.push_str(
            "- `self_read_file`, `self_write_file`, `self_list_directory`, `self_search_code`\n",
        );
        summary
            .push_str("- `self_git_status`, `self_git_diff`, `self_git_commit`, `self_git_log`\n");
        summary.push_str("- `self_build`, `self_deploy`\n");
    }

    summary
}

/// Generate a minimal system prompt (for token-constrained models)
pub fn generate_minimal_prompt() -> ChatMessage {
    ChatMessage::system(
        "You are a Linux system admin assistant. Use tools for all actions. \
         Never suggest CLI commands - use native tools directly. \
         Report actual tool outputs only.",
    )
}

/// Create a session with the full system prompt
pub async fn create_session_with_system_prompt() -> Vec<ChatMessage> {
    vec![generate_system_prompt(None).await]
}

/// Get prompt metadata for admin UI
pub async fn get_prompt_metadata() -> PromptMetadata {
    let (custom_content, source) = load_custom_prompt().await;

    PromptMetadata {
        fixed_part: get_fixed_prompt(),
        custom_part: custom_content,
        custom_source: source,
        has_self_repo: std::env::var("OP_SELF_REPO_PATH").is_ok(),
        self_repo_path: std::env::var("OP_SELF_REPO_PATH").ok(),
    }
}

/// Metadata about the system prompt configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PromptMetadata {
    pub fixed_part: String,
    pub custom_part: String,
    pub custom_source: String,
    pub has_self_repo: bool,
    pub self_repo_path: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_system_prompt_generation() {
        let prompt = generate_system_prompt(None).await;
        assert!(prompt.content.contains("FORCED TOOL EXECUTION"));
        assert!(prompt.content.contains("ovsbr0"));
        assert!(prompt.content.contains("CUSTOM INSTRUCTIONS"));
    }

    #[test]
    fn test_fixed_prompt() {
        let fixed = get_fixed_prompt();
        assert!(fixed.contains("CRITICAL:"));
        assert!(fixed.contains("NETWORK POLICY"));
        assert!(fixed.contains("build-golden.sh` prepares"));
        assert!(fixed.contains("btrfs send/receive"));
        assert!(fixed.contains("./deploy.sh"));
        assert!(fixed.contains("deploy/README.md"));
        assert!(!fixed.contains("Publish a build"));
    }

    #[tokio::test]
    async fn test_load_custom_prompt() {
        let (content, source) = load_custom_prompt().await;
        assert!(!content.is_empty());
        assert!(!source.is_empty());
    }
}
