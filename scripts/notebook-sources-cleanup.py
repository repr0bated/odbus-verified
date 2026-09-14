#!/usr/bin/env python3
"""
Consolidate /home/jeremy/.notebooklm-sources so total NotebookLM source files stay ≤300.

Strategy:
  • Per model folder (default): roll individual session *.md into consolidated
    bundle files:  _bundle_001.md, _bundle_002.md, …
  • New conversations are **appended to the end** of the latest bundle for that
    folder (tracked by content hash in .cleanup-state.json).
  • Global budget: max --max-sources (default 300) across all bundles + optional
    keep-loose files; when over budget, force denser packing (larger bundles /
    fewer bundles per folder).

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
SKIP_NAMES = {
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


def _archive_session_file(folder: Path, sess: Path) -> None:
    """Move session .md (+ companion .json) into folder/_archived_sessions/.

    If the archive already has the name (prior roll), remove the loose copy.
    """
    arch = folder / "_archived_sessions"
    arch.mkdir(exist_ok=True)
    if sess.is_file():
        dest = arch / sess.name
        if dest.exists():
            sess.unlink()
        else:
            sess.rename(dest)
    companion = folder / f"{sess.stem}.json"
    if companion.is_file():
        jdest = arch / companion.name
        if jdest.exists():
            companion.unlink()
        else:
            companion.rename(jdest)


def session_header(path: Path, model: str) -> str:
    rel = path.name
    return (
        f"\n\n{'=' * 72}\n"
        f"# SESSION FILE: {rel}\n"
        f"# model_folder: {model}\n"
        f"# ingested_at: {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}\n"
        f"{'=' * 72}\n\n"
    )


def consolidate_folder(
    folder: Path,
    state: dict,
    *,
    max_bundle_bytes: int,
    dry_run: bool,
    verbose: bool,
    archive_sessions: bool,
) -> Tuple[int, int]:
    """Append new session files into bundles. Returns (appended, skipped)."""
    model = folder.name
    ingested: Dict[str, Any] = state.setdefault("ingested", {})
    sessions = sorted(p for p in folder.iterdir() if p.is_file() and is_session_md(p))
    appended = skipped = 0

    bundle = latest_bundle(folder)
    if bundle is None or (bundle.stat().st_size if bundle.exists() else 0) >= max_bundle_bytes:
        bundle = next_bundle_path(folder)
        if verbose:
            print(f"  [{model}] new bundle {bundle.name}")

    # Ensure bundle has a header once
    if not dry_run and not bundle.exists():
        bundle.write_text(
            f"# Consolidated sessions — {model}\n"
            f"# Auto-maintained by notebook-sources-cleanup (append-only).\n"
            f"# Do not hand-edit; new conversations are cat'd to the end.\n\n",
            encoding="utf-8",
        )

    for sess in sessions:
        key = f"{model}/{sess.name}"
        try:
            body = sess.read_text(encoding="utf-8", errors="replace")
        except Exception:
            failed_key = key
            skipped += 1
            continue
        h = sha16(body)
        prev = ingested.get(key)
        if prev and prev.get("hash") == h:
            # Already rolled into a bundle — still move loose files out of the tree
            if archive_sessions and not dry_run:
                _archive_session_file(folder, sess)
            skipped += 1
            continue

        # rotate bundle if this append would exceed soft cap (unless empty session)
        if not dry_run and bundle.exists() and bundle.stat().st_size + len(body) > max_bundle_bytes:
            bundle = next_bundle_path(folder)
            bundle.write_text(
                f"# Consolidated sessions — {model}\n"
                f"# Auto-maintained by notebook-sources-cleanup (append-only).\n\n",
                encoding="utf-8",
            )
            if verbose:
                print(f"  [{model}] rotated → {bundle.name}")

        chunk = session_header(sess, model) + body
        if not chunk.endswith("\n"):
            chunk += "\n"

        if dry_run:
            if verbose:
                print(f"  [{model}] would append {sess.name} → {bundle.name} ({len(body)} bytes)")
            appended += 1
            continue

        with bundle.open("a", encoding="utf-8") as f:
            f.write(chunk)

        ingested[key] = {
            "hash": h,
            "bundle": str(bundle.relative_to(folder.parent)) if folder.parent else bundle.name,
            "bundle_name": bundle.name,
            "model": model,
            "bytes": len(body),
            "appended_at": time.time(),
        }
        appended += 1

        # companion json: mark as rolled
        j = sess.with_suffix(".json")
        if archive_sessions:
            arch = folder / "_archived_sessions"
            if not dry_run:
                arch.mkdir(exist_ok=True)
                dest = arch / sess.name
                if not dest.exists():
                    sess.rename(dest)
                if j.is_file():
                    jdest = arch / j.name
                    if not jdest.exists():
                        j.rename(jdest)
            if verbose:
                print(f"  [{model}] archived {sess.name}")

    return appended, skipped


def count_source_files(root: Path, *, bundles_only: bool) -> int:
    n = 0
    for folder in list_model_folders(root):
        for p in folder.iterdir():
            if not p.is_file() or p.suffix != ".md":
                continue
            if bundles_only:
                if p.name.startswith(BUNDLE_PREFIX):
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
            max_bundle_bytes=max_bundle_bytes,
            dry_run=dry_run,
            verbose=verbose,
            archive_sessions=archive_sessions,
        )
        total_app += app
        total_skip += sk
        if verbose:
            print(f"  [{folder.name}] appended={app} unchanged={sk} bundles={len(bundle_paths(folder))}")

    if not dry_run:
        save_state(root, state)

    # Count final sources (prefer bundles if any exist, else all md)
    n_bundles = count_source_files(root, bundles_only=True)
    n_all = count_source_files(root, bundles_only=False)
    if verbose:
        print(f"totals: appended={total_app} skipped={total_skip} bundles={n_bundles} all_md={n_all}")

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
        f"[cleanup] appended={total_app} skipped={total_skip} "
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
        dry_run=args.dry_run,
        verbose=args.verbose,
        archive_sessions=args.archive_sessions,
        agent_per_folder=args.agent_per_folder,
        repack=args.repack,
    )


if __name__ == "__main__":
    raise SystemExit(main())
