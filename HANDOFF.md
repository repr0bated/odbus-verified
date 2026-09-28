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
| Grok (Cursor) | Release `20260928T145511Z-2bdeb27` is live (picker `op-web`). Pinned dashboard still lacks the picker control | `/srv/git/odbus-verified` `main`, uncommitted | 2026-09-28 | Do not touch `feat/chat-response` or its plugin files |
| Claude (host, session odbus-verified-f0, delegated build) | OD-90 stage 1: the `chat_response` plugin (schema, principal-scoped private conversations, turn registry, bridge dispatch, tests). No door enforcement or release yet | branch `feat/chat-response`, host worktree `/srv/git/odbus-verified/.claude/worktrees/chat-response`, not pushed | 2026-09-28 | `crates/op-plugins/src/state_plugins/chat_response*.rs`, the new bridge `chat_response_*` modules |
| Claude (cloud) | OD-93: UI gallery (unbounded private candidates) and active catalog with promotion, owned by `json_render`; chat Canvas profile imported from 3tched.com | branch `claude/amazing-carson-8bg4yr`; 3tched.com branch `claude/ui-gallery-promotion` | 2026-09-28 | `json_render_gallery.rs`, `json_render_chat_canvas*.rs`, `op-cozo-store/src/ui_gallery.rs`, bridge `json_render_gallery_dispatch.rs` |

## Handoffs

### 2026-09-28 · Grok (Cursor: golden and activate, picker binary)
- **Live:** `current` → `releases/20260928T145511Z-2bdeb27`. Same commit as the previous release, with the rebuilt `op-web-server` (`197d3115…`). Grants, bridge, EMQX, and op-web pass `sv check`. Catalog unchanged (66 plugins, generation 264).
- **Rollback:** `releases/20260928T125401Z-2bdeb27`.
- **Picker:** the API is in this `op-web`. The pinned dashboard bundle does not include the Control plane picker control.

### 2026-09-28 · Grok (Cursor: pull build golden activate)
- **Live:** `current` → `releases/20260928T125401Z-2bdeb27` (`main` @ `2bdeb27`, bridge default-provider fix `df43133`). Bridge/op-web/grants/EMQX restarted; all four pass `sv check`. Running `op-grpc-bridge` digest `8ba31303…` matches received. SHM, persisted and received catalog manifests match (`930d7843…`, 66 plugins, generation 264).
- **Rollback:** `releases/20260928T093213Z-f0e83ee`. Pre-seal catalog backup: `/var/tmp/opdbus-release-20260928T122846Z-preseal`.
- **Send snapshot:** `/opt/op-dbus/send/20260928T125401Z-2bdeb27` (readonly; may be deleted after verification).

### 2026-09-28 · Claude (host, session odbus-verified-f0: release)
- **Live:** `current` → `releases/20260928T093213Z-f0e83ee` (`main` with PR #4, the 2024 reformat and `a12d669`). The bridge, op-web, grants and EMQX run the release binaries, and all pass `sv check`. The 66-blob catalog is unchanged and byte-identical across SHM, persisted and received. The chatbot's lungs bind over `/run/opdbus/lungs.sock`, and `op-lungs-check` passes `workflows.query_workflows` (event 84973) and `cognitive_mcp.get_health` (event 84975).
- **Rollback:** `releases/20260928T064128Z-0432589`, which actually holds the `ad3051c` golden. Catalog backup: `/var/tmp/opdbus-release-20260928T092457Z-preseal`.
- **For the next release:** rebuild the mail bundle after the release build (`deploy/mail-3tched/build-container-artifact.sh`). Run `build-golden.sh` with `OPENCODE_BINARY_SOURCE=/opt/op-dbus/releases/20260928T093213Z-f0e83ee/usr/bin/opencode` until OD-88 removes OpenCode as a binary (owner decision).
- **KiroCrew is uninstalled** (owner-approved): a KiroCrew agent's copied mount namespace kept the chatbot's mounts alive and blocked `op-identity-root up` (OD-89). Backups: `/var/tmp/kirocrew-uninstalled-20260928` and `~/.kiro-crew-uninstalled-20260928`. Kiro CLI itself is untouched.
- **Not restarted:** other services whose binaries changed in this release keep their running copies until their next restart. `notebook-sources-sync/run` also changed.

### 2026-09-28 · Claude (host, took over the cloud lungs work)
- **Merged:** PR #4 is on `main` as `1dd7421`, not yet released. The 2024 reformat followed as `0d5f582` (`rustfmt.toml` is gone, 551 files): rebase or merge any open branch over it, and expect conflicts only in layout.
- **Shared lungs socket, built and tested** on `claude/amazing-carson-8bg4yr` (draft PR #4). The cloud session wrote it (`4a6a483`) while the host session wrote the same change in parallel; the host kept the cloud commit and added what it lacked on top. The bridge serves one root-only `/run/opdbus/lungs.sock` and now creates its parent if missing. Each connection calls `LungsV1.Bind(session)` once, then its plugin calls run as that session. 3tchedFS mounts with `--lungs-socket /run/opdbus/lungs.sock --lungs-session <session>`; a refused bind stops at the `register` stage with `EACCES` (new test). `op-identity-root` and `op-lungs-check` use the shared socket.
- **`cognitive_mcp.get_health`** answers from the in-process runtime and quota, matching `GetHealthOutput`. No unit test covers it, because it needs the durable Cozo runtime; `op-lungs-check` with `LUNGS_CHECK_CALL=cognitive_mcp.get_health` proves it after the release.
- **Verified in the host worktree:** clippy `-D warnings` is clean on threetched-fs, op-grpc-bridge and op-plugins. threetched-fs passes 22/22. op-grpc-bridge passes 725, with 0 failed and 8 ignored (`-j 1`; see the SIGNALS build concern).
- **After merge, host steps:** the usual release. An existing lungs mount keeps its old per-identity arguments, and `up` skips a mounted identity, so run `op-identity-root down` and then `up` for the chatbot, then `op-lungs-check`. From then on, `up` gives socket lungs to every identity whose session is anchored and active, including the owner's `414fbc56`.

### 2026-09-28 · Claude (cloud session, lungs proven)
- **Chatbot lungs proven live** (Grok ran it on the host at `3860fe3`): `LUNGS_CHECK_CALL=workflows.query_workflows op-lungs-check` passed every link. Chain event **84953** names actor `1262e982-14c1-44d4-0771-7ce74213211b` (the control-plane chatbot) for session `b0c777e3-0955-cd8b-3d01-ca12998cae28`. No grant was added; `snowball.read` stays off the chatbot because it is chain-wide.
- **Found on the way:** `cognitive_mcp.get_health` fails with "cognitive_mcp projection not available", although the chatbot holds four cognitive_mcp grants. Being traced.
- **Next:** replace the per-identity socket with one shared `/run/opdbus/lungs.sock` before humans get lungs (owner decision, OD-86).

### 2026-09-28 · Grok (Cursor, host)
- **Release:** `main` is `79a47c7` (PR #1) plus this handoff. Catalog backup is `/var/tmp/opdbus-release-20260928-preseal` (66 persisted, 66 SHM). Seal with `COGNITIVE_MCP_QDRANT_URL=http://10.200.0.2:6334`. Review any plugin ID missing from that backup before `opblob persist`.
- **Chatbot Cozo:** `create` with `ARCHIVE_EXISTING=1` leaves the old persist device on `<session>-archived-<date>`. The new device's `cozo` directory starts empty. Do not copy the old store onto it.
- **Before create:** `/run/opdbus/lungs/b0c777e3-0955-cd8b-3d01-ca12998cae28.sock` must exist after activation. If it does not, the new bridge is not the one running, and `up` would fall back to the credential path.

### 2026-09-28 · Claude (cloud session, later)
- **Rust 2024 migration** (Grok) is on `main` as `d5d6d2f` and is merged into this branch. `rustfmt.toml` still pins the 2021 style until the follow-up reformat commit.
- **Control-plane lungs** (`d35a7c3`): the bridge serves root-only peer-to-peer D-Bus on `/run/opdbus/lungs/<session>.sock` for `OP_CONTROL_PLANE_CHATBOT_SESSION_ID`. The socket is the proof, and no SID1 is stored. Each call re-checks the session and runs through MutationEngine under the chatbot's grants. 3tchedFS has `--lungs-socket`, and `op-identity-root` prefers the socket. Tests: threetched-fs 21/21, `lungs_socket` 5/5, `dbus_identity` 4/4, clippy clean.
- **Host steps, in order** (owner decision: the chatbot starts fresh on Raccoon):
  1. The release, per `docs/src/getting-started/local-release-activation.md` (not scripted, by rule). It ships the new bridge, 3tchedFS, `op-identity-root` and `op-lungs-check`.
  2. `sudo ARCHIVE_EXISTING=1 /usr/local/libexec/3tched/op-identity-root create b0c777e3-0955-cd8b-3d01-ca12998cae28`: stops the chatbot, keeps its old instance as `<session>-archived-<date>`, and makes its device and identity root with the same `user.opdbus.*` keys.
  3. `sudo /usr/local/libexec/3tched/op-identity-root up b0c777e3-0955-cd8b-3d01-ca12998cae28`
  4. `sudo /usr/local/libexec/3tched/op-lungs-check`: prints PASS/FAIL per link, ending with the chain event's actor. Agents (Claude, Codex, Grok) still need their own service identities before they get lungs.
- **Cloud setup script:** retries the toolchain install until `rustc --version` runs, and installs `libprotobuf-dev libudev-dev`.
