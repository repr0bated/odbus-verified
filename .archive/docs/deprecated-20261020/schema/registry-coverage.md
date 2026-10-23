# Schema Registry Coverage

## Registry Component

`crates/op-state-store/src/plugin_schema.rs`

`SchemaRegistry` provides:

- Built-in plugin schema registration.
- State validation against plugin schema definitions.
- Template generation (`generate_template`) for materialization defaults.

## Extended Coverage

Registry now includes plugin-specific schemas for the broader plugin set, not only the original subset.

Examples of covered plugins:

- core: `lxc`, `net`, `openflow`, `dinit`, `systemd`, `privacy_router`, `netmaker`
- added: `adc`, `agent_config`, `config`, `dnsresolver`, `endpoint`, `full_system`, `gcloud_adc`, `hardware`, `keypair`, `keyring`, `login1`, `mcp`, `openflow_obfuscation`, `ovsdb_bridge`, `packagekit`, `pcidecl`, `privacy`, `proxmox`, `proxy_server`, `service`, `sess_decl`, `software`, `users`, `web_ui`, `wireguard`

## How Materialization Uses Registry

1. Look up plugin schema by plugin name.
2. Build template from schema defaults.
3. Deep-merge operator-provided values over template.
4. Use merged result for diff/apply.

For contract payloads, template is inserted under `tunable` while envelope defaults are filled.

## Current Practical Outcome

- New objects and updates propagate schema-defined defaults automatically.
- Plugin-specific default shapes are available for substantially more plugins.
- System behavior is closer to strict schema-as-code operation.
