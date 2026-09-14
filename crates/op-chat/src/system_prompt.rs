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
const FIXED_BASE_PROMPT: &str = r#"3tched AI infrastructure platform — Artix Linux, runit service supervision (controlled with `sv`), Incus containers, OVS switching fabric, Netmaker WireGuard mesh.

Capabilities:
- **OVS management** via rovs suite (rovs-ovsdb, rovs-openflow) — bridges, ports, flows
- **Container orchestration** via Incus (`assistant`, `mail-3tched`)
- **Xray + gRPC-bridge on the HOST** — xray via the `gbr-xray` runit service; the operation.v1 gRPC server (StateSync) at `10.200.0.2:50051` is served on the host by `op-dbus`. The deprecated `wg-xray` Incus container is stopped.
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
| `/etc/runit/sv/<name>/run` | The service definition |
| `/etc/runit/runsvdir/default/<name>` | Symlink that enables it at boot |
| `/run/runit/service` | The tree `runsvdir -P` supervises |
| `/etc/runit/sv/<name>/down` | Marker: do not auto-start |

### Adding a new service (exact pattern)

Service sources live in `deploy/runit/` (git-tracked), then install to
`/etc/runit/sv/`. A service is one directory with a `run` script, plus an
optional `log/run` companion:

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

exec chpst -u jeremy /usr/local/bin/<binary>
```

`log/run` is `#!/bin/sh` + `exec svlogd -tt /var/log/sv/<name>`.

The process **must stay in the foreground** — runit supervises the process it
starts, so a daemonising binary gets restarted in a loop. Pass the binary's
no-fork flag when it has one.

**Deploy sequence:**
```bash
sudo install -Dm755 deploy/runit/<name>/run /etc/runit/sv/<name>/run
sudo ln -sfn /etc/runit/sv/<name> /etc/runit/runsvdir/default/<name>
sudo sv start <name>          # runsvdir picks up the symlink on its own
```

Deployment is **btrfs send/receive** of subvolume snapshots (see
`deploy/btrfs-layout.sh`) — never a hand-copy onto a live host. Publish a build
to the golden subvolume and to this host with:
```bash
sudo deploy/runit/build-golden.sh
```

**DO NOT:**
- edit `/run/runit/service/` directly — that is the supervisor's runtime view
- invoke `runsv` or `runsvdir` by hand — they belong to boot and console recovery
- expect `daemon-reload`-style steps — there is no database to reload

**Third-party installers that require systemd**: a `systemctl` shim is installed
at `/usr/local/bin/systemctl`. It maps systemd verbs onto `sv` and converts any
`.service` unit into `/etc/runit/sv/<name>/run` via
`/usr/local/sbin/systemd-unit-to-runit`. Let the installer call `systemctl`; do
not hand-translate its unit files.

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

/// Network topology specification (FIXED - NOT EDITABLE)
const FIXED_TOPOLOGY_SPEC: &str = r#"
## CURRENT NETWORK TOPOLOGY

### Host
```
OS:       Artix Linux (xanmod kernel), runit service supervision (`sv`)
eth0      148.113.204.83/32   Physical NIC, enslaved into ovsbr0
          Gateway: 148.113.204.1
```

### Interfaces
```
INTERFACE      ADDRESS              TYPE              PURPOSE
────────────────────────────────────────────────────────────────────────────
eth0           148.113.204.83/32   Physical          WAN uplink, enslaved into ovsbr0
netmaker       100.90.37.254/32    WireGuard         Netmaker mesh (privacy-mesh)
               10.0.0.0/24 scope
               100.90.37.0/24 scope
ovsbr0         (DOWN)              OVS bridge        Switching fabric (system datapath)
```

### OVS Bridge — ovsbr0
```
BRIDGE     DATAPATH   FAIL_MODE    DESCRIPTION
────────────────────────────────────────────────────────────
ovsbr0     system     standalone   Switching fabric
```
- Uplink: `eth0` enslaved via `op-ovsbr0-setup` (migrates management IP to/from bridge)
- Managed via rovs suite — no ovs-vsctl

### Port Naming Convention
```
PREFIX   VLAN   SUBNET            PURPOSE
──────────────────────────────────────────────────────────────
gb-      100    10.100.0.128/25   GhostBridge — privacy/encrypted traffic
ai-      200    10.200.0.128/25   AI/ML workloads
web-     200    10.200.1.128/25   Web service containers
db-      200    10.200.2.128/25   Database containers
mgr-     300    10.30.0.128/25    Management plane
ctl-     300    10.30.1.128/25    Control plane
mon-     300    10.30.2.128/25    Monitoring/observability
```

### Incus Containers
```
NAME           STATE     IP                    PURPOSE
──────────────────────────────────────────────────────────────────────
assistant      RUNNING   (mesh only)           AI workspace / chatbot
mail-3tched    ERROR     —                     Mail server (needs repair)
```
Note: the `wg-xray` container is DEPRECATED and stopped. Xray + the gRPC-bridge
now run on the host (xray via the `gbr-xray` runit service; operation.v1 gRPC at
`10.200.0.2:50051` served by `op-dbus`).

### Traffic Flow
```
GhostBridge (gb-*) → ovsbr0 → netmaker (WireGuard) → encrypted mesh
gRPC traffic       → ovsbr0 bridge IP → host op-dbus (10.200.0.1:50051)
Xray/privacy egress→ host gbr-xray service → internet
Mesh access        → netmaker 100.90.37.254 → WireGuard peers
```

### Socket Paths
```
SERVICE          SOCKET PATH                                        PROTOCOL
────────────────────────────────────────────────────────────────────────────
OVSDB            /usr/local/var/run/openvswitch/db.sock (primary)  JSON-RPC (rovs)
                 /var/run/openvswitch/db.sock (fallback)
D-Bus System     /var/run/dbus/system_bus_socket                   D-Bus
gRPC + MCP       0.0.0.0:8090 (op-grpc-bridge, TLS)                gRPC/HTTP2
Netmaker         /var/run/netclient/netclient.sock                  gRPC
OpenFlow         tcp:10.88.88.1:6653                               OpenFlow 1.3
```

### runit Services
```
SERVICE              PURPOSE
────────────────────────────────────────────────────────────
incus                Container runtime
op-dbus              Core D-Bus daemon
op-dbus-mirror       OVSDB/D-Bus state mirror
op-grpc-bridge       Authenticated gRPC, MCP, chat, and cognitive ingress
op-web-srv           Unified HTTP API + embedded UI (port 8080)
op-projection        Plugin state projection
ovs-vswitchd         OVS virtual switch daemon
nextdns              DNS-over-HTTPS (NextDNS)
dbus-srv             D-Bus system bus
fail2ban             Intrusion prevention
```

### Expected State
When properly configured:
- ovsbr0 UP with datapath=system, eth0 enslaved as a port
- management IP on ovsbr0 internal port when bridge is up
- netmaker UP, 100.90.37.254/32, WireGuard mesh active
- Xray + gRPC-bridge running on the HOST (gbr-xray runit service; operation.v1
  gRPC at 10.200.0.2:50051 served by op-dbus); the wg-xray container is stopped
- All OVS operations via rovs suite — never ovs-vsctl or ovsdb-client
- All service management via runit/`sv` — never systemctl, never s6
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
        if let Some(ref cached) = *cache {
            if cached.loaded_at.elapsed().as_secs() < 60 {
                return (cached.content.clone(), cached.loaded_from.clone());
            }
        }
    }

    // Try environment variable first
    if let Ok(content) = std::env::var("CUSTOM_SYSTEM_PROMPT") {
        if !content.is_empty() {
            let source = "environment:CUSTOM_SYSTEM_PROMPT".to_string();
            cache_prompt(&content, &source).await;
            return (content, source);
        }
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
    summary.push_str("- **Systemd**: dbus_systemd_list_units, dbus_systemd_get_unit, dbus_systemd_start, dbus_systemd_stop\n");
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
        assert!(fixed.contains("TOPOLOGY"));
    }

    #[tokio::test]
    async fn test_load_custom_prompt() {
        let (content, source) = load_custom_prompt().await;
        assert!(!content.is_empty());
        assert!(!source.is_empty());
    }
}
