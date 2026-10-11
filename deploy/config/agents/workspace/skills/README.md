# Skills owned by the 3tched runtime

Skills are managed by the existing tched-router runtime and its configured skill
bundles. Do not create a second registry by copying a frozen skill list here.

skills points to the identity-bound tched_skills plugin in 3tchedFS. Its sealed
contract derives from the installed ZeroClaw binary's skills command group and
skill bundle configuration schema. Discover list_skills, list_bundles,
show_bundle and supported mutations through their operation declarations.
Each operation still requires its declared capability and mutation confirmation.

runtime points separately to tched_router, the routing and configuration owner
backed by that same ZeroClaw binary. Neither link copies skill contents or
starts another runtime. A missing skills projection means the plugin needs
release integration; do not substitute GetTools for skill inventory.

Agent operations remain available through /agent/shared/cognitive-mcp. A skill
does not grant capabilities or override the owner. Runtime bundle loading from
this directory requires explicit integration and has not been enabled here.
