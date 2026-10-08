//! Compile and exercise the generated contracts against disposable frozen captures.
//! Optional: typed_check --mount <mount-root> --session <declared-session> performs reads only.
#![allow(dead_code)]
#[path = "../src/dispatch.rs"]
mod dispatch;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/mounted.rs"]
mod mounted;
#[path = "../src/namespace.rs"]
mod namespace;
#[path = "../src/zeroclaw_surface.rs"]
mod zeroclaw_surface;
#[path = "../src/source.rs"]
mod source;
#[path = "../src/store.rs"]
mod store;
#[path = "../src/typed_bindings.rs"]
mod typed_bindings;
#[path = "../src/typed_runtime.rs"]
mod typed_runtime;

use anyhow::{Result, ensure};
use serde_json::json;
use std::collections::BTreeMap;
use typed_bindings::*;
use typed_runtime::TypedFilesystem;

fn fixture() -> Result<(tempfile::TempDir, TypedFilesystem)> {
    let directory = tempfile::tempdir()?;
    let store = store::ObjectStore::open(directory.path())?;
    let value = json!({"integer":1,"array":[false,{"text":"value"}],"escaped/key":{"..":7},"00000001":"object-key","":13,".value.json":17,"object.hash":19,"metadata.json":23,"unicode☃":29,"reserved":{".value.json":{"child":31}}});
    let object = model::TypedObjectBlob::new(
        "demo".into(),
        "a".repeat(64),
        model::PluginSchema {
            name: "demo".into(),
            version: "1".into(),
            fields: fixture_fields(value.as_object().unwrap()),
            public_state: json!({"paths":value.as_object().unwrap().keys().map(|key| format!("/{}",key.replace('~',"~0").replace('/',"~1"))).collect::<Vec<_>>()}),
            dependencies: vec!["demo".into()],
            methods: BTreeMap::from([(
                "Read".into(),
                model::MethodDecl {
                    name: "Read".into(),
                    args: json!({"type":"object","additionalProperties":false}),
                    returns: Some(json!({"type":"integer"})),
                    required_capability: Some("cap.demo.read@v1".into()),
                    side_effect: model::SideEffect::Read,
                    idempotent: true,
                    subid: "qry.demo.read".into(),
                },
            )]),
            ..Default::default()
        },
        value,
    )?;
    let view = store.capture_snapshot(
        source::SourceSnapshot {
            catalog_hash: "fixture".into(),
            catalog_generation: 1,
            objects: BTreeMap::from([("demo".into(), object)]),
        },
        false,
    )?;
    let filesystem = TypedFilesystem::direct_blob(store, ViewId::new(view.view_id)?)?;
    Ok((directory, filesystem))
}

fn fixture_fields(
    values: &serde_json::Map<String, serde_json::Value>,
) -> BTreeMap<String, model::FieldSchema> {
    values
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                model::FieldSchema {
                    field_type: fixture_type(value),
                    required: true,
                    description: String::new(),
                    default: None,
                    example: None,
                    constraints: Vec::new(),
                    read_only: false,
                    read_only_when: None,
                },
            )
        })
        .collect()
}
fn fixture_type(value: &serde_json::Value) -> model::FieldType {
    use model::FieldType as T;
    match value {
        serde_json::Value::Bool(_) => T::Boolean,
        serde_json::Value::Number(_) => T::Integer,
        serde_json::Value::String(_) => T::String,
        serde_json::Value::Object(values) => T::Object(fixture_fields(values)),
        serde_json::Value::Array(values) => {
            let mut types = Vec::new();
            for value in values {
                let ty = fixture_type(value);
                if !types.contains(&ty) {
                    types.push(ty);
                }
            }
            let item = if types.len() == 1 {
                types.remove(0)
            } else {
                T::OneOf(types)
            };
            T::Array(Box::new(item))
        }
        serde_json::Value::Null => panic!("fixture must declare a closed public type"),
    }
}
fn inspect(
    filesystem: &TypedFilesystem,
    directory: Directory,
    counts: &mut BTreeMap<&'static str, usize>,
) -> Result<()> {
    for entry in directory.list()? {
        match filesystem.node(entry)? {
            TypedNode::Directory(node) => inspect(filesystem, node, counts)?,
            TypedNode::StaticJson(node) => {
                node.get()?;
                *counts.entry("static-json").or_default() += 1;
            }
            TypedNode::ObjectHashFile(node) => {
                node.get()?;
                *counts.entry("object-hash").or_default() += 1;
            }
            TypedNode::FrozenValue(node) => {
                node.get()?;
                *counts.entry("frozen-value").or_default() += 1;
            }
            TypedNode::WorkspaceField(node) => {
                node.get()?;
                *counts.entry("workspace-field").or_default() += 1;
            }
            TypedNode::LiveValue(node) => {
                node.get()?;
                *counts.entry("live-snapshot").or_default() += 1;
            }
            TypedNode::Symlink(node) => {
                let target = node.target()?;
                if let Some(entry) = target.target_entry {
                    ensure!(
                        matches!(filesystem.node(entry)?, TypedNode::Directory(_)),
                        "declared symlink target is not a typed directory"
                    );
                }
                *counts.entry("semantic-symlink").or_default() += 1;
            }
            TypedNode::CallFile(_) => {
                *counts.entry("call-declaration").or_default() += 1;
            }
        }
    }
    Ok(())
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut counts = BTreeMap::new();
    if args.is_empty() {
        let (_directory, filesystem) = fixture()?;
        let root = filesystem.root().into_direct_blob()?;
        ensure!(!root.list()?.is_empty(), "empty direct namespace");
        inspect(&filesystem, filesystem.root(), &mut counts)?;
        ensure!(
            counts.get("frozen-value").copied().unwrap_or(0) > 4,
            "frozen fields were not read"
        );
        ensure!(
            filesystem
                .call_file(PluginId::new("demo")?, MethodId::new("Read")?)
                .is_err(),
            "frozen mode exposed call"
        );
    } else {
        ensure!(
            args.len() == 4 && args[0] == "--mount" && args[2] == "--session",
            "usage: typed_check [--mount ROOT --session SESSION]"
        );
        let filesystem = TypedFilesystem::via_fuse(
            MountRoot::new(args[1].clone())?,
            SessionId::new(args[3].clone())?,
        )?;
        let root = filesystem.root().into_via_fuse()?;
        ensure!(!root.list()?.is_empty(), "empty mounted namespace");
        inspect(&filesystem, filesystem.root(), &mut counts)?;
    }
    println!("typed filesystem routes checked: {counts:?}");
    Ok(())
}
#[cfg(test)]
mod binding_tests {
    use super::*;
    #[test]
    fn direct_shm_returns_fresh_owned_public_snapshots_and_rejects_catalog_drift() {
        let directory = tempfile::tempdir().unwrap();
        let schema: model::PluginSchema = serde_json::from_value(json!({
            "name":"demo","version":"1","fields":{
                "shown":{"field_type":"integer","required":true},
                "private":{"field_type":"string","required":true}
            },"public_state":{"paths":["/shown"]}
        }))
        .unwrap();
        let catalog = source::tests::fixture_catalog(directory.path(), &schema);
        let store_root = directory.path().join("store");
        let store = store::ObjectStore::open(&store_root).unwrap();
        let snapshot = catalog.capture_schema_only().unwrap();
        let schema_hash = snapshot.objects["demo"].source_schema_hash.clone();
        let view = store.capture_snapshot(snapshot, false).unwrap();
        let publish = |shown: i64| {
            let bytes = serde_json::to_vec(&json!({"shown":shown,"private":"secret"})).unwrap();
            op_core::projection_shm::update_projection_in(
                catalog.state_dir.to_str().unwrap(),
                "demo",
                |_| Ok(Some(bytes)),
            )
            .unwrap();
        };
        publish(1);
        let fs = TypedFilesystem::direct_shm(
            StoreRoot::new(store_root.to_str().unwrap()).unwrap(),
            ViewId::new(view.view_id).unwrap(),
            CatalogRoot::new(catalog.catalog_dir.to_str().unwrap()).unwrap(),
            StateDirectory::new(catalog.state_dir.to_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(
            !fs.root()
                .into_direct_shm()
                .unwrap()
                .list()
                .unwrap()
                .is_empty()
        );
        let live = fs
            .plugin_live_value(PluginId::new("demo").unwrap())
            .unwrap()
            .into_direct_shm()
            .unwrap();
        let owned = live.get().unwrap();
        assert_eq!(owned.get().clone().into_json().unwrap(), json!({"shown":1}));
        publish(2);
        assert_eq!(
            live.get().unwrap().get().clone().into_json().unwrap(),
            json!({"shown":2})
        );
        assert_eq!(owned.get().clone().into_json().unwrap(), json!({"shown":1}));
        assert_eq!(
            fs.plugin_state_typed::<serde_json::Value>(
                PluginId::new("demo").unwrap(),
                SchemaHash::new(schema_hash).unwrap()
            )
            .unwrap(),
            json!({"shown":2})
        );
        assert!(
            fs.call_file(
                PluginId::new("demo").unwrap(),
                MethodId::new("read").unwrap()
            )
            .is_err()
        );
        std::fs::write(
            catalog.catalog_dir.join(".manifest.json"),
            br#"{"catalog_hash":"changed","generation":2,"plugins":{}}"#,
        )
        .unwrap();
        assert!(live.get().is_err());
        assert_eq!(owned.get().clone().into_json().unwrap(), json!({"shown":1}));
    }
    #[test]
    fn frozen_owned_handles_and_paths_are_readable() {
        let (_dir, filesystem) = fixture().unwrap();
        let mut counts = BTreeMap::new();
        inspect(&filesystem, filesystem.root(), &mut counts).unwrap();
        assert!(counts["frozen-value"] >= 6);
        let root = filesystem.root().into_direct_blob().unwrap();
        assert!(!root.list().unwrap().is_empty());
    }
    #[test]
    fn factories_reject_wrong_owner_and_route() {
        let (_a, first) = fixture().unwrap();
        let (_b, second) = fixture().unwrap();
        let entry = first.root().list().unwrap().remove(0);
        assert!(second.node(entry).is_err());
        assert!(first.root().into_via_fuse().is_err());
        assert!(
            first
                .call_file(
                    PluginId::new("demo").unwrap(),
                    MethodId::new("Read").unwrap()
                )
                .is_err()
        );
    }
    #[test]
    fn identifier_and_json_pointer_bounds_are_explicit() {
        assert!(PluginId::new("").is_err());
        assert!(MethodId::new("x".repeat(129)).is_ok());
        assert!(SessionId::new("").is_err());
        assert!(SessionId::new("x".repeat(129)).is_err());
        assert!(SessionId::new("wrong/session").is_err());
        assert!(ObjectHash::new("a".repeat(63)).is_err());
        assert!(ViewId::new("g".repeat(64)).is_err());
        assert!(WorkspaceId::new("..").is_err());
        assert!(WorkspaceId::new("a/b").is_err());
        assert!(JsonPointer::new("/a~2b").is_err());
        assert!(JsonPointer::new("/a~1b").is_ok());
        assert!(MountRoot::new("/mount/../other").is_err());
    }
    #[test]
    fn captured_legal_field_names_do_not_confuse_node_classes_or_pointers() {
        let (_dir, filesystem) = fixture().unwrap();
        fn collect(fs: &TypedFilesystem, dir: Directory, values: &mut Vec<JsonValue>) {
            for entry in dir.list().unwrap() {
                match fs.node(entry).unwrap() {
                    TypedNode::Directory(dir) => collect(fs, dir, values),
                    TypedNode::FrozenValue(field) => {
                        values.push(field.get().unwrap().get().clone())
                    }
                    _ => {}
                }
            }
        }
        let mut values = Vec::new();
        collect(&filesystem, filesystem.root(), &mut values);
        for integer in [13, 17, 19, 23, 29, 31] {
            assert!(values.contains(&JsonValue::from_json(json!(integer)).unwrap()));
        }
        assert!(values.contains(&JsonValue::String("object-key".into())));
        assert!(values.contains(&JsonValue::Bool(false)));
    }
    #[test]
    fn typed_enumeration_keeps_original_field_keys_and_method_identifiers() {
        let (_dir, filesystem) = fixture().unwrap();
        fn visit(
            fs: &TypedFilesystem,
            dir: Directory,
            pointers: &mut Vec<String>,
            methods: &mut Vec<String>,
        ) {
            for entry in dir.list().unwrap() {
                if let Some(pointer) = entry.pointer() {
                    pointers.push(pointer.as_str().to_owned());
                }
                if let Some(method) = entry.method() {
                    methods.push(method.as_str().to_owned());
                }
                if let TypedNode::Directory(dir) = fs.node(entry).unwrap() {
                    visit(fs, dir, pointers, methods);
                }
            }
        }
        let mut pointers = Vec::new();
        let mut methods = Vec::new();
        visit(&filesystem, filesystem.root(), &mut pointers, &mut methods);
        assert!(pointers.contains(&"/".to_owned()));
        assert!(pointers.contains(&"/.value.json".to_owned()));
        assert!(!pointers.contains(&"/array/00000000".to_owned()));
        assert!(pointers.contains(&"/array/0".to_owned()));
        assert!(methods.contains(&"Read".to_owned()));
    }
    #[test]
    fn deserialized_identifiers_keep_source_validation() {
        assert!(serde_json::from_value::<SessionId>(json!("wrong/session")).is_err());
        assert!(serde_json::from_value::<ObjectHash>(json!("short")).is_err());
        assert_eq!(
            serde_json::from_value::<FieldKey>(json!(""))
                .unwrap()
                .as_str(),
            ""
        );
    }
    #[test]
    fn static_reads_do_not_reuse_retained_capture_payloads() {
        let (directory, filesystem) = fixture().unwrap();
        let entry = filesystem
            .root()
            .list()
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == ".3tchedfs.json")
            .unwrap();
        let TypedNode::StaticJson(metadata) = filesystem.node(entry).unwrap() else {
            panic!("expected metadata node");
        };
        metadata.get().unwrap();
        let plugins = filesystem
            .root()
            .list()
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "plugins")
            .unwrap();
        let TypedNode::Directory(plugins) = filesystem.node(plugins).unwrap() else {
            panic!("expected plugins directory");
        };
        let demo = plugins
            .list()
            .unwrap()
            .into_iter()
            .find(|entry| {
                entry
                    .plugin()
                    .is_some_and(|plugin| plugin.as_str() == "demo")
            })
            .unwrap();
        let TypedNode::Directory(demo) = filesystem.node(demo).unwrap() else {
            panic!("expected plugin directory");
        };
        let entry = demo
            .list()
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "value.json")
            .unwrap();
        let TypedNode::FrozenValue(value) = filesystem.node(entry).unwrap() else {
            panic!("expected frozen value");
        };
        let owned = value.get().unwrap();
        let expected = owned.get().clone();
        for object in std::fs::read_dir(directory.path().join("objects")).unwrap() {
            std::fs::write(object.unwrap().path(), b"corrupted fixture payload").unwrap();
        }
        assert!(
            metadata
                .get()
                .unwrap_err()
                .to_string()
                .contains("content-hash verification")
        );
        assert!(
            value
                .get()
                .unwrap_err()
                .to_string()
                .contains("content-hash verification")
        );
        assert_eq!(
            owned.get(),
            &expected,
            "caller-owned immutable handle did not survive backing corruption"
        );
    }
    #[test]
    fn ordinary_directory_never_becomes_identity_namespace() {
        let root = tempfile::tempdir().unwrap();
        assert!(
            TypedFilesystem::via_fuse(
                MountRoot::new(root.path().to_str().unwrap()).unwrap(),
                SessionId::new("session").unwrap()
            )
            .is_err()
        );
    }
}
