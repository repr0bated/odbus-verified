//! Reject in-place cycles before constructing unevaluated-location filters.
//!
//! JSON Schema Core 2020-12 section 9.4.1 leaves infinite same-instance
//! recursion undefined but forbids looping. Reject that unsupported graph;
//! do not invent annotation results by silently dropping a cyclic edge.
use ahash::AHashSet;
use serde_json::{Map, Value};

use crate::{compiler, paths::Location, Draft, ValidationError};

// Bound this recursive preflight independently of whether a cycle is present.
const MAX_ACTIVE_FILTERS: usize = 256;

pub(crate) fn check(
    ctx: &compiler::Context,
    parent: &Map<String, Value>,
    properties: bool,
) -> Result<(), ValidationError<'static>> {
    visit(
        ctx,
        parent,
        properties,
        ctx.draft() == Draft::Draft201909,
        &mut AHashSet::new(),
    )
}

fn visit(
    ctx: &compiler::Context,
    parent: &Map<String, Value>,
    properties: bool,
    recursive_draft: bool,
    active: &mut AHashSet<usize>,
) -> Result<(), ValidationError<'static>> {
    if active.len() >= MAX_ACTIVE_FILTERS {
        return Err(
            ValidationError::custom(
                Location::new(),
                ctx.location().clone(),
                &Value::Null,
                "in-place annotation dependency depth exceeds the supported limit of 256",
            )
            .to_owned(),
        );
    }
    // Registry resources and borrowed subschemas remain alive throughout this
    // traversal. Object identity also distinguishes fragments within one URI.
    // Only the current path is tracked: reuse in sibling branches is allowed.
    let identity = parent as *const Map<String, Value> as usize;
    if !active.insert(identity) {
        return Err(
            ValidationError::custom(
                Location::new(),
                ctx.location().clone(),
                &Value::Null,
                "cyclic in-place annotation dependency; recursion must advance to a child instance",
            )
            .to_owned(),
        );
    }
    let result = (|| {
        let dynamic_keyword = if recursive_draft { "$recursiveRef" } else { "$dynamicRef" };
        for keyword in ["$ref", dynamic_keyword] {
            let Some(value) = parent.get(keyword) else {
                continue;
            };
            let Some(reference) = value.as_str() else {
                // Draft2019 filters follow this key whenever it is present,
                // including when callers disable meta-schema validation.
                if keyword == "$recursiveRef" {
                    return Err(
                        ValidationError::custom(
                            Location::new(),
                            ctx.location().join(keyword),
                            &Value::Null,
                            "$recursiveRef must be a string",
                        )
                        .to_owned(),
                    );
                }
                continue;
            };
            let resolved = if keyword == "$recursiveRef" {
                ctx.lookup_recursive_reference()?
            } else {
                ctx.lookup(reference)?
            };
            let (contents, resolver, draft) = resolved.into_inner();
            if let Value::Object(subschema) = contents {
                let vocabularies = ctx.registry.find_vocabularies(draft, contents);
                let referenced = ctx.with_resolver_and_draft(
                    resolver,
                    draft,
                    vocabularies,
                    ctx.location().join(keyword),
                );
                visit(&referenced, subschema, properties, recursive_draft, active)?;
            }
        }
        for keyword in ["allOf", "anyOf", "oneOf"] {
            if let Some(Value::Array(subschemas)) = parent.get(keyword) {
                for (index, schema) in subschemas.iter().enumerate() {
                    visit_inline(
                        ctx,
                        schema,
                        ctx.location().join(keyword).join(index),
                        properties,
                        recursive_draft,
                        active,
                    )?;
                }
            }
        }
        if parent.contains_key("if") {
            for keyword in ["if", "then", "else"] {
                if let Some(schema) = parent.get(keyword) {
                    visit_inline(
                        ctx,
                        schema,
                        ctx.location().join(keyword),
                        properties,
                        recursive_draft,
                        active,
                    )?;
                }
            }
        }
        if properties {
            if let Some(Value::Object(dependencies)) = parent.get("dependentSchemas") {
                for (name, schema) in dependencies {
                    visit_inline(
                        ctx,
                        schema,
                        ctx.location().join("dependentSchemas").join(name),
                        properties,
                        recursive_draft,
                        active,
                    )?;
                }
            }
        }
        // properties/items/contains/unevaluated* apply to child instances.
        // Following them here would reject ordinary recursive tree schemas.
        Ok(())
    })();
    active.remove(&identity);
    result
}

fn visit_inline(
    ctx: &compiler::Context,
    schema: &Value,
    location: Location,
    properties: bool,
    recursive_draft: bool,
    active: &mut AHashSet<usize>,
) -> Result<(), ValidationError<'static>> {
    if let Value::Object(parent) = schema {
        // Inline filter constructors retain this resolver; only reference
        // edges replace it. Match the graph the filter actually constructs.
        let nested = ctx.with_location(location);
        visit(&nested, parent, properties, recursive_draft, active)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use referencing::Registry;
    use serde_json::json;
    use std::{rc::Rc, sync::Arc};

    fn check_filter(
        draft: Draft,
        schema: &Value,
        properties: bool,
    ) -> Result<(), ValidationError<'static>> {
        let base = compiler::DEFAULT_ROOT_URL;
        let registry = Arc::new(
            Registry::try_from_resources([(base, draft.create_resource(schema.clone()))])
                .unwrap(),
        );
        let resolver = Rc::new(registry.try_resolver(base).unwrap());
        let vocabularies = registry.find_vocabularies(draft, schema);
        let ctx = compiler::Context::new(
            Arc::new(crate::options().with_draft(draft).without_schema_validation()),
            Arc::clone(&registry),
            resolver,
            vocabularies,
            draft,
            Location::new(),
        );
        check(&ctx, schema.as_object().unwrap(), properties)
    }

    fn assert_cycle_error(draft: Draft, schema: &Value) {
        let error = crate::options().with_draft(draft).build(schema).unwrap_err();
        assert!(error
            .to_string()
            .contains("cyclic in-place annotation dependency"));
    }

    #[test]
    fn direct_and_mutual_annotation_cycles_are_compile_errors() {
        for draft in [Draft::Draft201909, Draft::Draft202012] {
            for keyword in ["unevaluatedItems", "unevaluatedProperties"] {
                let mut direct = json!({"$ref": "#"});
                direct[keyword] = json!(false);
                assert_cycle_error(draft, &direct);
                let mut mutual = json!({"$ref": "#/$defs/a", "$defs": {
                    "a": {"allOf": [{"$ref": "#/$defs/b"}]},
                    "b": {"$ref": "#/$defs/a"}
                }});
                mutual[keyword] = json!(false);
                assert_cycle_error(draft, &mutual);
                let mut dynamic = if draft == Draft::Draft201909 {
                    json!({"$recursiveAnchor": true, "$recursiveRef": "#"})
                } else {
                    json!({"$dynamicAnchor": "node", "$dynamicRef": "#node"})
                };
                dynamic[keyword] = json!(false);
                assert_cycle_error(draft, &dynamic);
            }
        }
    }

    #[test]
    fn malformed_recursive_reference_cannot_bypass_guard() {
        for properties in [false, true] {
            for invalid in [Value::Null, json!(false), json!(42), json!({})] {
                let schema = json!({"$recursiveAnchor": true, "$recursiveRef": invalid});
                let error = check_filter(Draft::Draft201909, &schema, properties).unwrap_err();
                assert!(error.to_string().contains("$recursiveRef must be a string"));
            }
        }
    }

    #[test]
    fn acyclic_reference_depth_is_bounded() {
        let mut definitions = Map::new();
        for index in 0..MAX_ACTIVE_FILTERS {
            definitions.insert(index.to_string(), json!({
                "$ref": format!("#/$defs/{}", index + 1),
            }));
        }
        definitions.insert(MAX_ACTIVE_FILTERS.to_string(), json!({}));
        let schema = json!({"$defs": definitions, "$ref": "#/$defs/0"});
        for draft in [Draft::Draft201909, Draft::Draft202012] {
            for properties in [false, true] {
                let error = check_filter(draft, &schema, properties).unwrap_err();
                assert!(error.to_string().contains("supported limit of 256"));
            }
        }
    }

    #[test]
    fn child_recursion_and_sibling_reference_reuse_remain_supported() {
        for draft in [Draft::Draft201909, Draft::Draft202012] {
            let schema = json!({"properties": {"child": {"$ref": "#"}}, "unevaluatedProperties": false});
            let validator = crate::options().with_draft(draft).build(&schema).unwrap();
            assert!(validator.is_valid(&json!({"child": {"child": {}}})));
            assert!(!validator.is_valid(&json!({"child": {"unknown": 1}})));
            let mut schema = json!({"unevaluatedItems": false});
            schema[if draft == Draft::Draft201909 { "items" } else { "prefixItems" }] =
                json!([{ "$ref": "#" }]);
            let validator = crate::options().with_draft(draft).build(&schema).unwrap();
            assert!(validator.is_valid(&json!([[[]]])));
            assert!(!validator.is_valid(&json!([[[], 1]])));
            let schema = if draft == Draft::Draft201909 {
                json!({"$recursiveAnchor": true, "properties": {"child": {"$recursiveRef": "#"}},
                    "unevaluatedProperties": false})
            } else {
                json!({"$dynamicAnchor": "node", "properties": {"child": {"$dynamicRef": "#node"}},
                    "unevaluatedProperties": false})
            };
            let validator = crate::options().with_draft(draft).build(&schema).unwrap();
            assert!(validator.is_valid(&json!({"child": {"child": {}}})));
            let schema = json!({"$defs": {"a": {"properties": {"a": true}}},
                "allOf": [{"$ref": "#/$defs/a"}, {"$ref": "#/$defs/a"}],
                "unevaluatedProperties": false});
            let validator = crate::options().with_draft(draft).build(&schema).unwrap();
            assert!(validator.is_valid(&json!({"a": 1})));
        }
    }
}
