//! Shared test vector: subsidiary's `disclosure.test.ts` and
//! `web3/test/EtchedRecord.t.sol` build the same tree and must get the same
//! root, bundle hash and entry hash, so the three implementations agree byte for byte.

use super::*;
use serde_json::json;

const ROOT: &str = "0x99e2f8cc8681e2b207d3b8862993558ddbbb031bbd4f8cde53ddc5875911591d";
const BUNDLE_HASH: &str = "0x0e077debe0e39ed68ea92e6f92df17198712072a0381133db09ea19a97a89634";
/// `bundleText(bundle)` from disclosure.ts for the vector, byte for byte.
const TS_BUNDLE: &str = r#"{"fields":[{"class":"redacted","leaf":"0x753684795253aade0c689233d7f9c8c12f4dfd7742d9ad01d13a9837d9b5e2f3","path":"client","proof":["0x8a98dbbab0ea6a198c5c29dd30e26617443b21bb591734b0e99ec93618940bdb","0xf75689bb4abba5003627237483dcf289ecb2f346c5c618ea2af052dc86c8104c"]},{"class":"public","path":"status","proof":["0xc65cce2193afa3c019d581dfa6aebb38ecd06c51ce50b534a2e020388def1c76"],"salt":"0x3333333333333333333333333333333333333333333333333333333333333333","value":"delivered"}],"root":"0x99e2f8cc8681e2b207d3b8862993558ddbbb031bbd4f8cde53ddc5875911591d","version":"subsidiary.record/1","withheld":1}"#;
const ENTRY_HASH: &str = "0xf7c0fb89e1ae43539f5a16ceb090a040d647c79b310896878acbdf4c4c867890";

fn salt(b: &str) -> String {
    format!("0x{}", b.repeat(32))
}

fn record() -> Value {
    json!({ "status": "delivered", "client": "Ada Lovelace", "design": { "patent": "pending" } })
}

fn floors() -> HashMap<String, ProtectionFloor> {
    HashMap::from([
        ("status".to_owned(), ProtectionFloor::None),
        ("client".to_owned(), ProtectionFloor::Private),
    ])
}

fn salts() -> HashMap<String, String> {
    HashMap::from([
        ("client".to_owned(), salt("11")),
        ("design".to_owned(), salt("22")),
        ("status".to_owned(), salt("33")),
    ])
}

fn built() -> BuiltRecord {
    build_record(&record(), &floors(), &salts()).expect("vector builds")
}

#[test]
fn gives_the_shared_test_vector() {
    let b = built();
    assert_eq!(b.root, ROOT);
    assert_eq!(b.bundle.root, ROOT);
    assert_eq!(b.entry.root, ROOT);
    assert_eq!(bundle_text(&b.bundle).unwrap(), TS_BUNDLE);
    assert_eq!(bundle_hash(&b.bundle).unwrap(), BUNDLE_HASH);
    assert_eq!(entry_hash(&b.entry).unwrap(), ENTRY_HASH);
    let leaves: Vec<(&str, &str)> =
        b.entry.fields.iter().map(|f| (f.path.as_str(), f.leaf.as_str())).collect();
    assert_eq!(
        leaves,
        [
            ("client", "0x753684795253aade0c689233d7f9c8c12f4dfd7742d9ad01d13a9837d9b5e2f3"),
            ("design", "0x8a98dbbab0ea6a198c5c29dd30e26617443b21bb591734b0e99ec93618940bdb"),
            ("status", "0xf75689bb4abba5003627237483dcf289ecb2f346c5c618ea2af052dc86c8104c"),
        ]
    );
    assert_eq!(node_hash(&node_hash(leaves[0].1, leaves[1].1).unwrap(), leaves[2].1).unwrap(), ROOT);
}

#[test]
fn shows_none_redacts_private_withholds_secret_and_unclassified() {
    let b = built();
    let shown: Vec<(&str, FieldClass)> = b.bundle.fields.iter().map(|f| (f.path(), f.class())).collect();
    assert_eq!(shown, [("client", FieldClass::Redacted), ("status", FieldClass::Public)]);
    assert_eq!(b.bundle.withheld, 1);
    let text = bundle_text(&b.bundle).unwrap();
    assert!(!text.contains("Ada Lovelace") && !text.contains("pending"));
    assert!(text.contains("delivered"));
}

#[test]
fn keeps_everything_salts_included_in_the_entry() {
    let b = built();
    let kept: Vec<(&str, FieldClass, &Value)> =
        b.entry.fields.iter().map(|f| (f.path.as_str(), f.class, &f.value)).collect();
    assert_eq!(kept[0], ("client", FieldClass::Redacted, &json!("Ada Lovelace")));
    assert_eq!(kept[1], ("design", FieldClass::Withheld, &json!({ "patent": "pending" })));
    assert_eq!(kept[2], ("status", FieldClass::Public, &json!("delivered")));
}

#[test]
fn verifies_bundle_alone_and_entry_against_bundle() {
    let b = built();
    assert!(verify_bundle(&b.bundle).is_empty());
    assert!(verify_entry(&b.entry, Some(&b.bundle)).is_empty());
}

#[test]
fn random_salts_differ_and_still_verify() {
    let a = build_record(&record(), &floors(), &HashMap::new()).unwrap();
    let b = build_record(&record(), &floors(), &HashMap::new()).unwrap();
    assert_ne!(a.root, b.root);
    assert!(verify_entry(&a.entry, Some(&a.bundle)).is_empty());
}

#[test]
fn refuses_a_non_object_and_an_empty_record() {
    assert!(matches!(build_record(&json!([1]), &floors(), &salts()), Err(RecordError::NotAnObject)));
    assert!(matches!(build_record(&json!({}), &floors(), &salts()), Err(RecordError::Empty)));
}

#[test]
fn reads_floors_and_bundles_as_the_typescript_writes_them() {
    let f: HashMap<String, ProtectionFloor> =
        serde_json::from_value(json!({ "status": "none", "client": "private", "x": "secret" })).unwrap();
    assert_eq!(f["x"], ProtectionFloor::Secret);
    assert!(serde_json::from_value::<HashMap<String, ProtectionFloor>>(json!({ "a": "public" })).is_err());
    let text = bundle_text(&built().bundle).unwrap();
    let back: PublicBundle = serde_json::from_str(&text).unwrap();
    assert_eq!(bundle_text(&back).unwrap(), text);
}

#[test]
fn catches_a_changed_public_value_and_a_swapped_leaf() {
    let mut b = built().bundle;
    if let BundleField::Public { value, .. } = &mut b.fields[1] {
        *value = json!("cancelled");
    }
    assert_eq!(verify_bundle(&b), ["field status does not prove into the root"]);
    let mut b = built().bundle;
    if let BundleField::Redacted { leaf, .. } = &mut b.fields[0] {
        *leaf = salt("44");
    }
    assert_eq!(verify_bundle(&b), ["field client does not prove into the root"]);
}

#[test]
fn catches_a_changed_withheld_value() {
    let mut e = built().entry;
    e.fields[1].value = json!({ "patent": "granted" });
    assert_eq!(verify_entry(&e, None), ["field design: value and salt do not give its leaf"]);
}

#[test]
fn catches_a_bundle_that_shows_a_withheld_field() {
    let b = built();
    let mut bundle = b.bundle.clone();
    bundle.fields.push(BundleField::Public {
        path: "design".to_owned(),
        value: json!({ "patent": "pending" }),
        salt: salt("22"),
        proof: vec![],
    });
    bundle.withheld = 0;
    assert_eq!(
        verify_entry(&b.entry, Some(&bundle)),
        ["bundle shows design as public, entry says withheld", "bundle says 0 withheld, entry has 1"]
    );
}

#[test]
fn catches_a_bundle_that_drops_a_redacted_field() {
    let b = built();
    let mut bundle = b.bundle.clone();
    bundle.fields.remove(0);
    assert_eq!(verify_entry(&b.entry, Some(&bundle)), ["bundle leaves out redacted field client"]);
}

#[test]
fn reports_an_entry_with_no_fields() {
    let mut e = built().entry;
    e.fields.clear();
    assert_eq!(verify_entry(&e, None), ["entry has no fields"]);
}

#[test]
fn proves_a_withheld_field_off_the_public_chain() {
    let r = reveal_field(&built().entry, "design").unwrap();
    assert!(verify_reveal(ROOT, &r));
    let mut wrong = r.clone();
    wrong.value = json!({ "patent": "granted" });
    assert!(!verify_reveal(ROOT, &wrong));
    assert!(matches!(reveal_field(&built().entry, "nope"), Err(RecordError::NoField(_))));
}

#[test]
fn sorts_node_children_and_separates_leaves_from_nodes() {
    let (a, b) = (salt("01"), salt("02"));
    assert_eq!(node_hash(&a, &b).unwrap(), node_hash(&b, &a).unwrap());
    assert_ne!(leaf_hash("x", &a, &b).unwrap(), node_hash(&a, &b).unwrap());
    assert_eq!(fold_proof(&a, &[]).unwrap(), a);
    assert!(leaf_hash("x", "0xABC", &b).is_err());
}

#[test]
fn canonical_json_is_jq_s_c() {
    assert_eq!(canonical_json(&json!({ "b": 1, "a": [2, true, null] })).unwrap(), r#"{"a":[2,true,null],"b":1}"#);
    assert_eq!(
        canonical_json(&json!("q\"\\\n\t\u{1}\u{7f}é")).unwrap(),
        "\"q\\\"\\\\\\n\\t\\u0001\\u007fé\""
    );
    // Keys by UTF-8 bytes: "Z" < "a" < "é".
    assert_eq!(canonical_json(&json!({ "é": 1, "a": 2, "Z": 3 })).unwrap(), r#"{"Z":3,"a":2,"é":1}"#);
    assert_eq!(canonical_json(&json!(2.0)).unwrap(), "2");
    assert!(canonical_json(&json!(1.5)).is_err());
    assert!(canonical_json(&json!(-0.0)).is_err());
    assert!(canonical_json(&json!(9_007_199_254_740_992_i64)).is_err());
    assert_eq!(canonical_json(&json!(-9_007_199_254_740_991_i64)).unwrap(), "-9007199254740991");
}
