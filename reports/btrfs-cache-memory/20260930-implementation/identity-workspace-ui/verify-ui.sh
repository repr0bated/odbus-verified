#!/bin/bash
set -eu
receipt_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
cd /srv/git/3tched.com
npm run type-check > "$receipt_dir/type-check.log" 2>&1
npm run lint > "$receipt_dir/lint.log" 2>&1
NODE_OPTIONS=--no-experimental-webstorage npm test > "$receipt_dir/tests.log" 2>&1
npm run build > "$receipt_dir/build.log" 2>&1
git diff --check > "$receipt_dir/diff-check.log" 2>&1
sha256sum src/workbench/*.ts src/workbench/*.tsx src/workbench/workbench.css src/json-render/catalog/chat-catalog.ts src/json-render/catalog/chat-registry.tsx src/json-render/pages/antigravity.pagespec.ts src/json-render/runtime/JsonRenderProvider.tsx > "$receipt_dir/ui-source.sha256"
