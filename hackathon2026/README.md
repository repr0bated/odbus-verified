# Hackathon 2026 conversation archive

Local conversations and tool-use records from files written on or after 2026-10-26 00:00 UTC.
That Monday is the start of this collection window. The archive was built on 2026-11-01.

This directory is gitignored. Do not force-add it. Copied text is redacted for private keys, API-key assignments, and SID1/OIA1 envelopes. The original stores were not modified.

Indexed at 2026-11-01T18:33:09Z. Files: 854. Bytes: 633484483.

Per-file detail is in `manifest.jsonl`. Tool-name counts are in `summary.json`.

## Sources

- claude: 31 files, 23211 records, 7570 tool events, 124424977 bytes
- codex: 18 files, 6714 records, 1742 tool events, 35481303 bytes
- cursor: 8 files, 714 records, 927 tool events, 640439 bytes
- gemini: 95 files, 11999 records, 4366 tool events, 16149592 bytes
- grok: 372 files, 225630 records, 18518 tool events, 253869825 bytes
- hermes: 7 files, 2485 records, 240 tool events, 1832771 bytes
- junie: 17 files, 17 records, 0 tool events, 357 bytes. Each `transcript.md` is only the heading `# Session transcript`. Two sessions in the window have settings and no transcript. Junie logs from those days do not contain the dialogue.
- kiro: 7 files, 425 records, 207 tool events, 571945 bytes
- omp: 299 files, 58411 records, 19647 tool events, 200513274 bytes

## Checked and not copied

- opencode: 0 sessions updated on or after cutoff. Latest session update 2026-10-24T01:19:11Z. Database not copied because it contains a credential table.
- kilo: 0 sessions updated on or after cutoff. Latest session update 2026-10-24T19:16:07Z. Database not copied because it contains a credential table.
- factory: 0 *.jsonl files written on or after cutoff under /home/jeremy/.factory/sessions
- pi: 0 *.jsonl files written on or after cutoff under /home/jeremy/.pi/agent/sessions
- paseo: 0 *.json files written on or after cutoff under /home/jeremy/.paseo/agent-requests
- claude-server-commander: tool-history.jsonl last written 2026-10-23T09:25:10Z, before the cutoff, so it was not copied.
- root-codex: /root/.codex is not readable by this user
- root-claude: /root/.claude is not readable by this user
- root-grok: /root/.grok is not readable by this user
- 3tched-claude: /home/3tched/.claude is not readable by this user
- 3tched-codex: /home/3tched/.codex is not readable by this user
- 3tched-grok: /home/3tched/.grok is not readable by this user
- notebooklm-sources: derived capture left in place at /home/jeremy/.notebooklm-sources/.local-capture: 4118 files touched since cutoff, 30316830277 bytes. Not duplicated into this archive.
- gemini-sqlite: Antigravity conversation *.db files are protobuf trajectory stores. Readable transcripts were copied from brain/**/transcript.jsonl. The databases were not copied.

A session file still written after the cutoff is included whole, so it can contain turns from before Monday.
OpenCode and Kilo had no session updated in the window. Their databases also hold credential tables, so those files stayed in place.

