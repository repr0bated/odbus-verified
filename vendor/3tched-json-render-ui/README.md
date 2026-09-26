# Pinned json-render UI source

This directory closes the source and release boundary for the active standalone
Vite UI. `source.tar.gz` is a deterministic archive of the explicitly
allowlisted files from the tested production checkout at `/srv/git/3tched.com`;
it records the source checkout HEAD and the tested modified worktree. The content manifest covers the exact archived bytes.

The archive contains `AGENTS.md`, licensing and guidance, `package.json`, the
lockfile, Vite/TypeScript/ESLint configuration, `index.html`, and `src/` (plus
`public/` when present). It excludes `.git`, `.env*`, `node_modules`, `dist`,
logs and deployment scripts.

Build without touching the external checkout or live dashboard:

```sh
vendor/3tched-json-render-ui/build-assets.sh \
  --npm-cache /var/cache/op-dbus-build/npm \
  --output-dir target/ui-artifacts/3tched-json-render-ui
```

The helper verifies every source file, runs the repository-required `npm ci`,
type check, lint, tests and Vite build at the dashboard root (`/`) with the
production door default `https://dashboard.ghostbridge.tech:8090`, then emits `assets.tar.gz` and `manifest.json`. Golden preparation validates those artifacts and stages only
the UI's `assets/` and `index.html` under
`/usr/local/share/op-dbus/dashboard/`, preserving sibling mounts and files. It
never deploys or uses the external checkout as a build input.
