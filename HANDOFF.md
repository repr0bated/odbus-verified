# Agent handoff

One shared baton for every agent on this project: Claude, Codex, Grok (Cursor),
and anything else. An agent's private memory can mirror this file, but never
replaces it. Nothing here counts as handed off until it is written here.

- **Tasks** stay in `WISHLIST.md` (`OD-##`). **Observations** stay in `SIGNALS.md`.
  This file only holds who is working on what right now, and where the last session left off.
- **Start of session:** read this file after `AGENTS.md`, then add your row to
  *Active work* before you change anything.
- **End of session, or before a context or usage limit:** remove or update your
  row, and add a *Handoff* entry at the top of that section.
- **Keep it short.** Keep the five newest handoffs; git history holds the rest.
- **Say where the work lives.** Name the host path or branch, and whether the work is
  committed, pushed or only in a working tree. A cloud session cannot see the
  host, and the host cannot see an unpushed cloud branch.

## Active work

Claim work here before starting. If a row touches the same files or live
state as your task, stop and ask the owner. Never reset, stash or overwrite
another agent's uncommitted work.

| Agent | Task | Where | Since | Do not touch |
|-------|------|-------|-------|--------------|
| Grok (Cursor) | Add Cheaper Inference (`api.cheaperinference.com`) as a router provider | `/srv/git/odbus-verified` `main` | 2026-09-28 | Do not touch `feat/chat-response` or its plugin files |
| Claude (host, session odbus-verified-f0) | OD-90, continuing the cloud's two-sided `chat_response` work at the owner's request. Stage 1 is verified on the host: fmt and clippy clean; op-cozo-store 58, op-plugins 528, op-grpc-bridge 734 passed, 0 failed. Next, stage 2 over the cognitive MCP door: a `chat_response_reply` toolset in `mcp-toolsets.json`, the door's `Chat` opening the turn, and first a live check of which identity the router presents to `/mcp` | branch `feat/chat-response`, host worktree `/srv/git/odbus-verified/.claude/worktrees/chat-response`, pushed | 2026-09-28 | `crates/op-plugins/src/state_plugins/chat_response*.rs`, the bridge `chat_response_*` modules. The cloud session is idle on this; do not push to this branch from it. |

## Handoffs

### 2026-10-30 · Codex (PR #45 review repair)
- **Where:** isolated worktree `/var/tmp/pr-relevance-20261030/pr45`, based on `7c05005`; local commit only, not pushed.
- **What:** cloud build exports precede bashrc early returns, use the provisioned Rust homes, and replace bounded blocks while retaining later user configuration and existing file modes. Missing ubuntu bashrc files are created. Generated `/etc/environment` CXXFLAGS values are quoted, including migration on rerun.
- **Validation:** `sh -n scripts/claude-cloud-setup.sh` and `git diff --check` only. No provisioning, Cargo, smoke checks, tests, or services run; smoke/tests remain manual/on-request. No claim of a fresh cloud boot verification.
- **History:** retained the five newest handoffs under this file's existing rule; older entries remain in git history at `7c05005:HANDOFF.md`.

### 2026-10-27 · Grok (Cursor cloud: development environment)
- **Where:** branch `cursor/cloud-agent-env-760f`, pushed, PR #45. Not on the host.
- **What:** `scripts/claude-cloud-setup.sh` pins `CC=gcc`, `CXX=g++`, `CXXFLAGS=-include cstdint`, and the multiarch `libstdc++.so` link. The environment install is that script. No start script. The control plane stays stopped.
- **Verified:** workspace check finished; `op-core` 50 passed; `op-plugins` 515 passed and 3 Snowball tests failed because this kernel has no btrfs. `op-plugin-lint` audited `proxy_server.rs`. Draft build `bld-20261027-5c870e78-d006-46bc-a81d-55c1d44d857e` preflight passed, and a fresh agent booted from that build passed preflight.
- **Still manual:** Save the environment in the panel. The three Snowball tests need a btrfs kernel. Do not skip them.

### 2026-10-01 · Grok (Cursor cloud: KeePassXC for Cursor in the CRD Plasma session)
- **Where:** branch `cursor/crd-cursor-keepass-5bbc`, pushed, draft PR #7. Not copied onto the host. The running launcher is still `/usr/local/lib/chrome-remote-desktop/crd-launch`.
- **What:** `crd-launch` runs `cursor-use-keepass` before Plasma starts. Cursor's `~/.cursor/argv.json` gets `"password-store": "gnome-libsecret"`. `[KSecretD] Enabled=false` keeps kwalletd from owning `org.freedesktop.secrets`. KeePassXC `FdoSecrets/Enabled` is turned on, with a session autostart and a per-user D-Bus activation file. The kdbx is not opened and KWallet entries are not migrated.
- **Still manual:** Database → Database Settings → Secret Service Integration → expose one group. Then quit Cursor and sign in once; the old login key is in KWallet.
- **Deployment:** prepare both scripts together in the declared release filesystem and use snapshot/send-receive activation. A deliberate remote-desktop restart requires an independent SSH connection; do not restart from inside that session.

### 2026-09-28 · Grok (Cursor: golden and activate, picker binary)
- **Live:** `current` → `releases/20260928T145511Z-2bdeb27`. Same commit as the previous release, with the rebuilt `op-web-server` (`197d3115…`). Grants, bridge, EMQX, and op-web pass `sv check`. Catalog unchanged (66 plugins, generation 264).
- **Rollback:** `releases/20260928T125401Z-2bdeb27`.
- **Picker:** the API is in this `op-web`. The pinned dashboard bundle does not include the Control plane picker control.

### 2026-09-28 · Grok (Cursor: pull build golden activate)
- **Live:** `current` → `releases/20260928T125401Z-2bdeb27` (`main` @ `2bdeb27`, bridge default-provider fix `df43133`). Bridge/op-web/grants/EMQX restarted; all four pass `sv check`. Running `op-grpc-bridge` digest `8ba31303…` matches received. SHM, persisted and received catalog manifests match (`930d7843…`, 66 plugins, generation 264).
- **Rollback:** `releases/20260928T093213Z-f0e83ee`. Pre-seal catalog backup: `/var/tmp/opdbus-release-20260928T122846Z-preseal`.
- **Send snapshot:** `/opt/op-dbus/send/20260928T125401Z-2bdeb27` (readonly; may be deleted after verification).
