# Requirements — Container chat

Status: draft, from the owner decisions of 2026-11-11. This spec authorizes
no file move, no container start, and no deploy. The container repo and the
3tched base are mostly already deployed. Chat is the piece that changes.

Spec id: `container-chat`
Spec home: `/srv/git/odbus-verified/.kiro/specs/container-chat/`

The container repo is `/srv/git/3tched.com`. It owns all of chat, the range
of services, and it installs on top of base 3tched. It also owns threetched,
the upstream ZeroClaw binary. Chat runs inside the container. That is where
the gate passes. Host port 8080 must not pass.

The hackathon spec `.kiro/specs/identity-chat-interface/` is not this base.
Its host placement of Chat_Route is the old line.

## Vocabulary

- **Container repo** — `/srv/git/3tched.com`. UI and all of chat.
- **threetched** — upstream ZeroClaw, renamed, no patches. Models, providers,
  and skills. Binary-backed. Not 3tched.
- **3tched** — our code. The base this repo installs on. Owns the gate,
  persistent memory, the cognitive layer, and 3tchedFS.
- **Gate** — grants and capabilities, evaluated by 3tched. The identity is
  the container.
- **Pass-through render** — the container passes the UI spec. The end user's
  browser renders it. The container does not need a desktop.

## Requirements

### Requirement 1: The container repo owns chat and installs on 3tched

**User Story:** As an operator, I want chat in the container repo, so that
the host is not where chat runs.

#### Acceptance Criteria

1. WHERE chat source lives, it SHALL live in `/srv/git/3tched.com`.
2. WHEN that repo is installed THEN it SHALL install on top of base 3tched
   AND SHALL NOT replace the 3tched base.
3. WHEN chat runs THEN it SHALL run inside the container.
4. IF chat is still the host process THEN that process SHALL NOT be treated
   as having passed the gate.
5. The container repo SHALL own the whole chat service range, not only a
   frontend.

### Requirement 2: threetched stays the binary

**User Story:** As an operator, I want model selection on the ZeroClaw
binary, so that our code is not a second copy of that binary.

#### Acceptance Criteria

1. WHERE models, providers, or skills are selected, the container install
   SHALL use the threetched binary.
2. WHEN threetched is installed THEN the install SHALL use the upstream
   binary, renamed, with no patches.
3. IF a change copies ZeroClaw into 3tched as our code THEN the container
   repo SHALL reject that change.
4. WHEN chat needs the gate, persistent memory, or the cognitive layer THEN
   chat SHALL call 3tched at runtime AND SHALL NOT reimplement them.

### Requirement 3: The gate passes inside the container

**User Story:** As the container identity, I want chat to pass the gate as
me, so that a host page cannot stand in for the container.

#### Acceptance Criteria

1. WHEN chat calls through the gate THEN the caller SHALL be inside the
   container.
2. WHEN the call is host port 8080 THEN the gate SHALL NOT pass.
3. WHILE the identity is the container, a host listener SHALL NOT be an
   acceptable substitute for that identity.
4. IF the container is absent THEN chat SHALL fail closed rather than fall
   back to the host.

### Requirement 4: Rendering passes through the container

**User Story:** As the end user, I want my browser to render the spec, so
that the host does not draw the page and the container does not run a
desktop.

#### Acceptance Criteria

1. WHEN a page is shown THEN the container SHALL pass the spec through, AND
   the end user's browser SHALL render it.
2. WHEN the spec is passed THEN the pass SHALL be as the container identity.
3. The container SHALL NOT run a desktop, a Wayland compositor, or wayvnc
   for this path.
4. IF the delivery mechanism is undecided THEN the spec SHALL still pass
   through the container. IPFS, a gated browser, and a local renderer stay
   open. None of them is a decision.
5. The network SHALL NOT draw the UI, and the container SHALL have no IP
   that the user types.

### Requirement 5: Shared surfaces at install and runtime

**User Story:** As an operator, I want install ownership and runtime calls
written down, so that a later edit does not merge the three repos.

#### Acceptance Criteria

1. WHEN the container repo is installed THEN chat and the threetched binary
   SHALL be that install's payload on top of base 3tched.
2. WHEN chat runs THEN it SHALL call 3tched for the gate, memory, the
   cognitive layer, and 3tchedFS.
3. WHEN chat selects a model, provider, or skill THEN it SHALL call the
   threetched binary.
4. WHEN chat uses lungs THEN it SHALL connect to the one 3tched socket and
   SHALL NOT bind its own listener.
5. The network SHALL remain the proof transport to `10.0.0.2:8090` and
   SHALL NOT gain chat, the binary, or the gate.
