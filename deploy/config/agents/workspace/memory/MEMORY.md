# Cognitive MCP memory

Durable memory belongs to the existing cognitive MCP memory system. This file
explains access; it is not a separate memory database or a synchronized export.

The memory_* links in this directory lead to this identity's 3tchedFS operation
directories. Read input.schema.json, submit through call, and inspect result and
error using the filesystem's documented invocation protocol. Available operations
include store, retrieve, recall, query, semantic query, delete and list namespaces.

The verified session, selected context and grants determine memory scope for
human and machine identities. Do not select another identity by changing a file
or supplying its namespace. Project memory additionally follows membership and
role. Credentials and identity sleds do not belong in memory entries.

The links become usable only after the correct identity's guest FUSE service
mounts /run/mount/3tchedfs. A missing mount, method or grant is an integration
issue; do not create a local fallback database or treat a local file edit as a
successful memory operation.
