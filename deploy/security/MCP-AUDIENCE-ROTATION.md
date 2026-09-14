# MCP singleton chatbot rotation

The compact MCP audience is one exact, registered `principal_id`. It is never
selected by client name, model, header, alias, session, genesis, or footprint.

1. Enroll the replacement chatbot key through the authenticated
   `human_principal.register_key` bootstrap path and install its exact grants.
   Confirm a fresh OIA for that key can authenticate while the old singleton is
   still configured.
2. Increment `rotation_epoch`, replace `singleton_chatbot_principal_id`, build
   one release, and stage it with `deploy/runit/build-golden.sh`. Create a
   read-only snapshot and deploy that snapshot with `btrfs send/receive`.
   Confirm the replacement sees the four compact tools and the old principal
   sees none; only then revoke/remove the old grants in a second audited
   release.

An absent, malformed, unregistered, revoked, or multiply represented singleton
fails closed: no caller gains compact visibility.
