# Model lookup UI implementation

Source repository: `/srv/git/3tched.com`. No publication or deployment performed.

The existing UI already read actual model IDs from `model_registry`, separately from router profile aliases. It had two defects: it held the initial options for the whole connection, and pagination silently returned partial results after a stalled cursor or 40 pages. It also ignored registry revisions, allowing a cron refresh to mix generations within one lookup.

The adapter now coalesces explicit reloads and replaces options only on successful reads. The runtime refreshes the rendered picker on focus or through “Refresh models”, retains previous options after errors, and preserves the selected route. Removed selected models remain visible as a disabled “no longer listed” option. Empty refreshed provider lists can be reloaded without crashing. Local adapters make no network calls. The refresh control shares the existing status column and remains usable on narrow screens.

Catalog reads validate pages and ascending provider/model cursors, and require one revision across providers and model pages. A revision change retries the complete catalog at most three times. The existing 40-page bound remains: a provider over 20,000 models now produces an explicit error and preserves the previous catalog instead of displaying a truncated result. All reads use the existing generic door with `model_registry.read`; no refresh/mutation, control-plane profile assignment, or route-selection call is issued by a catalog reload.

Live sealed contracts inspected using `/usr/local/bin/zcall help model_registry list_providers`, `list_models`, `set_model_profile`, and `zcall help tched_router SetSelection`, `config_get`, `config_set`. The registry returns ordered model records, `more`, `revision`, and a provider/model cursor. `control_plane_profile` and `recorded_profile` are associations distinct from actual `model_id`; existing profile configuration ownership is unchanged. The backend cron implementation is owned by the parent agent.

Validation: `npm run type-check`, `npm run lint`, `NODE_OPTIONS=--no-experimental-webstorage npm test`, `npm run build`, and `git diff --check`. The suite passes 114 tests, with one optional live-door test skipped. New tests exercise 501-row pagination, revision change/retry exhaustion, malformed/stalled cursors, the page safety bound, read-error propagation, coalesced adapter/runtime refresh, option preservation/retry after error, empty results, the offline adapter, and the rendered picker’s focus/button behavior and retained selection.

Host Node is 26.8.2. Its experimental global localStorage shadowed jsdom and caused existing mode tests to fail without a local-storage file. The test command disables that experimental Node feature, allowing jsdom’s browser storage. No source or test setup changes were made for that environmental issue. The build succeeds with the existing warning that the main minified chunk exceeds 500 kB.

Run `bash verify-ui.sh` to reproduce. Adjacent logs, source hash manifest, and tracked diff record the final validation. New test files are in the source checkout and included in the hash manifest.
