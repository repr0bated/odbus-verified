#!/usr/bin/env python3
"""Real-LSP (not Tree-sitter) chunking + vectorization, hover-augmented.

For repos where Tree-sitter AST chunking isn't good enough (rust-lang/rust:
too large/submodule-heavy for a flat per-file AST walk — a prior attempt
"took forever and never finished"), this drives an actual language server
(rust-analyzer for .rs, clangd for C/C++) over stdio via pylspclient and
captures the resolved hover text for each top-level definition — the same
type-signature/doc info a human sees mousing over a symbol in an IDE — and
folds it into the chunk alongside the source and any doc comment.

Processes one crate/subdirectory at a time (fresh LSP session per directory)
so a single huge workspace load never blocks progress: each directory's
points land in Qdrant before moving to the next, so a killed/resumed run
picks up wherever it left off (skip-existing via point-id lookup, same as
vectorize_code.py).

Shares the Voyage/Qdrant plumbing (credential rotation, contextualized
embedding calls, point-id scheme, "code_lsp" collection) with
vectorize_code.py — imported directly rather than duplicated.
"""
import json
import os
import re
import subprocess
import sys
import time
import uuid

import pylspclient
from pylspclient.lsp_pydantic_strcuts import (
    TextDocumentItem,
    TextDocumentIdentifier,
    SymbolKind,
)

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import vectorize_code as vc  # noqa: E402  (reuse rotation/embed/upsert/point_id)

BULK_DIR = "/home/admin/git/repos-bulk"

# kind name we store in the payload/point-id, keyed by pylspclient SymbolKind
HOVER_KINDS = {
    SymbolKind.Function: "function",
    SymbolKind.Method: "method",
    SymbolKind.Struct: "struct",
    SymbolKind.Class: "impl",
    SymbolKind.Enum: "enum",
    SymbolKind.Interface: "trait",
    SymbolKind.Constant: "const",
}

SKIP_DIR_NAMES = {
    ".git", "target", "node_modules", "vendor", "build", "dist",
    "__pycache__", ".venv", "venv", "tests", "test", "testdata", "examples",
}


class LspSession:
    """One language-server process rooted at a single crate/subdirectory."""

    def __init__(self, cmd, root_dir):
        self.root_dir = root_dir
        self.process = subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL
        )
        json_rpc_endpoint = pylspclient.JsonRpcEndpoint(self.process.stdin, self.process.stdout)
        # rust-analyzer needs real time to index even a single crate on cold
        # start; give requests a generous timeout rather than failing fast.
        self.endpoint = pylspclient.LspEndpoint(json_rpc_endpoint, timeout=45)
        self.client = pylspclient.LspClient(self.endpoint)
        root_uri = f"file://{root_dir}"
        self.client.initialize(
            processId=self.process.pid,
            rootPath=root_dir,
            rootUri=root_uri,
            initializationOptions=None,
            # Without hierarchicalDocumentSymbolSupport, rust-analyzer (and
            # clangd) fall back to the old flat SymbolInformation shape for
            # documentSymbol responses — a single location.range spanning
            # the WHOLE definition (doc-comments/attributes through closing
            # brace), no separate selectionRange for just the name. That
            # silently broke every hover lookup here until traced down.
            capabilities={
                "textDocument": {
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": True},
                    "hover": {"contentFormat": ["plaintext", "markdown"]},
                }
            },
            trace="off",
            workspaceFolders=[{"uri": root_uri, "name": os.path.basename(root_dir)}],
        )
        self.client.initialized()

    def open_file(self, path, language_id, settle_retries=20, settle_delay=1.0):
        """didOpen, then poll documentSymbol until the server actually has
        something to say about this file (or gives up). A flat sleep() before
        the first request was unreliable — rust-analyzer's per-file analysis
        lags behind didOpen by a variable amount, especially right after a
        cold crate-root start, and hover/symbol requests issued too early
        just come back empty rather than erroring."""
        with open(path, encoding="utf-8", errors="ignore") as f:
            text = f.read()
        self.client.didOpen(TextDocumentItem(
            uri=f"file://{path}", languageId=language_id, version=1, text=text,
        ))
        for _ in range(settle_retries):
            if self.symbols(path):
                break
            time.sleep(settle_delay)
        return text

    def symbols(self, path):
        """Raw JSON-RPC call, not pylspclient's typed documentSymbol()
        wrapper: that wrapper tries to parse the response as DocumentSymbol
        (range + selectionRange) and silently falls back to the older flat
        SymbolInformation shape (single location.range spanning the WHOLE
        definition, doc-comments/attributes included) whenever the strict
        parse fails — which happens on every response from this
        rust-analyzer version. That fallback is useless for hover: it
        makes every hover position land on a doc-comment/attribute line
        instead of the actual name. Returning raw dicts and reading
        selectionRange ourselves avoids the silent downgrade entirely.
        """
        try:
            return self.endpoint.call_method(
                "textDocument/documentSymbol", textDocument={"uri": f"file://{path}"}
            ) or []
        except Exception:
            return []

    def hover(self, path, line, character, retries=3, retry_delay=0.5):
        for attempt in range(retries):
            try:
                result = self.endpoint.call_method(
                    "textDocument/hover",
                    textDocument={"uri": f"file://{path}"},
                    position={"line": line, "character": character},
                )
            except Exception:
                result = None
            if result:
                contents = result.get("contents")
                if isinstance(contents, dict) and contents.get("value"):
                    return contents["value"]
                if isinstance(contents, list) and contents:
                    return "\n".join(c.get("value", c) if isinstance(c, dict) else str(c) for c in contents)
                if isinstance(contents, str) and contents:
                    return contents
            if attempt < retries - 1:
                time.sleep(retry_delay)
        return None

    def close(self):
        try:
            self.client.shutdown()
            self.client.exit()
        except Exception:
            pass
        self.process.kill()


DOC_COMMENT_RE = re.compile(r"(?:^|\n)((?:[ \t]*(?:///|//!|#\[.*\]|\*[^/]*|/\*\*).*\n)+)[ \t]*$")


def preceding_doc_comment(text: str, start_offset_line: int) -> str:
    lines = text.splitlines()
    doc_lines = []
    i = start_offset_line - 1
    while i >= 0:
        line = lines[i].strip()
        if line.startswith("///") or line.startswith("//!"):
            doc_lines.insert(0, line)
            i -= 1
            continue
        if line.startswith("#[") and line.endswith("]"):
            i -= 1
            continue
        break
    return "\n".join(doc_lines)


def flatten_symbols(symbols, depth=0):
    """Works on raw JSON-RPC documentSymbol dicts (see LspSession.symbols'
    docstring for why — pylspclient's typed wrapper silently loses
    selectionRange precision). Normalizes to a flat list of
    (name, kind, hover_line, hover_char, snippet_start_line, snippet_end_line)
    for top-level-ish definitions only (depth 0-1, to skip deeply nested
    locals/closures).

    Two different ranges matter here and must NOT be conflated:
    - selectionRange: just the name token — the only place hover reliably
      resolves. Querying hover over a leading `#[derive(...)]` attribute or
      doc comment (both included in the full `range`) returns nothing.
    - range: the whole definition span (attrs/doc comments through closing
      brace) — used only to bound the source snippet, not for hover.
    Hierarchical DocumentSymbol dicts have both `range` and `selectionRange`
    plus optional `children`; flat SymbolInformation dicts have only
    `location.range` (whole-span) and no selectionRange or children — in
    that case we fall back to hovering at the whole-span start, which is
    less precise but still correct for single-line symbols.
    """
    out = []
    for sym in symbols:
        kind = HOVER_KINDS.get(sym.get("kind"))
        if kind and depth <= 1:
            sel = sym.get("selectionRange")
            full_range = sym.get("range") or (sym.get("location") or {}).get("range")
            if full_range is not None:
                snippet_start, snippet_end = full_range["start"], full_range["end"]
                hover_pos = sel["start"] if sel is not None else snippet_start
                out.append((sym["name"], kind, hover_pos["line"], hover_pos["character"],
                             snippet_start["line"], snippet_end["line"]))
        children = sym.get("children")
        if children:
            out.extend(flatten_symbols(children, depth + 1))
    return out


def crate_chunks(session: LspSession, crate_dir: str, repo_name: str, ext: str, language_id: str):
    for root, dirs, files in os.walk(crate_dir):
        dirs[:] = [d for d in dirs if d not in SKIP_DIR_NAMES and not d.startswith(".")]
        for fname in files:
            if not fname.endswith(ext):
                continue
            path = os.path.join(root, fname)
            rel_path = os.path.relpath(path, os.path.dirname(crate_dir))
            try:
                text = session.open_file(path, language_id)
            except OSError:
                continue
            lines = text.splitlines()
            for name, kind, hover_line, hover_char, start_line, end_line in flatten_symbols(session.symbols(path)):
                hover_text = session.hover(path, hover_line, hover_char) or ""
                doc_comment = preceding_doc_comment(text, start_line)
                snippet = "\n".join(lines[max(0, start_line):min(len(lines), end_line + 1)])[:4000]
                chunk = (
                    f"repo: {repo_name}\nfile: {rel_path}\n{kind}: {name}\n\n"
                    f"{hover_text}\n\n{doc_comment}\n\n{snippet}"
                ).strip()
                yield ([f"{repo_name}:{rel_path}", kind, name], chunk[:vc.MAX_CHUNK_CHARS])


LANGUAGE_SERVERS = {
    "rust": {"cmd": ["rust-analyzer"], "ext": ".rs", "language_id": "rust"},
    "cpp": {"cmd": ["clangd"], "ext": ".cpp", "language_id": "cpp"},
    "c": {"cmd": ["clangd"], "ext": ".c", "language_id": "c"},
}


def process_crate_dir(crate_dir: str, repo_name: str, lang: str, rotator: "vc.VoyageRotator"):
    cfg = LANGUAGE_SERVERS[lang]
    print(f"  -- crate {os.path.basename(crate_dir)} ({lang}) --", file=sys.stderr)
    session = LspSession(cfg["cmd"], crate_dir)
    try:
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
                vectors_by_group = vc.embed_contextualized_groups(texts_only, rotator)
            except Exception as e:
                n = sum(len(t) for t in texts_only)
                print(f"    CALL FAILED ({n} chunks): {e}", file=sys.stderr)
                pending.clear()
                pending_chars = 0
                return
            points = []
            for (ids, texts), vectors in zip(pending, vectors_by_group):
                for id_parts, text, vec in zip(ids, texts, vectors):
                    pid = vc.point_id(repo_name, *id_parts)
                    payload = {
                        "repo": repo_name,
                        "file": id_parts[0].split(":", 1)[1],
                        "kind": id_parts[1],
                        "name": id_parts[2],
                        "text": text,
                        "source": "lsp-hover",
                    }
                    points.append({"id": pid, "vector": vec, "payload": payload})
            vc.upsert_batch(points)
            total += len(points)
            pending.clear()
            pending_chars = 0

        for ids, texts in vc.group_chunks(
            crate_chunks(session, crate_dir, repo_name, cfg["ext"], cfg["language_id"])
        ):
            if rotator.exhausted():
                print(f"    STOPPING (exhausted) after {n_chunks} chunks, {total} written", file=sys.stderr)
                break
            n_chunks += len(texts)
            group_ids = [vc.point_id(repo_name, *idp) for idp in ids]
            if len(vc.existing_point_ids(group_ids)) == len(group_ids):
                n_skipped += len(texts)
                continue
            group_chars = sum(len(t) for t in texts)
            if pending and (
                pending_chars + group_chars > vc.MAX_CHARS_PER_CALL
                or len(pending) >= vc.MAX_GROUPS_PER_CALL
            ):
                flush()
                print(f"    ... {total}/{n_chunks} points ({n_skipped} skipped)", file=sys.stderr)
            pending.append((ids, texts))
            pending_chars += group_chars
        flush()
        print(f"  {os.path.basename(crate_dir)}: {n_chunks} chunks -> {total} points, {n_skipped} skipped",
              flush=True)
    finally:
        session.close()


def process_repo_incrementally(repo_dir: str, repo_name: str, lang: str, rotator: "vc.VoyageRotator",
                                crate_roots: list):
    """One fresh LSP session per top-level crate/subdirectory, so a huge
    multi-crate workspace (rust-lang/rust) never needs one monolithic
    blocking load — and progress survives a kill/resume."""
    vc.ensure_collection(vc.COLLECTION)
    for rel in crate_roots:
        if rotator.exhausted():
            print(f"STOPPING repo {repo_name}: all credentials exhausted", file=sys.stderr)
            break
        crate_dir = os.path.join(repo_dir, rel)
        if not os.path.isdir(crate_dir):
            print(f"  SKIP (not found): {rel}", file=sys.stderr)
            continue
        try:
            process_crate_dir(crate_dir, repo_name, lang, rotator)
        except Exception as e:
            print(f"  CRATE FAILED {rel}: {e}", file=sys.stderr)


# rust-lang/rust crate roots to process one at a time (compiler + library +
# a handful of tools) — deliberately not every single crate in the tree;
# extend this list incrementally as needed.
RUST_LANG_RUST_CRATES = [
    "compiler/rustc_lexer",
    "compiler/rustc_ast",
    "compiler/rustc_parse",
    "compiler/rustc_span",
    "compiler/rustc_errors",
    "library/core/src",
    "library/alloc/src",
    "library/std/src",
]


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "rust-lang-rust":
        repo_dir = os.path.join(BULK_DIR, "rust-lang__rust")
        rotator = vc.VoyageRotator()
        process_repo_incrementally(repo_dir, "rust-lang/rust", "rust", rotator, RUST_LANG_RUST_CRATES)
    else:
        print("usage: vectorize_lsp.py rust-lang-rust")
