# op-llm Feature Review

## Summary
- Status: Partial
- Build: `cargo check -p op-llm` passed
- Tests in tree: 7
- Static incompleteness markers: 2
- Patch / backup artifacts in tree: 0
- Purpose: LLM provider integration with dynamic model discovery for HuggingFace
- Assessment: op-llm builds, but the codebase still exposes unfinished paths or contract drift relative to its advertised purpose.

## Spec References
- `crates/crates/op-llm/SPEC.md`
- `crates/crates/SPECS/19-op-llm.md`

## Coded Features
- Public/module surface: anthropic, antigravity, chat, gcloud_adc, gemini, gemini_cli, headless_oauth, huggingface, mcp_proxy, openclaw, perplexity, provider, pty_bridge, prelude
- Source files under `src/` recursively: 15

## Alignment Review
- Compared against `crates/crates/op-llm/SPEC.md` and `crates/crates/SPECS/19-op-llm.md` plus the crate source tree.

## Missing Or Risky Areas
- Provider integrations are extensive and build, but some provider behaviors are still explicitly incomplete, such as non-streaming example paths and several auth/PTY warning-heavy code paths.
- Static scan found 2 TODO/stub/placeholder markers in this crate.

## Verification Notes
- `cargo check -p op-llm` passed
- Static scan counted 7 test markers and 2 TODO/stub markers in this crate.

