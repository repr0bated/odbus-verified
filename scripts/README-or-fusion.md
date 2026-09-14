# `or-fusion` — OpenRouter Fusion CLI

**Binary name:** `or-fusion`  
**Paths:** `scripts/or-fusion`, installed as `/usr/local/bin/or-fusion` (and `~/.local/bin/or-fusion`)  
**Archive helper:** `scripts/or-fusion-archive.py`

Shell CLI for multi-model deliberation via [`openrouter/fusion`](https://openrouter.ai/openrouter/fusion).

A panel of models analyzes your prompt in parallel (with web search + fetch), a judge synthesizes structured analysis (consensus, contradictions, partial coverage, unique insights, blind spots), and the final answer is written from that analysis. Cost = sum of panel + judge completions.

## NotebookLM archive (every message)

On each successful completion, `or-fusion` writes:

```text
/home/jeremy/.notebooklm-sources/openrouter-fusion/or-fusion_<timestamp>_<id>.md
/home/jeremy/.notebooklm-sources/openrouter-fusion/or-fusion_<timestamp>_<id>.json
```

Disable with `--no-export` or `OR_FUSION_NO_EXPORT=1`.

## Install

```bash
# from repo
sudo install -m 0755 scripts/or-fusion /usr/local/bin/or-fusion
sudo install -m 0755 scripts/or-fusion-archive.py /usr/local/libexec/3tched/or-fusion-archive.py
export FUSION_API_KEY=sk-or-v1-...
```

## Usage

```bash
or-fusion "Compare ridge, lasso, and elastic-net regression"
or-fusion -p budget "Cheaper multi-model research pass"
or-fusion -p fast --stream "Latency-oriented multi-model turn"
or-fusion -m '~google/gemini-flash-latest,deepseek/deepseek-v4-flash' \
          -j '~openai/gpt-latest' "Custom panel + judge"
or-fusion -s "Be terse" -f notes.md "Summarize with multi-model critique"
echo "prompt" | or-fusion --json
```

### Presets (`-p` / `OR_FUSION_PRESET`)

| Slug | Alias | Panel |
| --- | --- | --- |
| `general-high` | `high`, `quality` | Claude Opus + GPT + Gemini Pro (default) |
| `general-budget` | `budget` | Gemini Flash + DeepSeek V4 Flash + Kimi |
| `general-fast` | `fast` | Same budget panel, latency-homogeneous |

Plugin fields map to OpenRouter’s fusion plugin: `preset`, `analysis_models`, `model` (judge), `max_tool_calls`.

## Auth / headers

- `FUSION_API_KEY` — preferred (OpenRouter key)  
- `OPENROUTER_API_KEY` / `OPENROUTER_KEY` — fallbacks if `FUSION_API_KEY` is unset  
- `OR_HTTP_REFERER`, `OR_APP_TITLE` — optional ranking headers  
- Docs: <https://openrouter.ai/docs/guides/features/plugins/fusion>
