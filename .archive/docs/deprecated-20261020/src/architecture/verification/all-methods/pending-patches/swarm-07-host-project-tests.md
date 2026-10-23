# Project owner fixture test design

Apply `swarm-07-host-project-seam.patch` only after root review. The project dispatch test should hold the existing project test serialization guard while installing `ProjectOwner::fixture(temp_root, fixture_socket, "a" * 64, "fixture-pool")` through `install_project_owner_for_test(Some(owner.clone()))`; clear it in a drop guard. The canonical `MutationEngine::dispatch_method_call_with_identity` then reaches the same dispatch body with a temp root and real Incus UDS transport.

The Unix HTTP fixture must parse the dynamic POST body rather than acknowledge an invented response. Assert the request is `POST /1.0/instances`, then verify `name == project_id`, `type == container`, `ephemeral == false`, empty profiles, image fingerprint, root disk pool/path, project disk source equals the generated temp-root project path, path `/project`, `readonly == true`, `shift == true`, and all project security/owner config keys. Return a real async operation envelope, assert the server-side wait request uses the fixed UUID operation path and `timeout=120`, then return a sync success envelope. The final `GET /1.0/instances/<id>?recursion=1` must return the binding fields from the POST body; this exercises production create plus ownership readback.

After create, send canonical `put_document` calls for two collections and `list_documents` for each collection. Every readback verification GET must be served by the fixture with the exact matching project owner/path binding. Assert each list contains only its requested collection and the exact Cozo document fields/author session; a second project store must not see the first project's rows. Send project-mode `resolve_context` and assert `project/<id>` namespace, selected agent, allowed agents, effective capability intersection, and `read_only == false`.

Use separate fixture responses to prove failures: a successful POST/wait followed by a mismatched owner/path GET must make `create` fail and leave the durable catalog row `Provisioning`; a 404 or malformed binding GET must make `put_document`, `list_documents`, and project-mode `resolve_context` fail before Cozo access. Exercise invalid document id/media type/oversize input and a non-project `resolve_context` carrying `project_id`; these must fail without a socket request. The fixture never opens `/var/lib/op-dbus/projects`, `/var/lib/incus/unix.socket`, or an existing container.

## Captured bases and exact test

The complete unapplied patch was generated against these shared-worktree files:

- `crates/op-grpc-bridge/src/project_dispatch.rs`: `5ea0f92228e2bdfda8d0997e103de36ba01a31af24e437eca101aaa0a2adc908`
- `crates/op-plugins/src/state_plugins/incus.rs`: `75d808f02aca3dacb7bad9ef5249408e958f4ee1b5aa06677376575600583ef9`
- `crates/op-grpc-bridge/src/project_dispatch_tests.rs` (preserved, not modified): `1f5e1167223a45a527608b24617145d6eea8122264f25f7636b742ff01025862`

The new exact test is `project_dispatch::native_tests::project_native_owner_covers_create_documents_context_and_isolation` (Cargo filter: `project_native_owner_covers_create_documents_context_and_isolation`). The source seam parses with `rustfmt --edition 2021 --check`; the patch passes `patch --dry-run -p0` against the captured bases. The patch SHA-256 is `068779aba008f88e8a05d39a9b151dbc947b3796b03a7516e42d698ea1523746`.
