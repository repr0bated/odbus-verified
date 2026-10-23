//! The plugin-owned upstream configuration contract for tched_router.
//! The schema is captured from the verified threetched binary at every seal
//! through the threetched plugin's pinned capture; nothing is imported or
//! stored here. All section/method schemas derive from that one capture.
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub fn document() -> &'static Value {
    super::threetched::config_schema()
        .expect("threetched config schema must be captured from the binary at seal")
}

fn references(node: &Value, out: &mut BTreeSet<String>) {
    match node {
        Value::Object(map) => {
            if let Some(name) = map
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|r| r.strip_prefix("#/$defs/"))
            {
                out.insert(name.to_string());
            }
            for child in map.values() {
                references(child, out);
            }
        }
        Value::Array(items) => {
            for child in items {
                references(child, out);
            }
        }
        _ => {}
    }
}

/// Derive a self-contained section from the plugin contract, retaining transitive definitions.
pub fn section_from(document: &Value, key: &str) -> Value {
    let property = &document["properties"][key];
    let mut section = property
        .as_object()
        .expect("config property is an object")
        .clone();
    let definitions = document["$defs"]
        .as_object()
        .expect("config contract has definitions");
    let mut pending = BTreeSet::new();
    let mut included = Map::new();
    references(property, &mut pending);
    while let Some(name) = pending.pop_first() {
        if included.contains_key(&name) {
            continue;
        }
        let definition = definitions.get(&name).expect("config reference resolves");
        references(definition, &mut pending);
        included.insert(name, definition.clone());
    }
    if !included.is_empty() {
        section.insert("$defs".into(), Value::Object(included));
    }
    Value::Object(section)
}

pub fn section(key: &str) -> Value {
    section_from(document(), key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_section_keeps_recursive_dependencies_without_unrelated_definitions() {
        let contract = serde_json::json!({
            "properties": {"network": {"$ref": "#/$defs/Network"}},
            "$defs": {
                "Network": {"type": "object", "properties": {"peer": {"$ref": "#/$defs/Peer"}}},
                "Peer": {"type": "object", "properties": {"network": {"$ref": "#/$defs/Network"}}},
                "Unrelated": {"type": "string"}
            }
        });
        let section = section_from(&contract, "network");
        assert_eq!(section["$defs"].as_object().unwrap().len(), 2);
        assert_eq!(section["$ref"], "#/$defs/Network");
        jsonschema::validator_for(&section).expect("recursive section is self-contained");
    }

    #[test]
    fn every_plugin_section_is_self_contained_and_validates_without_external_files() {
        super::super::threetched::prepare_schema_capture().expect("explicit schema capture");
        let contract = document();
        jsonschema::validator_for(contract).expect("full contract compiles");
        for key in contract["properties"].as_object().unwrap().keys() {
            jsonschema::validator_for(&section(key))
                .unwrap_or_else(|error| panic!("section {key}: {error}"));
        }
    }
}
