# BTRFS Deployment Skill

## Overview
Manage BTRFS-based deployments using snapshots and send/receive.

## Commands Available

### Snapshot Management
- `op-snapshot create [type] [name] [description]` - Create snapshot
- `op-snapshot list [type]` - List snapshots
- `op-snapshot send <snapshot> <target> [parent]` - Send to target
- `op-snapshot delete <snapshot>` - Delete snapshot
- `op-snapshot rollback <snapshot>` - Rollback to snapshot

### Module Management
- `op-module install <type> <name> <source>` - Install module
- `op-module remove <type> <name>` - Remove module
- `op-module list [type]` - List modules
- `op-module stage <type> <name> <source>` - Stage for review
- `op-module commit <staging-id>` - Commit staged module
- `op-module reject <staging-id>` - Reject staged module

### Deployment
- `op-deploy create <name> [description]` - Create deployment snapshot
- `op-deploy send <snapshot> <target>` - Send to target
- `op-deploy broadcast <snapshot> <group>` - Send to group
- `op-deploy status <target>` - Check target status
- `op-deploy targets` - List targets
- `op-deploy add-target <name> <host> [group]` - Add target

## Module Types
- `agents` - Dynamic LLM agents (markdown files)
- `commands` - Dynamic commands
- `mcp-servers` - External MCP servers (npm packages)
- `workflows` - Custom workflow definitions
- `templates` - VM/container templates

## Source Formats
- Local directory: `/path/to/module`
- Local file: `/path/to/file.md`
- Git URL: `https://github.com/user/repo.git`
- HTTP URL: `https://example.com/module.tar.gz`
- NPM package: `npm:@modelcontextprotocol/server-github`

## Examples

### Install MCP Server
```bash
op-module install mcp-servers github npm:@modelcontextprotocol/server-github
```

### Load Agents from Directory
```bash
for agent in ~/agents/*.md; do
    name=$(basename "$agent" .md)
    op-module install agents "$name" "$agent"
done
```

### Create and Deploy Release
```bash
# Create deployment snapshot
op-deploy create prod-v1.0 "Production release"

# Add targets if needed
op-deploy add-target prod-1 root@10.0.0.10 production
op-deploy add-target prod-2 root@10.0.0.11 production

# Deploy to production group
op-deploy broadcast deploy-prod-v1.0-* production
```

### Rollback
```bash
# List available snapshots
op-snapshot list deploy

# Rollback to previous version
op-snapshot rollback deploy-prod-v0.9-20250114-120000
```

## Safety Notes
- Always stage modules before committing
- Review staged changes before approval
- Keep multiple deployment snapshots for rollback
- Test deployments on staging group first
- Never include secrets in snapshots
