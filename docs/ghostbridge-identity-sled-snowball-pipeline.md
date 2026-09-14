# Ghostbridge Session Identity and Snowball Accountability Pipeline

**Status:** Current architecture as of 2026-08-31.

## Authorities

- The `identity_sled` plugin owns one identity record per session. Cozo is the
  durable store; `opdbus/state/identity_sled.json` is its read-only shared-memory
  projection.
- The sealed plugin-blob catalog owns plugin schemas and publishes the catalog
  hash used when a session genesis is minted.
- The retired process-global raw identity/schema record is not a system
  component, startup prerequisite, schema source, or authentication fallback.

## Session identity

Each anchored session records its derived `session_id`, WireGuard public key,
immutable genesis, trace ID, mutation index, arrival timestamp, and chain head
at arrival. A consumer must select a session explicitly by one of its handles.
Omitting a selector is allowed only when exactly one anchored session exists.

The session's Incus container does not need to be running merely to read or
verify the durable identity record. Session lifecycle policy may still mark a
record inactive or expired; that is distinct from depending on a raw host file.

## Authentication flow

1. Arrival creates or resolves a session through the `identity_sled` plugin.
2. Genesis is minted once from the WireGuard key, chain head, catalog hash, and
   arrival time, then stored with the session.
3. Clients send `x-ghostbridge-genesis` plus a trace ID or WireGuard public key.
4. Gatekeepers resolve that exact projected/durable session and compare the
   stored genesis. Unknown, ambiguous, unanchored, expired, or mismatched
   sessions fail closed.
5. `x-ghostbridge-footprint` remains a compatibility header name during client
   migration, but its value is the immutable session genesis.

## Schema flow

Plugin schema readers use the selected plugin's sealed blob. Catalog identity
comes from the sealed catalog manifest; request paths do not append schema data
to identity records and do not re-hash a monolithic schema file.

## Regression protection

`scripts/ci-gate-deprecated-plugin-schema-dat.sh` rejects active code or deploy
assets that recreate the retired file, environment overrides, or helper
binaries.
