Correction accepted: the deleted-source inspection supersedes my earlier install guidance.

- `install_skill` mapping **must include `"--no-tier-banner":true`** to preserve the old wrapper’s invocation.
- `remove_bundle` mapping includes **`"--yes":true`**, now confirmed from deleted source as well as native behavior.
- Native hashes remain unchanged.
- `edit_skill(contents)` is retired; Main reports editor-dependent native calls now reject before spawn.

No further research or changes from this scout.