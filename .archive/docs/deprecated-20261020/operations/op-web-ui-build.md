# op-web UI Build and Embed

This project embeds the web UI into the `op-web` binary from `crates/op-web/ui/dist`.

## Development Build (WASM optional)

From repo root:

```bash
cd crates/op-web/ui
npm ci
npm run build
cd ../../..
cargo check -p op-web
```

Behavior:
- If `wasm-pack` is available, the Rust WASM decoder is built.
- If `wasm-pack` is not available, a JS fallback decoder is generated automatically.

## Production Build (WASM required)

```bash
./scripts/install-wasm-pack.sh
cd crates/op-web/ui
npm ci
npm run build:prod
cd ../../..
cargo build -p op-web --release
```

Behavior:
- `npm run build:prod` requires `wasm-pack` and fails if unavailable.
- `cargo build -p op-web --release` fails if `ui/dist/index.html` is missing.
- On musl-only hosts, if wasm link fails, build falls back to JS decoder unless `FORCE_STRICT_WASM=1` is set.

## Notes

- Embedded UI is served by `crates/op-web/src/embedded_ui.rs`.
- Release builds should always use `build:prod` to avoid silent fallback behavior.
- CI enforces this path via `.github/workflows/op-web-production.yml`.
