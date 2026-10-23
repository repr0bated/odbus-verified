# Root crates/ UI Feature Review

## Summary
- Status: Buildable prototype
- Verification: After `npm ci`, `npx tsc --noEmit`, `npm test`, and `npm run build` all passed in `crates/`.
- Assessment: This frontend is smaller and more coherent than the embedded `op-web/ui` app, but it still includes a few endpoint and deployment assumptions that prevent me from calling it production-ready.

## Coded Features
- React Router application with pages for overview, chat, tools, agents, LLM, services, security, config, inspector, state, and knowledge search.
- React Query based API client in `crates/src/api/client.ts`.
- Layout shell and shared component library under `crates/src/components`.

## Missing Or Suspect Functionality
- `crates/src/api/types.ts` hardcodes `API_BASE = "https://mail.3tched.com/api"`, which means the app is pointed at a production domain by default instead of same-origin/local configuration.
- The client issues `GET /chat/sessions/:id`, but the current backend routes only define `DELETE /api/chat/sessions/:id`; there is no corresponding GET handler in `op-web`.
- The package still lacks the `typecheck` script expected by the repo instructions, even though a direct `npx tsc --noEmit` run succeeds.
- Test coverage is again minimal: one example Vitest file.

## Evidence
- `crates/src/App.tsx`
- `crates/src/api/client.ts`
- `crates/src/api/types.ts`
- `crates/package.json`
