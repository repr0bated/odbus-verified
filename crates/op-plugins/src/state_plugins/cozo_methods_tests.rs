use super::*;

fn query(store: &CozoGraphShuttle, script: &str) -> NamedRows {
    run(store, script, BTreeMap::new(), false).unwrap()
}

fn fixture() -> (Arc<CozoGraphShuttle>, tempfile::TempDir) {
    (
        Arc::new(CozoGraphShuttle::new_in_memory().unwrap()),
        tempfile::tempdir().unwrap(),
    )
}

fn call(store: &CozoGraphShuttle, root: &Path, method: &str, args: Value) -> Value {
    let output = dispatch_sync(store, root, method, &args).unwrap();
    let schema = super::super::cozo_schema();
    let output_schema =
        serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
    let validator = jsonschema::Validator::new(&output_schema).unwrap();
    if let Err(error) = validator.validate(&output) {
        panic!("{method} returned invalid output: {error}");
    }
    output
}

fn exported(store: &CozoGraphShuttle, name: &str) -> NamedRows {
    store
        .db()
        .export_relations(std::iter::once(name))
        .unwrap()
        .remove(name)
        .unwrap()
}

#[tokio::test]
async fn all_seven_methods_execute_against_the_supplied_native_handle() {
    let (store, root) = fixture();
    let output = dispatch_cozo_with_archive_dir(
        store.clone(), root.path().to_path_buf(), "run_query",
        &json!({"query": "?[z_id,value] <- [[1,'one'],[2,'two']] :create portable_items {z_id:Int => value:String}"}),
    ).await.unwrap();
    assert_eq!(output["results"][0]["headers"], json!(["status"]));
    let listed = call(&store, root.path(), "list_relations", json!({}));
    assert!(listed["relations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|relation| relation["name"] == "portable_items"));
    let described = call(
        &store,
        root.path(),
        "describe_relation",
        json!({"name":"portable_items"}),
    );
    assert_eq!(described["columns"][0]["column"], "z_id");
    assert_eq!(described["columns"][1]["column"], "value");
    let imported = call(
        &store,
        root.path(),
        "import_relations",
        json!({"data": {
            "portable_items": {"headers":["value","z_id"],"rows":[["updated",2],["three",3]]}
        }}),
    );
    assert_eq!(imported["rows"], 2);
    assert_eq!(imported["committed"], true);
    assert_eq!(imported["triggers_enabled"], true);
    let expected = json!({"headers":["z_id","value"],"rows":[[1,"one"],[2,"updated"],[3,"three"]]});
    let export = call(
        &store,
        root.path(),
        "export_relations",
        json!({"relations":["portable_items"]}),
    );
    assert_eq!(export["data"]["portable_items"], expected);
    query(&store, "::index create portable_items:by_value {value}");
    let receipt = call(
        &store,
        root.path(),
        "backup",
        json!({"path":"snapshot","relations":["portable_items"]}),
    );
    assert_eq!(
        fs::metadata(root.path().join(receipt["archive"].as_str().unwrap()))
            .unwrap()
            .mode()
            & 0o777,
        0o400
    );
    query(
        &store,
        "?[z_id,value] <- [[1,'changed'],[4,'extra']] :put portable_items {z_id=>value}",
    );
    let restored = call(
        &store,
        root.path(),
        "restore",
        json!({"path":receipt["archive"]}),
    );
    assert_eq!(restored["committed"], true);
    assert_eq!(restored["atomic"], true);
    let export = call(
        &store,
        root.path(),
        "export_relations",
        json!({"relations":["portable_items"]}),
    );
    assert_eq!(export["data"]["portable_items"], expected);
    assert_eq!(
        query(
            &store,
            "?[z_id] := *portable_items:by_value{value:'updated',z_id}"
        )
        .rows,
        vec![vec![DataValue::Num(Num::Int(2))]]
    );
}

#[test]
fn archive_recreates_typed_relations_and_survives_persistent_reopen() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("db");
    let archive_path = root.path().join("archives");
    let (receipt, expected) = {
        let store = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
        let rows = DataValue::List(vec![DataValue::List(vec![
            DataValue::from("key"),
            DataValue::Bytes(vec![0, 1, 255]),
            DataValue::Null,
            DataValue::List(vec![
                DataValue::Num(Num::Float(0.25)),
                DataValue::Num(Num::Float(0.75)),
            ]),
            DataValue::List(vec![DataValue::Num(Num::Int(7)), DataValue::from("tuple")]),
        ])]);
        run(&store, "?[id,payload,optional,vector,pair] <- $rows :create portable_typed {id:String => payload:Any,optional:Int?,vector:<F32;2>,pair:(Int,String)}", BTreeMap::from([("rows".into(),rows)]), false).unwrap();
        query(
            &store,
            "::index create portable_typed:by_optional {optional}",
        );
        let expected = exported(&store, "portable_typed").rows;
        let receipt = call(
            &store,
            &archive_path,
            "backup",
            json!({"path":"typed","relations":["portable_typed"]}),
        );
        (receipt, expected)
    };
    {
        let store = CozoGraphShuttle::new_persistent(db_path.clone()).unwrap();
        assert_eq!(exported(&store, "portable_typed").rows, expected);
        query(&store, "::index drop portable_typed:by_optional");
        query(&store, "::remove portable_typed");
        call(
            &store,
            &archive_path,
            "restore",
            json!({"path":receipt["archive"]}),
        );
        assert_eq!(exported(&store, "portable_typed").rows, expected);
        let description = call(
            &store,
            &archive_path,
            "describe_relation",
            json!({"name":"portable_typed"}),
        );
        assert_eq!(description["indices"][0]["name"], "by_optional");
        assert_eq!(description["columns"][3]["type"], "<F32;2>");
    }
    let reopened = CozoGraphShuttle::new_persistent(db_path).unwrap();
    assert_eq!(exported(&reopened, "portable_typed").rows, expected);
}

#[test]
fn malformed_later_relation_is_rejected_before_any_restore_write() {
    let (store, root) = fixture();
    for name in ["portable_a", "portable_b"] {
        query(
            &store,
            &format!("?[id,value] <- [[1,'original']] :create {name} {{id:Int=>value:String}}"),
        );
    }
    let names = vec!["portable_a".into(), "portable_b".into()];
    let mut archive = archive_from_snapshot(snapshot(&store, &names, true).unwrap()).unwrap();
    let expected_a = exported(&store, "portable_a").rows;
    let expected_b = exported(&store, "portable_b").rows;
    archive.relations[0].rows[0][1] =
        serde_json::to_value(DataValue::from("must-not-commit")).unwrap();
    archive.relations[1].rows[0][1] = serde_json::to_value(DataValue::Bool(true)).unwrap();
    let receipt = write_archive(root.path(), "malformed", &archive).unwrap();
    let error = dispatch_sync(
        &store,
        root.path(),
        "restore",
        &json!({"path":receipt.archive}),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("declared Cozo type"),
        "{error:#}"
    );
    assert_eq!(exported(&store, "portable_a").rows, expected_a);
    assert_eq!(exported(&store, "portable_b").rows, expected_b);
    archive.relations[0].metadata.columns[1].type_name = "String} {::remove portable_a}".into();
    assert!(restore_archive(&store, &archive).is_err());
    assert_eq!(exported(&store, "portable_a").rows, expected_a);
}

#[test]
fn import_preserves_triggers_and_rolls_back_every_relation_on_native_failure() {
    let (store, root) = fixture();
    for name in ["portable_input", "portable_log", "portable_z"] {
        query(&store, &format!(":create {name} {{id:Int=>value:String}}"));
    }
    query(
        &store,
        "::set_triggers portable_input on put { ?[id,value] := _new[id,value] :put portable_log {id=>value} }",
    );
    let error = dispatch_sync(
        &store,
        root.path(),
        "import_relations",
        &json!({"data":{
            "portable_input":{"headers":["id","value"],"rows":[[1,"rolled-back"]]},
            "portable_z":{"headers":["id","value"],"rows":[[2,true]]}
        }}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("Cozo:"));
    assert!(exported(&store, "portable_input").rows.is_empty());
    assert!(exported(&store, "portable_log").rows.is_empty());
    call(
        &store,
        root.path(),
        "import_relations",
        json!({"data":{
            "portable_input":{"headers":["id","value"],"rows":[[1,"committed"]]}
        }}),
    );
    assert_eq!(
        public_rows(exported(&store, "portable_log")).unwrap().rows,
        vec![vec![json!(1), json!("committed")]]
    );
    let error = dispatch_sync(
        &store,
        root.path(),
        "backup",
        &json!({"path":"triggered","relations":["portable_input"]}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("trigger"), "{error:#}");
}

#[test]
fn archives_report_unrepresentable_metadata_and_reject_schema_drift() {
    let (store, root) = fixture();
    query(
        &store,
        ":create portable_defaults {id:Int=>value:String default 'default'}",
    );
    let error = dispatch_sync(
        &store,
        root.path(),
        "backup",
        &json!({"path":"defaults","relations":["portable_defaults"]}),
    )
    .unwrap_err();
    assert!(error.to_string().contains("default"), "{error:#}");
    query(
        &store,
        "?[id,value] <- [[1,'one']] :create portable_plain {id:Int=>value:String}",
    );
    let receipt = call(
        &store,
        root.path(),
        "backup",
        json!({"path":"plain","relations":["portable_plain"]}),
    );
    query(&store, "::index create portable_plain:by_value {value}");
    let error = dispatch_sync(
        &store,
        root.path(),
        "restore",
        &json!({"path":receipt["archive"]}),
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("different metadata"),
        "{error:#}"
    );
    let mut archive =
        archive_from_snapshot(snapshot(&store, &["portable_plain".into()], true).unwrap()).unwrap();
    archive.relations[0].metadata.indices[0].index_type = "hnsw".into();
    assert!(decode_archive(&archive)
        .unwrap_err()
        .to_string()
        .contains("portable"));
    archive.relations[0].metadata.indices.clear();
    archive.relations[0].metadata.relation.description = "cannot restore this description".into();
    assert!(decode_archive(&archive)
        .unwrap_err()
        .to_string()
        .contains("description"));
}

#[test]
fn metadata_is_checked_again_inside_the_write_transaction() {
    let (store, _) = fixture();
    for name in ["portable_guarded", "portable_audit"] {
        query(&store, &format!(":create {name} {{id:Int=>value:String}}"));
    }
    let metadata = snapshot(&store, &["portable_guarded".into()], false)
        .unwrap()
        .remove(0)
        .0;
    let mut script = "{::relations} as _restore_catalog\n".to_owned();
    let mut params = BTreeMap::new();
    append_metadata_checks(&mut script, &mut params, 0, &metadata).unwrap();
    script.push_str("{ ?[id,value] <- [[1,'must-not-commit']] :put portable_guarded {id=>value} }");
    query(
        &store,
        "::set_triggers portable_guarded on put { ?[id,value] := _new[id,value] :put portable_audit {id=>value} }",
    );
    assert!(run(&store, &script, params, false).is_err());
    assert!(exported(&store, "portable_guarded").rows.is_empty());
    assert!(exported(&store, "portable_audit").rows.is_empty());
}

#[test]
fn archives_are_unique_immutable_confined_and_digest_verified() {
    let (store, root) = fixture();
    query(
        &store,
        "?[id,value] <- [[1,'one']] :create portable_paths {id:Int=>value:String}",
    );
    let args = json!({"path":"same","relations":["portable_paths"]});
    let first = call(&store, root.path(), "backup", args.clone());
    let second = call(&store, root.path(), "backup", args);
    assert_ne!(first["archive"], second["archive"]);
    assert_eq!(first["sha256"], second["sha256"]);
    for path in ["../escape", "/tmp/escape", "nested/file"] {
        assert!(dispatch_sync(
            &store,
            root.path(),
            "backup",
            &json!({"path":path,"relations":["portable_paths"]})
        )
        .is_err());
        assert!(dispatch_sync(&store, root.path(), "restore", &json!({"path":path})).is_err());
    }
    let first_name = first["archive"].as_str().unwrap();
    let first_path = root.path().join(first_name);
    fs::set_permissions(&first_path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&first_path, b"{}").unwrap();
    fs::set_permissions(&first_path, fs::Permissions::from_mode(0o400)).unwrap();
    assert!(read_archive(root.path(), first_name)
        .unwrap_err()
        .to_string()
        .contains("digest mismatch"));
    let linked_name = format!("link.{}.cozo.json", second["sha256"].as_str().unwrap());
    std::os::unix::fs::symlink(
        root.path().join(second["archive"].as_str().unwrap()),
        root.path().join(&linked_name),
    )
    .unwrap();
    assert!(read_archive(root.path(), &linked_name).is_err());
}

#[test]
fn archive_type_parser_rejects_programs_and_validates_native_values() {
    for invalid in [
        "String} {::remove x}",
        "Int default 2",
        "<F32;0>",
        "[Int;1+1]",
        "Unknown",
        "(Int,String",
    ] {
        assert!(TypeSpec::parse(invalid).is_err(), "accepted {invalid}");
    }
    let pair = TypeSpec::parse("[Int;2]").unwrap();
    assert!(pair.accepts(&DataValue::List(vec![
        DataValue::Num(Num::Int(1)),
        DataValue::Num(Num::Int(2))
    ])));
    assert!(!pair.accepts(&DataValue::List(vec![DataValue::Num(Num::Int(1))])));
    assert!(!TypeSpec::parse("Float")
        .unwrap()
        .accepts(&DataValue::Num(Num::Float(f64::INFINITY))));
    assert!(TypeSpec::parse("Int?").unwrap().accepts(&DataValue::Null));
    assert!(!TypeSpec::parse("Int").unwrap().accepts(&DataValue::Null));
}

#[test]
fn vector_codec_preserves_nested_bit_words_and_rejects_unsafe_native_tags() {
    let cell = DataValue::List(vec![
        DataValue::Vec(Vector::F32(vec![0.25, -0.0, 0.75].into())),
        DataValue::Vec(Vector::F64(vec![f64::MIN_POSITIVE, -0.0].into())),
    ]);
    let encoded = encode_archive_cell(&cell, 0).unwrap();
    assert_eq!(decode_archive_cell(&encoded, 0).unwrap(), cell);
    assert!(decode_archive_cell(&json!({"Regex":".*"}), 0).is_err());
    assert!(decode_archive_cell(&json!({"Vec":[0,[1,2,3]]}), 0).is_err());
    assert!(decode_archive_cell(&json!({"VecF32Bits":[u64::MAX]}), 0).is_err());
}
