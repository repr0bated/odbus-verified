#!/usr/bin/env python3
"""
Collect LLM CLI conversations + tool calls → /home/jeremy/.notebooklm-sources/<model>/

Sources (DB-backed called out explicitly):
  - OpenCode   ~/.local/share/opencode/opencode.db   (session/message/part)
  - Kilo       ~/.local/share/kilo/kilo.db           (same schema family)
  - Codex      ~/.codex/state_5.sqlite + rollout JSONL
  - AGY        ~/.gemini/antigravity-cli/conversations/*.db + brain/*/transcript.jsonl
  - Cursor     ~/.cursor/projects/*/agent-transcripts/**/*.jsonl (+ chats/*/store.db meta)
  - Factory    ~/.factory/sessions/**/*.jsonl
  - Grok       ~/.grok/sessions/**/chat_history.jsonl
  - Claude     ~/.claude/projects/**/*.jsonl

Output per session:
  /home/jeremy/.notebooklm-sources/<model-slug>/{cli}_{session_id}.md
  /home/jeremy/.notebooklm-sources/<model-slug>/{cli}_{session_id}.json
  /home/jeremy/.notebooklm-sources/MANIFEST.json   (for NotebookLM sync_sources / add_source_file)

Usage:
  python3 scripts/export-llm-sessions-to-notebooklm-sources.py -v
  python3 scripts/export-llm-sessions-to-notebooklm-sources.py --only opencode,codex,agy,cursor
  python3 scripts/export-llm-sessions-to-notebooklm-sources.py --sync   # best-effort MCP/CLI sync
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import time
from collections import defaultdict
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Tuple

HOME = Path.home()
EXPORT_ROOT = Path(os.environ.get("NOTEBOOKLM_SOURCES_DIR", "/home/jeremy/.notebooklm-sources"))

class Cfg:
    """Runtime export config. Persistent CLIs (factory/opencode) rely on hash watermarks."""
    export_root = EXPORT_ROOT
    force = False  # True during --backfill
    state_path = EXPORT_ROOT / ".export-state.json"


def get_state() -> dict:
    path = Cfg.state_path
    if path.is_file():
        try:
            return json.loads(path.read_text())
        except Exception:
            return {"sessions": {}}
    return {"sessions": {}}


def put_state(state: dict) -> None:
    Cfg.state_path.parent.mkdir(parents=True, exist_ok=True)
    tmp = Cfg.state_path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, indent=2, sort_keys=True))
    tmp.replace(Cfg.state_path)

TOOL_OUT_MAX = int(os.environ.get("EXPORT_TOOL_OUT_MAX", "12000"))
SKIP_SYSTEM_NOISE = True


# ---------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------

def slugify(s: Any) -> str:
    text = str(s or "default").lower().strip()
    text = re.sub(r"[^\w\s.-]", "-", text)
    text = re.sub(r"[\s_/]+", "-", text)
    text = re.sub(r"-+", "-", text).strip("-.")
    return text or "default"


def fmt_ts(ts: Any) -> str:
    if ts is None:
        return time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime())
    if isinstance(ts, str):
        return ts
    try:
        t = float(ts)
        if t > 1e12:
            t /= 1000.0
        return time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime(t))
    except Exception:
        return str(ts)


def truncate(s: str, n: int = TOOL_OUT_MAX) -> str:
    s = s if isinstance(s, str) else json.dumps(s, ensure_ascii=False, default=str)
    if len(s) <= n:
        return s
    return s[:n] + f"\n… [truncated {len(s) - n} chars]"


def redact(s: str) -> str:
    """Light redaction so NotebookLM sources don't store live secrets."""
    patterns = [
        (r"(?i)(api[_-]?key|token|password|secret|authorization)\s*[:=]\s*['\"]?([^\s'\"]+)",
         r"\1=***"),
        (r"sk-[a-zA-Z0-9_-]{10,}", "sk-***"),
        (r"sk-or-v1-[a-zA-Z0-9_-]{10,}", "sk-or-v1-***"),
        (r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
         "[REDACTED PRIVATE KEY]"),
    ]
    out = s
    for pat, rep in patterns:
        out = re.sub(pat, rep, out)
    return out


def open_ro(db: Path) -> Optional[sqlite3.Connection]:
    if not db.is_file():
        return None
    try:
        conn = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        return conn
    except Exception as e:
        sys.stderr.write(f"[export] sqlite open failed {db}: {e}\n")
        return None


def content_to_text(content: Any) -> str:
    if content is None:
        return ""
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for c in content:
            if isinstance(c, str):
                parts.append(c)
            elif isinstance(c, dict):
                if "text" in c:
                    parts.append(str(c["text"]))
                elif c.get("type") in ("input_text", "output_text", "text"):
                    parts.append(str(c.get("text") or c.get("content") or ""))
                else:
                    parts.append(json.dumps(c, ensure_ascii=False, default=str)[:2000])
        return "\n".join(p for p in parts if p)
    if isinstance(content, dict):
        if "text" in content:
            return str(content["text"])
        return json.dumps(content, ensure_ascii=False, default=str)[:4000]
    return str(content)


Turn = Dict[str, Any]


def save_session(
    model: str,
    cli: str,
    session_id: str,
    title: str,
    date_str: str,
    turns: List[Turn],
    meta: Optional[dict] = None,
) -> Optional[Path]:
    """Write session files. Returns path if written/updated, None if skipped/empty.

    Persistent CLIs (factory, opencode, kilo): sessions never "end" cleanly.
    We content-hash turns and skip rewrite when unchanged unless Cfg.force
    (backfill). Exit hooks still call export; they only emit deltas.
    """
    if not turns:
        return None
    # drop pure-empty
    turns = [t for t in turns if (t.get("content") or "").strip() or t.get("tool")]
    if not turns:
        return None

    source_hash = hashlib.sha256(
        json.dumps(turns, sort_keys=True, default=str).encode()
    ).hexdigest()[:16]
    state_key = f"{cli}:{session_id}"
    state = getattr(save_session, "_state", None)
    if state is None:
        state = get_state()
        save_session._state = state  # type: ignore[attr-defined]
    prev = (state.get("sessions") or {}).get(state_key) or {}
    if not getattr(Cfg, "force", False) and prev.get("hash") == source_hash:
        return None  # unchanged — critical for long-lived factory/opencode sessions

    model_dir = getattr(Cfg, "export_root", EXPORT_ROOT) / slugify(model)
    model_dir.mkdir(parents=True, exist_ok=True)
    base = f"{slugify(cli)}_{slugify(session_id)[:48]}"
    md_path = model_dir / f"{base}.md"
    json_path = model_dir / f"{base}.json"

    lines = [
        f"# {title or session_id}",
        f"**CLI:** {cli}",
        f"**Model:** {model}",
        f"**Session ID:** {session_id}",
        f"**Date:** {date_str}",
        "",
        "---",
        "",
    ]
    for i, t in enumerate(turns, 1):
        role = (t.get("role") or "unknown").capitalize()
        if t.get("tool"):
            lines.append(f"## Turn {i}: Tool `{t['tool']}`")
            if t.get("call_id"):
                lines.append(f"*call_id:* `{t['call_id']}`")
            if t.get("input") is not None:
                lines.append("### Input")
                lines.append("```")
                lines.append(redact(truncate(content_to_text(t["input"]), 8000)))
                lines.append("```")
            if t.get("output") is not None:
                lines.append("### Output")
                lines.append("```")
                lines.append(redact(truncate(content_to_text(t["output"]))))
                lines.append("```")
        else:
            lines.append(f"## Turn {i}: {role}")
            lines.append(redact(truncate(t.get("content") or "", 50000)))
        lines.append("")

    md_path.write_text("\n".join(lines), encoding="utf-8")
    payload = {
        "session_id": session_id,
        "cli_tool": cli,
        "model": model,
        "title": title,
        "date": date_str,
        "turns": turns,
        "metadata": meta or {},
        "source_hash": hashlib.sha256(
            json.dumps(turns, sort_keys=True, default=str).encode()
        ).hexdigest()[:16],
    }
    json_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False, default=str), encoding="utf-8")

    # watermark for incremental / persistent sessions
    st = getattr(save_session, "_state", None)
    if st is not None:
        st.setdefault("sessions", {})[f"{cli}:{session_id}"] = {
            "hash": source_hash,
            "model": model,
            "cli": cli,
            "path": str(md_path),
            "updated_at": time.time(),
            "turn_count": len(turns),
        }
        # debounce disk writes: flush at end of exporter run via flush_state()
        save_session._dirty = True  # type: ignore[attr-defined]
    return md_path


def flush_state() -> None:
    st = getattr(save_session, "_state", None)
    if st is not None and getattr(save_session, "_dirty", False):
        put_state(st)
        save_session._dirty = False  # type: ignore[attr-defined]


# ---------------------------------------------------------------------------
# OpenCode / Kilo (shared message+part schema)
# ---------------------------------------------------------------------------

def export_opencode_family(db_path: Path, cli_name: str) -> int:
    conn = open_ro(db_path)
    if not conn:
        return 0
    count = 0
    try:
        sessions = conn.execute("SELECT * FROM session").fetchall()
    except Exception as e:
        sys.stderr.write(f"[export] {cli_name} session table: {e}\n")
        conn.close()
        return 0

    for sess in sessions:
        sid = sess["id"]
        title = sess["title"] if "title" in sess.keys() else sid
        model_raw = sess["model"] if "model" in sess.keys() else None
        model = "default"
        if model_raw:
            try:
                m = json.loads(model_raw) if isinstance(model_raw, str) and model_raw.startswith("{") else model_raw
                if isinstance(m, dict):
                    model = m.get("id") or m.get("modelID") or m.get("providerID") or "default"
                else:
                    model = str(m)
            except Exception:
                model = str(model_raw)
        # enrich from first assistant message
        try:
            msgs = conn.execute(
                "SELECT id, time_created, data FROM message WHERE session_id=? ORDER BY time_created",
                (sid,),
            ).fetchall()
        except Exception:
            continue

        turns: List[Turn] = []
        for msg in msgs:
            try:
                data = json.loads(msg["data"])
            except Exception:
                continue
            role = data.get("role") or "unknown"
            if role == "assistant":
                model = data.get("modelID") or data.get("model", {}).get("modelID") if isinstance(data.get("model"), dict) else data.get("modelID") or model
                if isinstance(data.get("model"), dict):
                    model = data["model"].get("modelID") or data["model"].get("id") or model

            parts = conn.execute(
                "SELECT data, time_created FROM part WHERE message_id=? ORDER BY time_created",
                (msg["id"],),
            ).fetchall()
            text_chunks: List[str] = []
            for part in parts:
                try:
                    p = json.loads(part["data"])
                except Exception:
                    continue
                ptype = p.get("type")
                if ptype == "text" and p.get("text"):
                    text_chunks.append(p["text"])
                elif ptype == "reasoning" and p.get("text"):
                    # keep short reasoning breadcrumb
                    turns.append({"role": "assistant", "content": f"[reasoning]\n{truncate(p['text'], 2000)}"})
                elif ptype == "tool":
                    st = p.get("state") or {}
                    turns.append({
                        "role": "tool",
                        "tool": p.get("tool") or st.get("tool") or "tool",
                        "call_id": p.get("callID") or p.get("call_id"),
                        "input": st.get("input") or p.get("input"),
                        "output": st.get("output") or p.get("output"),
                    })
            if text_chunks:
                turns.append({"role": role, "content": "\n".join(text_chunks)})

        date_str = fmt_ts(sess["time_created"] if "time_created" in sess.keys() else None)
        meta = {k: sess[k] for k in sess.keys() if k not in ("permission",)}
        if save_session(model, cli_name, sid, title, date_str, turns, meta):
            count += 1
    conn.close()
    return count


# ---------------------------------------------------------------------------
# Codex (DB index + JSONL rollouts)
# ---------------------------------------------------------------------------

def export_codex() -> int:
    state = HOME / ".codex" / "state_5.sqlite"
    conn = open_ro(state)
    if not conn:
        return 0
    count = 0
    try:
        threads = conn.execute("SELECT * FROM threads").fetchall()
    except Exception as e:
        sys.stderr.write(f"[export] codex threads: {e}\n")
        conn.close()
        return 0

    for th in threads:
        sid = th["id"]
        model = th["model"] or th["model_provider"] or "default"
        title = th["title"] or th["first_user_message"] or sid
        rollout = th["rollout_path"]
        turns: List[Turn] = []
        if rollout and Path(rollout).is_file():
            try:
                with open(rollout, encoding="utf-8") as f:
                    for line in f:
                        if not line.strip():
                            continue
                        try:
                            e = json.loads(line)
                        except Exception:
                            continue
                        et = e.get("type")
                        p = e.get("payload") or {}
                        if et == "event_msg":
                            mt = p.get("type")
                            if mt == "user_message" and p.get("message"):
                                turns.append({"role": "user", "content": p["message"]})
                            elif mt == "agent_message" and p.get("message"):
                                # prefer final over commentary noise somewhat
                                turns.append({"role": "assistant", "content": p["message"]})
                        elif et == "response_item":
                            pt = p.get("type")
                            if pt == "message" and p.get("role") in ("user", "assistant"):
                                text = content_to_text(p.get("content"))
                                if text and p.get("role") != "developer":
                                    turns.append({"role": p["role"], "content": text})
                            elif pt in ("function_call", "custom_tool_call"):
                                args = p.get("arguments") or p.get("input") or p.get("args")
                                turns.append({
                                    "role": "tool",
                                    "tool": p.get("name") or "tool",
                                    "call_id": p.get("call_id") or p.get("id"),
                                    "input": args,
                                })
                            elif pt in ("function_call_output", "custom_tool_call_output"):
                                out = p.get("output")
                                turns.append({
                                    "role": "tool",
                                    "tool": "(output)",
                                    "call_id": p.get("call_id"),
                                    "output": content_to_text(out),
                                })
            except Exception as e:
                sys.stderr.write(f"[export] codex rollout {rollout}: {e}\n")

        date_str = fmt_ts(th["created_at_ms"] or th["created_at"])
        if save_session(model, "codex", sid, title, date_str, turns, dict(th)):
            count += 1
    conn.close()
    return count


# ---------------------------------------------------------------------------
# AGY / Antigravity (per-conversation DB + transcript JSONL)
# ---------------------------------------------------------------------------

def export_agy() -> int:
    brain = HOME / ".gemini" / "antigravity-cli" / "brain"
    conv_dir = HOME / ".gemini" / "antigravity-cli" / "conversations"
    count = 0
    if not brain.is_dir() and not conv_dir.is_dir():
        return 0

    sids = set()
    if brain.is_dir():
        sids.update(p.name for p in brain.iterdir() if p.is_dir())
    if conv_dir.is_dir():
        sids.update(p.stem for p in conv_dir.glob("*.db"))

    for sid in sorted(sids):
        model = "gemini-3.5-flash"
        # try model from db gen_metadata / path
        db_path = conv_dir / f"{sid}.db"
        if db_path.is_file():
            conn = open_ro(db_path)
            if conn:
                try:
                    for row in conn.execute("SELECT data FROM gen_metadata"):
                        blob = row["data"]
                        if isinstance(blob, memoryview):
                            blob = blob.tobytes()
                        if not blob:
                            continue
                        text = blob.decode("utf-8", errors="ignore")
                        for key in ("gemini", "claude", "gpt", "flash", "pro", "sonnet", "laguna"):
                            if key in text.lower():
                                # grab a token-ish model id
                                m = re.search(r"[A-Za-z0-9._-]{0,20}" + re.escape(key) + r"[A-Za-z0-9._-]{0,30}", text, re.I)
                                if m:
                                    model = m.group(0)
                                    break
                except Exception:
                    pass
                conn.close()

        turns: List[Turn] = []
        transcript = brain / sid / ".system_generated" / "logs" / "transcript.jsonl"
        date_str = fmt_ts(None)
        title = sid
        if transcript.is_file():
            try:
                with open(transcript, encoding="utf-8") as f:
                    for line in f:
                        if not line.strip():
                            continue
                        step = json.loads(line)
                        st = step.get("type")
                        if step.get("created_at"):
                            date_str = step["created_at"]
                        content = step.get("content")
                        if st == "USER_INPUT":
                            text = content_to_text(content)
                            if text:
                                if title == sid:
                                    title = text[:80]
                                turns.append({"role": "user", "content": text})
                        elif st == "PLANNER_RESPONSE":
                            # may carry tool_calls + text
                            text = content_to_text(content)
                            if text:
                                turns.append({"role": "assistant", "content": text})
                            for tc in step.get("tool_calls") or []:
                                turns.append({
                                    "role": "tool",
                                    "tool": tc.get("name") or "tool",
                                    "input": tc.get("args") or tc.get("arguments") or tc.get("input"),
                                })
                        elif st in ("RUN_COMMAND", "VIEW_FILE", "CODE_ACTION", "GENERIC", "SYSTEM_MESSAGE"):
                            text = content_to_text(content)
                            turns.append({
                                "role": "tool" if st in ("RUN_COMMAND", "VIEW_FILE", "CODE_ACTION") else "assistant",
                                "tool": st if st in ("RUN_COMMAND", "VIEW_FILE", "CODE_ACTION") else None,
                                "content": text if st not in ("RUN_COMMAND", "VIEW_FILE", "CODE_ACTION") else None,
                                "input": text if st in ("RUN_COMMAND", "VIEW_FILE", "CODE_ACTION") else None,
                                "output": step.get("result") or step.get("output"),
                            })
            except Exception as e:
                sys.stderr.write(f"[export] agy transcript {sid}: {e}\n")

        if save_session(model, "antigravity", sid, title, date_str, turns, {"db": str(db_path)}):
            count += 1
    return count


# ---------------------------------------------------------------------------
# Cursor (agent-transcripts JSONL is authoritative; store.db often empty blobs)
# ---------------------------------------------------------------------------

def export_cursor() -> int:
    count = 0
    # meta from chats/*/store.db + agent-transcripts
    transcript_files = list((HOME / ".cursor" / "projects").glob("**/agent-transcripts/**/*.jsonl"))
    # also prompt_history as weak fallback
    for path in transcript_files:
        sid = path.stem
        model = "default"
        title = sid
        # find matching meta.json near chats
        for meta_path in (HOME / ".cursor" / "chats").glob(f"**/{sid}/meta.json"):
            try:
                meta = json.loads(meta_path.read_text())
                model = meta.get("lastUsedModel") or meta.get("model") or model
                title = meta.get("name") or title
            except Exception:
                pass
        # store.db meta table (hex-encoded json)
        for db_path in (HOME / ".cursor" / "chats").glob(f"**/{sid}/store.db"):
            conn = open_ro(db_path)
            if not conn:
                continue
            try:
                for row in conn.execute("SELECT key, value FROM meta"):
                    val = row["value"]
                    if isinstance(val, str) and re.fullmatch(r"[0-9a-fA-F]+", val) and len(val) > 8:
                        try:
                            val = bytes.fromhex(val).decode("utf-8")
                        except Exception:
                            pass
                    try:
                        obj = json.loads(val)
                        if isinstance(obj, dict):
                            model = obj.get("lastUsedModel") or obj.get("model") or model
                            title = obj.get("name") or title
                    except Exception:
                        pass
            except Exception:
                pass
            conn.close()

        turns: List[Turn] = []
        date_str = fmt_ts(None)
        try:
            with open(path, encoding="utf-8") as f:
                for line in f:
                    if not line.strip():
                        continue
                    e = json.loads(line)
                    et = e.get("type") or e.get("role")
                    if et in ("user", "human") or e.get("role") == "user":
                        turns.append({"role": "user", "content": content_to_text(e.get("content") or e.get("message") or e.get("text"))})
                    elif et in ("assistant", "ai") or e.get("role") == "assistant":
                        turns.append({"role": "assistant", "content": content_to_text(e.get("content") or e.get("message") or e.get("text"))})
                    elif et in ("tool", "tool_result", "tool_call") or e.get("toolName") or e.get("tool_name"):
                        turns.append({
                            "role": "tool",
                            "tool": e.get("toolName") or e.get("tool_name") or e.get("name") or "tool",
                            "call_id": e.get("toolCallId") or e.get("id"),
                            "input": e.get("args") or e.get("input") or e.get("parameters"),
                            "output": e.get("result") or e.get("output") or e.get("content"),
                        })
                    elif "message" in e and isinstance(e["message"], dict):
                        m = e["message"]
                        role = m.get("role") or "assistant"
                        if role in ("user", "assistant"):
                            turns.append({"role": role, "content": content_to_text(m.get("content"))})
                    if e.get("timestamp"):
                        date_str = e["timestamp"]
        except Exception as e:
            sys.stderr.write(f"[export] cursor {path}: {e}\n")
            continue

        # prompt_history enrichment
        for ph in (HOME / ".cursor" / "chats").glob(f"**/{sid}/prompt_history.json"):
            try:
                hist = json.loads(ph.read_text())
                if isinstance(hist, list) and not turns:
                    for item in hist:
                        if isinstance(item, str):
                            turns.append({"role": "user", "content": item})
                        elif isinstance(item, dict):
                            turns.append({"role": item.get("role") or "user", "content": content_to_text(item.get("text") or item.get("content"))})
            except Exception:
                pass

        if save_session(model, "cursor", sid, title, date_str, turns, {"transcript": str(path)}):
            count += 1
    return count


# ---------------------------------------------------------------------------
# Factory (droid) JSONL
# ---------------------------------------------------------------------------

def export_factory() -> int:
    root = HOME / ".factory" / "sessions"
    if not root.is_dir():
        return 0
    count = 0
    for path in root.rglob("*.jsonl"):
        if path.name.endswith(".settings.json"):
            continue
        sid = path.stem
        model = "factory-default"
        title = sid
        date_str = fmt_ts(None)
        turns: List[Turn] = []
        try:
            with open(path, encoding="utf-8") as f:
                for line in f:
                    if not line.strip():
                        continue
                    e = json.loads(line)
                    et = e.get("type")
                    if et == "session_start":
                        title = e.get("title") or title
                        continue
                    if et == "message":
                        msg = e.get("message") or {}
                        role = msg.get("role") or "user"
                        content = msg.get("content")
                        # strip huge system-reminder blobs for readability
                        text = content_to_text(content)
                        if SKIP_SYSTEM_NOISE and role == "user" and text.lstrip().startswith("<system-reminder>"):
                            # keep only non-reminder text blocks if mixed
                            if isinstance(content, list):
                                kept = []
                                for c in content:
                                    if isinstance(c, dict) and "text" in c:
                                        t = c["text"]
                                        if not t.lstrip().startswith("<system-reminder>"):
                                            kept.append(t)
                                text = "\n".join(kept)
                            else:
                                continue
                        if text.strip():
                            turns.append({"role": role, "content": text})
                        if e.get("timestamp"):
                            date_str = e["timestamp"]
                    elif et in ("tool_use", "tool_result", "tool_call"):
                        turns.append({
                            "role": "tool",
                            "tool": e.get("name") or e.get("tool") or "tool",
                            "call_id": e.get("id") or e.get("tool_use_id"),
                            "input": e.get("input") or e.get("arguments"),
                            "output": e.get("output") or e.get("content"),
                        })
                    # model hints
                    if e.get("model"):
                        model = e["model"]
        except Exception as ex:
            sys.stderr.write(f"[export] factory {path}: {ex}\n")
            continue
        settings = path.with_suffix(".settings.json")
        if not settings.exists():
            settings = Path(str(path).replace(".jsonl", ".settings.json"))
        if settings.is_file():
            try:
                s = json.loads(settings.read_text())
                model = s.get("model") or s.get("activeModel") or model
            except Exception:
                pass
        if save_session(model, "factory", sid, title, date_str, turns, {"path": str(path)}):
            count += 1
    return count


# ---------------------------------------------------------------------------
# Grok CLI sessions
# ---------------------------------------------------------------------------

def export_grok() -> int:
    root = HOME / ".grok" / "sessions"
    if not root.is_dir():
        return 0
    count = 0
    for path in root.rglob("chat_history.jsonl"):
        sid = path.parent.name
        model = "grok"
        title = sid
        date_str = fmt_ts(path.stat().st_mtime)
        turns: List[Turn] = []
        try:
            with open(path, encoding="utf-8") as f:
                for line in f:
                    if not line.strip():
                        continue
                    e = json.loads(line)
                    et = e.get("type") or e.get("role")
                    if et in ("user", "human") or e.get("role") == "user":
                        turns.append({"role": "user", "content": content_to_text(e.get("content") or e.get("text"))})
                    elif et in ("assistant", "model") or e.get("role") == "assistant":
                        turns.append({"role": "assistant", "content": content_to_text(e.get("content") or e.get("text"))})
                    elif et in ("tool", "tool_result", "function"):
                        turns.append({
                            "role": "tool",
                            "tool": e.get("name") or e.get("tool") or "tool",
                            "input": e.get("input") or e.get("arguments"),
                            "output": e.get("output") or e.get("content"),
                        })
                    elif et == "system":
                        continue
                    # user_query style
                    content = e.get("content")
                    if isinstance(content, str) and "<user_query>" in content:
                        m = re.search(r"<user_query>\s*(.*?)\s*</user_query>", content, re.S)
                        if m:
                            turns.append({"role": "user", "content": m.group(1).strip()})
        except Exception as ex:
            sys.stderr.write(f"[export] grok {path}: {ex}\n")
            continue
        # summary title
        summary = path.parent / "summary.json"
        if summary.is_file():
            try:
                s = json.loads(summary.read_text())
                title = s.get("title") or s.get("summary") or title
                model = s.get("model") or model
            except Exception:
                pass
        if save_session(model, "grok", sid, title, date_str, turns, {"path": str(path)}):
            count += 1
    return count


# ---------------------------------------------------------------------------
# Claude Code JSONL
# ---------------------------------------------------------------------------

def export_claude() -> int:
    root = HOME / ".claude" / "projects"
    if not root.is_dir():
        return 0
    count = 0
    for path in root.rglob("*.jsonl"):
        if "subagents" in path.parts and path.parent.name == "subagents":
            # still include
            pass
        sid = path.stem
        model = "claude"
        title = sid
        date_str = fmt_ts(None)
        turns: List[Turn] = []
        try:
            with open(path, encoding="utf-8") as f:
                for line in f:
                    if not line.strip():
                        continue
                    e = json.loads(line)
                    et = e.get("type")
                    if e.get("timestamp"):
                        date_str = e["timestamp"]
                    if et == "user" or e.get("role") == "user":
                        text = content_to_text(e.get("message", {}).get("content") if isinstance(e.get("message"), dict) else e.get("content") or e.get("message"))
                        if text and not text.lstrip().startswith("<command-"):
                            turns.append({"role": "user", "content": text})
                            if title == sid:
                                title = text[:80]
                    elif et == "assistant" or e.get("role") == "assistant":
                        msg = e.get("message") if isinstance(e.get("message"), dict) else e
                        content = msg.get("content") if isinstance(msg, dict) else e.get("content")
                        # content may list tool_use blocks
                        if isinstance(content, list):
                            texts = []
                            for block in content:
                                if not isinstance(block, dict):
                                    continue
                                btype = block.get("type")
                                if btype == "text":
                                    texts.append(block.get("text") or "")
                                elif btype == "tool_use":
                                    turns.append({
                                        "role": "tool",
                                        "tool": block.get("name") or "tool",
                                        "call_id": block.get("id"),
                                        "input": block.get("input"),
                                    })
                            if texts:
                                turns.append({"role": "assistant", "content": "\n".join(texts)})
                        else:
                            text = content_to_text(content)
                            if text:
                                turns.append({"role": "assistant", "content": text})
                        if isinstance(msg, dict) and msg.get("model"):
                            model = msg["model"]
                    elif et == "tool_result":
                        turns.append({
                            "role": "tool",
                            "tool": "(result)",
                            "call_id": e.get("tool_use_id"),
                            "output": content_to_text(e.get("content") or e.get("output")),
                        })
        except Exception as ex:
            sys.stderr.write(f"[export] claude {path}: {ex}\n")
            continue
        if save_session(model, "claude", sid, title, date_str, turns, {"path": str(path)}):
            count += 1
    return count


# ---------------------------------------------------------------------------
# Manifest + optional sync
# ---------------------------------------------------------------------------

def write_manifest(counts: Dict[str, int]) -> Path:
    files = []
    for md in getattr(Cfg, "export_root", EXPORT_ROOT).rglob("*.md"):
        if md.name.upper() == "README.MD":
            continue
        rel = md.relative_to(getattr(Cfg, "export_root", EXPORT_ROOT))
        files.append({
            "path": str(md),
            "rel": str(rel),
            "model": rel.parts[0] if rel.parts else "default",
            "mtime": md.stat().st_mtime,
            "size": md.stat().st_size,
        })
    manifest = {
        "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "export_root": str(EXPORT_ROOT),
        "counts_by_cli": counts,
        "file_count": len(files),
        "files": sorted(files, key=lambda x: x["mtime"], reverse=True),
        "notebooklm": {
            "methods": ["add_source_file", "list_sources", "sync_drive_sources"],
            "note": "Point NotebookLM MCP sync_sources / add_source_file at export_root or individual .md files.",
        },
    }
    path = EXPORT_ROOT / "MANIFEST.json"
    path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return path


def try_sync() -> int:
    """Best-effort: nlm CLI or zcall notebooklm.add_source_file for each new md."""
    notebook_id = os.environ.get("OP_NOTEBOOK_ID") or os.environ.get("NOTEBOOKLM_NOTEBOOK_ID")
    if not notebook_id:
        sys.stderr.write("[sync] set OP_NOTEBOOK_ID to push sources; wrote MANIFEST only\n")
        return 0
    synced = 0
    nlm = shutil_which("nlm")
    for md in getattr(Cfg, "export_root", EXPORT_ROOT).rglob("*.md"):
        if md.name == "README.md":
            continue
        if nlm:
            # jacob-bd notebooklm-mcp-cli style
            cmd = [nlm, "source", "add", str(md), "--notebook", notebook_id]
            try:
                r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
                if r.returncode == 0:
                    synced += 1
                else:
                    sys.stderr.write(f"[sync] nlm failed {md.name}: {r.stderr[:200]}\n")
            except Exception as e:
                sys.stderr.write(f"[sync] nlm error: {e}\n")
        # zcall / grpcurl path is environment-specific; skip if no nlm
    return synced


def shutil_which(cmd: str) -> Optional[str]:
    from shutil import which
    return which(cmd)


# ---------------------------------------------------------------------------
# main
# ---------------------------------------------------------------------------

EXPORTERS = {
    "opencode": lambda: export_opencode_family(HOME / ".local/share/opencode/opencode.db", "opencode"),
    "kilo": lambda: export_opencode_family(HOME / ".local/share/kilo/kilo.db", "kilo"),
    "codex": export_codex,
    "agy": export_agy,
    "antigravity": export_agy,
    "cursor": export_cursor,
    "factory": export_factory,
    "grok": export_grok,
    "claude": export_claude,
}


def main() -> int:
    ap = argparse.ArgumentParser(description="Export LLM CLI sessions → /home/jeremy/.notebooklm-sources")
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--only", help="comma list: opencode,kilo,codex,agy,cursor,factory,grok,claude")
    ap.add_argument("--sync", action="store_true", help="best-effort NotebookLM source upload (needs OP_NOTEBOOK_ID + nlm)")
    ap.add_argument("--root", default=str(EXPORT_ROOT), help="override export root")
    ap.add_argument(
        "--backfill",
        action="store_true",
        help="full re-export of all known sessions (ignore watermark; rewrite files)",
    )
    ap.add_argument(
        "--incremental",
        action="store_true",
        default=True,
        help="skip unchanged sessions via content hash (default; use with persistent factory/opencode)",
    )
    args = ap.parse_args()

    Cfg.export_root = Path(args.root).expanduser()
    Cfg.export_root.mkdir(parents=True, exist_ok=True)
    Cfg.state_path = Cfg.export_root / ".export-state.json"
    Cfg.force = bool(args.backfill)

    # Keep module-level EXPORT_ROOT in sync for helpers that still read it
    import sys as _sys
    _sys.modules[__name__].EXPORT_ROOT = Cfg.export_root

    only = None
    if args.only:
        only = {x.strip().lower() for x in args.only.split(",") if x.strip()}

    counts: Dict[str, int] = {}
    ran_agy = False
    for name, fn in EXPORTERS.items():
        if only and name not in only:
            continue
        if name in ("agy", "antigravity"):
            if ran_agy:
                continue
            ran_agy = True
            name = "agy"
        try:
            n = fn()
        except Exception as e:
            sys.stderr.write(f"[export] {name} failed: {e}\n")
            n = 0
        counts[name] = n
        if args.verbose:
            print(f"  {name}: {n} sessions written/updated")

    flush_state()
    manifest = write_manifest(counts)
    total = sum(counts.values())
    mode = "backfill" if Cfg.force else "incremental"
    print(f"[export-sessions] mode={mode} wrote/updated {total} → {Cfg.export_root}")
    if args.verbose:
        print(f"  manifest: {manifest}")
        print(f"  state: {Cfg.state_path}")
        print(f"  models: {sorted(p.name for p in Cfg.export_root.iterdir() if p.is_dir())}")

    if args.sync:
        n = try_sync()
        print(f"[sync] uploaded {n} sources (best-effort)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
