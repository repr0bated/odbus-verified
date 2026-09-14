//! Pinned nlm 0.11.2 response shapes exercised through the real generated
//! descriptors and protobuf encoding. No provider process or credentials needed.

use op_grpc_bridge::nlm_cli::normalize_notebooklm_result;
use op_plugins::state_plugins::notebooklm::NotebookLmPlugin;
use op_state::StatePlugin;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use serde::de::DeserializeSeed;
use serde_json::{json, Value};

fn response_name(method: &str) -> String {
    let suffix: String = method
        .split('_')
        .map(|part| format!("{}{}", part[..1].to_uppercase(), &part[1..]))
        .collect();
    format!("operation.plugin.v1.Notebooklm{suffix}Response")
}

fn round_trip(method: &str, provider: Value) -> (Value, Vec<u8>) {
    let result = normalize_notebooklm_result(method, provider).unwrap();
    let schema = NotebookLmPlugin::new().schema().unwrap();
    let returns = serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
    jsonschema::validate(&returns, &result)
        .unwrap_or_else(|error| panic!("{method} output violates its schema: {error}"));
    let pool = DescriptorPool::decode(op_grpc_bridge::proto::FILE_DESCRIPTOR_SET).unwrap();
    let descriptor = pool.get_message_by_name(&response_name(method)).unwrap();
    let text = serde_json::to_string(&result).unwrap();
    let mut deserializer = serde_json::Deserializer::from_str(&text);
    let message = descriptor
        .clone()
        .deserialize(&mut deserializer)
        .unwrap_or_else(|error| panic!("{method} cannot encode its provider result: {error}"));
    deserializer.end().unwrap();
    let bytes = message.encode_to_vec();
    let decoded = DynamicMessage::decode(descriptor, bytes.as_slice()).unwrap();
    let mut output = Vec::new();
    decoded
        .serialize_with_options(
            &mut serde_json::Serializer::new(&mut output),
            &SerializeOptions::new()
                .use_proto_field_name(true)
                .stringify_64_bit_integers(false)
                .skip_default_fields(false),
        )
        .unwrap();
    let decoded: Value = serde_json::from_slice(&output).unwrap();
    for (field, expected) in result.as_object().unwrap() {
        // Proto3 scalar null is represented by an absent/default value; nested
        // provider Value/Struct fields preserve null and their complete content.
        if !expected.is_null() {
            assert_json_preserved(expected, &decoded[field], &format!("{method}.{field}"));
        }
    }
    (decoded, bytes)
}

fn assert_json_preserved(expected: &Value, actual: &Value, path: &str) {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64(), "{path}"),
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_json_preserved(a, b, &format!("{path}[{index}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                assert_json_preserved(value, &b[key], &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

#[test]
fn notebook_get_preserves_emoji_sources_and_generated_rust_fields() {
    let (_, bytes) = round_trip(
        "notebook_get",
        json!({
            "notebook_id": "nb", "title": "Research", "source_count": 1,
            "url": "https://notebooklm.google.com/notebook/nb", "emoji": "📚",
            "sources": [{"id": "src", "title": "Reference"}]
        }),
    );
    let native = op_grpc_bridge::proto::plugin_methods::NotebooklmNotebookGetResponse::decode(
        bytes.as_slice(),
    )
    .unwrap();
    assert_eq!(
        native.emoji.and_then(|value| value.kind),
        Some(prost_types::value::Kind::StringValue("📚".into()))
    );
    assert_eq!(native.notebook_id.as_deref(), Some("nb"));
    assert_eq!(native.source_count, Some(1));
    assert_eq!(native.sources.len(), 1);
}

#[test]
fn pinned_provider_objects_and_collections_encode_without_losing_fields() {
    let cases = [
        (
            "notebook_list",
            json!([{"id": "nb", "title": "Research", "source_count": 1, "updated_at": "2026-09-13T00:00:00Z"}]),
        ),
        (
            "notebook_get",
            json!({"notebook_id": "nb", "title": "Empty", "source_count": 0, "url": "https://example.invalid/nb", "sources": [], "emoji": null}),
        ),
        (
            "notebook_create",
            json!({"notebook_id": "nb", "title": "New", "url": "https://example.invalid/nb", "message": "Created"}),
        ),
        ("notebook_delete", json!({"message": "Deleted notebook"})),
        (
            "notebook_describe",
            json!({"summary": "Findings", "suggested_topics": [{"question": "Why?", "prompt": "Explain why"}]}),
        ),
        (
            "notebook_query",
            json!({"answer": "A cited answer", "question": "Why?", "conversation_id": null,
            "sources_used": ["src", {"id": "src", "relevance": 0.9}],
            "citations": {"1": {"source_id": "src", "offset": 12, "extra": null}},
            "references": [{"source_id": "src", "text": "Original passage", "nested": [true, null, 3]}]}),
        ),
        (
            "source_add",
            json!({"source_type": "text", "source_id": "src", "title": "Reference"}),
        ),
        (
            "source_add",
            json!({"results": [{"source_type": "url", "source_id": "src", "title": "Reference"}], "added_count": 1}),
        ),
        (
            "source_delete",
            json!({"status": "success", "deleted_source_ids": ["src"], "deleted_count": 1}),
        ),
        (
            "source_describe",
            json!({"summary": "Reference material", "keywords": ["research"]}),
        ),
        (
            "source_get_content",
            json!({"content": "Reference", "title": "Document", "source_type": "text", "char_count": 9}),
        ),
        (
            "source_list_drive",
            json!([{"id": "src", "title": "Drive doc", "type": "google_docs", "url": "https://example.invalid/doc", "status": null, "is_stale": false}]),
        ),
        (
            "chat_list",
            json!({"notebook_id": "nb", "notebook_title": "Research", "sessions": [{"conversation_id": "chat", "turn_count": 1, "preview": "Why?", "is_active": true}]}),
        ),
        (
            "chat_get",
            json!({"notebook_id": "nb", "notebook_title": "Research", "conversation_id": "chat", "turn_count": 1, "transcript": [{"turn": 1, "query": "Why?", "answer": "Because", "references": ["src"]}]}),
        ),
        (
            "studio_create",
            json!({"artifact_type": "audio", "artifact_id": "audio", "status": "in_progress", "message": "Generating"}),
        ),
        (
            "studio_create",
            json!({"artifact_type": "mind_map", "artifact_id": "map", "title": "Map", "root_name": "Root", "children_count": 1, "mind_map_json": "{\"name\":\"Root\",\"children\":[{\"name\":\"Child\"}]}", "message": "Created"}),
        ),
        (
            "studio_status",
            json!([{"id": "audio", "artifact_id": "audio", "type": "audio", "status": "completed", "custom_instructions": null, "visual_style_prompt": null, "source_ids": ["src"], "duration_seconds": 10, "audio_url": "https://example.invalid/audio"}]),
        ),
        (
            "download_all_artifacts",
            json!({"notebook_id": "nb", "notebook_title": "Research", "output_dir": "/tmp/artifacts", "items": [{"artifact_id": "audio", "artifact_type": "audio", "title": "Audio", "path": "/tmp/artifacts/audio.mp3", "success": true, "error": null}], "skipped": [{"artifact_id": null, "artifact_type": "video", "title": "Video", "reason": "Not ready"}], "total_artifacts": 2, "downloaded": 1, "failed": 0}),
        ),
        (
            "download_all_artifacts",
            json!({"output_dir": "/tmp/artifacts", "notebooks": [{"notebook_id": "nb", "notebook_title": "Research", "output_dir": null, "downloaded": 0, "failed": 0, "skipped": 0, "error": "Unavailable"}], "total_notebooks": 1, "downloaded": 0, "failed": 0, "errored_notebooks": 1}),
        ),
        (
            "export_artifact",
            json!({"status": "success", "notebook_id": "nb", "artifact_id": "report", "export_type": "docs", "url": "https://example.invalid/export", "message": "Exported"}),
        ),
        (
            "note",
            json!({"notebook_id": "nb", "notes": [{"id": "note", "title": "Note", "preview": "Remember"}], "count": 1}),
        ),
        (
            "label",
            json!({"notebook_id": "nb", "labels": [{"id": "label", "name": "Research", "emoji": "📚", "source_ids": ["src"]}], "count": 1}),
        ),
        (
            "label",
            json!({"status": "success", "label_id": "label", "message": "Label updated"}),
        ),
        (
            "label",
            json!({"status": "success", "deleted_label_ids": ["label"], "count": 1, "message": "Deleted label"}),
        ),
        (
            "label",
            json!({"status": "success", "notebook_id": "nb", "labels": [], "count": 0}),
        ),
        (
            "notebook_share_status",
            json!({"notebook_id": "nb", "is_public": false, "access_level": "restricted", "public_link": null, "collaborators": [{"email": "reader@example.invalid", "role": "viewer", "is_pending": false, "display_name": null}], "collaborator_count": 1}),
        ),
        (
            "select_notebook",
            json!({"selected_notebook_id": "nb", "session_id": "session"}),
        ),
    ];
    for (method, value) in cases {
        round_trip(method, value);
    }
}

#[test]
fn text_receipts_and_auth_probes_have_real_native_response_fields() {
    let receipt = json!({"status": "command_completed", "output_format": "text", "message": "Provider diagnostic output", "diagnostic": ""});
    for method in [
        "notebook_rename",
        "source_sync_drive",
        "source_get_content",
        "source_rename",
        "chat_configure",
        "chat_export",
        "studio_delete",
        "studio_revise",
        "download_artifact",
        "research_start",
        "research_status",
        "research_import",
        "notebook_share_public",
        "notebook_share_invite",
        "notebook_share_batch",
        "batch",
        "cross_notebook_query",
        "pipeline",
        "tag",
        "note",
        "studio_status",
    ] {
        round_trip(method, receipt.clone());
    }
    for method in [
        "get_health",
        "server_info",
        "setup_auth",
        "reauth",
        "refresh_auth",
        "save_auth_tokens",
    ] {
        round_trip(
            method,
            json!({"provider": "jacob-bd/notebooklm-mcp-cli", "auth_status": "configured", "authenticated": true, "notebook_count": 37}),
        );
    }
    round_trip(
        "get_health",
        json!({"provider": "jacob-bd/notebooklm-mcp-cli", "auth_status": "unverified", "authenticated": false, "diagnostic": "Authentication unavailable"}),
    );
}

#[test]
fn every_notebook_method_has_a_typed_native_response() {
    let schema = NotebookLmPlugin::new().schema().unwrap();
    let pool = DescriptorPool::decode(op_grpc_bridge::proto::FILE_DESCRIPTOR_SET).unwrap();
    for method in schema.methods.keys() {
        let descriptor = pool.get_message_by_name(&response_name(method)).unwrap();
        assert!(
            descriptor.fields().count() > 0,
            "{method} lost its response contract"
        );
    }
}

#[test]
fn asynchronous_query_responses_preserve_pending_and_completed_results() {
    round_trip(
        "notebook_query_start",
        json!({"query_id":"q", "notebook_id":"n", "expires_at":1900000000}),
    );
    round_trip(
        "notebook_query_status",
        json!({"query_id":"q", "notebook_id":"n", "expires_at":1900000000, "state":"running", "result":null, "failure":null}),
    );
    round_trip(
        "notebook_query_status",
        json!({"query_id":"q", "notebook_id":"n", "expires_at":1900000000, "state":"completed", "result":{"answer":"資料 🌊", "question":"q", "conversation_id":null,"sources_used":[],"citations":{},"references":[]}, "failure":null}),
    );
}
