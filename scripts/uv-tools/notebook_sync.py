#!/usr/bin/env python3
"""Lossless local capture and resumable per-model/topic NotebookLM synchronization.

prepare-groups snapshots native chat/tool records and raw external conversation
database rows, with explicit legacy-only gaps; no summaries/redaction/truncation.
sync-groups requires an explicit session UUID and identity-bound mounted 3tchedFS.
The installed threetched-fs helper validates the mounted schema and receipt.
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
import uuid
from collections.abc import Mapping, Sequence
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

MAX_CHUNK_BYTES = 8 * 1024 * 1024
MAX_CHUNK_WORDS = 400_000
MAX_CALLS_PER_CYCLE = 128
MAX_HELPER_OUTPUT = 32 * 1024 * 1024  # Pretty JSON may expand a 16 MiB receipt.
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
    """Mounted dispatch or its synchronization contract failed."""
    def __init__(self, message: str, diagnostic: dict | None = None):
        super().__init__(message)
        self.diagnostic = diagnostic or {}


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
    """Retired transport entry point; fail old callers explicitly."""
    raise NotebookSyncError("direct D-Bus sync is retired; use mounted sync-groups")


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
    """Retired unauthenticated path; never fall back to another transport."""
    raise NotebookSyncError("legacy tool invocation is retired; use mounted sync-groups")


def unwrap_tool_payload(result: Any) -> Any:
    """Unwrap MCP structured/text content while preserving plain JSON results."""

    if not isinstance(result, dict):
        return result
    if result.get("isError") is True:
        raise NotebookSyncError("tool reported an error")

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
            f"{CREATE_NOTEBOOK_TOOL} returned no notebook identifier"
        )
    return notebook_id


def ensure_tool_success(tool_name: str, result: Any) -> Any:
    payload = unwrap_tool_payload(result)
    if isinstance(payload, dict):
        for flag in ("success", "ok", "added"):
            if payload.get(flag) is False:
                raise NotebookSyncError(f"{tool_name} reported failure")
        status = payload.get("status")
        if isinstance(status, str) and status.lower() in {"error", "failed", "failure"}:
            raise NotebookSyncError(f"{tool_name} reported failure")
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


def ingest(title: str | None = None, notebook_id: str | None = None,
           session: str | None = None, state_dir: Path | None = None) -> dict:
    if os.environ.get("OPDBUS_NOTEBOOK_SYNC_MODE") == "per-model":
        session = selected_session(session or os.environ.get("OPDBUS_NOTEBOOK_SESSION_ID", ""))
        state_dir = state_dir or SOURCES / ".grouped-sync"
        state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        with (state_dir / ".lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            remote_path = state_dir / "remote-state.json"
            remote = json.loads(remote_path.read_text()) if remote_path.exists() else {"groups": {}}
            # Drain the exact frozen snapshot before capturing later appends.
            recover_repair(state_dir)
            remote = json.loads(remote_path.read_text()) if remote_path.exists() else {"groups": {}}
            if not remote.get("cycle_incomplete") and not any(
                group.get("pending") or group.get("retired") or
                (group.get("create_intent") and not group.get("notebook_id")) or
                any(item.get("status") != "confirmed_absent" for item in group.get("deletions", {}).values())
                for group in remote["groups"].values()
            ):
                prepare_groups(state_dir)
            counts = sync_groups(state_dir, session, defer_processing=True)
            print(json.dumps(counts))
        return counts
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
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
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
    if isinstance(parts, bool) or not isinstance(parts, int) or not 2 <= parts <= 32:
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
    if not pending or not pending.get("source_id") or pending["sha256"] != chunk["sha256"]:
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


class MountedPayload(dict):
    """Domain result plus the verified receipt; mutation receipts are persisted."""
    def __init__(self, payload: dict, receipt: dict):
        super().__init__(payload)
        self.receipt = receipt


def selected_session(session: str) -> str:
    try:
        if str(uuid.UUID(session)) != session:
            raise ValueError
    except (ValueError, AttributeError, TypeError) as error:
        raise NotebookSyncError("an explicit canonical session UUID is required") from error
    return session


def safe_helper_diagnostic(stderr: str | bytes | None, method: str) -> dict:
    """Retain only attributed, typed recovery locators, never provider text."""
    if isinstance(stderr, bytes):
        stderr = stderr.decode("utf-8", errors="replace")
    text = (stderr or "")[:16384]
    decoder = json.JSONDecoder()
    for match in re.finditer(r"\{", text):
        try:
            value, _ = decoder.raw_decode(text[match.start():])
        except json.JSONDecodeError:
            continue
        if not isinstance(value, dict) or value.get("plugin") != "notebooklm" or value.get("method") != method:
            continue
        result = {}
        if value.get("stage") in {"parse", "validate", "credential", "register", "call", "transport", "outcome_unknown", "contract", "result"}:
            result["stage"] = value["stage"]
        operation = value.get("operation_id")
        if isinstance(operation, str):
            try:
                result["operation_id"] = str(uuid.UUID(operation))
            except ValueError:
                pass
        return result
    return {}


def grouped_call(method: str, arguments: Mapping[str, Any], session: str) -> Any:
    read = {"notebook_list", "notebook_get", "source_get_content", "get_health"}
    write = {"notebook_create", "source_add", "source_delete"}
    if method not in read | write:
        raise NotebookSyncError("unsupported sync method")
    selected_session(session)
    mount = os.environ.get("OPDBUS_NOTEBOOK_FS_ROOT", "")
    if not mount or not Path(mount).is_absolute():
        raise NotebookSyncError("OPDBUS_NOTEBOOK_FS_ROOT must name the configured absolute mount")
    command = [
        "/usr/local/bin/threetched-fs", "call", "notebooklm", method,
        "--mount", mount, "--expected-session", session, "--args", "-",
    ]
    if method in write:
        command.append("--confirm-side-effects")
    try:
        reply = subprocess.run(command, input=compact_json(dict(arguments)),
                               capture_output=True, text=True, timeout=250)
    except subprocess.TimeoutExpired as error:
        raise NotebookSyncError(f"mounted notebooklm.{method} timed out; do not retry mutations",
                                safe_helper_diagnostic(error.stderr, method)) from error
    except OSError as error:
        raise NotebookSyncError(f"mounted notebooklm.{method} helper unavailable; do not retry mutations") from error
    if reply.returncode:
        # Helper diagnostics may contain provider payloads. Do not log them.
        raise NotebookSyncError(f"mounted notebooklm.{method} failed (exit {reply.returncode}); receipt unresolved",
                                {"exit_code": reply.returncode, **safe_helper_diagnostic(reply.stderr, method)})
    if len(reply.stdout.encode("utf-8")) > MAX_HELPER_OUTPUT:
        raise NotebookSyncError("mounted helper output exceeds the receipt budget")
    try:
        envelope = json.loads(reply.stdout)
    except (json.JSONDecodeError, UnicodeError) as error:
        raise NotebookSyncError("mounted helper returned a malformed receipt") from error
    if not isinstance(envelope, dict) or envelope.get("plugin_id") != "notebooklm" or envelope.get("method") != method:
        raise NotebookSyncError("mounted receipt attribution mismatch")
    engine = envelope.get("result")
    if not isinstance(engine, dict) or engine.get("success") is not True or engine.get("outcome") != "completed":
        raise NotebookSyncError("mounted operation has no verified completed result")
    payload = unwrap_dispatch(engine)
    if not isinstance(payload, dict):
        raise NotebookSyncError("notebooklm returned a non-object result")
    return MountedPayload(payload, envelope)


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
    state_path = state_dir / "remote-state.json"
    if state_path.exists():
        remote = json.loads(state_path.read_text())
        if remote.get("cycle_incomplete") or any(
            group.get("pending") or group.get("retired") or
            (group.get("create_intent") and not group.get("notebook_id")) or
            any(item.get("status") != "confirmed_absent" for item in group.get("deletions", {}).values())
            for group in remote["groups"].values()
        ):
            raise NotebookSyncError("drain the frozen plan before preparing later appends")
    repo = repo or Path(os.environ.get("OPDBUS_NOTEBOOK_PROJECT_ROOT", str(Path(__file__).resolve().parents[2])))
    if not repo.is_absolute() or not (repo / "AGENTS.md").is_file():
        raise NotebookSyncError("project capture root must be an absolute repository with AGENTS.md")
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

    doc_paths = sorted(set(repo.glob("docs/**/*.md")) |
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
        if isinstance(value.get("results"), list) and len(value["results"]) == 1:
            return source_id(value["results"][0])
    raise NotebookSyncError("provider returned no source id")


class CycleLimit(RuntimeError):
    """A bounded cycle ended; its exact plan and receipts remain resumable."""


def mutation_record(result: Any) -> dict:
    receipt = getattr(result, "receipt", None)
    return {"result": dict(result), **({"receipt": receipt} if receipt else {})}


def sync_groups(state_dir: Path, session: str, only: str | None = None,
                defer_processing: bool = False) -> dict:
    selected_session(session)
    counts = {"created": 0, "uploaded": 0, "skipped": 0, "replaced": 0, "pending": 0}
    try:
        _sync_groups(state_dir, session, only, defer_processing, counts)
    except CycleLimit:
        counts["pending"] += 1
    return counts


def _sync_groups(state_dir: Path, session: str, only: str | None,
                 defer_processing: bool, counts: dict) -> None:
    recover_repair(state_dir)
    plan = json.loads((state_dir / "plan.json").read_text())
    state_path = state_dir / "remote-state.json"
    state = json.loads(state_path.read_text()) if state_path.exists() else {"groups": {}}
    if state.get("session_id", session) != session:
        raise NotebookSyncError("saved remote state belongs to another session")
    # Legacy receipts are retained in place, bound to the explicitly selected
    # session on migration. No other identity or notebook is inferred.
    state["session_id"] = session
    state["version"] = 2
    state["cycle_incomplete"] = True
    atomic_json(state_path, state)
    calls = 0
    deadline = time.monotonic() + 900

    def check_budget() -> None:
        if calls >= MAX_CALLS_PER_CYCLE or time.monotonic() >= deadline:
            raise CycleLimit

    def call(method: str, args: dict, intent: dict | None = None) -> Any:
        nonlocal calls
        check_budget()
        calls += 1
        if intent is not None:
            # Reserve the bounded call before publishing; once fsynced, dispatch
            # this one call without another deadline gate or automatic retry.
            intent["attempted_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
            publish()
        try:
            return grouped_call(method, args, session)
        except NotebookSyncError as error:
            if intent is not None:
                intent["diagnostic"] = error.diagnostic
                publish()
            raise

    def publish() -> None:
        atomic_json(state_path, state)

    notebooks = call("notebook_list", {})
    if isinstance(notebooks, dict):
        notebooks = notebooks.get("notebooks")
    if not isinstance(notebooks, list):
        raise NotebookSyncError("invalid notebook list result")
    notebook_ids = {item["id"] for item in notebooks}
    keys = list(plan["groups"])
    cursor = state.get("next_group")
    if cursor in keys:
        start = keys.index(cursor)
        keys = keys[start:] + keys[:start]
    for index, key in enumerate(keys):
        group = plan["groups"][key]
        if only and only not in (key, group["title"]):
            continue
        state["next_group"] = key
        publish()
        saved = state["groups"].setdefault(key, {"sources": {}, "title": group["title"]})
        notebook_id = saved.get("notebook_id")
        if not notebook_id:
            intent = saved.get("create_intent")
            if intent:
                record = intent.get("confirmed")
                if not record:
                    raise NotebookSyncError("notebook creation outcome unresolved; caller-scoped receipt required before retry")
                notebook_id = record["result"]["notebook_id"]
            else:
                if any(item.get("title") == group["title"] for item in notebooks):
                    raise NotebookSyncError("matching notebook is unowned; explicit ownership reconciliation required")
                check_budget()
                intent = {"title": group["title"], "status": "outcome_unknown"}
                saved["create_intent"] = intent
                result = call("notebook_create", {"title": group["title"]}, intent)
                intent["confirmed"] = mutation_record(result)
                publish()
                notebook_id = result.get("notebook_id")
                if not notebook_id:
                    raise NotebookSyncError("notebook creation returned no exact notebook_id")
                notebook_ids.add(notebook_id)
                counts["created"] += 1
            saved["notebook_id"] = notebook_id
            publish()
        if notebook_id not in notebook_ids:
            raise NotebookSyncError("owned notebook absent from caller-scoped list; never recreate automatically")
        remote = call("notebook_get", {"notebook_id": notebook_id})
        if remote.get("notebook_id", notebook_id) != notebook_id:
            raise NotebookSyncError("notebook result attribution mismatch")
        existing = {item["id"]: item for item in remote["sources"]}
        saved.setdefault("pending", {})
        saved.setdefault("deletions", {})

        def delete_owned(sid: str) -> None:
            intent = saved["deletions"].get(sid)
            if sid not in existing:
                if intent:
                    intent["status"] = "confirmed_absent"
                    publish()
                return
            if intent:
                # A successful receipt still requires authoritative absence.
                # A timeout never justifies reissuing this non-idempotent method.
                raise NotebookSyncError("source deletion unresolved; authoritative absence required before retry")
            check_budget()
            intent = {"source_id": sid, "notebook_id": notebook_id, "status": "outcome_unknown"}
            saved["deletions"][sid] = intent
            result = call("source_delete", {"source_id": sid, "confirm": True}, intent)
            intent["confirmed"] = mutation_record(result)
            publish()
            if sid not in result.get("deleted_source_ids", []):
                raise NotebookSyncError("deletion receipt did not confirm the exact owned source")
            after = call("notebook_get", {"notebook_id": notebook_id})
            if after.get("notebook_id", notebook_id) != notebook_id:
                raise NotebookSyncError("post-deletion notebook attribution mismatch")
            if any(item["id"] == sid for item in after["sources"]):
                raise NotebookSyncError("deleted source still exists; do not retry mutation")
            existing.pop(sid, None)
            intent["status"] = "confirmed_absent"
            publish()

        for chunk in group["chunks"]:
            check_budget()
            slot, digest = chunk["slot"], chunk["sha256"]
            old = saved["sources"].get(slot)
            if old and old.get("sha256") == digest and old.get("source_id") in existing and old.get("verified"):
                counts["skipped"] += 1
                continue
            path = Path(chunk["path"])
            # The frozen artifact, including legacy larger chunks, is not replaced
            # or truncated. New captures use the existing 8 MiB packing bound.
            if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                raise NotebookSyncError("prepared source changed; frozen plan must be reconciled")
            pending = saved["pending"].get(slot)
            if pending and pending.get("sha256") != digest:
                raise NotebookSyncError("previous pending upload requires verification before replacing the plan")
            if pending and not pending.get("source_id") and pending.get("confirmed"):
                pending["source_id"] = source_id(pending["confirmed"]["result"])
                pending["status"] = "accepted"
                publish()
            verified = False
            if not pending or not pending.get("source_id"):
                matches = [item for item in existing.values() if item.get("title") == path.name]
                if len(matches) > 1:
                    raise NotebookSyncError("ambiguous content-addressed remote source")
                if matches:
                    # Titles alone do not prove ownership or processing. The
                    # caller-scoped content must reproduce the frozen bytes.
                    content = call("source_get_content", {"source_id": matches[0]["id"]})
                    text = content.get("content")
                    if not isinstance(text, str) or hashlib.sha256(text.encode("utf-8")).hexdigest() != digest:
                        raise NotebookSyncError("upload outcome unresolved; authoritative content does not match frozen bytes")
                    pending = {**(pending or {}), "source_id": matches[0]["id"], "sha256": digest,
                               "path": str(path), "recovered_by_content_sha256": True}
                    saved["pending"][slot] = pending
                    publish()
                    verified = True
                elif pending:
                    raise NotebookSyncError("upload outcome unresolved; missing source is not evidence to retry")
                else:
                    check_budget()
                    pending = {"sha256": digest, "path": str(path), "status": "outcome_unknown"}
                    saved["pending"][slot] = pending
                    result = call("source_add", {"notebook_id": notebook_id,
                                  "source_type": "file", "file_path": str(path)}, pending)
                    pending["confirmed"] = mutation_record(result)
                    # Persist the receipt even if extracting its identifier fails.
                    publish()
                    pending["source_id"] = source_id(result)
                    pending["status"] = "accepted"
                    publish()
                    counts["uploaded"] += 1
            # Accepted legacy pending records have an owned source ID. A source
            # missing from the scoped notebook is unresolved, never reuploaded.
            if pending.get("source_id") not in existing and not pending.get("confirmed"):
                raise NotebookSyncError("legacy pending source absent; explicit receipt reconciliation required")
            if not verified:
                for attempt in range(1 if defer_processing else 6):
                    # Permissions, missing mount and transport failures fail
                    # the cycle. Empty successful content means processing.
                    content = call("source_get_content", {"source_id": pending["source_id"]})
                    text = content.get("content")
                    if isinstance(text, str) and text.strip():
                        verified = True
                        break
                    if not defer_processing:
                        time.sleep(min(2 ** attempt, 10))
            if not verified:
                if defer_processing:
                    counts["pending"] += 1
                    continue
                raise NotebookSyncError("source processing not verified; old source retained")
            if pending["source_id"] not in existing:
                refreshed = call("notebook_get", {"notebook_id": notebook_id})
                if refreshed.get("notebook_id", notebook_id) != notebook_id:
                    raise NotebookSyncError("post-upload notebook attribution mismatch")
                existing = {item["id"]: item for item in refreshed["sources"]}
                if pending["source_id"] not in existing:
                    raise NotebookSyncError("accepted source absent from owned notebook; do not reupload")
            if old and old.get("source_id") != pending["source_id"]:
                delete_owned(old["source_id"])
                counts["replaced"] += 1
            saved["sources"][slot] = {**pending, "verified": True}
            del saved["pending"][slot]
            publish()
        for sid, retired in list(saved.get("retired", {}).items()):
            if retired.get("receipt", {}).get("source_id") != sid:
                raise NotebookSyncError("retired source has no matching owned receipt")
            children = retired["replacement_slots"]
            if children and all(saved["sources"].get(child, {}).get("verified")
                                and saved["sources"][child].get("source_id") in existing
                                and child not in saved["pending"] for child in children):
                delete_owned(sid)
                del saved["retired"][sid]
                counts["replaced"] += 1
                publish()
            else:
                counts["pending"] += 1
        state["next_group"] = keys[(index + 1) % len(keys)]
        publish()
    if only is None:
        state["cycle_incomplete"] = bool(counts["pending"])
        publish()


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Lossless grouped sync through the configured identity-bound 3tchedFS mount"
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
        counts = ingest(args.title, session=args.session, state_dir=args.state_dir)
        return 2 if counts["pending"] else 0
    if args.cmd == "create-and-ingest":
        notebook_id = create(args.title)
        ingest(args.title, notebook_id)
        return 0
    return 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except NotebookSyncError as error:
        print(f"notebook-sync: {error}", file=sys.stderr)
        raise SystemExit(1)
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error) as error:
        # Invalid persisted/local data may include raw records in exception
        # messages. Report the category and preserve evidence in place.
        print(f"notebook-sync: local state/capture failure ({type(error).__name__}); evidence retained", file=sys.stderr)
        raise SystemExit(1)
