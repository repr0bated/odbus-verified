# Shared 3tched resources

cognitive-mcp links to the existing cognitive MCP plugin, including the declared
agent_* operations. agent-config links to the existing agent configuration owner.
Use their current schemas and admitted operations. These links do not create a
second agent registry, start agents, or grant tools.

Discover current operations through this identity's mounted 3tchedFS namespace
at /run/mount/3tchedFS. Verify supported paths and method schemas there rather
than assuming a copied catalog matches the running service.

Keep shared resources free of personal records, private conversations and secrets.
Changes to an accepted shared base require a new base and deliberate migration.
