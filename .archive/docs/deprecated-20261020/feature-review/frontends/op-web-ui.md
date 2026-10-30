# Embedded op-web UI Feature Review

## Summary
- Status: Partial
- Verification: `npx tsc --noEmit`, `npm test`, and `npm run build` all passed in `crates/crates/op-web/ui`.
- Assessment: The embedded UI is mechanically buildable, but it does not match the Kiro task checklist or the current backend API surface closely enough to call it feature-complete.

## Spec References
- `.kiro/specs/op-web/design.md`
- `.kiro/specs/op-web/tasks.md`
- `.kiro/specs/op-web-ui/requirements.md` (empty file)

## Coded Features
- React Router application with dashboard-style pages for chat, agents, tools, mail, MCP, analytics, workflows, debugger, settings, and OpenClaw.
- Embedded deployment via `op-web/src/embedded_ui.rs` and `rust-embed`.
- REST client in `crates/crates/op-web/ui/src/lib/api.ts`.
- React Query wiring via `App.tsx`.

## Alignment Review
- The Kiro task list marks large feature areas as complete, but the source tree does not contain the underlying infrastructure those tasks describe.
- The dedicated Kiro requirements file for `op-web-ui` is empty, so there is no usable acceptance contract there.

## Missing Or Suspect Functionality
- `package.json` does not include several dependencies the task list claims are already in use, including `zustand`, `@tanstack/virtual`, `dagre`, or any gRPC-web / protobuf-ts client stack.
- Source search found no implementations of `authStore`, `quotaStore`, `uiStore`, `ProtectedRoute`, `RBACGate`, `VirtualList`, `VirtualTree`, `useWebSocket`, `LiveLogTail`, or workflow-canvas components such as `DAGCanvas`.
- The REST client expects many routes that the backend does not expose today, especially for analytics, workflows, workstacks, settings, orchestration, debugger, and execution logs.
- Test coverage is minimal: the UI has one example test file, not the component-level test suite implied by the task list.

## Evidence
- `crates/crates/op-web/ui/package.json`
- `crates/crates/op-web/ui/src/App.tsx`
- `crates/crates/op-web/ui/src/lib/api.ts`
- `.kiro/specs/op-web/tasks.md`
- `.kiro/specs/op-web-ui/requirements.md`
