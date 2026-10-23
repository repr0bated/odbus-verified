# Skills owned by the 3tched runtime

Skills are managed by the existing tched-router runtime and its configured skill
bundles. Do not create a second registry by copying a frozen skill list here.

skills points to the identity-bound threetched plugin in 3tchedFS. Its sealed
contract derives from the verified native binary. Discover skills.list,
skills.bundle.list, skills.bundle.show and supported mutations through their
dotted operation declarations. Each operation requires its exact native
capability and mutation confirmation; retired tched_skills grants are not aliases.
Native arguments use positional arrays and declared CLI flags; results expose
stdout, stderr and exit rather than the retired text envelope. Bundle removal
requires --yes. To preserve the former install_skill output, select
--no-tier-banner explicitly. The contents-based edit_skill API is retired:
skills.edit and skills.add with --edit are unavailable in the noninteractive
executor. Do not discard contents or substitute an uncaptured content-write API.

runtime points separately to tched_router, the routing and configuration owner
backed by that same threetched binary. Neither link copies skill contents or
starts another runtime. A missing skills projection means the plugin needs
release integration; do not substitute GetTools for skill inventory.

Agent operations remain available through /agent/shared/cognitive-mcp. A skill
does not grant capabilities or override the owner. Runtime bundle loading from
this directory requires explicit integration and has not been enabled here.
