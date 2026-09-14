#!/usr/bin/env python3
"""Archive one or-fusion turn into /home/jeremy/.notebooklm-sources/openrouter-fusion/."""
from __future__ import annotations

import hashlib
import json
import os
import sys
import time
from pathlib import Path


def main() -> int:
    if os.environ.get("OR_FUSION_NO_EXPORT", "0").lower() in ("1", "true", "yes"):
        return 0

    user = os.environ.get("OR_FUSION_EXPORT_USER", "")
    system = os.environ.get("OR_FUSION_EXPORT_SYSTEM", "")
    content = sys.stdin.read() if not sys.stdin.isatty() else (sys.argv[1] if len(sys.argv) > 1 else "")
    # Also accept content file
    if len(sys.argv) > 1 and sys.argv[1] == "--file":
        content = Path(sys.argv[2]).read_text(encoding="utf-8")
    if not content.strip():
        return 0

    root = Path(os.environ.get("NOTEBOOKLM_SOURCES_DIR", "/home/jeremy/.notebooklm-sources"))
    folder = root / "openrouter-fusion"
    folder.mkdir(parents=True, exist_ok=True)

    model = os.environ.get("OR_FUSION_EXPORT_MODEL", "openrouter/fusion")
    preset = os.environ.get("OR_FUSION_EXPORT_PRESET", "")
    judge = os.environ.get("OR_FUSION_EXPORT_JUDGE", "")
    ts = time.strftime("%Y%m%dT%H%M%SZ", time.gmtime())
    hid = hashlib.sha256((user + content).encode()).hexdigest()[:10]
    base = f"or-fusion_{ts}_{hid}"
    title = (user.strip().splitlines() or ["fusion"])[0][:80]

    turns = []
    md = [
        f"# Fusion: {title}",
        f"**CLI:** or-fusion",
        f"**Model:** {model}",
        f"**Preset:** {preset}",
        f"**Date:** {ts}",
        "",
        "---",
        "",
    ]
    n = 1
    if system:
        turns.append({"role": "system", "content": system})
        md += [f"## Turn {n}: System", system, ""]
        n += 1
    turns.append({"role": "user", "content": user})
    md += [f"## Turn {n}: User", user, ""]
    n += 1
    turns.append({"role": "assistant", "content": content})
    md += [f"## Turn {n}: Assistant", content, ""]

    md_path = folder / f"{base}.md"
    json_path = folder / f"{base}.json"
    md_path.write_text("\n".join(md), encoding="utf-8")
    payload = {
        "session_id": base,
        "cli_tool": "or-fusion",
        "model": model,
        "preset": preset,
        "judge": judge,
        "title": title,
        "date": ts,
        "turns": turns,
        "metadata": {
            "stream": os.environ.get("OR_FUSION_EXPORT_STREAM", "0") == "1",
        },
    }
    meta_raw = os.environ.get("OR_FUSION_EXPORT_META_JSON")
    if meta_raw:
        try:
            payload["metadata"].update(json.loads(meta_raw))
        except Exception:
            pass
    json_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"[or-fusion] archived → {md_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
