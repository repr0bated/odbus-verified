# `op-plugin-lint` — complete plugin from `.rs` + introspect gaps

**Goal:** CLI takes a plugin `.rs` and writes a full contract-shaped plugin
document (fields, typed methods, audit). With `--introspect`, also lists what
upstream discovery found that the plugin does **not** have.

ZeroClaw is just one example of that workflow.

```bash
cd /home/jeremy/zeroclaw && repomix   # once

cargo run -p op-plugin-lint -- \
  --input crates/op-plugins/src/state_plugins/zeroclaw.rs \
  --output /tmp/zeroclaw.complete.json \
  --format complete \
  --introspect /home/jeremy/zeroclaw/repomix-output.xml \
  --surface-out /tmp/zeroclaw.surface.json
```

| Output section | Meaning |
|---|---|
| `plugin.fields` / `plugin.methods` | Complete plugin from the `.rs` (typed args/returns) |
| `audit` | Contract lint findings |
| `introspect.gaps` | Upstream findings **missing from the plugin** |

Example artifacts in this folder: `zeroclaw.complete.json`, `zeroclaw.complete.md`.
