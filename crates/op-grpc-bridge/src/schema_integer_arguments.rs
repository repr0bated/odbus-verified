//! Preserve declared JSON integers after protobuf Value decodes them as f64.
//! Opaque JSON and declared floating-point numbers keep their original shape.
use anyhow::{bail, ensure, Context, Result};
use serde_json::Value;

pub(crate) fn normalize_declared_integers(schema: &Value, input: &mut Value) -> Result<()> {
    visit(schema, schema, input, 0)
}

fn visit(root: &Value, schema: &Value, input: &mut Value, depth: usize) -> Result<()> {
    ensure!(
        depth <= 128,
        "integer argument schema nesting exceeds limit"
    );
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let pointer = reference
            .strip_prefix('#')
            .context("external argument schema reference is unsupported")?;
        let referred = root
            .pointer(pointer)
            .context("argument schema reference is missing")?;
        visit(root, referred, input, depth + 1)?;
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
            let mut matching = Vec::new();
            for branch in branches {
                let mut contract = branch.clone();
                if let Some(object) = contract.as_object_mut() {
                    for definitions in ["$defs", "definitions"] {
                        if let Some(value) = root.get(definitions) {
                            object.entry(definitions).or_insert_with(|| value.clone());
                        }
                    }
                }
                if jsonschema::Validator::new(&contract)?.is_valid(input) {
                    matching.push(branch);
                }
            }
            // Do not decide an ambiguous union's numeric representation.
            if matching.len() == 1 {
                visit(root, matching[0], input, depth + 1)?;
            }
        }
    }
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            visit(root, branch, input, depth + 1)?;
        }
    }
    let integer = match schema.get("type") {
        Some(Value::String(kind)) => kind == "integer",
        Some(Value::Array(kinds)) => {
            kinds.iter().any(|kind| kind == "integer")
                && kinds.iter().all(|kind| kind == "integer" || kind == "null")
        }
        _ => false,
    };
    if integer {
        if let Value::Number(number) = input {
            if number.is_f64() {
                let value = number.as_f64().context("invalid numeric argument")?;
                ensure!(
                    value.is_finite() && value.fract() == 0.0,
                    "declared integer argument is not integral"
                );
                // Protobuf Value transports every JSON number as a double.
                // Outside the exact IEEE-754 integer range, the caller's
                // integer has already been rounded and cannot be recovered.
                ensure!(
                    value.abs() <= 9_007_199_254_740_991.0,
                    "declared integer argument cannot be represented exactly by the transport"
                );
                // Exclusive upper limits prevent Rust's saturating float cast
                // from accepting 2^64 or rounding it to u64::MAX.
                if (0.0..18_446_744_073_709_551_616.0).contains(&value) {
                    *number = (value as u64).into();
                } else if (-9_223_372_036_854_775_808.0..0.0).contains(&value) {
                    *number = (value as i64).into();
                } else {
                    bail!("declared integer argument exceeds supported integer range");
                }
            }
        }
    }
    match input {
        Value::Object(object) => {
            for (key, value) in object {
                let field = schema
                    .get("properties")
                    .and_then(|fields| fields.get(key))
                    .or_else(|| {
                        schema
                            .get("additionalProperties")
                            .filter(|extra| extra.is_object())
                    });
                if let Some(field) = field {
                    visit(root, field, value, depth + 1)?;
                }
            }
        }
        Value::Array(array) => {
            for (index, value) in array.iter_mut().enumerate() {
                let item = schema
                    .get("prefixItems")
                    .and_then(Value::as_array)
                    .and_then(|items| items.get(index))
                    .or_else(|| schema.get("items").filter(|items| items.is_object()));
                if let Some(item) = item {
                    visit(root, item, value, depth + 1)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn nullable_nested_and_referenced_integers_preserve_exact_integer_shape() {
        let schema = json!({"type":"object", "$defs":{"counter":{"type":"integer","minimum":0}},
            "properties":{"limit":{"anyOf":[{"$ref":"#/$defs/counter"},{"type":"null"}]},
                "items":{"type":"array","items":{"type":"object","properties":{"id":{"$ref":"#/$defs/counter"}}}},
                "number":{"type":"number"},"opaque":{} }});
        let mut args =
            json!({"limit":1.0,"items":[{"id":2.0}],"number":3.0,"opaque":{"count":4.0}});
        normalize_declared_integers(&schema, &mut args).unwrap();
        assert_eq!(args["limit"].as_u64(), Some(1));
        assert_eq!(args["items"][0]["id"].as_u64(), Some(2));
        assert!(args["number"].as_number().unwrap().is_f64());
        assert!(args["opaque"]["count"].as_number().unwrap().is_f64());
        let mut null = json!({"limit":null});
        normalize_declared_integers(&schema, &mut null).unwrap();
        assert_eq!(null, json!({"limit":null}));
    }

    #[test]
    fn integer_boundaries_are_checked_and_native_u64_is_never_rounded() {
        let schema = json!({"type":"integer"});
        let mut exact = json!(u64::MAX);
        normalize_declared_integers(&schema, &mut exact).unwrap();
        assert_eq!(exact.as_u64(), Some(u64::MAX));
        let mut min = json!(-9_223_372_036_854_775_808.0);
        assert!(normalize_declared_integers(&schema, &mut min).is_err());
        for mut invalid in [
            json!(9_007_199_254_740_992.0),
            json!(18_446_744_073_709_551_616.0),
            json!(1.5),
        ] {
            assert!(normalize_declared_integers(&schema, &mut invalid).is_err());
        }
    }
}
