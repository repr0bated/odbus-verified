# Shared 3tched resources

cognitive-mcp links to the existing cognitive MCP plugin, including the declared
agent_* operations. agent-config links to the existing agent configuration owner.
Use their current schemas and admitted operations. These links do not create a
second agent registry, start agents, or grant tools.

threetched-plugins links to the tched_plugins owner for installed extension
packages. Its methods map the threetched binary's plugin commands. Installation
and runtime selection are separate: the existing tched_router configuration
owner controls plugins.enabled, plugins.plugins_dir and signature policy.
Agent skill selection uses agents.<alias>.skill_bundles and the referenced
skill_bundles.<alias>.include/exclude settings through that same owner.

Discover current operations through this identity's mounted 3tchedFS namespace
at /run/mount/3tchedfs. Verify supported paths and method schemas there rather
than assuming a copied catalog matches the running service.

Keep shared resources free of personal records, private conversations and secrets.
Changes to an accepted shared base require a new base and deliberate migration.
