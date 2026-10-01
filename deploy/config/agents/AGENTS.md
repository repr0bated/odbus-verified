# Agent defaults

These versioned defaults live at `/usr/share/op-dbus/agents` in the read-only
identity seed and upgrade with its executable. Read this file when an agent
runtime is configured to use the bundle. Provisioning files does not register
hooks or cause a model to load these instructions.

- `conf/`: shared, non-secret configuration and runtime integration notes.
- `hooks/`: reviewed hooks, enabled explicitly by the runtime that owns them.
- `scripts/`: reusable helpers, invoked explicitly for the current task.
- `docs/`: operational receipts and notes that can be searched later.

Explicit operator overrides and setup receipts live separately at `/opt/agents`,
bound read-only from host `/var/lib/opdbus-runtime/agents`. Consumers must choose
their override files explicitly; do not copy all defaults there, because stale
copies would hide seed upgrades. Keep private instructions, generated files and other
identity-specific data in the identity's existing persistent storage at
`/opt/run-mounts/persist`, with its existing access controls.

Use the identity's granted 3tchedFS methods for operations when its lungs are
mounted at `/opt/run-mounts/3tched`. A shared file or script does not grant a
capability. Preserve the caller's identity and use the existing admitted
D-Bus path when appropriate.

Conversation history remains in its existing scoped Cozo store. This bundle
does not copy private chat transcripts, mount a database or enable chat search
by itself. Retrieve history through the owning identity's supported methods.
