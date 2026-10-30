# Plugin Catalog

This catalog summarizes current state plugins and their primary object domains.

## Core State Plugins

- `adc`: ADC configured state.
- `agent_config`: agent tool/model configuration.
- `config`: central configuration store.
- `dinit`: service runtime control via dinit.
- `systemd`: legacy/compat service schema alias.
- `dnsresolver`: DNS declaration state.
- `endpoint`: endpoint declarations.
- `full_system`: aggregate disaster-recovery snapshot.
- `gcloud_adc`: Google cloud auth material state.
- `hardware`: host hardware inventory.
- `keypair`: keypair declarations.
- `keyring`: secret service/keyring state.
- `login1`: runtime login sessions.
- `lxc`: container network/runtime declarations.
- `mail_server`: mail server container state and D-Bus registration.
- `mcp`: MCP server/tool-group config.
- `net`: network interfaces and tunables.
- `netmaker`: mesh membership/state.
- `openflow`: flow policy and bridge flow state.
- `openflow_obfuscation`: flow obfuscation policy.
- `ovsdb_bridge`: OVS bridge declarations.
- `packagekit`: declarative package state.
- `pcidecl`: PCI declaration state.
- `privacy`: privacy orchestration config.
- `privacy_router`: privacy tunnel topology config.
- `proxmox`: proxmox container declarations.
- `proxy_server`: proxy runtime config.
- `service`: service definitions (schema source for service management).
- `sess_decl`: session declarations.
- `software`: package inventory declarations.
- `users`: user/group declarations.
- `web_ui`: web UI tunables.
- `wireguard`: wireguard interface/peer declarations.

## Design Note

Plugins define the object domains. Schema controls shape and default propagation. Mutations should not bypass plugin + schema flow.
