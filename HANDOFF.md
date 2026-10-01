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
| Claude (host, session odbus-verified-f0, delegated build) | OD-90 stage 1: the `chat_response` plugin (schema, principal-scoped private conversations, turn registry, bridge dispatch, tests). No door enforcement or release yet | branch `feat/chat-response`, host worktree `/srv/git/odbus-verified/.claude/worktrees/chat-response`, not pushed | 2026-09-28 | `crates/op-plugins/src/state_plugins/chat_response*.rs`, the new bridge `chat_response_*` modules |

## Handoffs

### 2026-10-01 · Grok (Cursor cloud: KeePassXC for Cursor in the CRD Plasma session)
- **Where:** branch `cursor/crd-cursor-keepass-5bbc`. Source only. Not copied onto the host. The running launcher is still `/usr/local/lib/chrome-remote-desktop/crd-launch`.
- **What:** `crd-launch` runs `cursor-use-keepass` before Plasma starts. Cursor's `~/.cursor/argv.json` gets `"password-store": "gnome-libsecret"`. `[KSecretD] Enabled=false` keeps kwalletd from owning `org.freedesktop.secrets`. KeePassXC `FdoSecrets/Enabled` is turned on, with a session autostart and a per-user D-Bus activation file. The kdbx is not opened and KWallet entries are not migrated.
- **Still manual:** Database → Database Settings → Secret Service Integration → expose one group. Then quit Cursor and sign in once; the old login key is in KWallet.
- **Do not** restart `chrome-remote-desktop-jeremy` from inside the remote session. Copy both scripts over SSH, then restart from SSH.

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
