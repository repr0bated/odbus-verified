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
| Claude (cloud) | Control-plane lungs socket (OD-86), cloud environment, OD-87 | branch `claude/amazing-carson-8bg4yr`, repr0bated/odbus-verified#1 | 2026-09-28 | — |

## Handoffs

### 2026-09-28 · Claude (cloud session, later)
- **Rust 2024 migration** (Grok) is on `main` as `d5d6d2f` and is merged into this branch. `rustfmt.toml` still pins the 2021 style until the follow-up reformat commit.
- **Control-plane lungs** (`d35a7c3`): the bridge serves root-only peer-to-peer D-Bus on `/run/opdbus/lungs/<session>.sock` for `OP_CONTROL_PLANE_CHATBOT_SESSION_ID`. The socket is the proof, and no SID1 is stored. Each call re-checks the session and runs through MutationEngine under the chatbot's grants. 3tchedFS has `--lungs-socket`, and `op-identity-root` prefers the socket. Tests: threetched-fs 21/21, `lungs_socket` 5/5, `dbus_identity` 4/4, clippy clean.
- **Not done yet (host):** ship the release; move the chatbot onto the identity-root model; make one `call` write from inside it and confirm with `snowball.query_events` that the chain names the chatbot. Agents (Claude, Codex, Grok) still need their own service identities before they get lungs.
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
