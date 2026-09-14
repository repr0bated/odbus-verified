from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import notebook_sync


def busctl_reply(envelope: object) -> str:
    body = json.dumps(envelope, separators=(",", ":"))
    return "s " + json.dumps(body) + "\n"


class NotebookSyncHelpersTest(unittest.TestCase):
    def test_builds_argv_only_invoke_tool_call(self) -> None:
        command = notebook_sync.build_plugin_call_command(
            "invoke_tool",
            {
                "tool_name": "add_source_text",
                "arguments": {"title": "a file.md", "text": "$(not a shell)"},
            },
            bus_address="unix:path=/tmp/test-session-bus.sock",
        )

        self.assertEqual(command[0], "busctl")
        self.assertEqual(command[1], "--address=unix:path=/tmp/test-session-bus.sock")
        self.assertEqual(command[3], notebook_sync.BRIDGE_BUS_NAME)
        self.assertEqual(command[4], notebook_sync.COGNITIVE_OBJECT_PATH)
        self.assertEqual(command[5], notebook_sync.PLUGIN_INTERFACE)
        self.assertEqual(command[6:9], ["Call", "ss", "invoke_tool"])
        self.assertEqual(
            json.loads(command[9]),
            {
                "tool_name": "add_source_text",
                "arguments": {"title": "a file.md", "text": "$(not a shell)"},
            },
        )

    def test_parses_accountability_envelope(self) -> None:
        result = {"content": [{"type": "text", "text": '{"ok":true}'}]}
        output = busctl_reply(
            {
                "success": True,
                "event_id": 42,
                "event_hash": "abc",
                "result": result,
            }
        )
        self.assertEqual(notebook_sync.parse_call_reply(output), result)
        self.assertEqual(notebook_sync.unwrap_tool_payload(result), {"ok": True})

    def test_rejects_non_successful_bridge_envelope(self) -> None:
        output = busctl_reply({"success": False, "result": {}})
        with self.assertRaises(notebook_sync.NotebookSyncError):
            notebook_sync.parse_call_reply(output)

    def test_extracts_notebook_id_from_mcp_text_content(self) -> None:
        result = {
            "content": [
                {
                    "type": "text",
                    "text": '{"notebook":{"id":"123e4567-e89b-42d3-a456-426614174000"}}',
                }
            ]
        }
        self.assertEqual(
            notebook_sync.extract_notebook_id(result),
            "123e4567-e89b-42d3-a456-426614174000",
        )

    def test_saved_notebook_id_prefers_environment_then_file(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            sources = Path(temporary)
            (sources / "NOTEBOOK_ID").write_text("file-id\n", encoding="utf-8")
            self.assertEqual(
                notebook_sync.saved_notebook_id(
                    sources, {"OP_NOTEBOOK_ID": "environment-id"}
                ),
                "environment-id",
            )
            self.assertEqual(notebook_sync.saved_notebook_id(sources, {}), "file-id")

    def test_notebook_env_quotes_single_quotes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            sources = Path(temporary)
            notebook_sync.write_notebook_state("notebook-id", "Jeremy's Notes", sources)
            state = (sources / "notebook.env").read_text(encoding="utf-8")
            self.assertIn("NOTEBOOK_TITLE='Jeremy'\"'\"'s Notes'", state)
            self.assertEqual(
                notebook_sync.saved_notebook_id(sources, {}), "notebook-id"
            )


class NotebookSyncGroupingTest(unittest.TestCase):
    def test_raw_chunks_are_lossless_and_hashes_are_stable(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "conversation.jsonl"
            payload = ("héllo " * 5000).encode("utf-8")
            source.write_bytes(payload)
            old_bytes = notebook_sync.MAX_CHUNK_BYTES
            old_words = notebook_sync.MAX_CHUNK_WORDS
            notebook_sync.MAX_CHUNK_BYTES = 20_000
            notebook_sync.MAX_CHUNK_WORDS = 10_000
            try:
                first = notebook_sync.RawChunks(root / "chunks", "model-a", "Model A")
                first.capture(source, "native_raw_transcript")
                first.flush()
                second = notebook_sync.RawChunks(root / "chunks", "model-a", "Model A")
                second.capture(source, "native_raw_transcript")
                second.flush()
            finally:
                notebook_sync.MAX_CHUNK_BYTES = old_bytes
                notebook_sync.MAX_CHUNK_WORDS = old_words

            self.assertGreater(len(first.chunks), 1)
            self.assertEqual([item["sha256"] for item in first.chunks],
                             [item["sha256"] for item in second.chunks])
            reconstructed = b"".join(Path(item["path"]).read_bytes() for item in first.chunks)
            marker = f"\n\nSOURCE {source} | native_raw_transcript | snapshot_bytes={len(payload)}\n\n".encode()
            self.assertEqual(reconstructed, marker + payload)
            self.assertEqual(first.inputs[0]["sha256"],
                             notebook_sync.hashlib.sha256(payload).hexdigest())

    def test_model_names_never_guesses_from_conversation_text(self) -> None:
        self.assertEqual(notebook_sync.model_names({
            "message": "The model is gpt-5 and it answered this",
            "prompt": "use claude-opus-4",
        }), set())
        self.assertEqual(notebook_sync.model_names({"metadata": {"model": "gpt-5"}}), set())
        self.assertEqual(notebook_sync.model_names({"model": "gpt-5", "message": "x"}), {"gpt-5"})

    def test_unavailable_model_is_unattributed_in_prepared_native_group(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            home = Path(temporary) / "home"
            state = Path(temporary) / "state"
            transcript = home / ".codex/sessions/2026/session.jsonl"
            transcript.parent.mkdir(parents=True)
            transcript.write_text('{"message":"model gpt-5 appeared in text"}\n', encoding="utf-8")
            with mock.patch.object(notebook_sync, "SOURCES", Path(temporary) / "empty-sources"):
                plan = notebook_sync.prepare_groups(state, home=home, repo=Path(temporary) / "repo")
            self.assertIn("Unattributed", " ".join(group["title"] for group in plan["groups"].values()))
            self.assertNotIn("gpt-5", " ".join(plan["groups"]))

    def test_unwrap_dispatch_rejects_provider_error(self) -> None:
        with self.assertRaises(notebook_sync.NotebookSyncError):
            notebook_sync.unwrap_dispatch({"success": True, "result": {"success": False}})
        with self.assertRaises(notebook_sync.NotebookSyncError):
            notebook_sync.unwrap_dispatch({"success": True, "result": None})
        with self.assertRaises(notebook_sync.NotebookSyncError):
            notebook_sync.unwrap_dispatch({"error": "provider unavailable"})

    def test_raw_chunks_respect_small_utf8_limits(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            old_bytes = notebook_sync.MAX_CHUNK_BYTES
            old_words = notebook_sync.MAX_CHUNK_WORDS
            notebook_sync.MAX_CHUNK_BYTES = 8
            notebook_sync.MAX_CHUNK_WORDS = 2
            try:
                chunks = notebook_sync.RawChunks(Path(temporary), "tiny", "Tiny")
                chunks.append("é " * 20)
                chunks.flush()
            finally:
                notebook_sync.MAX_CHUNK_BYTES = old_bytes
                notebook_sync.MAX_CHUNK_WORDS = old_words
            self.assertTrue(chunks.chunks)
            self.assertTrue(all(item["bytes"] <= 8 for item in chunks.chunks))
            self.assertTrue(all(item["words_upper_bound"] <= 2 for item in chunks.chunks))

    def test_grouped_call_keeps_credential_out_of_argv_and_stdin(self) -> None:
        header = "c2VjcmV0LXNpZA"
        completed = mock.Mock(returncode=0, stdout=json.dumps({"x-opdbus-sealed-id-bin": header}), stderr="")
        reply = mock.Mock(returncode=0, stdout=json.dumps({"success": True, "result": {"success": True, "result": {"content": []}}}), stderr="")
        with mock.patch.object(notebook_sync.subprocess, "run", side_effect=[completed, reply]) as run:
            notebook_sync.grouped_call("get_health", {}, "session-id")
        command = run.call_args_list[1].args[0]
        self.assertTrue(any("${OPDBUS_NOTEBOOK_CALL_SID}" in item for item in command))
        self.assertNotIn(header, " ".join(command))
        self.assertNotIn(header, run.call_args_list[1].kwargs["input"])


def write_plan_and_state(root: Path, digest: str, source_path: Path, *, old_id: str | None = None) -> None:
    chunk = {"slot": "model-a__0001", "path": str(source_path), "sha256": digest,
             "bytes": source_path.stat().st_size, "words_upper_bound": 1}
    notebook_sync.atomic_json(root / "plan.json", {"groups": {"model-a": {
        "title": "Model A", "chunks": [chunk], "inputs": []}}})
    sources = {}
    if old_id:
        sources[chunk["slot"]] = {"source_id": old_id, "sha256": "old", "verified": True}
    notebook_sync.atomic_json(root / "remote-state.json", {"groups": {"model-a": {
        "notebook_id": "nb-1", "sources": sources}}})


class NotebookSyncRemoteStateTest(unittest.TestCase):
    def _repartition_fixture(self) -> tuple[Path, str, str]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        source = root / "parent.txt"
        source.write_bytes(("α\r\nβ — raw\r\n" * 8).encode("utf-8"))
        digest = notebook_sync.hashlib.sha256(source.read_bytes()).hexdigest()
        write_plan_and_state(root, digest, source, old_id="old-src")
        state = json.loads((root / "remote-state.json").read_text())
        state["groups"]["model-a"]["pending"] = {
            "model-a__0001": {"source_id": "old-src", "sha256": digest, "path": str(source)}
        }
        notebook_sync.atomic_json(root / "remote-state.json", state)
        return root, "model-a__0001", digest

    def test_unchanged_verified_source_is_skipped(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "chunk.txt"
            source.write_text("same source", encoding="utf-8")
            digest = notebook_sync.hashlib.sha256(source.read_bytes()).hexdigest()
            write_plan_and_state(root, digest, source)
            state = json.loads((root / "remote-state.json").read_text())
            state["groups"]["model-a"]["sources"]["model-a__0001"] = {
                "source_id": "src-1", "sha256": digest, "verified": True}
            notebook_sync.atomic_json(root / "remote-state.json", state)
            with mock.patch.object(notebook_sync, "grouped_call", side_effect=[
                [{"id": "nb-1", "title": "Model A"}], {"sources": [{"id": "src-1", "title": source.name}]}
            ]) as call:
                counts = notebook_sync.sync_groups(root, "session")
            self.assertEqual(counts["skipped"], 1)
            self.assertEqual(call.call_count, 2)

    def test_new_upload_is_readable_before_old_source_delete(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "chunk.txt"
            source.write_text("new source", encoding="utf-8")
            digest = notebook_sync.hashlib.sha256(source.read_bytes()).hexdigest()
            write_plan_and_state(root, digest, source, old_id="old-src")
            events = []
            def fake(method, args, session):
                events.append(method)
                return {"notebook_list": [{"id": "nb-1", "title": "Model A"}],
                        "notebook_get": {"sources": [{"id": "old-src", "title": "old.txt"}]},
                        "source_add": {"source_id": "new-src"},
                        "source_get_content": {"content": "readable"},
                        "source_delete": {"success": True}}[method]
            with mock.patch.object(notebook_sync, "grouped_call", side_effect=fake):
                counts = notebook_sync.sync_groups(root, "session")
            self.assertEqual(counts["replaced"], 1)
            self.assertLess(events.index("source_get_content"), events.index("source_delete"))

    def test_failed_processing_preserves_old_and_pending_for_retry(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "chunk.txt"
            source.write_text("new source", encoding="utf-8")
            digest = notebook_sync.hashlib.sha256(source.read_bytes()).hexdigest()
            write_plan_and_state(root, digest, source, old_id="old-src")
            first_calls = {"notebook_list": [{"id": "nb-1", "title": "Model A"}],
                           "notebook_get": {"sources": [{"id": "old-src", "title": "old.txt"}]},
                           "source_add": {"source_id": "new-src"}}
            def first(method, args, session):
                if method == "source_get_content":
                    raise notebook_sync.NotebookSyncError("not ready")
                return first_calls[method]
            with mock.patch.object(notebook_sync, "grouped_call", side_effect=first), \
                 mock.patch.object(notebook_sync.time, "sleep"):
                with self.assertRaises(notebook_sync.NotebookSyncError):
                    notebook_sync.sync_groups(root, "session")
            saved = json.loads((root / "remote-state.json").read_text())
            group = saved["groups"]["model-a"]
            self.assertEqual(group["sources"]["model-a__0001"]["source_id"], "old-src")
            self.assertEqual(group["pending"]["model-a__0001"]["source_id"], "new-src")

            events = []
            def retry(method, args, session):
                events.append(method)
                return {"notebook_list": [{"id": "nb-1", "title": "Model A"}],
                        "notebook_get": {"sources": [{"id": "old-src"}, {"id": "new-src"}]},
                        "source_get_content": {"content": "ready"},
                        "source_delete": {"success": True}}[method]
            with mock.patch.object(notebook_sync, "grouped_call", side_effect=retry):
                notebook_sync.sync_groups(root, "session")
            self.assertEqual(events.count("source_add"), 0)
            self.assertIn("source_delete", events)

    def test_deferred_processing_keeps_pending_and_never_deletes_old(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "chunk.txt"
            source.write_text("new source", encoding="utf-8")
            digest = notebook_sync.hashlib.sha256(source.read_bytes()).hexdigest()
            write_plan_and_state(root, digest, source, old_id="old-src")
            events = []

            def fake(method, args, session):
                events.append(method)
                if method == "source_get_content":
                    return {"content": ""}
                return {
                    "notebook_list": [{"id": "nb-1", "title": "Model A"}],
                    "notebook_get": {"sources": [{"id": "old-src"}]},
                    "source_add": {"source_id": "new-src"},
                }[method]

            with mock.patch.object(notebook_sync, "grouped_call", side_effect=fake):
                counts = notebook_sync.sync_groups(root, "session", defer_processing=True)
            self.assertEqual(counts["pending"], 1)
            self.assertNotIn("source_delete", events)
            saved = json.loads((root / "remote-state.json").read_text())
            group = saved["groups"]["model-a"]
            self.assertEqual(group["sources"]["model-a__0001"]["source_id"], "old-src")
            self.assertEqual(group["pending"]["model-a__0001"]["source_id"], "new-src")

    def test_repartition_is_lossless_and_retires_original_receipt(self) -> None:
        root, slot, digest = self._repartition_fixture()
        result = notebook_sync.repartition_pending(root, slot)
        self.assertEqual(result["replacement_parts"], 2)
        plan = json.loads((root / "plan.json").read_text())
        children = plan["groups"]["model-a"]["chunks"]
        self.assertEqual(len(children), 2)
        self.assertEqual(
            b"".join(Path(child["path"]).read_bytes() for child in children),
            (root / "parent.txt").read_bytes(),
        )
        remote = json.loads((root / "remote-state.json").read_text())["groups"]["model-a"]
        self.assertIn("old-src", remote["retired"])
        self.assertEqual(remote["retired"]["old-src"]["replacement_slots"],
                         [child["slot"] for child in children])
        self.assertEqual(remote["retired"]["old-src"]["receipt"]["sha256"], digest)

    def test_repartition_8_and_16_parts_preserves_unicode_and_crlf(self) -> None:
        for parts in (8, 16):
            root, slot, _ = self._repartition_fixture()
            result = notebook_sync.repartition_pending(root, slot, parts=parts)
            self.assertEqual(result["replacement_parts"], parts)
            plan = json.loads((root / "plan.json").read_text())
            children = plan["groups"]["model-a"]["chunks"]
            self.assertEqual(len(children), parts)
            self.assertEqual(
                b"".join(Path(child["path"]).read_bytes() for child in children),
                (root / "parent.txt").read_bytes(),
            )

    def test_invalid_repartition_count_does_not_change_state(self) -> None:
        root, slot, _ = self._repartition_fixture()
        before = {name: (root / name).read_bytes() for name in (
            "plan.json", "remote-state.json")}
        for parts in (0, 1, 33, True):
            with self.assertRaises(notebook_sync.NotebookSyncError):
                notebook_sync.repartition_pending(root, slot, parts=parts)
            self.assertEqual({name: (root / name).read_bytes() for name in before}, before)
        self.assertFalse((root / "repair-journal.json").exists())

    def test_recursive_repartition_expands_ancestor_replacement_slots(self) -> None:
        root, slot, _ = self._repartition_fixture()
        notebook_sync.repartition_pending(root, slot, parts=2)
        plan = json.loads((root / "plan.json").read_text())
        children = plan["groups"]["model-a"]["chunks"]
        first = children[0]
        state = json.loads((root / "remote-state.json").read_text())
        state["groups"]["model-a"]["pending"][first["slot"]] = {
            "source_id": "child-old", "sha256": first["sha256"], "path": first["path"]
        }
        notebook_sync.atomic_json(root / "remote-state.json", state)
        notebook_sync.repartition_pending(root, first["slot"], parts=2)
        remote = json.loads((root / "remote-state.json").read_text())["groups"]["model-a"]
        retired = remote["retired"]["old-src"]["replacement_slots"]
        self.assertEqual(retired, [
            f"{first['slot']}-part1__0001", f"{first['slot']}-part2__0001", children[1]["slot"]
        ])

    def test_interrupted_repartition_journal_replays_all_state_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            expected = {
                "plan": {"version": 1, "groups": {"a": {"chunks": []}}},
                "remote-state": {"groups": {"a": {"sources": {}}}},
                "repartitions": {"slot:hash": [{"slot": "part"}]},
            }
            notebook_sync.atomic_json(root / "repair-journal.json", expected)
            notebook_sync.atomic_json(root / "plan.json", {"corrupt": True})
            notebook_sync.atomic_json(root / "remote-state.json", {"corrupt": True})
            notebook_sync.recover_repair(root)
            self.assertFalse((root / "repair-journal.json").exists())
            for name in ("plan", "remote-state", "repartitions"):
                self.assertEqual(json.loads((root / f"{name}.json").read_text()), expected[name])

    def test_retired_parent_is_not_deleted_until_all_replacements_verify(self) -> None:
        root, slot, _ = self._repartition_fixture()
        notebook_sync.repartition_pending(root, slot)
        plan = json.loads((root / "plan.json").read_text())
        children = plan["groups"]["model-a"]["chunks"]
        state = json.loads((root / "remote-state.json").read_text())
        state["groups"]["model-a"]["sources"][children[0]["slot"]] = {
            "source_id": "child-ready", "sha256": children[0]["sha256"], "verified": True,
        }
        notebook_sync.atomic_json(root / "remote-state.json", state)
        events = []

        def fake(method, args, session):
            events.append(method)
            if method == "notebook_list":
                return [{"id": "nb-1", "title": "Model A"}]
            if method == "notebook_get":
                return {"sources": [{"id": "old-src"}, {"id": "child-ready"}]}
            if method == "source_add":
                return {"source_id": "child-pending"}
            if method == "source_get_content":
                return {"content": ""}
            raise AssertionError(method)

        with mock.patch.object(notebook_sync, "grouped_call", side_effect=fake):
            counts = notebook_sync.sync_groups(root, "session", defer_processing=True)
        self.assertEqual(counts["pending"], 1)
        self.assertNotIn("source_delete", events)
        remote = json.loads((root / "remote-state.json").read_text())["groups"]["model-a"]
        self.assertIn("old-src", remote["retired"])

    def test_apply_repartitions_is_scoped_by_slot_and_hash(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            original = root / "original.txt"
            original.write_text("same bytes", encoding="utf-8")
            digest = notebook_sync.hashlib.sha256(original.read_bytes()).hexdigest()
            child = root / "child.txt"
            child.write_text("same", encoding="utf-8")
            chunks = [
                {"slot": "model-a__0001", "path": str(original), "sha256": digest},
                {"slot": "model-b__0001", "path": str(original), "sha256": digest},
            ]
            repairs = {f"model-a__0001:{digest}": [{
                "slot": "model-a__0001-part1", "path": str(child), "sha256": "bad"
            }]}
            with self.assertRaises(notebook_sync.NotebookSyncError):
                notebook_sync.apply_repartitions(chunks, repairs)
            # An unrelated slot with the same content hash remains untouched when
            # the scoped repair is valid for the first group.
            child.write_text("same bytes", encoding="utf-8")
            child_digest = notebook_sync.hashlib.sha256(child.read_bytes()).hexdigest()
            repairs[f"model-a__0001:{digest}"][0]["sha256"] = child_digest
            # A nested repair is followed recursively, while model-b's
            # same-hash chunk remains outside the repair scope.
            nested = root / "nested.txt"
            nested.write_text("same bytes", encoding="utf-8")
            nested_digest = notebook_sync.hashlib.sha256(nested.read_bytes()).hexdigest()
            repairs[f"model-a__0001-part1:{child_digest}"] = [{
                "slot": "model-a__0001-final", "path": str(nested), "sha256": nested_digest
            }]
            result = notebook_sync.apply_repartitions(chunks, repairs)
            self.assertEqual([item["slot"] for item in result], ["model-a__0001-final", "model-b__0001"])


if __name__ == "__main__":
    unittest.main()
