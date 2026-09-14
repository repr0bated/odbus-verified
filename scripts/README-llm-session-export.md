# LLM session export → `/home/jeremy/.notebooklm-sources`

Collect conversations **and tool calls** from all agent CLIs into model-sorted folders for NotebookLM MCP (`add_source_file` / source sync).

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

# Optional NotebookLM push (needs OP_NOTEBOOK_ID + nlm CLI)
OP_NOTEBOOK_ID=... llm-session-backfill --sync
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

1. **CLI wrapper exit** (`~/.bashrc`): `claude`, `codex`, `opencode`, `grok`, `kilo`, `droid`/`factory`, `agy`, `agent` → `_post_cli_export` → `llm-session-on-exit <cli>` (background).
2. **Shell EXIT trap**: same incremental export for anything still dirty when the shell dies.
3. **Manual backfill**: `llm-session-backfill` when you want a clean full rewrite (e.g. before a NotebookLM sync).

Persistent CLIs: exit ≠ end of conversation. The watermark in `.export-state.json` is the trick — every exit re-scans the DB/JSONL; only new turns change the hash and rewrite the NotebookLM source file.

## NotebookLM

Plugin methods: `add_source_file`, `list_sources`, `sync_drive_sources`.  
Point sync at `/home/jeremy/.notebooklm-sources` (or per-file paths from `MANIFEST.json`). Override with `NOTEBOOKLM_SOURCES_DIR`.

## Keep sources ≤300 (cleanup agent)

NotebookLM caps sources; we roll per-session files into append-only bundles:

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
- Exit hooks run cleanup after export so the tree stays under the cap.

Then re-ingest bundles only (preferred):

```bash
notebook-sync ingest   # uses title key; prefer adding only _bundle_*.md if you filter
```