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
| Grok (Cursor) | Host release of merged lungs work (`79a47c7`): build, seal, golden, btrfs receive, activate, then chatbot create/up/lungs-check | `/srv/git/odbus-verified` `main`, pushed | 2026-09-28 | Do not reset this tree. Catalog copy goes to `/var/tmp` before seal. |
| Claude (cloud) | Single shared lungs socket (OD-86) and the cognitive_mcp projection failure | branch `claude/amazing-carson-8bg4yr` | 2026-09-28 | — |

## Handoffs

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

### 2026-09-28 · Claude (cloud session)
- **Cloud environment:** the setup script's rustup download was cut off, so it
  left a half-installed toolchain. It was repaired in-session. The setup script still needs a retry
  loop, a `rustc --version` check, and `libprotobuf-dev libudev-dev` added to its package line.
  The prerequisite docs are fixed in PR #1.
- **OD-87:** the Oct 1 record epoch is written up (owner intent: Oct 1 marks where
  the record turns from build history into what others see it do). It is not called
  "anchor", because that word names the interceptor's session anchoring.
- **Waiting on owner:** the two OD-87 decisions (an epoch chain event, and the chain
  embedding text).

### 2026-09-28 · Claude (host session 3035bc0a, transcript in `docs/`)
Moved here from Claude's private memory so every agent can see it.
- **Rust 2024 migration:** about 187 files are changed in the host working tree, not committed.
  Now owned by Grok (see *Active work*).
- **Unpushed on host:** 3tched.com has 4 commits on `main`; the owner reviews them before pushing.
  `/srv/git/3tchedFS` can be archived, since its history is now in `crates/threetched-fs`.
- **Live host state:** the owner's identity `414fbc56` runs on the overlay
  root with its lungs mounted. It was set up by hand, so after a reboot it needs `op-identity-root up`
  until a release ships the `identity-roots` service.
- **Oct 1 plan:** traceable compliance. The NIST 800-53 control graph goes into Cozo
  during the release window, while the bridge is down, and the demo apps' methods get
  `control_refs` that the owner reviews. Connected mode is the critical path:
  the owner tests `http://10.0.0.4:8080/demo/` and reports the "Tried …" line.
- **Chain vectors:** on hold until the embedding text is settled. Ingest goes
  through a local received subvolume (OD-64 replica path).
- **NotebookLM sync:** uploads have been skipped since about Sep 4–10 because no notebook
  ID is configured, and the service's log writer is down.
- **Recovered CMS spec** (from the Aug 21 Codex session): `3tched.com/references/recovered/cms-aug21.spec.json`,
  not committed.
