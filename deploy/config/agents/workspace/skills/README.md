# Skills owned by the 3tched runtime

Skills are managed by the existing tched-router runtime and its configured skill
bundles. Do not create a second registry by copying a frozen skill list here.

runtime points to the identity-bound tched_router plugin in 3tchedFS. Read its
schema and declared operations for supported discovery and configuration. This
is an owner/discovery link, not a mounted view of the runtime's skill bundles.
The current mounted contract has GetTools and configuration operations, but no
dedicated skill-listing operation; do not invent one or claim tools are skills.

Agent operations remain available through /agent/shared/cognitive-mcp. A skill
does not grant capabilities or override the owner. Runtime bundle loading from
this directory requires explicit integration and has not been enabled here.
