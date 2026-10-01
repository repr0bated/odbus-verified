# Agent defaults and operator overrides

Seed defaults: `/usr/share/op-dbus/agents`

Operator files: host `/var/lib/opdbus-runtime/agents`, container `/opt/agents`

Incus disk device: `agents`, `shift=true`, `readonly=true`.

Edit shared files on the host. All attached containers see those edits.
The device configuration survives container restarts; the directory is ordinary
persistent host storage, outside the replaceable container root. Back it up with
the operator-managed runtime data.

The read-only OS seed supplies the binary, service template and these default
files together. Provisioning does not copy them into an identity's writable
overlay or the operator directory. Replacing the seed upgrades the defaults;
explicit operator overrides remain separate and must be reviewed for compatibility.
Mounting either directory does not configure a runtime to read `AGENTS.md`,
execute hooks, or index chats; each consumer must explicitly opt in.
