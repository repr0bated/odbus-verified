# LLM session export → `/home/jeremy/.notebooklm-sources`

Local exports collect conversations **and tool calls** into model-sorted folders.
The scheduled NotebookLM sync separately captures raw records and uses the
identity-bound 3tchedFS namespace. Formatted exports are preserved as explicitly
labelled fallback history when the original transcript is unavailable.

The received `notebook-sources-sync` service selects the existing human session
and approved `/run/mount/3tchedFS-native` mount explicitly. Its code comes from
the received release; project documents come from `/srv/git/odbus-verified`, and
the corpus plus upload journal stay in `~/.notebooklm-sources/.grouped-sync`.
There is no direct `nlm`, gRPC or unauthenticated D-Bus fallback in sync.

The client resumes the exact saved plan before capturing later appends. It
records mutation intent before dispatch and retains receipts; an unknown
outcome is not permission to repeat an upload or deletion. Old owned sources
remain until replacements are readable. Exit 0 means the cycle completed,
2 means processing or bounded work remains, and other exits mean failure.
The service publishes that distinction in
`/run/opdbus/notebook-sync/status.json`; a running process alone is not proof
of synchronization. Tests are owned by the separate testing agent for the
2026-10-05 repair; deployment and live synchronization remain pending.

## Storage reality

| CLI | Where history lives | Session lifetime |
| --- | --- | --- |
| **OpenCode** | SQLite `~/.local/share/opencode/opencode.db` (`session` / `message` / `part`) | **Persistent** (DB rows grow across runs) |
| **Kilo** | SQLite `~/.local/share/kilo/kilo.db` (same shape) | **Persistent** |
| **Codex** | SQLite `~/.codex/state_5.sqlite` + rollout JSONL | Per thread; rollouts grow |
| **AGY** | `conversations/*.db` + `brain/*/transcript.jsonl` | Per conversation |
| **Cursor** | `agent-transcripts/**/*.jsonl` (+ thin `store.db` meta) | Per agent id |
| **Factory / droid** | `~/.factory/sessions/**/*.jsonl` | **Persistent** (same JSONL append) |
| **Grok** | `~/.grok/sessions/**/chat_history.jsonl` | Per session dir |
| **Claude** | `~/.claude/projects/**/*.jsonl` | Per session file |

Because **factory** and **opencode** (and kilo) do not “close” a session on CLI exit, exit hooks must **re-read the store** and only rewrite when content changes.

## Commands

```bash
# Full backfill (force rewrite everything, refresh watermarks)
llm-session-backfill -v
# or
python3 ~/git/odbus/scripts/export-llm-sessions-to-notebooklm-sources.py --backfill -v

# Incremental (default): skip unchanged content hashes
llm-session-on-exit              # all CLIs
llm-session-on-exit opencode     # one CLI
llm-session-on-exit factory

# Optional push uses the same explicitly configured mounted sync client.
# It requires the OPDBUS_NOTEBOOK_* settings from the received service.
llm-session-backfill --sync
```

Output:

```text
/home/jeremy/.notebooklm-sources/
  <model-slug>/
    opencode_<id>.md|.json
    factory_<id>.md|.json
    codex_<id>.md|.json
    …
  MANIFEST.json
  .export-state.json    # per-session content hashes (watermark)
```

## Triggers

1. **Prepared sync service:** one cycle after dependencies become ready, then
   300 seconds after each cycle ends by default. This service is currently
   stopped pending the combined release; the repaired source is not active.
2. **Optional CLI/shell exit hook:** `scripts/llm-session-on-exit.sh` supports
   export on exit, but the 2026-10-05 inspection found no matching wiring in
   Jeremy's current `.bashrc` or `.profile`, and no installed
   `llm-session-on-exit` at the checked user/local bin paths. Automatic exit
   capture is therefore unverified; older claims that it was installed are
   superseded.
3. **Manual backfill:** `scripts/llm-session-backfill.sh` performs local export.
   The separately configured mounted sync can also be invoked manually.

Persistent CLIs: exit ≠ end of conversation. The watermark in `.export-state.json` is the trick — every exit re-scans the DB/JSONL; only new turns change the hash and rewrite the NotebookLM source file.

## NotebookLM

The client uses the mounted declarations for `notebook_list`, `notebook_get`,
`notebook_create`, `source_add`, `source_get_content` and `source_delete`.
The installed `threetched-fs` helper validates arguments, exact session,
permissions, result contracts and operation receipts. Backend grants remain
unchanged. Native router autonomy remains read-only; the separately requested
sync service performs its owned corpus mutations under existing human grants.

Manual runs require explicit `OPDBUS_NOTEBOOK_SYNC_MODE=per-model`,
`OPDBUS_NOTEBOOK_FS_ROOT`, `OPDBUS_NOTEBOOK_SESSION_ID`,
`OPDBUS_NOTEBOOK_PROJECT_ROOT` and `NOTEBOOKLM_SOURCES_DIR`. Use
`scripts/notebook-sources-sync-up`; no notebook or session is guessed from a
title, source address or credential helper.

## Legacy local export cleanup

This older cleanup maintains local formatted export bundles. It is not part
of the raw grouped sync pipeline and must not replace its pending snapshots:

```bash
# one agent does all model folders (default)
notebook-sources-cleanup --archive-sessions -v

# or one folder at a time
notebook-sources-cleanup --folder kimi-k2.7-code --archive-sessions -v

# dry-run
notebook-sources-cleanup --dry-run -v
```

- New conversations are **cat'd to the end** of the latest `_bundle_NNN.md` in each model folder.
- State: `/home/jeremy/.notebooklm-sources/.cleanup-state.json` (per-file content hashes — no dupes).
- Rolled files move to `<folder>/_archived_sessions/` when `--archive-sessions`.
- Cap: `--max-sources 300` (default), rotate bundles at **195 MiB** (`--max-bundle-bytes`, env `NLM_MAX_BUNDLE_BYTES`).
- **OpenCode** + **Kilo** use the same path as factory/codex: export sessions → append into that model’s `_bundle_*.md` → archive loose files.
- The optional exit-hook script invokes cleanup after export; installed hook
  activation is unverified as described above.

The grouped sync reads original native histories and records missing-original
gaps explicitly. It does not upload these legacy bundles through a second
provider transport.
