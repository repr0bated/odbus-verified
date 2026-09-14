#!/usr/bin/env python3
"""Structure-aware chunking + vectorization for the compliance bundle.

Two source shapes handled:
- oscal-specs.md: split by ##/### headers; within any section containing
  OSCAL <control ...>...</control> blocks, extract each block (incl.
  nested enhancements) as its own chunk keyed by control id. Sections
  with no controls fall back to line-window chunking.
- compliance-specs.md: split by "## File: `path`" boundaries (repomix-
  style per-source-file dump). Within each file:
    - fenced ```json blocks: parsed; any top-level key whose value is a
      list of >=5 dict items is exploded into one chunk per item (e.g.
      controls-mapping.json's "procedures" array); other top-level keys
      each become one chunk.
    - everything else: sub-split by ##/### headers if present, else
      line-window chunked.
"""
import glob
import json
import os
import re
import sys
import time
import uuid
import urllib.request
import urllib.error

QDRANT_URL = "http://10.200.0.2:6333"
VOYAGE_DIM = 1024
TOKEN_SAFETY_MARGIN = 180_000_000  # real cap confirmed at 200M/account; leave a 20M buffer

# Rotation pool: separate accounts/keys, each with its own free-tier quota.
# VOYAGE_API_KEY / VOYAGE_API_KEY_RUST are the same underlying "pa-" key
# (direct Voyage); VOYAGE_API_KEY_LITE is the "al-" MongoDB-routed key,
# confirmed working against voyage-context-4 via ai.mongodb.com.
_VOYAGE_CREDENTIALS = [
    ("primary", os.environ["VOYAGE_API_KEY"], "https://api.voyageai.com/v1/contextualizedembeddings"),
    ("lite", os.environ["VOYAGE_API_KEY_LITE"], "https://ai.mongodb.com/v1/contextualizedembeddings"),
    # Confirmed via live test: only works against ai.mongodb.com (403 on
    # direct api.voyageai.com) — a second Mongo-routed account, despite
    # the name suggesting otherwise.
    ("mongo_voyager", os.environ["MONGO_VOYAGER"], "https://ai.mongodb.com/v1/contextualizedembeddings"),
    # Fourth credential: also ai.mongodb.com-routed, confirmed working with
    # voyage-context-4 given the correct request shape (inputs list-of-lists,
    # Authorization: Bearer, input_type: document).
    ("mongo_lsp", os.environ["MONGO_LSP_API"], "https://ai.mongodb.com/v1/contextualizedembeddings"),
]


class VoyageRotator:
    """Tracks cumulative tokens spent per credential and advances to the
    next one once the current is within TOKEN_SAFETY_MARGIN of its real
    per-model free-tier cap, instead of waiting to hit a hard 429."""

    def __init__(self):
        self.idx = 0
        self.used = {name: 0 for name, _, _ in _VOYAGE_CREDENTIALS}

    def current(self):
        return _VOYAGE_CREDENTIALS[self.idx]

    def record(self, tokens: int):
        name, _, _ = self.current()
        self.used[name] += tokens
        if self.used[name] >= TOKEN_SAFETY_MARGIN and self.idx + 1 < len(_VOYAGE_CREDENTIALS):
            self.idx += 1
            print(
                f"  [voyage] '{name}' hit {self.used[name]} tokens (>= {TOKEN_SAFETY_MARGIN} safety margin) "
                f"-> switching to '{self.current()[0]}'",
                file=sys.stderr,
            )

    def exhausted(self) -> bool:
        name, _, _ = self.current()
        return self.used[name] >= TOKEN_SAFETY_MARGIN and self.idx + 1 >= len(_VOYAGE_CREDENTIALS)
WINDOW_LINES = 150
NAMESPACE = uuid.UUID(int=0x3c1e_2a7d_9f04_4b6a_8d21_5e7c0a9f2b6d)


def point_id(*parts: str) -> str:
    return str(uuid.uuid5(NAMESPACE, "|".join(str(p) for p in parts)))


# ── oscal-specs.md chunking ──────────────────────────────────────────────────

def split_headers(text: str):
    """Yields (full_heading, section_text). full_heading keeps the ## parent
    context on ### subsections (e.g. "Operation-DBus SubID Taxonomy
    Extension :: Category Allowed-Values") so callers can detect ancestry,
    not just the immediate header text."""
    lines = text.splitlines()
    h2 = "(preamble)"
    heading = "(preamble)"
    buf = []
    for line in lines:
        if re.match(r"^## ", line):
            if buf:
                yield heading, "\n".join(buf)
            h2 = line.lstrip("#").strip()
            heading = h2
            buf = []
        elif re.match(r"^### ", line):
            if buf:
                yield heading, "\n".join(buf)
            h3 = line.lstrip("#").strip()
            heading = f"{h2} :: {h3}"
            buf = []
        else:
            buf.append(line)
    if buf:
        yield heading, "\n".join(buf)


def extract_controls(text: str):
    """Returns (control_id, control_text, parent_id_or_None) for every
    <control ...>...</control> block, including nested enhancements. Parent
    is whichever control was still open on the stack when this one started
    (None for top-level controls) — feeds the Cozo derived_from relation."""
    lines = text.splitlines()
    stack = []
    results = []
    for i, line in enumerate(lines):
        m = re.search(r'<control[^>]*\bid="([^"]+)"', line)
        if m:
            parent = stack[-1][0] if stack else None
            stack.append([m.group(1), i, parent])
        if "</control>" in line and stack:
            cid, start, parent = stack.pop()
            results.append((cid, "\n".join(lines[start:i + 1]), parent))
    return results


ENUM_RE = re.compile(r'<enum\s+value="([^"]+)"[^>]*>(.*?)</enum>', re.DOTALL)


def extract_enums(text: str):
    """Yields (value, description) for every <enum value="...">desc</enum> —
    the OSCAL metaschema's allowed-values leaf. Used both for the project's
    own SubID Taxonomy Extension (7 categories, ~25 property names) and
    generically wherever the OSCAL specs define a controlled vocabulary."""
    for m in ENUM_RE.finditer(text):
        value = m.group(1)
        desc = re.sub(r"\s+", " ", m.group(2)).strip()
        yield value, desc


TITLE_RE = re.compile(r"<title>(.*?)</title>", re.DOTALL)


def control_title(control_text: str) -> str:
    m = TITLE_RE.search(control_text)
    return re.sub(r"\s+", " ", m.group(1)).strip() if m else ""


MAX_CHUNK_CHARS = 20_000  # hard ceiling on any single chunk's text


def window_chunks(heading: str, text: str):
    lines = [l for l in text.splitlines() if l.strip()]
    for i in range(0, len(lines), WINDOW_LINES):
        window = lines[i:i + WINDOW_LINES]
        chunk = f"section: {heading}\n\n" + "\n".join(window)
        yield chunk[:MAX_CHUNK_CHARS]


def chunk_oscal(path: str):
    """Yields (id_parts, text, cozo_row_or_None). cozo_row is a dict ready
    for the `controls`/`derived_from` relations when this chunk is a real
    OSCAL control; None for preamble/window chunks (Qdrant-only)."""
    text = open(path, encoding="utf-8", errors="ignore").read()
    for heading, sec_text in split_headers(text):
        controls = extract_controls(sec_text)
        if controls:
            for cid, ctext, parent in controls:
                chunk_text = f"section: {heading}\ncontrol: {cid}\n\n{ctext[:4000]}"
                cozo_row = {
                    "id": cid,
                    "framework": "OSCAL/NIST-800-53",
                    "title": control_title(ctext),
                    "description": "",
                    "severity": "",
                    "source_file": os.path.basename(path),
                    "parent_id": parent,
                }
                yield ([heading, "control", cid], chunk_text, cozo_row)
            first_idx = sec_text.find("<control")
            if first_idx > 200:
                yield ([heading, "preamble"], f"section: {heading}\n\n{sec_text[:first_idx]}", None)
        else:
            enums = list(extract_enums(sec_text))
            if enums:
                is_project_taxonomy = "SubID" in heading or "subid" in heading.lower()
                framework = "operation-dbus-subid-taxonomy" if is_project_taxonomy else "OSCAL-metaschema-vocab"
                for value, desc in enums:
                    yield (
                        [heading, "enum", value],
                        f"section: {heading}\nvalue: {value}\n\n{desc}",
                        {
                            "id": value, "framework": framework, "title": value,
                            "description": desc, "severity": "", "source_file": os.path.basename(path),
                            "parent_id": None,
                        },
                    )
            else:
                for wi, chunk in enumerate(window_chunks(heading, sec_text)):
                    yield ([heading, "window", wi], chunk, None)


# ── compliance-specs.md chunking ─────────────────────────────────────────────

FILE_RE = re.compile(r"^## File: `([^`]+)`\s*$")
FENCE_RE = re.compile(r"```(?:json)?\n(.*?)\n```", re.DOTALL)


def split_by_file(text: str):
    lines = text.splitlines()
    path = "(preamble)"
    buf = []
    for line in lines:
        m = FILE_RE.match(line)
        if m:
            if buf:
                yield path, "\n".join(buf)
            path = m.group(1)
            buf = []
        else:
            buf.append(line)
    if buf:
        yield path, "\n".join(buf)


def _procedure_cozo_row(file_path: str, item: dict, ident: str):
    """controls-mapping.json shape: {"id": ..., "implements": [{"standard":
    ..., "requirements": [...]}]}. Emits a control row for the procedure
    itself; maps_to rows are yielded separately by the caller since one
    procedure can map to many requirements (relation, not a single row)."""
    if "implements" not in item or not isinstance(item.get("implements"), list):
        return None, []
    control_row = {
        "id": ident, "framework": os.path.basename(file_path), "title": ident,
        "description": "", "severity": "", "source_file": file_path, "parent_id": None,
    }
    maps = []
    for impl in item["implements"]:
        standard = impl.get("standard", "")
        for req in impl.get("requirements", []) or []:
            maps.append({"from_control": ident, "to_control": req, "to_framework": standard})
    return control_row, maps


def json_value_chunks(file_path: str, obj, prefix=""):
    if isinstance(obj, dict):
        for key, value in obj.items():
            if isinstance(value, list) and len(value) >= 5 and all(isinstance(v, dict) for v in value):
                for i, item in enumerate(value):
                    ident = item.get("id") or item.get("name") or item.get("standard") or str(i)
                    control_row, maps = _procedure_cozo_row(file_path, item, ident)
                    yield ([file_path, "json_item", key, ident],
                           f"file: {file_path}\nkey: {key}\nitem: {ident}\n\n{json.dumps(item, indent=2)[:3000]}",
                           {"control": control_row, "maps_to": maps} if control_row else None)
            else:
                yield ([file_path, "json_key", key],
                       f"file: {file_path}\nkey: {key}\n\n{json.dumps(value, indent=2)[:3000]}", None)
    elif isinstance(obj, list):
        for i, item in enumerate(obj):
            yield ([file_path, "json_item", "root", i],
                   f"file: {file_path}\nitem: {i}\n\n{json.dumps(item, indent=2)[:3000]}", None)


def chunk_frameworks(path: str):
    text = open(path, encoding="utf-8", errors="ignore").read()
    for file_path, content in split_by_file(text):
        fence = FENCE_RE.search(content)
        handled_json = False
        if fence and file_path.endswith(".json"):
            try:
                obj = json.loads(fence.group(1))
                yield from json_value_chunks(file_path, obj)
                handled_json = True
            except Exception:
                handled_json = False
        if not handled_json:
            has_headers = bool(re.search(r"^#{2,3} ", content, re.MULTILINE))
            if has_headers:
                for heading, sec_text in split_headers(content):
                    for wi, chunk in enumerate(window_chunks(f"{file_path} :: {heading}", sec_text)):
                        yield ([file_path, "window", heading, wi], chunk, None)
            else:
                for wi, chunk in enumerate(window_chunks(file_path, content)):
                    yield ([file_path, "window", wi], chunk, None)


# ── Qdrant / Voyage plumbing ─────────────────────────────────────────────────

def _urlopen_retrying(req, retries=5, base_delay=2.0):
    """Qdrant has had transient outages mid-batch (connection reset,
    connection refused — a container restart or momentary proxy hiccup
    under heavy concurrent load) that previously killed an entire
    multi-hour run for a problem that resolved itself within seconds.
    Retry network-level failures (not HTTP error responses, which callers
    handle themselves) with exponential backoff before giving up."""
    for attempt in range(retries):
        try:
            return urllib.request.urlopen(req)
        except urllib.error.HTTPError:
            raise  # a real HTTP error response; not a connectivity problem
        except urllib.error.URLError as e:
            if attempt == retries - 1:
                raise
            delay = base_delay * (2 ** attempt)
            print(f"  [qdrant] connection error ({e}), retrying in {delay:.0f}s "
                  f"(attempt {attempt + 1}/{retries})", file=sys.stderr)
            time.sleep(delay)


def ensure_collection(name: str):
    req = urllib.request.Request(f"{QDRANT_URL}/collections/{name}")
    try:
        _urlopen_retrying(req)
        return
    except urllib.error.HTTPError as e:
        if e.code != 404:
            raise
    body = json.dumps({"vectors": {"size": VOYAGE_DIM, "distance": "Cosine"}}).encode()
    req = urllib.request.Request(f"{QDRANT_URL}/collections/{name}", data=body, method="PUT",
                                  headers={"Content-Type": "application/json"})
    _urlopen_retrying(req).read()


def embed_contextualized_groups(groups: list, rotator: "VoyageRotator") -> list:
    """groups: list of list[str] (each inner list = chunks from one logical
    document/section, ordered). Returns list of list[vector], same shape.
    Uses voyage-context-4 so chunks embed with awareness of their siblings
    in the same group, instead of independently. Records real token usage
    against whichever credential is currently active in `rotator`."""
    if rotator.exhausted():
        raise RuntimeError("all Voyage credentials exhausted their safety-margin budget")
    name, api_key, api_url = rotator.current()
    body = json.dumps({
        "inputs": groups,
        "model": "voyage-context-4",
        "input_type": "document",
    }).encode()
    req = urllib.request.Request(
        api_url, data=body, method="POST",
        headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError as e:
        detail = e.read().decode(errors="replace")
        raise RuntimeError(f"HTTP {e.code}: {detail}") from None
    used_tokens = data.get("usage", {}).get("total_tokens", 0)
    rotator.record(used_tokens)
    return [[c["embedding"] for c in doc["data"]] for doc in data["data"]]


def upsert_batch(collection: str, points: list):
    body = json.dumps({"points": points}).encode()
    req = urllib.request.Request(f"{QDRANT_URL}/collections/{collection}/points?wait=true",
                                  data=body, method="PUT", headers={"Content-Type": "application/json"})
    _urlopen_retrying(req).read()


def existing_point_ids(collection: str, ids: list) -> set:
    """Which of these point ids already exist in the collection — a free
    lookup (no Voyage tokens) so a resumed run can skip groups that were
    already fully embedded in a prior, interrupted run."""
    if not ids:
        return set()
    body = json.dumps({"ids": ids, "with_payload": False, "with_vector": False}).encode()
    req = urllib.request.Request(f"{QDRANT_URL}/collections/{collection}/points",
                                  data=body, method="POST", headers={"Content-Type": "application/json"})
    try:
        with _urlopen_retrying(req) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError:
        return set()
    return {p["id"] for p in data.get("result", [])}


MAX_CHUNKS_PER_GROUP = 40     # keep well under the 16K total-chunks limit per call
# voyage-context-4's PER-DOCUMENT context window is 32,000 tokens (separate
# from and much smaller than the 120K *total* per-call limit). Cap well
# under that — real text often runs denser than 4 chars/token.
MAX_CHARS_PER_GROUP = 80_000


def group_chunks(chunk_fn, path: str):
    """Groups consecutive chunks sharing the same logical document key
    (id_parts[0]) into sub-batches bounded by count/size, so each becomes
    one contextualized-embedding 'document'."""
    current_key = None
    current_ids, current_texts, current_cozo = [], [], []
    current_chars = 0

    def group_ready(new_key, incoming_len):
        return current_key is not None and (
            new_key != current_key
            or len(current_ids) >= MAX_CHUNKS_PER_GROUP
            # look-ahead: would *adding* this chunk push us over the cap?
            or current_chars + incoming_len > MAX_CHARS_PER_GROUP
        )

    for id_parts, text, cozo_data in chunk_fn(path):
        key = id_parts[0]
        if group_ready(key, len(text)):
            yield current_ids, current_texts, current_cozo
            current_ids, current_texts, current_cozo, current_chars = [], [], [], 0
        current_key = key
        current_ids.append(id_parts)
        current_texts.append(text)
        current_cozo.append(cozo_data)
        current_chars += len(text)
    if current_ids:
        yield current_ids, current_texts, current_cozo


def write_cozo(cozo_db, cozo_data: dict, qdrant_point_id: str):
    """cozo_data is either a plain control-row dict (from chunk_oscal) or
    {'control': row, 'maps_to': [rows]} (from the procedure/JSON path)."""
    if cozo_data is None:
        return
    if "control" in cozo_data:
        row = cozo_data["control"]
        maps = cozo_data.get("maps_to", [])
    else:
        row = cozo_data
        maps = []

    cozo_db.run(
        """
        ?[id, framework, title, description, severity, source_file, qdrant_point_id] <-
            [[$id, $framework, $title, $description, $severity, $source_file, $qdrant_point_id]]
        :put controls {id => framework, title, description, severity, source_file, qdrant_point_id}
        """,
        {**row, "qdrant_point_id": qdrant_point_id},
    )
    cozo_db.run(
        """
        ?[name, description] <- [[$name, '']]
        :put frameworks {name => description}
        """,
        {"name": row["framework"]},
    )
    cozo_db.run(
        "?[control_id, framework] <- [[$control_id, $framework]]\n:put belongs_to {control_id, framework}",
        {"control_id": row["id"], "framework": row["framework"]},
    )
    if row.get("parent_id"):
        cozo_db.run(
            "?[control_id, parent_id] <- [[$control_id, $parent_id]]\n:put derived_from {control_id, parent_id}",
            {"control_id": row["id"], "parent_id": row["parent_id"]},
        )
    for m in maps:
        cozo_db.run(
            "?[from_control, to_control, to_framework] <- [[$from_control, $to_control, $to_framework]]\n"
            ":put maps_to {from_control, to_control, to_framework}",
            m,
        )


# The API's *per-call* limit is 120,000 tokens across all groups combined
# (separate from and larger than the 32K per-group limit). Observed ratio
# on this content is ~3.2-3.3 chars/token, so cap well under that with margin.
MAX_CHARS_PER_CALL = 250_000
MAX_GROUPS_PER_CALL = 5  # secondary safety cap


def process(chunk_fn, path: str, collection: str, source_file: str, cozo_db=None,
            rotator: "VoyageRotator" = None):
    ensure_collection(collection)
    if rotator is None:
        rotator = VoyageRotator()
    total = 0
    n_chunks = 0
    n_cozo = 0
    pending_groups = []  # list of (ids, texts, cozo_list)
    pending_chars = 0

    def flush():
        nonlocal total, n_cozo, pending_chars
        if not pending_groups:
            return
        texts_only = [texts for _, texts, _ in pending_groups]
        try:
            vectors_by_group = embed_contextualized_groups(texts_only, rotator)
        except Exception as e:
            n = sum(len(t) for t in texts_only)
            print(f"  CALL FAILED ({n} chunks across {len(pending_groups)} groups): {e}", file=sys.stderr)
            pending_groups.clear()
            pending_chars = 0
            return
        points = []
        for (ids, texts, cozo_list), vectors in zip(pending_groups, vectors_by_group):
            for id_parts, text, cozo_data, vec in zip(ids, texts, cozo_list, vectors):
                pid = point_id(source_file, *id_parts)
                payload = {
                    "source_file": source_file,
                    "kind": id_parts[1],
                    "text": text,
                    "path": "|".join(str(p) for p in id_parts),
                }
                points.append({"id": pid, "vector": vec, "payload": payload})
                if cozo_db is not None and cozo_data is not None:
                    try:
                        write_cozo(cozo_db, cozo_data, pid)
                        n_cozo += 1
                    except Exception as e:
                        print(f"  COZO WRITE FAILED for {id_parts}: {e}", file=sys.stderr)
        upsert_batch(collection, points)
        total += len(points)
        pending_groups.clear()
        pending_chars = 0

    n_skipped = 0
    for ids, texts, cozo_list in group_chunks(chunk_fn, path):
        if rotator.exhausted():
            print(f"  STOPPING: all Voyage credentials exhausted after {n_chunks} chunks seen "
                  f"({total} points written so far). Re-run later to resume (idempotent point ids).",
                  file=sys.stderr)
            break
        n_chunks += len(texts)

        group_point_ids = [point_id(source_file, *idp) for idp in ids]
        already = existing_point_ids(collection, group_point_ids)
        if len(already) == len(group_point_ids):
            n_skipped += len(texts)
            continue  # fully covered by a prior run — zero Voyage cost

        group_chars = sum(len(t) for t in texts)
        if pending_groups and (
            pending_chars + group_chars > MAX_CHARS_PER_CALL
            or len(pending_groups) >= MAX_GROUPS_PER_CALL
        ):
            flush()
            print(f"  ... {total}/{n_chunks} points upserted so far "
                  f"({n_cozo} cozo rows, {n_skipped} chunks skipped as already-done)", file=sys.stderr)
        pending_groups.append((ids, texts, cozo_list))
        pending_chars += group_chars
    flush()
    print(f"{source_file}: {n_chunks} chunks -> {total} points in '{collection}', {n_cozo} cozo control rows", flush=True)
    print(f"  token usage this call: {dict(rotator.used)}")


# ── raw compliance repo walking (usnistgov/OSCAL, ComplianceAsCode/content, etc.) ──

REPO_SKIP_DIRS = {".git", "node_modules", "vendor", "dist", "build", "__pycache__", ".venv", "venv", "target"}
REPO_TEXT_EXTS = {".md", ".markdown", ".json", ".yaml", ".yml", ".xml", ".rego", ".txt", ".docx"}
REPO_MAX_FILE_BYTES = 2_000_000


def docx_to_markdown(path: str) -> str:
    """Flattens a .docx into a markdown-ish text blob (Heading N styles become
    '#'*N lines, everything else stays a plain paragraph, tables are rendered
    as pipe rows) so it can flow through the same has_headers/window_chunks
    path already used for .md files — no separate chunking logic needed."""
    import docx as docx_lib

    doc = docx_lib.Document(path)
    lines = []
    for para in doc.paragraphs:
        text = para.text.strip()
        if not text:
            continue
        style = (para.style.name or "") if para.style else ""
        if style.startswith("Heading"):
            # split_headers() only recognizes exactly "## "/"### " (2/3
            # hashes) — flatten every Word heading level to "## " since
            # these templates are short enough that one section tier is
            # plenty, rather than teaching split_headers a 3rd format.
            lines.append(f"## {text}")
        else:
            lines.append(text)
    for table in doc.tables:
        for row in table.rows:
            cells = [c.text.strip() for c in row.cells]
            if any(cells):
                lines.append("| " + " | ".join(cells) + " |")
    return "\n\n".join(lines)


def iter_repo_files(repo_dir: str):
    for root, dirs, files in os.walk(repo_dir):
        dirs[:] = [d for d in dirs if d not in REPO_SKIP_DIRS and not d.startswith(".")]
        for fn in files:
            if os.path.splitext(fn)[1].lower() not in REPO_TEXT_EXTS:
                continue
            full = os.path.join(root, fn)
            try:
                if os.path.getsize(full) > REPO_MAX_FILE_BYTES:
                    continue
            except OSError:
                continue  # broken symlink or race with a concurrent process
            yield full


def chunk_repo_file(repo_name: str, repo_dir: str, path: str):
    rel = os.path.relpath(path, repo_dir)
    file_key = f"{repo_name}/{rel}"
    ext = os.path.splitext(path)[1].lower()

    if ext == ".docx":
        try:
            text = docx_to_markdown(path)
        except Exception:
            return
        if not text.strip():
            return
        for heading, sec_text in split_headers(text) if re.search(r"^## ", text, re.MULTILINE) else [("", text)]:
            for wi, chunk in enumerate(window_chunks(f"{file_key} :: {heading}" if heading else file_key, sec_text)):
                yield ([file_key, "window", heading, wi], chunk, None)
        return

    try:
        text = open(path, encoding="utf-8", errors="ignore").read()
    except Exception:
        return
    if not text.strip():
        return

    if ext == ".json":
        try:
            obj = json.loads(text)
            yield from json_value_chunks(file_key, obj)
            return
        except Exception:
            pass
    elif ext in (".yaml", ".yml"):
        try:
            import yaml
            obj = yaml.safe_load(text)
            if obj is not None:
                yield from json_value_chunks(file_key, obj)
                return
        except Exception:
            pass
    elif ext == ".xml" and "<control" in text:
        for cid, ctext, parent in extract_controls(text):
            chunk_text = f"file: {file_key}\ncontrol: {cid}\n\n{ctext[:4000]}"
            cozo_row = {
                "id": cid, "framework": repo_name, "title": control_title(ctext),
                "description": "", "severity": "", "source_file": file_key, "parent_id": parent,
            }
            yield ([file_key, "control", cid], chunk_text, cozo_row)
        return

    has_headers = bool(re.search(r"^#{2,3} ", text, re.MULTILINE))
    if has_headers:
        for heading, sec_text in split_headers(text):
            for wi, chunk in enumerate(window_chunks(f"{file_key} :: {heading}", sec_text)):
                yield ([file_key, "window", heading, wi], chunk, None)
    else:
        for wi, chunk in enumerate(window_chunks(file_key, text)):
            yield ([file_key, "window", wi], chunk, None)


def make_repo_chunk_fn(repo_name: str, repo_dir: str):
    def _fn(_unused_path):
        for f in iter_repo_files(repo_dir):
            yield from chunk_repo_file(repo_name, repo_dir, f)
    return _fn


# repo dir name (under /home/admin/git/repos-bulk) -> (display name, collection)
COMPLIANCE_REPOS = {
    "usnistgov__OSCAL": ("usnistgov/OSCAL", "compliance_official"),
    "ComplianceAsCode__content": ("ComplianceAsCode/content", "compliance_general"),
    "opencontrol__schemas": ("opencontrol/schemas", "compliance_general"),
    "LINCnil__GDPR-Developer-Guide": ("LINCnil/GDPR-Developer-Guide", "compliance_general"),
    "OpenGovDataMirror__A_gsa_fedramp-automation": ("gsa/fedramp-automation", "compliance_general"),
    "TechShieldOlamide__trustgrid-compliance-templates": ("trustgrid-compliance-templates", "compliance_general"),
    "vaibhavjain2608__compliance-policy-templates": ("compliance-policy-templates", "compliance_general"),
    "microsoft__presidio": ("microsoft/presidio", "compliance_general"),
    "cloud-custodian__cloud-custodian": ("cloud-custodian/cloud-custodian", "compliance_general"),
    "Auxin-io__cybersecurity-templates": ("Auxin-io/cybersecurity-templates", "compliance_general"),
}

REPOS_BULK_DIR = "/home/admin/git/repos-bulk"


COZO_DB_PATH = "/home/admin/compliance-cozo/db"


def open_cozo():
    from pycozo.client import Client
    return Client("rocksdb", COZO_DB_PATH)


# oscal-specs.md (official standards text) -> compliance_official
# compliance-specs.md (Rego/GDPR/security-templates, tangible artifacts) -> compliance_general
COLLECTION_FOR_TARGET = {
    "oscal": "compliance_official",
    "frameworks": "compliance_general",
}

if __name__ == "__main__":
    target = sys.argv[1] if len(sys.argv) > 1 else "oscal"
    bundle = "/home/admin/voyage-vectorize-bundle"
    cozo_db = open_cozo()
    rotator = VoyageRotator()
    if target == "oscal":
        process(chunk_oscal, f"{bundle}/oscal-specs.md", COLLECTION_FOR_TARGET["oscal"],
                "oscal-specs.md", cozo_db=cozo_db, rotator=rotator)
    elif target == "frameworks":
        process(chunk_frameworks, f"{bundle}/compliance-specs.md", COLLECTION_FOR_TARGET["frameworks"],
                "compliance-specs.md", cozo_db=cozo_db, rotator=rotator)
    elif target == "all":
        process(chunk_oscal, f"{bundle}/oscal-specs.md", COLLECTION_FOR_TARGET["oscal"],
                "oscal-specs.md", cozo_db=cozo_db, rotator=rotator)
        process(chunk_frameworks, f"{bundle}/compliance-specs.md", COLLECTION_FOR_TARGET["frameworks"],
                "compliance-specs.md", cozo_db=cozo_db, rotator=rotator)
    elif target == "repos":
        for repo_dirname, (repo_name, collection) in COMPLIANCE_REPOS.items():
            repo_dir = os.path.join(REPOS_BULK_DIR, repo_dirname)
            if not os.path.isdir(repo_dir):
                print(f"skip {repo_name}: {repo_dir} not found", file=sys.stderr)
                continue
            if rotator.exhausted():
                print("STOPPING: all Voyage credentials exhausted, re-run later to resume", file=sys.stderr)
                break
            print(f"=== {repo_name} ===")
            process(make_repo_chunk_fn(repo_name, repo_dir), repo_dir, collection,
                    repo_name, cozo_db=cozo_db, rotator=rotator)
    elif target == "repo":
        repo_dirname, repo_name = sys.argv[2], sys.argv[3]
        collection = sys.argv[4] if len(sys.argv) > 4 else "compliance_general"
        repo_dir = os.path.join(REPOS_BULK_DIR, repo_dirname)
        process(make_repo_chunk_fn(repo_name, repo_dir), repo_dir, collection,
                repo_name, cozo_db=cozo_db, rotator=rotator)
    else:
        print("usage: vectorize_compliance.py [oscal|frameworks|all|repos|repo <dir> <name> [collection]]")
