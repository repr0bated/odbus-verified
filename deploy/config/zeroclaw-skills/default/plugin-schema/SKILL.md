---
name: plugin-schema
description: >-
  Drill into a sealed PluginSchema: fields, methods, and CallMethod dispatch for any plugin id.
author: 3tched
version: 0.1.0
category: control-plane
---

# Plugin Schema

Use the owner's identity-bound filesystem at `/run/mount/3tchedFS`. Discover
plugins under `plugins/`, read `plugins/<id>/schema.json`, and inspect each
declared operation's `input.schema.json` and `metadata.json`.

To trigger a D-Bus operation, open `plugins/<id>/operations/<method>/call`
for reading and writing, write its JSON arguments, fsync, seek to the start,
and read the terminal receipt from that same open handle. Inspect that handle
also after an fsync error: it contains the operation's diagnostic. Do not read
another caller's method-global result, retry an indeterminate mutation, or
treat successful writing as completion.

Existing identity grants and capability checks govern every call. Preserve
denials; never substitute an MCP/HTTP endpoint, another identity or a direct store
write. Use returned committed state and receipts as evidence. Do not invoke
chat/generation recursively from a generation tool turn. Native stateless
generation deliberately has no tools; this bundle applies to tool-bearing
agent turns.
