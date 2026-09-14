# Pinned json-render UI source

This directory closes the source and release boundary for the active standalone
Vite UI. `source.tar.gz` is a deterministic archive of the explicitly
allowlisted files from `/srv/git/vercel-json-render-ui`; it records base commit
`e8aad07bf02fc16bbb45554fbc570b66513ec019` and `source_worktree=dirty` because
the required catalog/registry/event-handler work was not committed in that
external checkout when captured. The content manifest covers the actual bytes,
including those working-tree changes.

The archive contains `AGENTS.md`, licensing and guidance, `package.json`, the
lockfile, Vite/TypeScript/ESLint configuration, `index.html`, and `src/` (plus
`public/` when present). It excludes `.git`, `.env*`, `node_modules`, `dist`,
logs and deployment scripts.

Build without touching the external checkout or live dashboard:

```sh
vendor/vercel-json-render-ui/build-assets.sh \
  --npm-cache /var/cache/op-dbus-build/npm \
  --output-dir target/ui-artifacts/vercel-json-render-ui
```

The helper verifies every source file, runs the repository-required `npm ci`,
type check, lint, tests and Vite build with `/vercel-json-render/` as its base,
then emits `assets.tar.gz` and `manifest.json`. Golden preparation validates
those artifacts and stages them under
`/usr/local/share/op-dbus/dashboard/vercel-json-render/`. It never deploys or
uses the external checkout as a build input.
