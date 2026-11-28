# Requirements — 3tched core

Status: draft, from the owner decisions of 2026-11-11. This spec authorizes
no build, no release, no file move, and no service change. 3tched is mostly
already deployed. The work in this spec is the boundary, and the shared
surfaces at install and at runtime.

Spec id: `3tched-core`
Spec home: `/srv/git/odbus-verified/.kiro/specs/3tched-core/`

3tched is our code. threetched is not. threetched is the upstream ZeroClaw
binary. The container repo (`/srv/git/3tched.com`) owns chat and installs
that binary on top of this base. The network owns identity transport only.
Those three stay separate repos. At runtime they call across.

## Vocabulary

- **3tched** — our code. Gates, grants and capabilities, persistent memory,
  the cognitive layer, 3tchedFS, and the backend runtime.
- **threetched** — the upstream ZeroClaw binary, renamed, with no patches.
  Models, providers, and skills. Not a crate in this repo.
- **Gate** — the grants and capabilities check. The identity is the
  container, so the check passes for a caller inside the container.
- **Bridge** — our code that binds `10.0.0.2:8090` after the network has
  delivered a proof. It includes 3tchedFS.
- **Host :8080** — the old host page. It must not pass the gate.

## Requirements

### Requirement 1: 3tched is our code and the base

**User Story:** As an operator, I want one repo for the code we write, so
that a binary we do not write cannot be mistaken for it.

#### Acceptance Criteria

1. WHERE this repository is built, 3tched SHALL be the base runtime: gates,
   grants and capabilities, persistent memory, the cognitive layer, and
   3tchedFS.
2. WHEN threetched is installed THEN 3tched SHALL treat it as an upstream
   binary placed on top of this base, AND SHALL NOT vendor, patch, or
   reimplement that binary.
3. WHEN chat is installed THEN 3tched SHALL NOT become the owner of the chat
   source. Chat source stays in `/srv/git/3tched.com`.
4. IF a change would move network transport, WireGuard, or the host bridge
   into this repository THEN 3tched SHALL refuse that change.

### Requirement 2: The gate belongs to 3tched and passes in the container

**User Story:** As the container identity, I want the gate enforced by our
code, so that a host process cannot pass as me.

#### Acceptance Criteria

1. WHERE a call needs a grant or a capability, 3tched SHALL evaluate that
   gate.
2. WHEN the caller is inside the container THEN 3tched SHALL be able to pass
   that caller.
3. WHEN the caller is host chat THEN 3tched SHALL NOT treat that caller as
   having passed the gate.
4. WHEN the caller is a listener on host port 8080 THEN 3tched SHALL NOT
   pass that listener.
5. WHILE chat runs, 3tched SHALL keep persistent memory and the cognitive
   layer on this side of the boundary, and chat SHALL reach them by a
   runtime call.

### Requirement 3: The bridge includes 3tchedFS

**User Story:** As an operator, I want the bridge to contain the filesystem
it serves, so that the door cannot ship without it.

#### Acceptance Criteria

1. WHEN the bridge is built THEN the bridge SHALL include 3tchedFS.
2. IF a bridge build has no 3tchedFS THEN the build SHALL fail closed.
3. WHERE 3tchedFS is mounted, the mount SHALL carry the caller identity.
4. WHERE lungs is served, 3tched SHALL serve one socket,
   `/run/opdbus/lungs/lungs.sock`. Containers connect to it. 3tched SHALL
   NOT create a socket per container.

### Requirement 4: Shared surfaces stay owned at install

**User Story:** As an operator, I want install ownership fixed, so that
runtime calls across the boundary do not merge the repos.

#### Acceptance Criteria

1. WHEN the base is installed THEN 3tched SHALL install our code only.
2. WHEN the container repo is installed on that base THEN 3tched SHALL leave
   chat and the threetched binary as that install's payload.
3. WHEN a chat turn runs THEN 3tched SHALL accept the runtime call for the
   gate, memory, the cognitive layer, and 3tchedFS.
4. WHEN a model, provider, or skill is selected THEN 3tched SHALL leave that
   selection to the threetched binary.
5. IF this spec is executed THEN 3tched SHALL NOT rebuild or redeploy the
   already-running base except where a task says a shared surface is in the
   wrong owner.
