//! The FUSE toolset manifest (`deploy/config/toolsets/cognitive.toolsets.json`)
//! and the door's own tiers/sets/readiness gates must agree.

use op_mcp_door::catalog::{TIER_HOT, TIER_MEMORY};
use op_mcp_door::readiness::{PENDING, default_gates};
use op_mcp_door::toolsets::{COGNITIVE_SET, LAZY};
use serde_json::Value;

fn manifest() -> Value {
    serde_json::from_slice(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/config/toolsets/cognitive.toolsets.json"
    )))
    .expect("manifest is JSON")
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect()
}

fn methods(keys: &[String]) -> Vec<&str> {
    keys.iter()
        .map(|k| k.split_once('.').expect("<plugin>.<method>").1)
        .collect()
}

#[test]
fn tiers_match_the_door() {
    let m = manifest();
    assert_eq!(m["format"], "opdbus/toolset-manifest@1");
    let memory = &m["tiers"]["memory"];
    assert_eq!(memory["never_grouped"], true);
    assert_eq!(memory["per_identity"], true);
    assert_eq!(memory["always_loaded"], true);
    let memory = strings(&memory["tools"]);
    assert_eq!(methods(&memory), TIER_MEMORY);
    let hot = strings(&m["tiers"]["hot"]["tools"]);
    assert_eq!(hot.len(), 10);
    assert_eq!(methods(&hot), TIER_HOT);
}

#[test]
fn cognitive_is_one_lazy_set_excluding_memory_and_hot() {
    let m = manifest();
    let sets = m["toolsets"].as_array().unwrap();
    assert_eq!(sets.len(), 1, "cognitive is grouped into exactly one set");
    let set = &sets[0];
    assert_eq!(set["id"], COGNITIVE_SET);
    assert_eq!(set["load"], LAZY);
    assert_eq!(set["projection"], "toolsets/cognitive/");
    assert_eq!(set["grant_gated"], true);
    assert_eq!(set["members"]["plugin"], "cognitive");
    let excluded = strings(&set["members"]["exclude_tiers"]);
    assert!(excluded.contains(&"memory".to_owned()));
    assert!(excluded.contains(&"hot".to_owned()));
}

#[test]
fn readiness_matches_the_door_gates() {
    let m = manifest();
    let gates = default_gates(None, None);
    let listed = m["readiness"].as_array().unwrap();
    assert_eq!(listed.len(), gates.len());
    let hot = strings(&m["tiers"]["hot"]["tools"]);
    for (entry, gate) in listed.iter().zip(&gates) {
        assert_eq!(entry["group"], gate.group.as_str());
        assert_eq!(strings(&entry["tools"]), gate.tools);
        assert_eq!(strings(&entry["collections"]), gate.collections);
        assert_eq!(entry["pending_status"], PENDING);
        for tool in &gate.tools {
            assert!(hot.contains(tool), "{tool} is gated but not hot");
        }
    }
}

#[test]
fn live_generated_example_has_the_layout() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/config/op-mcp-door/mcp-toolsets.json.example"
    );
    let file = op_mcp_door::toolsets::read_file(std::path::Path::new(path)).unwrap();
    assert!(file.missing.is_empty());
    assert_eq!(methods(&file.tiers.memory), TIER_MEMORY);
    assert_eq!(methods(&file.tiers.hot), TIER_HOT);
    let cognitive: Vec<_> = file
        .toolsets
        .iter()
        .filter(|s| s.tools.iter().any(|k| k.starts_with("cognitive.")))
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(cognitive, vec!["memory", COGNITIVE_SET]);
    let set = file
        .toolsets
        .iter()
        .find(|s| s.id == COGNITIVE_SET)
        .unwrap();
    assert_eq!(set.temperature, LAZY);
    for key in file.tiers.memory.iter().chain(&file.tiers.hot) {
        assert!(!set.tools.contains(key), "{key}");
    }
}
