# Implementation Plan — Container chat

Draft only. Do not start these tasks until the owner approves them. Chat is
the piece that changes. Do not redeploy the network bridge and do not
rebuild the 3tched base to begin this list.

- [ ] 1. Place the chat process inside the container
  - The running caller that must pass the gate is in the container.
  - A leftover host chat process is not a pass.
  - Source ownership stays `/srv/git/3tched.com`.
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

- [ ] 2. Install threetched on top of base 3tched
  - [ ] 2.1 Install the upstream ZeroClaw binary, renamed, with no patches
    - Models, providers, and skills resolve to that binary.
    - 3tched does not gain a copy of the binary's source.
    - _Requirements: 2.1, 2.2, 2.3_
  - [ ] 2.2 Call 3tched for the gate, memory, and the cognitive layer
    - Those stay our code.
    - Chat reaches them at runtime.
    - _Requirements: 2.4, 5.2_

- [ ] 3. Deny host port 8080
  - A listener on host port 8080 does not pass the gate.
  - No fallback from a missing container to that port.
  - _Requirements: 3.2, 3.3, 3.4_

- [ ] 4. Pass the render through the container
  - The container passes the spec as the container identity.
  - The end user's browser renders it.
  - No desktop, Wayland, or wayvnc in the container for this path.
  - Leave the byte transport undecided.
  - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

- [ ] 5. Keep the shared sockets on their owners
  - Chat connects to the one lungs socket and does not bind another.
  - Chat uses 3tchedFS as disk I/O.
  - The network is not given chat, the binary, or the gate.
  - _Requirements: 5.1, 5.3, 5.4, 5.5_
