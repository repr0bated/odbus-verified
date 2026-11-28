# Implementation Plan — 3tched core

Draft only. Do not start these tasks until the owner approves them. Do not
rebuild the deployed base as a side effect. Most of 3tched is already
installed. The tasks separate shared surfaces.

- [ ] 1. Fail a bridge build that has no 3tchedFS
  - Add a check the bridge build cannot skip.
  - A missing 3tchedFS dependency fails closed.
  - _Requirements: 3.1, 3.2_

- [ ] 2. Keep the gate on 3tched and deny host callers
  - [ ] 2.1 Deny a caller that is not inside the container
    - Host chat does not pass.
    - A listener on host port 8080 does not pass.
    - _Requirements: 2.1, 2.3, 2.4_
  - [ ] 2.2 Allow the pass for a caller inside the container
    - The identity used at the gate is the container.
    - _Requirements: 2.2_

- [ ] 3. Keep one lungs socket
  - Serve `/run/opdbus/lungs/lungs.sock` from 3tched.
  - Do not add a socket per container.
  - Distinguish callers by the kernel peer.
  - _Requirements: 3.4_

- [ ] 4. Refuse ownership that belongs to the other two repos
  - 3tched does not take chat source from `/srv/git/3tched.com`.
  - 3tched does not vendor or patch the ZeroClaw binary.
  - 3tched does not take the network bridge or WireGuard.
  - _Requirements: 1.2, 1.3, 1.4, 4.1, 4.2, 4.4_
