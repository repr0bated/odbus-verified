# Model registry and picker repair

The provider list remains the configured source for discovery. Provider responses
are recorded in Cozo; picker options come from the admitted registry queries.
No parallel catalog, inferred model availability, direct database write, or new
transport was introduced.

## Implemented source changes

- The backend admits at most four model-list requests/responses at once, commits
  each provider incrementally, and preserves configured report order. This bounds
  response memory independently of the maximum 32 configured providers. Existing
  per-provider error handling and last-known model history remain intact.
- The UI refreshes its registry lookup on focus and on the explicit refresh
  action. Concurrent refreshes coalesce; a failed refresh preserves the previous
  options and selection. A removed selected model remains visible as unavailable.
- Provider/model pagination must represent one revision, with advancing cursors
  and explicit bounds. A changing revision retries the complete lookup at most
  three times; exhausting bounds is an error rather than a silently partial list.
- Native profile aliases remain distinct from model IDs. Refreshing the lookup
  does not change the user's selected route or rewrite profile mutations.

The UI working tree is `/srv/git/3tched.com`; these assets have not been published.
See `modelpicker-ui/RESULTS.md` for file inventory, commands, and validation.

## Live registry evidence

An authorized refresh through the existing chatbot identity's lungs succeeded
at 2026-09-30 09:52:14 UTC. Durable readback confirmed revision 28 and 858 available
models across seven successful providers. Three provider errors remain visible:
xAI HTTP 403, the absent configured Anthropic key, and VoyageAI HTTP 404. These
were not converted into invented model choices or silently disabled providers.
No inference request was required. Exact responses are under `cron/`.

## Scheduler is not repaired yet

The existing hourly job is enabled but fails its named read-only security
policy. That profile is shared by eight agent aliases. Its tool list also names
a backend method instead of an executable native tool. Changing the generic
dispatcher allowlist would not enforce a method-specific grant.

The owner's intended execution surface is the identity's 3tchedFS namespace:
write `{}` to `plugins/model_registry/operations/refresh/call`, inspect
`result` and `error`, and verify the resulting authoritative lookup state.
The scheduled runtime must actually have that mount and an appropriate grant.
Source paths alone do not establish a functioning mounted runtime.

No job, shared risk profile, grant, transport configuration, service, or identity
container was changed. `cron/NOTES.md` records the current runtime evidence and
remaining integration boundary. A successful manual refresh does not establish
that the next scheduled refresh will work.

The owner confirmed the existing control-plane chatbot identity as the intended
scheduled owner. Its configured in-container mount is `/opt/run-mounts/3tched`;
the container and backing lungs are currently stopped. The pinned native
scheduler has a global host-native runtime and no per-job identity filesystem
context. Its filesystem tools directly access host paths. A job prompt or
shared-policy toggle alone cannot repair that execution-context mismatch.

## Validation and rollback

UI: 114 tests passed, one live-door test skipped; type checking, lint, build,
and diff checks passed. Node 26's local test environment required
`NODE_OPTIONS=--no-experimental-webstorage`; the source does not depend on that
setting. The build retains its existing large-chunk warning.

Backend: all nine model-registry dispatch tests pass in the normal GNU Cargo
harness, including bounded concurrency, ordered persistence and partial failure
handling. See `verification/model-registry-tests-final.log`. Earlier compile
failures are preserved separately; the final source fixes those failures.

Revert only these owned source hunks in an isolated checkout for rollback;
preserve other agents' profile/chat response changes and the configured provider
file. There is no catalog format migration. The live refresh updated ordinary
registry observations and retained per-provider error/history semantics.
