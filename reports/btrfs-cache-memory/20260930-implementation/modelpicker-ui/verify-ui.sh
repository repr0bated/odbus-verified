#!/bin/bash
set -eu
receipt_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
cd /srv/git/3tched.com
node --version > "$receipt_dir/node-version.txt"
npm run type-check > "$receipt_dir/type-check.log" 2>&1
npm run lint > "$receipt_dir/lint.log" 2>&1
NODE_OPTIONS=--no-experimental-webstorage npm test > "$receipt_dir/tests.log" 2>&1
npm run build > "$receipt_dir/build.log" 2>&1
git diff --check > "$receipt_dir/diff-check.log" 2>&1
sha256sum src/chat/model-catalog.ts src/chat/model-catalog.test.ts src/chat/chat-adapter.ts src/chat/door-chat-adapter.ts src/chat/door-chat-adapter.test.ts src/chat/ChatRuntimeProvider.tsx src/chat/ChatRuntimeProvider.test.tsx src/json-render/catalog/chat-registry.tsx src/json-render/pages/antigravity.pagespec.test.tsx src/styles.css > "$receipt_dir/ui-source.sha256"
git diff -- src/chat/model-catalog.ts src/chat/chat-adapter.ts src/chat/door-chat-adapter.ts src/chat/door-chat-adapter.test.ts src/chat/ChatRuntimeProvider.tsx src/json-render/catalog/chat-registry.tsx src/json-render/pages/antigravity.pagespec.test.tsx src/styles.css > "$receipt_dir/ui-source.diff"
