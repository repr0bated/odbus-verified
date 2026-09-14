#!/usr/bin/env python3
"""Lossless local capture and resumable per-model/topic NotebookLM synchronization.

prepare-groups snapshots native chat/tool records and raw external conversation
database rows, with explicit legacy-only gaps; no summaries/redaction/truncation.
sync-groups requires an explicit current SID1 session and uses the public TLS
PluginService adapter to the same schema dispatcher as D-Bus PluginV1.Call.
Old unauthenticated cognitive-tool helpers remain for compatibility tests only.
Remote writes never use direct nlm execution, change grants, or create identities.
"""

from __future__ import annotations

import argparse
import ast
import base64
import codecs
import fcntl
import hashlib
import json
import os
import re
import sqlite3
import subprocess
import sys
import tempfile
import time
from collections.abc import Mapping, Sequence
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import closing
from pathlib import Path
from typing import Any

BRIDGE_BUS_NAME = "org.opdbus.v1.plugins"
PLUGIN_INTERFACE = "org.opdbus.v1.PluginV1"
COGNITIVE_OBJECT_PATH = "/org/opdbus/v1/plugins/cognitive_mcp"
DEFAULT_SESSION_BUS_ADDRESS = "unix:path=/run/opdbus/session-bus.sock"

CREATE_NOTEBOOK_TOOL = "create_notebook"
ADD_SOURCE_TEXT_TOOL = "add_source_text"
MAX_SOURCE_CHARS = 180_000

# This is a client of the existing schema dispatcher, not another MCP listener.
GROUPED_SERVICE = "operation.v1.PluginService/CallMethod"
MAX_CHUNK_BYTES = 8 * 1024 * 1024
MAX_CHUNK_WORDS = 400_000
TOPICS = {
    "architecture": "odBus — Architecture & Plugin Contracts",
    "security": "odBus — Identity, Security & OSCAL",
    "network": "odBus — Networking & Ghostbridge",
    "agents": "odBus — Agents, Memory & Knowledge",
    "ui": "odBus — UI & json-render",
    "operations": "odBus — Builds, Storage & Operations",
}

HOME = Path.home()
SOURCES = Path(os.environ.get("NOTEBOOKLM_SOURCES_DIR", "/home/jeremy/.notebooklm-sources"))
DEFAULT_TITLE = os.environ.get(
    "NOTEBOOK_TITLE", "3tched LLM Sessions / Control Plane"
)


class NotebookSyncError(RuntimeError):
    """The canonical D-Bus call or its response contract failed."""


def session_bus_address(environ: Mapping[str, str] | None = None) -> str:
    """Resolve the bridge session bus without consulting legacy network settings."""

    env = os.environ if environ is None else environ
    return (
        env.get("DBUS_SESSION_BUS_ADDRESS")
        or env.get("COGNITIVE_MCP_BUS_ADDRESS")
        or DEFAULT_SESSION_BUS_ADDRESS
    )


def compact_json(value: Any) -> str:
    """Serialize stable, single-line JSON for the D-Bus ``ss`` argument."""

    return json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True)


def build_plugin_call_command(
    method: str,
    arguments: Mapping[str, Any],
    *,
    bus_address: str | None = None,
) -> list[str]:
    """Build an argv-only PluginV1.Call command; no shell quoting is involved."""

    address = bus_address or session_bus_address()
    return [
        "busctl",
        f"--address={address}",
        "call",
        BRIDGE_BUS_NAME,
        COGNITIVE_OBJECT_PATH,
        PLUGIN_INTERFACE,
        "Call",
        "ss",
        method,
        compact_json(arguments),
    ]


def parse_busctl_string(stdout: str) -> str:
    """Decode the single D-Bus string emitted by ``busctl call ... Call``."""

    parts = stdout.strip().split(None, 1)
    if len(parts) != 2 or parts[0] != "s":
        raise NotebookSyncError(f"expected one D-Bus string, got: {stdout!r}")
    try:
        value = ast.literal_eval(parts[1])
    except (SyntaxError, ValueError) as error:
        raise NotebookSyncError(f"invalid busctl string encoding: {stdout!r}") from error
    if not isinstance(value, str):
        raise NotebookSyncError(f"busctl returned a non-string value: {value!r}")
    return value


def parse_call_reply(stdout: str) -> Any:
    """Decode and validate the bridge accountability envelope."""

    body = parse_busctl_string(stdout)
    try:
        envelope = json.loads(body)
    except json.JSONDecodeError as error:
        raise NotebookSyncError(f"PluginV1.Call returned invalid JSON: {body!r}") from error
    if not isinstance(envelope, dict):
        raise NotebookSyncError("PluginV1.Call returned a non-object envelope")
    if envelope.get("success") is not True:
        raise NotebookSyncError(f"PluginV1.Call failed: {envelope!r}")
    if "result" not in envelope:
        raise NotebookSyncError("PluginV1.Call envelope has no result")
    return envelope["result"]


def invoke_tool(
    tool_name: str,
    arguments: Mapping[str, Any],
    *,
    timeout: int,
    bus_address: str | None = None,
) -> Any:
    """Invoke one live cognitive tool through the schema method ``invoke_tool``."""

    call_args = {"tool_name": tool_name, "arguments": dict(arguments)}
    command = build_plugin_call_command(
        "invoke_tool", call_args, bus_address=bus_address
    )
    try:
        completed = subprocess.run(
            command,
            capture_output=True,
            check=False,
            text=True,
            timeout=timeout,
        )
    except FileNotFoundError as error:
        raise NotebookSyncError("busctl is required for session-bus dispatch") from error
    except subprocess.TimeoutExpired as error:
        raise NotebookSyncError(
            f"{tool_name} timed out after {timeout}s through PluginV1.Call"
        ) from error
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        raise NotebookSyncError(
            f"PluginV1.Call({tool_name}) failed with exit {completed.returncode}: {detail}"
        )
    return parse_call_reply(completed.stdout)


def unwrap_tool_payload(result: Any) -> Any:
    """Unwrap MCP structured/text content while preserving plain JSON results."""

    if not isinstance(result, dict):
        return result
    if result.get("isError") is True:
        raise NotebookSyncError(f"tool returned isError: {result!r}")

    structured = result.get("structuredContent")
    if structured is not None:
        return structured

    content = result.get("content")
    if not isinstance(content, list):
        return result

    text_blocks: list[str] = []
    for block in content:
        if not isinstance(block, dict) or block.get("type") != "text":
            continue
        text = block.get("text")
        if not isinstance(text, str):
            continue
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            text_blocks.append(text)
    return "\n".join(text_blocks) if text_blocks else result


def _find_notebook_id(value: Any) -> str | None:
    if isinstance(value, dict):
        for key in ("notebook_id", "notebookId", "id"):
            candidate = value.get(key)
            if isinstance(candidate, str) and candidate.strip():
                return candidate.strip()
        notebook = value.get("notebook")
        candidate = _find_notebook_id(notebook)
        if candidate:
            return candidate
        for nested in value.values():
            candidate = _find_notebook_id(nested)
            if candidate:
                return candidate
    elif isinstance(value, Sequence) and not isinstance(value, (str, bytes, bytearray)):
        for nested in value:
            candidate = _find_notebook_id(nested)
            if candidate:
                return candidate
    elif isinstance(value, str):
        match = re.search(
            r"(?:notebook[ _-]*id|\bid)\s*[:=]\s*['\"]?"
            r"([A-Za-z0-9][A-Za-z0-9._:-]{5,})",
            value,
            flags=re.IGNORECASE,
        )
        if match:
            return match.group(1)
        uuid_match = re.search(
            r"\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-"
            r"[89ab][0-9a-f]{3}-[0-9a-f]{12}\b",
            value,
            flags=re.IGNORECASE,
        )
        if uuid_match:
            return uuid_match.group(0)
    return None


def extract_notebook_id(result: Any) -> str:
    payload = unwrap_tool_payload(result)
    notebook_id = _find_notebook_id(payload)
    if not notebook_id:
        raise NotebookSyncError(
            f"{CREATE_NOTEBOOK_TOOL} returned no notebook identifier: {payload!r}"
        )
    return notebook_id


def ensure_tool_success(tool_name: str, result: Any) -> Any:
    payload = unwrap_tool_payload(result)
    if isinstance(payload, dict):
        for flag in ("success", "ok", "added"):
            if payload.get(flag) is False:
                raise NotebookSyncError(f"{tool_name} reported failure: {payload!r}")
        status = payload.get("status")
        if isinstance(status, str) and status.lower() in {"error", "failed", "failure"}:
            raise NotebookSyncError(f"{tool_name} reported failure: {payload!r}")
    return payload


def shell_single_quote(value: str) -> str:
    """Quote one value for the POSIX-shell-compatible notebook.env file."""

    return "'" + value.replace("'", "'\"'\"'") + "'"


def write_notebook_state(notebook_id: str, title: str, sources: Path = SOURCES) -> None:
    sources.mkdir(parents=True, exist_ok=True)
    (sources / "NOTEBOOK_ID").write_text(notebook_id + "\n", encoding="utf-8")
    quoted_title = shell_single_quote(title)
    (sources / "notebook.env").write_text(
        f"OP_NOTEBOOK_ID={notebook_id}\n"
        f"NOTEBOOKLM_NOTEBOOK_ID={notebook_id}\n"
        f"OP_NOTEBOOK_SOURCE_KEY={quoted_title}\n"
        f"NOTEBOOK_TITLE={quoted_title}\n"
        f"NOTEBOOK_NAME={shell_single_quote(f'project:{title}')}\n",
        encoding="utf-8",
    )


def saved_notebook_id(
    sources: Path = SOURCES,
    environ: Mapping[str, str] | None = None,
) -> str:
    env = os.environ if environ is None else environ
    for key in ("OP_NOTEBOOK_ID", "NOTEBOOKLM_NOTEBOOK_ID"):
        candidate = env.get(key, "").strip()
        if candidate:
            return candidate

    id_file = sources / "NOTEBOOK_ID"
    if id_file.is_file():
        candidate = id_file.read_text(encoding="utf-8").strip()
        if candidate:
            return candidate

    env_file = sources / "notebook.env"
    if env_file.is_file():
        for line in env_file.read_text(encoding="utf-8").splitlines():
            key, separator, raw_value = line.partition("=")
            if separator and key in {"OP_NOTEBOOK_ID", "NOTEBOOKLM_NOTEBOOK_ID"}:
                candidate = raw_value.strip().strip("'\"")
                if candidate:
                    return candidate
    raise NotebookSyncError("no saved notebook id; run notebook-sync create first")


def create(title: str = DEFAULT_TITLE) -> str:
    raise NotebookSyncError("legacy create retired; use prepare-groups and authenticated sync-groups")


def ingest(title: str | None = None, notebook_id: str | None = None) -> None:
    if os.environ.get("OPDBUS_NOTEBOOK_SYNC_MODE") == "per-model":
        state_dir = SOURCES / ".grouped-sync"
        state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        with (state_dir / ".lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            remote_path = state_dir / "remote-state.json"
            remote = json.loads(remote_path.read_text()) if remote_path.exists() else {"groups": {}}
            # Drain the exact frozen snapshot before capturing later appends.
            recover_repair(state_dir)
            remote = json.loads(remote_path.read_text()) if remote_path.exists() else {"groups": {}}
            if not any(group.get("pending") or group.get("retired") for group in remote["groups"].values()):
                prepare_groups(state_dir)
            counts = sync_groups(state_dir, os.environ.get("OPDBUS_NOTEBOOK_SESSION_ID", ""), defer_processing=True)
            print(json.dumps(counts))
            if counts["pending"]:
                raise NotebookSyncError(f"{counts['pending']} sources still processing; receipts preserved")
        return
    raise NotebookSyncError(
        "legacy ingest retired: it truncated and duplicated history; "
        "run prepare-groups, then sync-groups --session SESSION_ID"
    )


def atomic_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    descriptor, name = tempfile.mkstemp(prefix=".state-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def recover_repair(state_dir: Path) -> None:
    journal = state_dir / "repair-journal.json"
    if journal.exists():
        transaction = json.loads(journal.read_text())
        for name in ("plan", "remote-state", "repartitions"):
            atomic_json(state_dir / f"{name}.json", transaction[name])
        journal.unlink()


def repartition_pending(state_dir: Path, slot: str, parts: int = 2) -> dict:
    """Operator-selected failed upload: split exact bytes, keep old receipt.

    Use only after the provider reports processing failure, not merely preparing.
    A journal makes the local three-file update recoverable after interruption.
    """
    if not isinstance(parts, int) or not 2 <= parts <= 32:
        raise NotebookSyncError("replacement part count must be between 2 and 32")
    recover_repair(state_dir)
    plan = json.loads((state_dir / "plan.json").read_text())
    remote = json.loads((state_dir / "remote-state.json").read_text())
    repairs_path = state_dir / "repartitions.json"
    repairs = json.loads(repairs_path.read_text()) if repairs_path.exists() else {}
    matches = [(key, group, chunk) for key, group in plan["groups"].items()
               for chunk in group["chunks"] if chunk["slot"] == slot]
    if len(matches) != 1:
        raise NotebookSyncError("repartition requires one exact current source slot")
    key, group, chunk = matches[0]
    saved = remote["groups"][key]
    pending = saved.get("pending", {}).get(slot)
    if not pending or pending["sha256"] != chunk["sha256"]:
        raise NotebookSyncError("repartition requires an owned pending upload of the exact chunk")
    text = Path(chunk["path"]).read_bytes().decode("utf-8")
    if hashlib.sha256(text.encode()).hexdigest() != chunk["sha256"]:
        raise NotebookSyncError("source bytes do not match the receipt")
    if len(text) < parts:
        raise NotebookSyncError("source is too small to split")
    children = []
    for index in range(1, parts + 1):
        part = text[len(text) * (index - 1) // parts:len(text) * index // parts]
        writer = RawChunks(state_dir / "chunks", f"{slot}-part{index}", group["title"])
        writer.append(part)
        writer.flush()
        children.extend(writer.chunks)
    if b"".join(Path(child["path"]).read_bytes() for child in children) != text.encode():
        raise NotebookSyncError("split did not preserve the complete byte stream")
    repairs[f"{slot}:{chunk['sha256']}"] = children
    group["chunks"] = [child for candidate in group["chunks"]
                       for child in (children if candidate["slot"] == slot else [candidate])]
    saved.setdefault("retired", {})[pending["source_id"]] = {
        "receipt": pending, "replacement_slots": [child["slot"] for child in children]}
    for retired in saved["retired"].values():
        retired["replacement_slots"] = [replacement for previous in retired["replacement_slots"]
            for replacement in ([child["slot"] for child in children] if previous == slot else [previous])]
    del saved["pending"][slot]
    transaction = {"plan": plan, "remote-state": remote, "repartitions": repairs}
    atomic_json(state_dir / "repair-journal.json", transaction)
    recover_repair(state_dir)
    return {"slot": slot, "replacement_parts": len(children), "original_retained": True}


def apply_repartitions(chunks: list[dict], repairs: dict) -> list[dict]:
    result = []
    for chunk in chunks:
        children = repairs.get(f"{chunk['slot']}:{chunk['sha256']}")
        if children:
            # A repair is scoped to these bytes; a newly appended version cannot inherit it.
            rebuilt = b"".join(Path(child["path"]).read_bytes() for child in children)
            if hashlib.sha256(rebuilt).hexdigest() != chunk["sha256"]:
                raise NotebookSyncError("repartition evidence does not match original source")
            result.extend(apply_repartitions(children, repairs))
        else:
            result.append(chunk)
    return result


def unwrap_dispatch(value: Any) -> Any:
    """Validate both gRPC and dispatcher envelopes, then the provider result."""
    for _ in range(4):
        if not isinstance(value, dict):
            break
        if value.get("error") or value.get("success") is False:
            raise NotebookSyncError("plugin dispatch/provider reported failure")
        if "result" not in value or "success" not in value:
            break
        if value["success"] is not True:
            raise NotebookSyncError("plugin dispatch did not confirm success")
        value = value["result"]
    if not isinstance(value, (dict, list)):
        raise NotebookSyncError("plugin dispatch returned no structured result")
    return ensure_tool_success("notebooklm", value)


def grouped_call(method: str, arguments: Mapping[str, Any], session: str) -> Any:
    read = {"notebook_list", "notebook_get", "source_get_content", "get_health"}
    write = {"notebook_create", "source_add", "source_delete"}
    if method not in read | write:
        raise NotebookSyncError("unsupported sync method")
    capability = "notebooklm.read" if method in read else "notebooklm.invoke"
    helper = ["/usr/local/bin/op-identity-headers", "--session", session]
    if os.geteuid() != 0:
        helper = ["sudo", "-n", *helper]
    credential = subprocess.run(helper, capture_output=True, text=True, timeout=15)
    if credential.returncode:
        raise NotebookSyncError("current selected session credential is unavailable")
    header = json.loads(credential.stdout)["x-opdbus-sealed-id-bin"]
    env = dict(os.environ)
    env["OPDBUS_NOTEBOOK_CALL_SID"] = base64.b64encode(
        base64.urlsafe_b64decode(header + "=" * (-len(header) % 4))
    ).decode()
    request = {
        "plugin_id": "notebooklm", "method_name": method,
        "capability_id": capability, "arguments": [dict(arguments)],
    }
    command = [
        "grpcurl", "-max-time", "240", "-max-msg-sz", "67108864",
        "-expand-headers", "-H",
        "x-opdbus-sealed-id-bin: ${OPDBUS_NOTEBOOK_CALL_SID}",
        "-H", f"x-opdbus-capability: {capability}", "-d", "@",
        os.environ.get("OPDBUS_NOTEBOOK_GRPC_TARGET", "10.0.0.3:8090"),
        GROUPED_SERVICE,
    ]
    reply = subprocess.run(command, input=compact_json(request), capture_output=True,
                           text=True, env=env, timeout=250)
    if reply.returncode:
        # Neither credentials nor raw source content belongs in diagnostic logs.
        raise NotebookSyncError(f"authenticated notebooklm.{method} failed (exit {reply.returncode})")
    envelope = json.loads(reply.stdout)
    if not isinstance(envelope, dict) or envelope.get("success") is not True:
        raise NotebookSyncError(f"notebooklm.{method} dispatch denied or failed")
    return unwrap_dispatch(envelope)


def model_names(record: Any) -> set[str]:
    """Only model metadata positions; never search prompts/tool inputs for names."""
    if not isinstance(record, dict):
        return set()
    values = [record.get("model"), record.get("modelID"), record.get("modelId")]
    for field in ("payload", "message", "model"):
        child = record.get(field)
        if isinstance(child, dict):
            values.extend(child.get(key) for key in ("model", "modelID", "modelId"))
    result = set()
    for value in values:
        if isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/-]{0,119}", value):
            if value.lower() not in {"auto", "default", "unknown", "synthetic"}:
                result.add(value)
    return result


def group_key(model: str) -> str:
    slug = re.sub(r"[^a-z0-9._-]+", "-", model.lower()).strip("-.") or "unattributed"
    # Avoid collisions between distinct provider-qualified model identifiers.
    return f"model-{slug}-{hashlib.sha256(model.encode()).hexdigest()[:8]}"


class RawChunks:
    """Bounded lossless UTF-8 packing; concatenating chunks recovers the stream."""
    def __init__(self, directory: Path, key: str, title: str):
        self.directory, self.key, self.title = directory, key, title
        self.buffer = bytearray()
        self.words = 0
        self.chunks: list[dict[str, Any]] = []
        self.inputs: list[dict[str, Any]] = []

    def append(self, text: str) -> None:
        # A small fragment bounds extra memory and avoids splitting UTF-8 bytes.
        if MAX_CHUNK_BYTES < 4 or MAX_CHUNK_WORDS < 1:
            raise NotebookSyncError("chunk limits cannot hold a UTF-8 character")
        stride = max(1, min(16384, MAX_CHUNK_BYTES // 4, MAX_CHUNK_WORDS))
        for start in range(0, len(text), stride):
            fragment = text[start:start + stride]
            data = fragment.encode("utf-8")
            words = len(fragment.split())  # overcount at boundaries is conservative
            if self.buffer and (len(self.buffer) + len(data) > MAX_CHUNK_BYTES
                                or self.words + words > MAX_CHUNK_WORDS):
                self.flush()
            self.buffer.extend(data)
            self.words += words

    def flush(self) -> None:
        if not self.buffer:
            return
        digest = hashlib.sha256(self.buffer).hexdigest()
        slot = f"{self.key}__{len(self.chunks) + 1:04d}"
        path = self.directory / f"{slot}__{digest[:16]}.txt"
        if not path.exists():
            self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
            with path.open("xb") as stream:
                os.chmod(path, 0o600)
                stream.write(self.buffer)
                stream.flush()
                os.fsync(stream.fileno())
        elif hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise NotebookSyncError("existing chunk hash mismatch")
        self.chunks.append({"slot": slot, "path": str(path), "sha256": digest,
                            "bytes": len(self.buffer), "words_upper_bound": self.words})
        self.buffer.clear()
        self.words = 0

    def capture(self, path: Path, kind: str) -> None:
        digest = hashlib.sha256()
        total = 0
        with path.open("rb") as stream:
            remaining = os.fstat(stream.fileno()).st_size
            self.append(f"\n\nSOURCE {path} | {kind} | snapshot_bytes={remaining}\n\n")
            decoder = codecs.getincrementaldecoder("utf-8")("strict")
            while remaining:
                data = stream.read(min(65536, remaining))
                if not data:
                    raise NotebookSyncError(f"source shrank during snapshot: {path}")
                digest.update(data)
                total += len(data)
                remaining -= len(data)
                self.append(decoder.decode(data))
            self.append(decoder.decode(b"", final=True))
        self.inputs.append({"path": str(path), "kind": kind,
                            "bytes": total, "sha256": digest.hexdigest()})


def topic_for(path: str) -> str:
    name = path.lower()
    rules = [
        ("security", r"identity|oscal|security|auth|trust|principal|credential|subid|compliance"),
        ("ui", r"json.render|dashboard|frontend|lovable|figma|ui[-_/\.]|gallery|pagespec"),
        ("network", r"network|wireguard|ghostbridge|openflow|ovs|xray|dns|router|netmaker|emqx|mqtt|mail.server"),
        ("agents", r"cognitive|memory|notebook|agent|llm|gemma|qdrant|embedding|context|mcp|inception"),
        ("operations", r"build|deploy|install|runit|btrfs|cache|numa|timing|test|verify|storage|runbook|release"),
    ]
    return next((topic for topic, pattern in rules if re.search(pattern, name)), "architecture")


def prepare_groups(state_dir: Path, home: Path = HOME, repo: Path | None = None) -> dict:
    repo = repo or Path(__file__).resolve().parents[2]
    groups: dict[str, RawChunks] = {}
    gaps: list[dict[str, str]] = []

    def bucket(model: str) -> RawChunks:
        key = group_key(model)
        return groups.setdefault(key, RawChunks(state_dir / "chunks", key,
                                               f"Conversations & Tools — {model}"))

    # Metadata from the existing exporter is a fallback, never its trimmed text.
    hints: dict[str, set[str]] = {}
    legacy: list[tuple[Path, dict]] = []
    for path in sorted(SOURCES.rglob("*.json")):
        if any(part.startswith(".") for part in path.relative_to(SOURCES).parts):
            continue
        if path.name == "MANIFEST.json":
            continue
        value = json.loads(path.read_text())
        if not isinstance(value, dict) or "turns" not in value:
            continue
        legacy.append((path, {key: value.get(key) for key in ("model", "cli_tool", "session_id", "metadata")}))
        meta = value.get("metadata") or {}
        for field in ("path", "rollout_path", "transcript"):
            source = meta.get(field)
            if isinstance(source, str):
                hints.setdefault(Path(source).name, set()).update(model_names(value))

    patterns = [
        ".codex/sessions/**/*.jsonl", ".codex/archived_sessions/**/*.jsonl",
        ".claude/projects/**/*.jsonl", ".factory/sessions/**/*.jsonl",
        ".grok/sessions/**/chat_history.jsonl", ".cursor/projects/**/agent-transcripts/**/*.jsonl",
        ".gemini/tmp/**/chats/session-*.jsonl", ".gemini/tmp/**/chats/session-*.json",
        ".gemini/antigravity-cli/brain/**/transcript.jsonl",
    ]
    native = sorted({path.resolve() for pattern in patterns for path in home.glob(pattern)
                     if path.is_file()})
    captured_names = set()
    for path in native:
        models: set[str] = set()
        with path.open("rb") as stream:
            for line in stream:
                try:
                    models.update(model_names(json.loads(line)))
                except (json.JSONDecodeError, UnicodeDecodeError):
                    pass  # Preserve malformed/partial records in the raw snapshot.
        models = models or hints.get(path.name, set()) or {"Unattributed"}
        for model in sorted(models):
            bucket(model).capture(path, "native_raw_transcript")
        captured_names.add(path.name)

    # External applications own SQLite; read raw conversation tables only.
    # This is not an odBus state store and does not add a SQLite dependency.
    database_sessions: set[tuple[str, str]] = set()
    for cli in ("opencode", "kilo"):
        db = home / f".local/share/{cli}/{cli}.db"
        if not db.exists():
            continue
        with closing(sqlite3.connect(f"{db.as_uri()}?mode=ro", uri=True)) as connection:
            connection.row_factory = sqlite3.Row
            connection.execute("BEGIN")
            tables = {row[0] for row in connection.execute("SELECT name FROM sqlite_master WHERE type='table'")}
            for session_row in connection.execute("SELECT * FROM session ORDER BY id"):
                sid = session_row["id"]
                database_sessions.add((cli, sid))
                rows = [("session", dict(session_row))]
                for table in ("message", "part", "session_input", "session_message", "session_context_epoch"):
                    if table not in tables:
                        continue
                    columns = {row[1] for row in connection.execute(f'PRAGMA table_info("{table}")')}
                    if "session_id" not in columns:
                        continue
                    rows.extend((table, dict(row)) for row in connection.execute(
                        f'SELECT * FROM "{table}" WHERE session_id=? ORDER BY rowid', (sid,)))
                models = model_names(dict(session_row))
                for table, row in rows:
                    if table == "message" and isinstance(row.get("data"), str):
                        try:
                            models.update(model_names(json.loads(row["data"])))
                        except json.JSONDecodeError:
                            pass
                for model in sorted(models or {"Unattributed"}):
                    target = bucket(model)
                    for table, row in rows:
                        encoded = {key: {"base64_bytes": base64.b64encode(value).decode()}
                                   if isinstance(value, bytes) else value for key, value in row.items()}
                        target.append(compact_json({"database": str(db), "table": table, "row": encoded}) + "\n")
                    target.inputs.append({"path": str(db), "session_id": sid,
                                          "kind": "raw_database_rows", "rows": len(rows)})

    # Antigravity stores protobuf/BLOB fields, not just readable transcripts.
    # Preserve every conversation-table field losslessly; do not guess a model
    # by searching arbitrary binary data. Legacy text stays separately labelled.
    for db in sorted(home.glob(".gemini/antigravity-cli/conversations/*.db")):
        target = bucket("Unattributed")
        row_count = 0
        with closing(sqlite3.connect(f"{db.as_uri()}?mode=ro", uri=True)) as connection:
            connection.row_factory = sqlite3.Row
            connection.execute("BEGIN")
            tables = [row[0] for row in connection.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
            for table in tables:
                quoted = table.replace('"', '""')
                for row in connection.execute(f'SELECT * FROM "{quoted}" ORDER BY rowid'):
                    encoded = {key: {"base64_bytes": base64.b64encode(value).decode()}
                               if isinstance(value, bytes) else value for key, value in dict(row).items()}
                    target.append(compact_json({"database": str(db), "table": table, "row": encoded}) + "\n")
                    row_count += 1
        target.inputs.append({"path": str(db), "kind": "raw_database_rows", "rows": row_count})

    # Preserve exports when the original was lost; explicitly label the gap.
    for path, value in legacy:
        cli = value.get("cli_tool")
        if (cli, value.get("session_id")) in database_sessions:
            continue
        meta = value.get("metadata") or {}
        names = [Path(meta[key]).name for key in ("path", "rollout_path", "transcript")
                 if isinstance(meta.get(key), str)]
        if any(name in captured_names for name in names):
            continue
        model = next(iter(sorted(model_names(value))), "Unattributed")
        bucket(model).capture(path, "legacy_export_original_unavailable")
        gaps.append({"path": str(path), "reason": "original raw transcript unavailable; export is not raw"})

    doc_paths = sorted(set(repo.glob("docs/**/*.md")) | set(repo.glob("schemas/plugin/*.json")) |
                       {repo / name for name in ("AGENTS.md", "SIGNALS.md", "WISHLIST.md")})
    for path in doc_paths:
        if not path.is_file() or path.is_symlink():
            continue
        topic = topic_for(str(path.relative_to(repo)))
        key = f"topic-{topic}"
        target = groups.setdefault(key, RawChunks(state_dir / "chunks", key, TOPICS[topic]))
        target.capture(path, "project_document")
    plan = {"version": 1, "grouping": "per_model_and_project_topic", "gaps": gaps, "groups": {}}
    repairs_path = state_dir / "repartitions.json"
    repairs = json.loads(repairs_path.read_text()) if repairs_path.exists() else {}
    for key, target in sorted(groups.items()):
        target.flush()
        if target.chunks:
            plan["groups"][key] = {"title": target.title, "chunks": apply_repartitions(target.chunks, repairs), "inputs": target.inputs}
    atomic_json(state_dir / "plan.json", plan)
    return plan


def source_id(value: Any) -> str:
    if isinstance(value, dict):
        for key in ("source_id", "id"):
            if isinstance(value.get(key), str) and value[key]:
                return value[key]
        for key in ("source", "notebook"):
            if isinstance(value.get(key), dict):
                return source_id(value[key])
    raise NotebookSyncError("provider returned no source id")


def sync_groups(state_dir: Path, session: str, only: str | None = None,
                defer_processing: bool = False) -> dict:
    if not session:
        raise NotebookSyncError("--session is required; sync never adopts another identity")
    recover_repair(state_dir)
    plan = json.loads((state_dir / "plan.json").read_text())
    state_path = state_dir / "remote-state.json"
    state = json.loads(state_path.read_text()) if state_path.exists() else {"groups": {}}
    call = lambda method, args: grouped_call(method, args, session)
    notebooks = call("notebook_list", {})
    if isinstance(notebooks, dict):
        notebooks = notebooks.get("notebooks")
    if not isinstance(notebooks, list):
        raise NotebookSyncError("invalid notebook list result")
    counts = {"created": 0, "uploaded": 0, "skipped": 0, "replaced": 0, "pending": 0}
    for key, group in plan["groups"].items():
        if only and only not in (key, group["title"]):
            continue
        saved = state["groups"].setdefault(key, {"sources": {}, "title": group["title"]})
        notebook_id = saved.get("notebook_id")
        if not notebook_id:
            matches = [n for n in notebooks if n.get("title") == group["title"]]
            if len(matches) > 1:
                raise NotebookSyncError(f"ambiguous notebook title: {group['title']}")
            if matches:
                notebook_id = matches[0]["id"]
            else:
                created = call("notebook_create", {"title": group["title"]})
                notebook_id = extract_notebook_id(created)
                counts["created"] += 1
            saved["notebook_id"] = notebook_id
            atomic_json(state_path, state)
        remote = call("notebook_get", {"notebook_id": notebook_id})
        existing = {item["id"]: item for item in remote["sources"]}
        saved.setdefault("pending", {})
        if defer_processing:
            # Bounded client-side concurrency; all state publication stays on
            # the main thread. Provider processing is checked independently.
            uploads = []
            for chunk in group["chunks"]:
                old = saved["sources"].get(chunk["slot"])
                if old and old.get("sha256") == chunk["sha256"] and old.get("source_id") in existing and old.get("verified"):
                    continue
                if chunk["slot"] in saved["pending"]:
                    continue
                path = Path(chunk["path"])
                if hashlib.sha256(path.read_bytes()).hexdigest() != chunk["sha256"]:
                    raise NotebookSyncError("prepared source changed; prepare again")
                matches = [item for item in existing.values() if item.get("title") == path.name]
                if len(matches) > 1:
                    raise NotebookSyncError("ambiguous content-addressed remote source")
                if matches:
                    saved["pending"][chunk["slot"]] = {"source_id": matches[0]["id"],
                        "sha256": chunk["sha256"], "path": str(path)}
                    atomic_json(state_path, state)
                else:
                    uploads.append(chunk)
            failures = []
            with ThreadPoolExecutor(max_workers=3) as executor:
                futures = {executor.submit(call, "source_add", {
                    "notebook_id": notebook_id, "source_type": "file", "file_path": chunk["path"],
                }): chunk for chunk in uploads}
                for future in as_completed(futures):
                    chunk = futures[future]
                    try:
                        sid = source_id(future.result())
                    except Exception:
                        failures.append(chunk["slot"])
                        continue
                    saved["pending"][chunk["slot"]] = {"source_id": sid,
                        "sha256": chunk["sha256"], "path": chunk["path"]}
                    atomic_json(state_path, state)
                    counts["uploaded"] += 1
                    print(f"uploaded {group['title']} {chunk['slot']}", flush=True)
            if failures:
                raise NotebookSyncError(f"{len(failures)} uploads failed; successful receipts retained for retry")
        for chunk in group["chunks"]:
            slot, digest = chunk["slot"], chunk["sha256"]
            old = saved["sources"].get(slot)
            if old and old.get("sha256") == digest and old.get("source_id") in existing and old.get("verified"):
                counts["skipped"] += 1
                continue
            path = Path(chunk["path"])
            if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                raise NotebookSyncError("prepared source changed; prepare again")
            pending = saved.setdefault("pending", {}).get(slot)
            if pending and pending["sha256"] != digest:
                raise NotebookSyncError("previous pending upload needs verification before replacing the plan")
            if not pending:
                # Recover a successful upload whose response/state write was lost.
                matches = [item for item in existing.values() if item.get("title") == path.name]
                if len(matches) > 1:
                    raise NotebookSyncError("ambiguous content-addressed remote source")
                sid = matches[0]["id"] if matches else source_id(call("source_add", {
                    "notebook_id": notebook_id, "source_type": "file", "file_path": str(path),
                }))
                pending = {"source_id": sid, "sha256": digest, "path": str(path)}
                saved["pending"][slot] = pending
                atomic_json(state_path, state)
                counts["uploaded"] += not bool(matches)
            # Upload acceptance is not processing success. Keep the old source until readable.
            verified = False
            for attempt in range(1 if defer_processing else 6):
                try:
                    content = call("source_get_content", {"source_id": pending["source_id"]})
                    text = content.get("content") if isinstance(content, dict) else None
                    if isinstance(text, str) and text.strip():
                        verified = True
                        break
                except NotebookSyncError:
                    pass
                if not defer_processing:
                    time.sleep(min(2 ** attempt, 10))
            if not verified:
                if defer_processing:
                    counts["pending"] += 1
                    continue
                raise NotebookSyncError(f"source processing not verified; old source retained: {slot}")
            if old and old.get("source_id") != pending["source_id"] and old.get("source_id") in existing:
                call("source_delete", {"source_id": old["source_id"], "confirm": True})
                counts["replaced"] += 1
            saved["sources"][slot] = {**pending, "verified": True}
            del saved["pending"][slot]
            atomic_json(state_path, state)
            print(f"synced {group['title']} {slot}", flush=True)
        for sid, retired in list(saved.get("retired", {}).items()):
            if all(saved["sources"].get(child, {}).get("verified") and child not in saved["pending"]
                   for child in retired["replacement_slots"]):
                if sid in existing:
                    call("source_delete", {"source_id": sid, "confirm": True})
                del saved["retired"][sid]
                counts["replaced"] += 1
                atomic_json(state_path, state)
        print(f"notebook: {group['title']} ({len(group['chunks'])} sources; {len(saved['pending'])} pending)", flush=True)
    return counts


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Notebook create/ingest through session-bus PluginV1.Call"
    )
    parser.add_argument(
        "cmd",
        choices=["create", "ingest", "create-and-ingest", "show", "prepare-groups", "sync-groups", "groups", "repartition"],
        help="create notebook, ingest Markdown, both, or show saved state",
    )
    parser.add_argument("--title", default=DEFAULT_TITLE)
    parser.add_argument("--state-dir", type=Path, default=SOURCES / ".grouped-sync")
    parser.add_argument("--session", default=os.environ.get("OPDBUS_NOTEBOOK_SESSION_ID", ""))
    parser.add_argument("--only", help="exact prepared group key or notebook title")
    parser.add_argument("--slot", help="exact failed pending source slot to repartition losslessly")
    parser.add_argument("--parts", type=int, default=2,
                        help="replacement part count for a confirmed failed import (2–32)")
    args = parser.parse_args()

    if args.cmd in {"prepare-groups", "sync-groups", "groups", "repartition"}:
        args.state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        with (args.state_dir / ".lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            recover_repair(args.state_dir)
            if args.cmd == "repartition":
                print(json.dumps(repartition_pending(args.state_dir, args.slot, args.parts)))
                return 0
            if args.cmd == "sync-groups":
                result = sync_groups(args.state_dir, args.session, args.only, defer_processing=True)
                print(json.dumps(result))
                if result["pending"]:
                    return 2
            else:
                plan = prepare_groups(args.state_dir) if args.cmd == "prepare-groups" else json.loads((args.state_dir / "plan.json").read_text())
                print(json.dumps({"groups": [{"key": key, "title": value["title"], "sources": len(value["chunks"]), "inputs": len(value["inputs"])} for key, value in plan["groups"].items()], "legacy_only_gaps": len(plan["gaps"])}, indent=2))
        return 0

    if args.cmd == "show":
        state = SOURCES / "notebook.env"
        print(state.read_text() if state.is_file() else "no notebook.env yet")
        return 0
    if args.cmd == "create":
        create(args.title)
        return 0
    if args.cmd == "ingest":
        ingest(args.title)
        return 0
    if args.cmd == "create-and-ingest":
        notebook_id = create(args.title)
        ingest(args.title, notebook_id)
        return 0
    return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (NotebookSyncError, OSError, ValueError) as error:
        print(f"notebook-sync: {error}", file=sys.stderr)
        raise SystemExit(1)
