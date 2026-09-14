#!/usr/bin/env python3
"""Tree-sitter AST-aware chunking + vectorization for the LSP-tier code repos.

Per language, finds "definition" nodes (function/struct/class/trait/impl/
enum/...), pairs each with its immediately-preceding doc comment (or, for
Python, its docstring — the first string-literal statement in the body),
and embeds each as one chunk with a contextual "repo/path :: kind name"
prefix. This is the AST-chunking-with-attached-docs approach (matches
Kiro's own chunker design: parse -> extract -> chunk at semantic
boundaries, never mid-statement).

rust-lang/rust is explicitly NOT handled here — too large/submodule-heavy
for a flat per-file AST walk; that one goes through a real LSP
(rust-analyzer) session instead, in a separate script.
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

import tree_sitter
import tree_sitter_c
import tree_sitter_cpp
import tree_sitter_go
import tree_sitter_javascript
import tree_sitter_python
import tree_sitter_rust
import tree_sitter_typescript

QDRANT_URL = "http://10.200.0.2:6333"
COLLECTION = "code_lsp"
VOYAGE_DIM = 1024
NAMESPACE = uuid.UUID(int=0x1a2b_3c4d_5e6f_7081_92a3_b4c5d6e7f809)
MAX_CHUNK_CHARS = 20_000
TOKEN_SAFETY_MARGIN = 180_000_000  # real cap confirmed at 200M/account; leave a 20M buffer
MAX_CHARS_PER_GROUP = 80_000
MAX_CHARS_PER_CALL = 250_000
MAX_GROUPS_PER_CALL = 5
MAX_CHUNKS_PER_GROUP = 40

# ── Per-language config ──────────────────────────────────────────────────────
# (tree_sitter language object, {definition node types}, doc-comment node type)

def _lang(mod, fn="language"):
    return tree_sitter.Language(getattr(mod, fn)())


LANGUAGES = {}


def _register(name, lang_obj, def_types, comment_type, docstring_style=False):
    LANGUAGES[name] = {
        "language": lang_obj,
        "parser": tree_sitter.Parser(lang_obj),
        "def_types": def_types,
        "comment_type": comment_type,
        "docstring_style": docstring_style,
    }


_register("rust", _lang(tree_sitter_rust), {
    "function_item", "struct_item", "trait_item", "impl_item", "enum_item", "mod_item",
}, "line_comment")

_register("go", _lang(tree_sitter_go), {
    "function_declaration", "method_declaration", "type_declaration",
}, "comment")

_register("c", _lang(tree_sitter_c), {
    "function_definition", "struct_specifier",
}, "comment")

_register("cpp", _lang(tree_sitter_cpp), {
    "function_definition", "class_specifier", "struct_specifier",
}, "comment")

_register("python", _lang(tree_sitter_python), {
    "function_definition", "class_definition",
}, None, docstring_style=True)

_register("javascript", _lang(tree_sitter_javascript), {
    "function_declaration", "class_declaration", "method_definition",
}, "comment")

_register("typescript", _lang(tree_sitter_typescript, "language_typescript"), {
    "function_declaration", "class_declaration", "method_definition",
    "interface_declaration", "type_alias_declaration",
}, "comment")

EXT_TO_LANG = {
    ".rs": "rust", ".go": "go", ".c": "c", ".h": "c",
    ".cc": "cpp", ".cpp": "cpp", ".cxx": "cpp", ".hpp": "cpp",
    ".py": "python", ".js": "javascript", ".jsx": "javascript",
    ".ts": "typescript", ".tsx": "typescript",
}

SKIP_DIR_NAMES = {".git", "node_modules", "target", "vendor", "third_party", "testdata", "dist", "build"}


def node_name(node, src: bytes) -> str:
    for child in node.children:
        if child.type in ("identifier", "type_identifier", "field_identifier"):
            return src[child.start_byte:child.end_byte].decode(errors="replace")
    return "(anonymous)"


def preceding_comment(node, src: bytes, comment_type: str) -> str:
    prev = node.prev_sibling
    if prev is not None and prev.type == comment_type:
        return src[prev.start_byte:prev.end_byte].decode(errors="replace")
    return ""


def python_docstring(node, src: bytes) -> str:
    # body -> block -> first expression_statement -> string
    for child in node.children:
        if child.type == "block":
            for stmt in child.children:
                if stmt.type == "expression_statement":
                    for s in stmt.children:
                        if s.type == "string":
                            return src[s.start_byte:s.end_byte].decode(errors="replace")
                    break
            break
    return ""


def chunk_file(repo: str, rel_path: str, lang_name: str, src: bytes):
    cfg = LANGUAGES[lang_name]
    tree = cfg["parser"].parse(src)
    def_types = cfg["def_types"]

    def walk(node):
        if node.type in def_types:
            name = node_name(node, src)
            if cfg["docstring_style"]:
                doc = python_docstring(node, src)
            else:
                doc = preceding_comment(node, src, cfg["comment_type"])
            body = src[node.start_byte:node.end_byte].decode(errors="replace")
            text = (
                f"repo: {repo}\nfile: {rel_path}\nkind: {node.type}\nname: {name}\n\n"
                f"{doc}\n\n{body}"
            )[:MAX_CHUNK_CHARS]
            yield ([f"{repo}:{rel_path}", node.type, name], text)
        for child in node.children:
            yield from walk(child)

    yield from walk(tree.root_node)


def iter_repo_chunks(repo_dir: str, repo_name: str):
    for root, dirs, files in os.walk(repo_dir):
        dirs[:] = [d for d in dirs if d not in SKIP_DIR_NAMES and not d.startswith(".")]
        for fname in files:
            ext = os.path.splitext(fname)[1]
            lang_name = EXT_TO_LANG.get(ext)
            if lang_name is None:
                continue
            path = os.path.join(root, fname)
            rel_path = os.path.relpath(path, repo_dir)
            try:
                src = open(path, "rb").read()
            except OSError:
                continue
            if len(src) > 2_000_000:  # skip absurd generated files
                continue
            try:
                yield from chunk_file(repo_name, rel_path, lang_name, src)
            except Exception as e:
                print(f"  PARSE FAILED {rel_path}: {e}", file=sys.stderr)


# ── Voyage rotation (same design as vectorize_compliance.py) ────────────────

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
            print(f"  [voyage] '{name}' hit {self.used[name]} tokens -> switching to '{self.current()[0]}'",
                  file=sys.stderr)

    def exhausted(self) -> bool:
        name, _, _ = self.current()
        return self.used[name] >= TOKEN_SAFETY_MARGIN and self.idx + 1 >= len(_VOYAGE_CREDENTIALS)


def point_id(*parts: str) -> str:
    return str(uuid.uuid5(NAMESPACE, "|".join(parts)))


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


def embed_contextualized_groups(groups: list, rotator: VoyageRotator) -> list:
    if rotator.exhausted():
        raise RuntimeError("all Voyage credentials exhausted their safety-margin budget")
    name, api_key, api_url = rotator.current()
    body = json.dumps({"inputs": groups, "model": "voyage-context-4", "input_type": "document"}).encode()
    req = urllib.request.Request(api_url, data=body, method="POST",
                                  headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(req) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"HTTP {e.code}: {e.read().decode(errors='replace')}") from None
    rotator.record(data.get("usage", {}).get("total_tokens", 0))
    return [[c["embedding"] for c in doc["data"]] for doc in data["data"]]


def upsert_batch(points: list):
    body = json.dumps({"points": points}).encode()
    req = urllib.request.Request(f"{QDRANT_URL}/collections/{COLLECTION}/points?wait=true",
                                  data=body, method="PUT", headers={"Content-Type": "application/json"})
    _urlopen_retrying(req).read()


def existing_point_ids(ids: list) -> set:
    if not ids:
        return set()
    body = json.dumps({"ids": ids, "with_payload": False, "with_vector": False}).encode()
    req = urllib.request.Request(f"{QDRANT_URL}/collections/{COLLECTION}/points",
                                  data=body, method="POST", headers={"Content-Type": "application/json"})
    try:
        with _urlopen_retrying(req) as resp:
            data = json.loads(resp.read())
    except urllib.error.HTTPError:
        return set()
    return {p["id"] for p in data.get("result", [])}


def group_chunks(chunks_iter):
    current_key = None
    current_ids, current_texts = [], []
    current_chars = 0

    def ready(new_key, incoming_len):
        return current_key is not None and (
            new_key != current_key
            or len(current_ids) >= MAX_CHUNKS_PER_GROUP
            or current_chars + incoming_len > MAX_CHARS_PER_GROUP
        )

    for id_parts, text in chunks_iter:
        key = id_parts[0]
        if ready(key, len(text)):
            yield current_ids, current_texts
            current_ids, current_texts, current_chars = [], [], 0
        current_key = key
        current_ids.append(id_parts)
        current_texts.append(text)
        current_chars += len(text)
    if current_ids:
        yield current_ids, current_texts


def process_repo(repo_dir: str, repo_name: str, rotator: VoyageRotator):
    ensure_collection(COLLECTION)
    total = 0
    n_chunks = 0
    n_skipped = 0
    pending, pending_chars = [], 0

    def flush():
        nonlocal total, pending, pending_chars
        if not pending:
            return
        texts_only = [texts for _, texts in pending]
        try:
            vectors_by_group = embed_contextualized_groups(texts_only, rotator)
        except Exception as e:
            n = sum(len(t) for t in texts_only)
            print(f"  CALL FAILED ({n} chunks): {e}", file=sys.stderr)
            pending, pending_chars = [], 0
            return
        points = []
        for (ids, texts), vectors in zip(pending, vectors_by_group):
            for id_parts, text, vec in zip(ids, texts, vectors):
                pid = point_id(repo_name, *id_parts)
                payload = {"repo": repo_name, "file": id_parts[0].split(":", 1)[1],
                           "kind": id_parts[1], "name": id_parts[2], "text": text}
                points.append({"id": pid, "vector": vec, "payload": payload})
        upsert_batch(points)
        total += len(points)
        pending, pending_chars = [], 0

    for ids, texts in group_chunks(iter_repo_chunks(repo_dir, repo_name)):
        if rotator.exhausted():
            print(f"  STOPPING (exhausted) after {n_chunks} chunks seen, {total} written", file=sys.stderr)
            break
        n_chunks += len(texts)
        group_ids = [point_id(repo_name, *idp) for idp in ids]
        if len(existing_point_ids(group_ids)) == len(group_ids):
            n_skipped += len(texts)
            continue
        group_chars = sum(len(t) for t in texts)
        if pending and (pending_chars + group_chars > MAX_CHARS_PER_CALL or len(pending) >= MAX_GROUPS_PER_CALL):
            flush()
            print(f"  ... {total}/{n_chunks} points ({n_skipped} skipped)", file=sys.stderr)
        pending.append((ids, texts))
        pending_chars += group_chars
    flush()
    print(f"{repo_name}: {n_chunks} chunks -> {total} points, {n_skipped} skipped", flush=True)
    print(f"  token usage: {dict(rotator.used)}")


# Direct-dependency tier: actual runtime/build deps of this workspace,
# plus the closest adjacent tooling (s6/dinit for the runit migration
# comparison). rust-lang/rust deliberately excluded (see README).
DIRECT_DEP_REPOS = [
    "z-galaxy__zbus", "lxc__incus", "openvswitch__ovs", "dbus__dbus", "bus1__dbus-broker",
    "GREsau__schemars", "qdrant__qdrant", "qdrant__qdrant-client",
    "tokio-rs__tokio", "tokio-rs__axum", "tokio-rs__tracing", "tokio-rs__bytes",
    "tokio-rs__mini-redis", "tokio-rs__prost",
    "hyperium__hyper", "hyperium__http", "hyperium__h2", "hyperium__tonic",
    "serde-rs__serde", "serde-rs__json", "rustls__rustls",
    "rust-netlink__netlink-packet-core", "rust-netlink__rtnetlink",
    "kdave__btrfs-progs",
    "skarnet__s6", "skarnet__s6-rc", "skarnet__s6-linux-init", "skarnet__execline", "skarnet__skalibs",
    "davmac314__dinit",
]

BULK_DIR = "/home/admin/git/repos-bulk"

# Ecosystem/adjacent tier: everything else cloned into repos-bulk that isn't
# a direct dep, isn't compliance (handled by vectorize_compliance.py's
# `repos` target), isn't rust-lang/rust (LSP-only, separate script), and
# isn't a pure schema/spec-text dump with no real source to chunk.
_EXCLUDED_FROM_ECOSYSTEM = set(DIRECT_DEP_REPOS) | {
    "rust-lang__rust",  # too large/submodule-heavy for tree-sitter; LSP-only
    # compliance repos (see vectorize_compliance.py COMPLIANCE_REPOS)
    "usnistgov__OSCAL", "ComplianceAsCode__content", "opencontrol__schemas",
    "LINCnil__GDPR-Developer-Guide", "OpenGovDataMirror__A_gsa_fedramp-automation",
    "TechShieldOlamide__trustgrid-compliance-templates",
    "vaibhavjain2608__compliance-policy-templates", "microsoft__presidio",
    "cloud-custodian__cloud-custodian",
    "Auxin-io__cybersecurity-templates",  # docx, needs different extraction
    # pure spec/schema text dumps, not code
    "json-schema-org__json-schema-spec", "jsontypedef__json-typedef-spec",
    "OAI__OpenAPI-Specification", "xdg__xdg-specs", "APIs-guru__openapi-directory",
    "rust-lang__rustlings",  # tutorial exercises, low value
}

ECOSYSTEM_REPOS = sorted(
    d for d in os.listdir(BULK_DIR)
    if os.path.isdir(os.path.join(BULK_DIR, d)) and d not in _EXCLUDED_FROM_ECOSYSTEM
)


def _run_batch(repo_list):
    rotator = VoyageRotator()
    for repo in repo_list:
        repo_dir = os.path.join(BULK_DIR, repo)
        if not os.path.isdir(repo_dir):
            print(f"SKIP (not cloned): {repo}", file=sys.stderr)
            continue
        if rotator.exhausted():
            print(f"STOPPING batch: all credentials exhausted before reaching '{repo}'", file=sys.stderr)
            break
        print(f"=== {repo} ===", file=sys.stderr)
        process_repo(repo_dir, repo, rotator)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "direct-deps":
        _run_batch(DIRECT_DEP_REPOS)
    elif len(sys.argv) > 1 and sys.argv[1] == "ecosystem":
        _run_batch(ECOSYSTEM_REPOS)
    else:
        repo_dir = sys.argv[1]
        repo_name = sys.argv[2] if len(sys.argv) > 2 else os.path.basename(repo_dir)
        rotator = VoyageRotator()
        process_repo(repo_dir, repo_name, rotator)
