# Adapter Workflow: From Documentation to Integrated Tool

This document describes the workflow for creating an introspection adapter for any external system (gcloud, Active Directory, Docker, etc.) and integrating it into the op-dbus tool system.

## Overview

The workflow that was used for the gcloud adapter:

```
┌─────────────────────────────────────────────────────────────────┐
│ 1. FEED DOCUMENTATION                                           │
│    Provide Claude with the external system's documentation,     │
│    reference material, or access to introspect the system       │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│ 2. DISCOVER HIGH-LEVEL SURFACES                                 │
│    Enumerate all introspectable objects/entry points:           │
│    - gcloud: top-level command groups (compute, storage, ...)   │
│    - D-Bus: list all services on the bus                        │
│    - LDAP: query rootDSE, list naming contexts                  │
│    - Docker: list containers, images, networks, volumes         │
│    This gives the "table of contents" for full introspection    │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│ 3. DESIGN SCHEMA STRUCTURES                                     │
│    Create Rust structs that represent the system's surface:     │
│    - Hierarchy/tree structure                                   │
│    - Commands/methods/operations                                │
│    - Parameters/flags/arguments                                 │
│    - Properties/attributes                                      │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│ 4. IMPLEMENT PARSER                                             │
│    Create a parser that can introspect the external system:     │
│    - Implement ObjectParser trait                               │
│    - Parse help output, API responses, or documentation         │
│    - Build the schema structures                                │
│    - Cache results for efficiency                               │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│ 5. CREATE TOOLS                                                 │
│    Wrap the adapter in tools for the ToolRegistry:              │
│    - List/search operations                                     │
│    - Introspect specific items                                  │
│    - Execute commands (if applicable)                           │
└─────────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────────┐
│ 6. REGISTER TOOLS                                               │
│    Wire tools into register_all_builtin_tools()                 │
└─────────────────────────────────────────────────────────────────┘
```

## Step 1: Feed Documentation

Provide Claude with comprehensive documentation about the external system:

- **CLI tools**: Help output, man pages, reference documentation
- **APIs**: OpenAPI specs, SDK documentation, protocol specs
- **Services**: D-Bus introspection XML, LDAP schemas, etc.

For gcloud, Claude was given access to run `gcloud --help` recursively to discover the entire command hierarchy.

## Step 2: Discover High-Level Surfaces

Before deep introspection, enumerate all introspectable entry points. This is the "table of contents" that tells you what exists to introspect.

### Discovery Methods by System

| System | Discovery Command | What It Returns |
|--------|-------------------|-----------------|
| gcloud | `gcloud --help` | Top-level groups: compute, storage, container, iam, ... |
| D-Bus | `busctl list` | All services: org.freedesktop.UDisks2, org.freedesktop.login1, ... |
| LDAP | Query rootDSE | Naming contexts, supported controls, schema location |
| Docker | `docker info`, `docker ps -a` | Containers, images, networks, volumes |
| Kubernetes | `kubectl api-resources` | All resource types: pods, services, deployments, ... |
| Active Directory | LDAP rootDSE + `CN=Schema,CN=Configuration` | Domain info, all object classes, attributes |

### gcloud Discovery Example

```bash
$ gcloud --help
# GROUPS section lists all top-level surfaces:
#   access-approval, access-context-manager, active-directory,
#   ai, ai-platform, alloydb, anthos, api-gateway, apigee,
#   app, artifacts, asset, assured, auth, batch, bigtable,
#   billing, bms, builds, certificate-manager, cloud-shell,
#   composer, compute, config, container, data-catalog,
#   database-migration, dataflow, dataplex, dataproc,
#   datastore, datastream, deploy, deployment-manager,
#   dns, domains, edge-cache, edge-cloud, emulators,
#   endpoints, essential-contacts, eventarc, filestore,
#   firebase, firestore, functions, healthcare, iam,
#   identity, ids, immersive-stream, infra-manager,
#   kms, logging, looker, memcache, metastore, ml,
#   ml-engine, monitoring, netapp, network-connectivity,
#   network-management, network-security, network-services,
#   notebooks, org-policies, organizations, pam, policy-intelligence,
#   policy-troubleshoot, privateca, projects, publicca,
#   pubsub, recaptcha, recommender, redis, resource-manager,
#   resource-settings, run, scc, scheduler, secrets,
#   service-directory, services, source, spanner, sql,
#   storage, tasks, telco-automation, topic, transcoder,
#   transfer, vmware, workbench, workflows, workspace-add-ons, ...
```

This discovery step identifies ~100+ top-level groups, each of which will be recursively introspected in step 4.

### D-Bus Discovery Example

```bash
$ busctl --system list
# Returns all services on the system bus:
org.freedesktop.Accounts
org.freedesktop.DBus
org.freedesktop.UDisks2
org.freedesktop.login1
org.freedesktop.NetworkManager
org.freedesktop.PolicyKit1
org.freedesktop.systemd1
...
```

Each service is then introspected to discover its object paths, interfaces, methods, properties, and signals.

### Why Discovery Matters

1. **Scoping**: Know the full surface area before diving deep
2. **Incremental introspection**: Can introspect one group at a time
3. **Caching strategy**: Cache at the right granularity
4. **Progress tracking**: "Introspected 45/100 command groups"
5. **Schema design**: Informs what structures are needed

## Step 3: Design Schema Structures

Create Rust structs that capture the system's surface. Key patterns:

### Root Schema
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudSchema {
    pub schema_version: String,
    pub gcloud_version: String,
    pub account: Option<String>,
    pub hierarchy: GCloudCommand,      // The tree structure
    pub statistics: GCloudStats,       // Introspection metadata
}
```

### Hierarchical Items
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudCommand {
    pub name: String,
    pub full_path: String,
    pub description: String,
    pub is_group: bool,                          // Has children?
    pub flags: Vec<GCloudFlag>,                  // Parameters
    pub positional_args: Vec<GCloudArg>,         // Required args
    pub subcommands: HashMap<String, GCloudCommand>,  // Children
}
```

### Parameters/Flags
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GCloudFlag {
    pub name: String,
    pub short_name: Option<String>,
    pub description: String,
    pub required: bool,
    pub value_type: String,
    pub default: Option<String>,
    pub choices: Vec<String>,
}
```

## Step 4: Implement Parser

Create a parser that implements `ObjectParser` trait:

```rust
pub struct GCloudParser {
    cache: Arc<Mutex<HashMap<String, String>>>,
}

#[async_trait]
impl ObjectParser for GCloudParser {
    async fn parse(&self, input: &InspectionInput) -> Result<ParsedObject> {
        // 1. Extract parameters from input
        // 2. Run introspection (e.g., gcloud --help)
        // 3. Parse output into schema structures
        // 4. Return ParsedObject with data and schema
    }
}
```

### Introspection Strategy

For gcloud, the parser:
1. Runs `gcloud [command_path] --help`
2. Parses the help text with regex to extract:
   - GROUPS section → subcommand groups
   - COMMANDS section → leaf commands
   - FLAGS section → available flags
   - DESCRIPTION section → command description
3. Recursively introspects subcommands up to max_depth
4. Caches results to avoid redundant calls

```rust
async fn introspect_command(
    &self,
    command_path: &[String],
    depth: usize,
    max_depth: usize,
) -> Result<GCloudCommand> {
    let help = self.run_help(command_path).await?;

    let groups = self.parse_groups(&help);
    let commands = self.parse_commands(&help);
    let flags = self.parse_flags(&help);
    let description = self.parse_description(&help);

    // Recursively introspect children
    for group in groups {
        let sub_path = [command_path, &[group]].concat();
        let sub_cmd = self.introspect_command(&sub_path, depth + 1, max_depth).await?;
        cmd.subcommands.insert(group, sub_cmd);
    }

    Ok(cmd)
}
```

## Step 5: Create Tools (Integration Gap)

This is where gcloud integration is incomplete. Need to create tools in `op-tools/src/builtin/gcloud_tools.rs`:

```rust
pub async fn register_gcloud_tools(registry: &ToolRegistry) -> Result<()> {
    let parser = Arc::new(GCloudParser::new());

    registry.register_tool(Arc::new(GCloudIntrospectTool::new(parser.clone()))).await?;
    registry.register_tool(Arc::new(GCloudSearchTool::new(parser.clone()))).await?;
    registry.register_tool(Arc::new(GCloudGetCommandTool::new(parser.clone()))).await?;

    Ok(())
}

struct GCloudIntrospectTool {
    parser: Arc<GCloudParser>,
}

#[async_trait]
impl Tool for GCloudIntrospectTool {
    fn name(&self) -> &str { "gcloud_introspect" }

    fn description(&self) -> &str {
        "Introspect gcloud CLI command hierarchy"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command_path": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command path to introspect (e.g., ['compute', 'instances'])"
                },
                "max_depth": {
                    "type": "integer",
                    "default": 3
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let schema = self.parser.introspect_full(max_depth).await?;
        Ok(serde_json::to_value(schema)?)
    }
}
```

## Step 6: Register Tools

Add to `op-tools/src/builtin/mod.rs`:

```rust
pub mod gcloud_tools;

pub async fn register_all_builtin_tools(registry: &ToolRegistry) -> Result<()> {
    // ... existing registrations ...

    // Register gcloud tools
    gcloud_tools::register_gcloud_tools(registry).await?;

    Ok(())
}
```

## Applying to Other Systems

### Active Directory / LDAP

```rust
pub struct LdapSchema {
    pub schema_version: String,
    pub base_dn: String,
    pub object_classes: HashMap<String, LdapObjectClass>,
    pub attribute_types: HashMap<String, LdapAttribute>,
}

pub struct LdapParser {
    // Connect to LDAP, query schema
    // Parse objectClass and attributeType definitions
}
```

### Docker

```rust
pub struct DockerSchema {
    pub containers: Vec<ContainerInfo>,
    pub images: Vec<ImageInfo>,
    pub networks: Vec<NetworkInfo>,
    pub volumes: Vec<VolumeInfo>,
}

pub struct DockerParser {
    // Run docker inspect, docker ps, etc.
    // Parse JSON output
}
```

### D-Bus (Already Done)

The D-Bus adapter in `op-introspection` follows this same pattern:
- `IntrospectionService` - the parser
- `ServiceScanner` - runs introspection
- `dbus_introspection.rs` - the tools (12 tools registered)

## File Locations

```
crates/
├── op-inspector/
│   └── src/
│       ├── lib.rs                    # Export adapters
│       ├── gcloud.rs                 # GCloud adapter (complete)
│       ├── ldap.rs                   # LDAP adapter (future)
│       └── introspective_gadget.rs   # Generic inspection framework
│
├── op-introspection/
│   └── src/
│       ├── lib.rs                    # D-Bus introspection service
│       └── scanner.rs                # D-Bus scanner
│
└── op-tools/
    └── src/
        └── builtin/
            ├── mod.rs                # Register all tools
            ├── dbus_introspection.rs # D-Bus tools (complete)
            └── gcloud_tools.rs       # GCloud tools (TODO)
```

## Summary

| Step | gcloud Status | D-Bus Status |
|------|---------------|--------------|
| 1. Documentation | Fed to Claude | Built-in introspection |
| 2. Discover surfaces | `gcloud --help` → 100+ groups | `busctl list` → all services |
| 3. Schema structs | `GCloudSchema`, `GCloudCommand`, etc. | `ObjectInfo`, `InterfaceInfo`, etc. |
| 4. Parser | `GCloudParser` | `IntrospectionService` |
| 5. Tools | **Missing** | 12 tools in `dbus_introspection.rs` |
| 6. Registration | **Missing** | In `register_all_builtin_tools()` |

The gcloud adapter is complete through step 4. Steps 5-6 need implementation to make it available to agents through the tool system.
