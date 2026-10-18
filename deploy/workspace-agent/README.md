# Shared agent workspace preparation

`populate.sh BASE_TOPLEVEL PREPARED_ASSETS` populates the six empty sibling
subvolumes of an unaccepted base. The assets use the member directories from
`deploy/config/agents/workspace`. It refuses populated destinations and leaves
rootfs `/agent/{memory,rules,shared,skills,soul,user}` mountpoints empty for the
external provisioner. Ordinary `/agent` contains the Markdown entrypoint links.

Each of memory, rules, shared, skills, soul and user also contains `host`, a
directory symlink to `/etc/3tched/<member>`. The host supplies six initially empty
directories under `/etc/3tched`; Incus mounts the parent read-only at the same
guest path. The directories' contents are deferred to the owner. These are
shared code/configuration defaults, not private namespace-memory or identity
records. Existing defaults and private data stay where they are; the links do
not move records or expose a host directory without its Incus mount.

The storage owner must prove there are no dependent leaves before preparing a
writable base and must restore its filesystem, block-device and mount read-only
state afterward. This helper does not mount, snapshot, clone, unseal, reprovision,
start services or change grants.

Memory links target the existing cognitive MCP memory operations in the guest's
identity-bound 3tchedFS mount at `/run/mount/3tchedfs`. Shared owner links expose
cognitive MCP agent operations and agent configuration. Skills link to the
existing tched-router owner; its current sealed interface does not expose native
SkillsService inventory/contents, so a skill projection remains unfinished.
Do not confuse GetTools with effective skill inventory or add a frozen duplicate.

`@agent_user/identity` is the private destination reserved for the matching
human or machine identity sled during provisioning. The shared base deliberately
contains no identity record. The per-session writer and its refresh/revocation
integration are still required; copying the global credential/state projections
would expose other identities and is not supported here.

No runtime is configured to consume the Markdown files by this script. No memory
call, agent execution, identity write or skill lookup is claimed by the file and
checksum checks. Live checks depend on completing correct identity provisioning
and guest-owned FUSE startup. The pinned native router currently uses host workspaces.
