#!/usr/bin/env python3
"""
Keep /home/jeremy/.notebooklm-sources as frozen history plus one live feed per LLM.

Strategy:
  • Each LLM (model folder) has one live feed, `_live.md`: every session's text
    not yet frozen. It is the only file ever replaced, and only when it changes.
  • When the feed would pass --max-source-words (NotebookLM caps a source at
    500,000 words), it is frozen into the next `_bundle_NNN.md`, written once and
    never rewritten, and the feed starts empty. NotebookLM indexes each frozen
    bundle once; replacing a source re-processes it, so history is never replaced.
  • A session that keeps growing after being frozen adds only its new text.
  • Bundles from older layouts are set aside once in `_legacy_bundles/`.
  • Upload rule: `_live.md` is replaced when it changes; `_bundle_NNN.md` is
    uploaded once. Nothing in `_archived_sessions/` or `_legacy_bundles/` is a source.

Usage:
  notebook-sources-cleanup              # all folders
  notebook-sources-cleanup --folder factory
  notebook-sources-cleanup --agent-per-folder   # same logic, one pass per dir
  notebook-sources-cleanup --dry-run -v
  notebook-sources-cleanup --max-sources 300 --max-bundle-bytes 400000

After cleanup, re-ingest:
  notebook-sync ingest
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple

HOME = Path.home()
DEFAULT_ROOT = Path(os.environ.get("NOTEBOOKLM_SOURCES_DIR", "/home/jeremy/.notebooklm-sources"))
STATE_NAME = ".cleanup-state.json"
BUNDLE_PREFIX = "_bundle_"
LIVE_NAME = "_live.md"
SKIP_NAMES = {
    LIVE_NAME,
    "README.md",
    "MANIFEST.json",
    "NOTEBOOK_ID",
    "notebook.env",
    STATE_NAME,
    ".export-state.json",
}


def sha16(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8", errors="replace")).hexdigest()[:16]


def load_state(root: Path) -> dict:
    p = root / STATE_NAME
    if p.is_file():
        try:
            return json.loads(p.read_text())
        except Exception:
            pass
    return {"version": 1, "ingested": {}, "bundles": {}, "updated_at": None}


def save_state(root: Path, state: dict) -> None:
    state["updated_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    tmp = root / (STATE_NAME + ".tmp")
    tmp.write_text(json.dumps(state, indent=2, sort_keys=True))
    tmp.replace(root / STATE_NAME)


def is_session_md(path: Path) -> bool:
    if path.suffix != ".md":
        return False
    if path.name in SKIP_NAMES:
        return False
    if path.name.startswith(BUNDLE_PREFIX):
        return False
    if path.name.startswith("."):
        return False
    return True


def list_model_folders(root: Path) -> List[Path]:
    return sorted(
        p
        for p in root.iterdir()
        if p.is_dir() and not p.name.startswith(".")
    )


def bundle_paths(folder: Path) -> List[Path]:
    return sorted(folder.glob(f"{BUNDLE_PREFIX}*.md"))


def next_bundle_path(folder: Path) -> Path:
    existing = bundle_paths(folder)
    if not existing:
        return folder / f"{BUNDLE_PREFIX}001.md"
    # highest number
    nums = []
    for p in existing:
        m = re.search(r"_bundle_(\d+)\.md$", p.name)
        if m:
            nums.append(int(m.group(1)))
    n = max(nums) + 1 if nums else 1
    return folder / f"{BUNDLE_PREFIX}{n:03d}.md"


def latest_bundle(folder: Path) -> Optional[Path]:
    b = bundle_paths(folder)
    return b[-1] if b else None


def session_header(path: Path, model: str) -> str:
    rel = path.name
    return (
        f"\n\n{'=' * 72}\n"
        f"# SESSION FILE: {rel}\n"
        f"# model_folder: {model}\n"
        f"# ingested_at: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}\n"
        f"{'=' * 72}\n\n"
    )


def session_section(name: str, model: str, body: str) -> str:
    """A session's section in its LLM's source. Stable text only (no clock), so an
    unchanged session never changes the file."""
    return (
        f"\n\n{'=' * 72}\n"
        f"# SESSION FILE: {name}\n"
        f"# model_folder: {model}\n"
        f"{'=' * 72}\n\n"
        f"{body if body.endswith(chr(10)) else body + chr(10)}"
    )


def _archive_replacing(folder: Path, sess: Path) -> Path:
    """Move a session .md (+ companion .json) into _archived_sessions/, replacing
    the archived copy: the loose file is the newer export of the same session."""
    arch = folder / "_archived_sessions"
    arch.mkdir(exist_ok=True)
    dest = arch / sess.name
    os.replace(sess, dest)
    companion = folder / f"{sess.stem}.json"
    if companion.is_file():
        os.replace(companion, arch / companion.name)
    return dest


def consolidate_folder(
    folder: Path,
    state: dict,
    *,
    max_source_words: int,
    dry_run: bool,
    verbose: bool,
    archive_sessions: bool,
) -> Tuple[int, int]:
    """Keep this LLM's sources as frozen history plus one live feed.

    NotebookLM re-processes a source when it is replaced, so only the live feed
    (`_live.md`) is ever rewritten, and only when its content changes. When the
    feed would pass `max_source_words` its sections are frozen into the next
    `_bundle_NNN.md`, which is written once and never touched again, and the
    feed starts empty. A session that keeps growing after being frozen puts only
    its new text in the feed. Returns (files_written, files_unchanged).
    """
    model = folder.name
    arch = folder / "_archived_sessions"
    sessions: Dict[str, Path] = {}
    if arch.is_dir():
        for p in arch.iterdir():
            if p.is_file() and is_session_md(p):
                sessions[p.name] = p
    for p in sorted(folder.iterdir()):
        if p.is_file() and is_session_md(p):
            # A loose file is the latest export of its session and wins.
            if archive_sessions and not dry_run:
                sessions[p.name] = _archive_replacing(folder, p)
            else:
                sessions[p.name] = p

    llm = state.setdefault("llm", {}).setdefault(model, {"frozen": [], "sessions": {}})
    if not llm["frozen"] and not dry_run:
        # First run on this layout: bundles from the older append layouts hold
        # duplicated sessions. Set them aside; every session is in the archive.
        for old in bundle_paths(folder):
            legacy = folder / "_legacy_bundles"
            legacy.mkdir(exist_ok=True)
            os.replace(old, legacy / old.name)
            if verbose:
                print(f"  [{model}] set aside {old.name}")

    # What each session has not yet frozen: all of it, or only what it grew.
    pending: List[Tuple[str, str, int]] = []
    for name in sorted(sessions):
        try:
            body = sessions[name].read_text(encoding="utf-8", errors="replace")
        except Exception:
            continue
        mark = llm["sessions"].get(name)
        if mark and len(body) >= mark["chars"] and sha16(body[: mark["chars"]]) == mark["hash"]:
            new = body[mark["chars"] :]
        else:
            new = body  # never frozen, or rewritten rather than grown
        if new.strip():
            pending.append((name, session_section(name, model, new), len(body)))

    header = (
        f"# Sessions — {model}\n"
        f"# Auto-maintained by notebook-sources-cleanup. This LLM's live feed:\n"
        f"# replaced as sessions grow, frozen into _bundle_NNN.md when full.\n"
    )
    written = unchanged = 0
    live, live_words, included = header, 0, []
    for name, section, chars in pending:
        words = len(section.split())
        if included and live_words + words > max_source_words:
            written += _freeze(folder, llm, live, included, sessions, dry_run, verbose)
            live, live_words, included = header, 0, []
        live += section
        live_words += words
        included.append((name, chars))

    target = folder / LIVE_NAME
    if target.is_file() and target.read_text(encoding="utf-8", errors="replace") == live:
        unchanged += 1
    else:
        if verbose:
            print(f"  [{model}] {'would replace' if dry_run else 'replaced'} {LIVE_NAME} ({live_words} words)")
        if not dry_run:
            tmp = target.with_suffix(".md.tmp")
            tmp.write_text(live, encoding="utf-8")
            os.replace(tmp, target)
        written += 1
    return written, unchanged


def _freeze(folder, llm, content, included, sessions, dry_run, verbose) -> int:
    """Write the full feed as the next frozen bundle and mark its sessions."""
    path = next_bundle_path(folder)
    if verbose:
        print(f"  [{folder.name}] {'would freeze' if dry_run else 'froze'} {path.name}")
    if dry_run:
        return 1
    tmp = path.with_suffix(".md.tmp")
    tmp.write_text(content.replace("This LLM's live feed", "Frozen history"), encoding="utf-8")
    os.replace(tmp, path)
    llm["frozen"].append(path.name)
    for name, chars in included:
        body = sessions[name].read_text(encoding="utf-8", errors="replace")[:chars]
        llm["sessions"][name] = {"chars": chars, "hash": sha16(body)}
    return 1


def count_source_files(root: Path, *, bundles_only: bool) -> int:
    n = 0
    for folder in list_model_folders(root):
        for p in folder.iterdir():
            if not p.is_file() or p.suffix != ".md":
                continue
            if bundles_only:
                if p.name.startswith(BUNDLE_PREFIX) or p.name == LIVE_NAME:
                    n += 1
            else:
                if p.name not in SKIP_NAMES and not p.name.startswith("."):
                    n += 1
    return n


def pack_to_budget(
    root: Path,
    state: dict,
    *,
    max_sources: int,
    max_bundle_bytes: int,
    dry_run: bool,
    verbose: bool,
) -> None:
    """If total bundle count > max_sources, merge smallest bundles (not implemented as
    multi-file merge — instead reduce by raising effective pack density: archive
    remaining loose sessions more aggressively and merge tiny model folders into
    _misc bundles).

    Simple approach under budget pressure:
      1. Ensure all sessions are in bundles (archive_sessions=True path already).
      2. If bundle count still > max_sources, concatenate whole small folders
         into root-level _global_bundle_NNN.md until under cap.
    """
    bundles: List[Path] = []
    for folder in list_model_folders(root):
        bundles.extend(bundle_paths(folder))

    if len(bundles) <= max_sources:
        if verbose:
            print(f"budget ok: {len(bundles)} bundles ≤ {max_sources}")
        return

    if verbose:
        print(f"over budget: {len(bundles)} bundles > {max_sources} — packing globals")

    # Sort by size ascending; fold smallest into global packs
    bundles.sort(key=lambda p: p.stat().st_size)
    overflow = bundles  # all candidates
    # Rebuild: create global bundles at root
    global_dir = root / "_all"
    if not dry_run:
        global_dir.mkdir(exist_ok=True)

    # Write consolidated supersets from existing bundles (read-only merge)
    idx = 1
    current: Optional[Path] = None
    current_size = 0
    kept_globals = 0
    for b in overflow:
        data = b.read_text(encoding="utf-8", errors="replace")
        need = len(data) + 80
        if current is None or current_size + need > max_bundle_bytes:
            if kept_globals >= max_sources:
                # last file absorbs rest (hard cap)
                pass
            else:
                current = global_dir / f"{BUNDLE_PREFIX}{idx:03d}.md"
                idx += 1
                current_size = 0
                kept_globals += 1
                if not dry_run:
                    current.write_text(
                        f"# Global consolidated sources (budget pack ≤{max_sources})\n\n",
                        encoding="utf-8",
                    )
        assert current is not None
        header = f"\n\n# FROM {b.parent.name}/{b.name}\n\n"
        if not dry_run:
            with current.open("a", encoding="utf-8") as f:
                f.write(header + data)
        current_size += need

    if verbose:
        print(f"  wrote {kept_globals} global bundles under {global_dir}")


def repack_folder(
    folder: Path,
    *,
    max_bundle_bytes: int,
    dry_run: bool,
    verbose: bool,
) -> int:
    """Rewrite all bundles (+ archived sessions) into fewer files at max_bundle_bytes.

    Used after lowering/raising the rotate threshold (e.g. 195MiB for opencode/kilo).
    """
    model = folder.name
    pieces: List[Tuple[str, str]] = []  # (label, body)

    for b in bundle_paths(folder):
        try:
            pieces.append((b.name, b.read_text(encoding="utf-8", errors="replace")))
        except Exception:
            continue
    arch = folder / "_archived_sessions"
    if arch.is_dir():
        for sess in sorted(arch.glob("*.md")):
            try:
                pieces.append((f"archived/{sess.name}", sess.read_text(encoding="utf-8", errors="replace")))
            except Exception:
                continue
    # loose sessions still in folder
    for sess in sorted(p for p in folder.iterdir() if is_session_md(p)):
        try:
            pieces.append((sess.name, sess.read_text(encoding="utf-8", errors="replace")))
        except Exception:
            continue

    if not pieces:
        if verbose:
            print(f"  [{model}] repack: nothing to pack")
        return 0

    if dry_run:
        total = sum(len(b) for _, b in pieces)
        est = max(1, (total + max_bundle_bytes - 1) // max_bundle_bytes)
        print(f"  [{model}] repack dry-run: {len(pieces)} pieces ~{total} bytes → ~{est} bundles @ {max_bundle_bytes}")
        return est

    # remove old bundles (content captured in pieces)
    for b in bundle_paths(folder):
        b.unlink(missing_ok=True)

    written = 0
    idx = 1
    current: Optional[Path] = None
    size = 0
    for label, body in pieces:
        chunk = session_header(Path(label), model) + body
        if not chunk.endswith("\n"):
            chunk += "\n"
        if current is None or size + len(chunk) > max_bundle_bytes:
            current = folder / f"{BUNDLE_PREFIX}{idx:03d}.md"
            idx += 1
            current.write_text(
                f"# Consolidated sessions — {model}\n"
                f"# Repacked @ max_bundle_bytes={max_bundle_bytes}\n\n",
                encoding="utf-8",
            )
            size = current.stat().st_size
            written += 1
        assert current is not None
        with current.open("a", encoding="utf-8") as f:
            f.write(chunk)
        size += len(chunk)

    if verbose:
        print(f"  [{model}] repack → {written} bundles")
    return written


def run_all(
    root: Path,
    *,
    only_folder: Optional[str],
    max_sources: int,
    max_bundle_bytes: int,
    max_source_words: int,
    dry_run: bool,
    verbose: bool,
    archive_sessions: bool,
    agent_per_folder: bool,
    repack: bool,
) -> int:
    state = load_state(root)
    folders = list_model_folders(root)
    if only_folder:
        folders = [f for f in folders if f.name == only_folder]
        if not folders:
            print(f"no folder {only_folder!r}", file=sys.stderr)
            return 1

    if repack:
        total_b = 0
        for folder in folders:
            if agent_per_folder and verbose:
                print(f"=== repack agent: {folder.name} ===")
            total_b += repack_folder(
                folder,
                max_bundle_bytes=max_bundle_bytes,
                dry_run=dry_run,
                verbose=verbose,
            )
        print(f"[cleanup] repack done bundles≈{total_b} max_bundle_bytes={max_bundle_bytes}")
        return 0

    total_app = total_skip = 0
    for folder in folders:
        if agent_per_folder and verbose:
            print(f"=== agent folder: {folder.name} ===")
        app, sk = consolidate_folder(
            folder,
            state,
            max_source_words=max_source_words,
            dry_run=dry_run,
            verbose=verbose,
            archive_sessions=archive_sessions,
        )
        total_app += app
        total_skip += sk
        if verbose:
            print(f"  [{folder.name}] rewritten={app} unchanged={sk} files={len(bundle_paths(folder))}")

    if not dry_run:
        save_state(root, state)

    # Count final sources (prefer bundles if any exist, else all md)
    n_bundles = count_source_files(root, bundles_only=True)
    n_all = count_source_files(root, bundles_only=False)
    if verbose:
        print(f"totals: rewritten={total_app} unchanged={total_skip} sources={n_bundles} all_md={n_all}")

    if n_bundles > max_sources:
        pack_to_budget(
            root,
            state,
            max_sources=max_sources,
            max_bundle_bytes=max_bundle_bytes,
            dry_run=dry_run,
            verbose=verbose,
        )
        if not dry_run:
            save_state(root, state)

    n_final = count_source_files(root, bundles_only=True)
    if n_final == 0:
        n_final = n_all
    print(
        f"[cleanup] rewritten={total_app} unchanged={total_skip} "
        f"bundle_sources={count_source_files(root, bundles_only=True)} "
        f"loose_sessions={sum(1 for f in list_model_folders(root) for p in f.iterdir() if is_session_md(p))} "
        f"max_sources={max_sources}"
    )
    if count_source_files(root, bundles_only=True) > max_sources:
        print(
            f"[cleanup] WARNING: still over {max_sources} — raise --max-bundle-bytes or pack globals",
            file=sys.stderr,
        )
        return 2
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Consolidate notebooklm-sources ≤300 files")
    ap.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    ap.add_argument("--folder", help="only this model folder")
    ap.add_argument(
        "--agent-per-folder",
        action="store_true",
        help="process each folder as its own unit (same result; clearer logs)",
    )
    ap.add_argument("--max-sources", type=int, default=int(os.environ.get("NLM_MAX_SOURCES", "300")))
    ap.add_argument(
        "--max-bundle-bytes",
        type=int,
        # Default 195 MiB — start a new _bundle_NNN.md past this size.
        # Override: NLM_MAX_BUNDLE_BYTES or --max-bundle-bytes.
        default=int(os.environ.get("NLM_MAX_BUNDLE_BYTES", str(195 * 1024 * 1024))),
        help="rotate _bundle_NNN.md when exceeding this size (default 195MiB)",
    )
    ap.add_argument(
        "--max-source-words",
        type=int,
        default=int(os.environ.get("NLM_MAX_SOURCE_WORDS", "450000")),
        help="freeze an LLM's live feed into the next _bundle_NNN.md past this many words "
        "(NotebookLM caps a source at 500,000 words)",
    )
    ap.add_argument(
        "--archive-sessions",
        action="store_true",
        help="move rolled session .md/.json into <folder>/_archived_sessions/",
    )
    ap.add_argument(
        "--repack",
        action="store_true",
        help="rewrite all bundles from archives at --max-bundle-bytes (e.g. 195MiB); "
        "use after changing rotate size so opencode/kilo/etc. stop at one file until 195MB",
    )
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()

    root = args.root.expanduser()
    if not root.is_dir():
        print(f"missing {root}", file=sys.stderr)
        return 1

    return run_all(
        root,
        only_folder=args.folder,
        max_sources=args.max_sources,
        max_bundle_bytes=args.max_bundle_bytes,
        max_source_words=args.max_source_words,
        dry_run=args.dry_run,
        verbose=args.verbose,
        archive_sessions=args.archive_sessions,
        agent_per_folder=args.agent_per_folder,
        repack=args.repack,
    )


if __name__ == "__main__":
    raise SystemExit(main())
