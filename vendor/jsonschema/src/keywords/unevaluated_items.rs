use referencing::Draft;
use serde_json::{Map, Value};

use crate::{
    compiler,
    node::SchemaNode,
    paths::{LazyLocation, Location},
    validator::Validate,
    ValidationError,
};

use super::CompilationResult;

pub(crate) trait ItemsFilter: Send + Sync + Sized + 'static {
    fn new<'a>(
        ctx: &'a compiler::Context<'a>,
        parent: &'a Map<String, Value>,
    ) -> Result<Self, ValidationError<'a>>;
    fn unevaluated(&self) -> Option<&SchemaNode>;

    fn is_valid(&self, instance: &Value) -> bool {
        self.unevaluated()
            .as_ref()
            .map(|u| u.is_valid(instance))
            .unwrap_or(false)
    }

    fn mark_evaluated_indexes(&self, instance: &Value, indexes: &mut Vec<bool>);
}

pub(crate) struct UnevaluatedItemsValidator<F: ItemsFilter> {
    location: Location,
    filter: F,
}

impl<F: ItemsFilter> UnevaluatedItemsValidator<F> {
    #[inline]
    pub(crate) fn compile<'a>(
        ctx: &'a compiler::Context,
        parent: &'a Map<String, Value>,
    ) -> CompilationResult<'a> {
        Ok(Box::new(UnevaluatedItemsValidator {
            location: ctx.location().join("unevaluatedItems"),
            filter: F::new(ctx, parent)?,
        }))
    }
}

impl<F: ItemsFilter> Validate for UnevaluatedItemsValidator<F> {
    fn is_valid(&self, instance: &Value) -> bool {
        if let Value::Array(items) = instance {
            // NOTE: It could be a packed bitset instead
            let mut indexes = vec![false; items.len()];
            self.filter.mark_evaluated_indexes(instance, &mut indexes);

            for (item, is_evaluated) in items.iter().zip(indexes) {
                if !is_evaluated && !self.filter.is_valid(item) {
                    return false;
                }
            }
        }
        true
    }

    fn validate<'i>(
        &self,
        instance: &'i Value,
        location: &LazyLocation,
    ) -> Result<(), ValidationError<'i>> {
        if let Value::Array(items) = instance {
            // NOTE: It could be a packed bitset instead
            let mut indexes = vec![false; items.len()];
            self.filter.mark_evaluated_indexes(instance, &mut indexes);
            let mut unevaluated = vec![];
            for (item, is_evaluated) in items.iter().zip(indexes) {
                if !is_evaluated && !self.filter.is_valid(item) {
                    unevaluated.push(item.to_string());
                }
            }
            if !unevaluated.is_empty() {
                return Err(ValidationError::unevaluated_items(
                    self.location.clone(),
                    location.into(),
                    instance,
                    unevaluated,
                ));
            }
        }
        Ok(())
    }
}

struct Draft2019ItemsFilter {
    unevaluated: Option<SchemaNode>,
    contains: Option<SchemaNode>,
    ref_: Option<Box<Self>>,
    recursive_ref: Option<Box<Self>>,
    items: Option<usize>,
    conditional: Option<Box<ConditionalFilter<Self>>>,
    all_of: Option<CombinatorFilter<Self>>,
    any_of: Option<CombinatorFilter<Self>>,
    one_of: Option<CombinatorFilter<Self>>,
}

impl ItemsFilter for Draft2019ItemsFilter {
    fn new<'a>(
        ctx: &'a compiler::Context<'_>,
        parent: &'a Map<String, Value>,
    ) -> Result<Self, ValidationError<'a>> {
        let mut ref_ = None;

        if let Some(Value::String(reference)) = parent.get("$ref") {
            let (contents, resolver, draft) = ctx.lookup(reference)?.into_inner();
            let vocabularies = ctx.registry.find_vocabularies(draft, contents);
            let referenced_ctx = ctx.with_resolver_and_draft(
                resolver,
                draft,
                vocabularies,
                ctx.location().clone(),
            );
            if let Value::Object(subschema) = contents {
                ref_ = Some(Box::new(
                    Self::new(&referenced_ctx, subschema).map_err(|error| error.to_owned())?,
                ));
            }
        }
        let mut recursive_ref = None;

        if parent.contains_key("$recursiveRef") {
            let (contents, resolver, draft) = ctx.lookup_recursive_reference()?.into_inner();
            let vocabularies = ctx.registry.find_vocabularies(draft, contents);
            let referenced_ctx = ctx.with_resolver_and_draft(
                resolver,
                draft,
                vocabularies,
                ctx.location().clone(),
            );
            if let Value::Object(subschema) = contents {
                recursive_ref = Some(Box::new(
                    Self::new(&referenced_ctx, subschema).map_err(|error| error.to_owned())?,
                ));
            }
        }

        let mut conditional = None;

        if let Some(subschema) = parent.get("if") {
            let mut then_ = None;
            if let Some(Value::Object(subschema)) = parent.get("then") {
                then_ = Some(Self::new(ctx, subschema)?);
            }
            let mut else_ = None;
            if let Some(Value::Object(subschema)) = parent.get("else") {
                else_ = Some(Self::new(ctx, subschema)?);
            }
            conditional = Some(Box::new(ConditionalFilter {
                condition: compiler::compile(ctx, ctx.as_resource_ref(subschema))?,
                if_: subschema
                    .as_object()
                    .map(|parent| Self::new(ctx, parent))
                    .transpose()?,
                then_,
                else_,
            }));
        }

        let mut contains = None;
        if let Some(subschema) = parent.get("contains") {
            contains = Some(compiler::compile(ctx, ctx.as_resource_ref(subschema))?);
        };
        let mut unevaluated = None;
        if let Some(subschema) = parent.get("unevaluatedItems") {
            unevaluated = Some(compiler::compile(ctx, ctx.as_resource_ref(subschema))?);
        };
        let mut all_of = None;
        if let Some(Some(subschemas)) = parent.get("allOf").map(Value::as_array) {
            all_of = Some(CombinatorFilter::new(ctx, subschemas)?);
        };
        let mut any_of = None;
        if let Some(Some(subschemas)) = parent.get("anyOf").map(Value::as_array) {
            any_of = Some(CombinatorFilter::new(ctx, subschemas)?);
        };
        let mut one_of = None;
        if let Some(Some(subschemas)) = parent.get("oneOf").map(Value::as_array) {
            one_of = Some(CombinatorFilter::new(ctx, subschemas)?);
        };
        let mut items = None;
        if let Some(subschema) = parent.get("items") {
            let limit = if parent.contains_key("additionalItems")
                || subschema.is_object()
                || subschema.is_boolean()
            {
                usize::MAX
            } else {
                subschema
                    .as_array()
                    .expect("Items are a schema or an array of schemas")
                    .len()
            };
            items = Some(limit);
        };

        Ok(Draft2019ItemsFilter {
            unevaluated,
            contains,
            ref_,
            recursive_ref,
            items,
            conditional,
            all_of,
            any_of,
            one_of,
        })
    }
    fn unevaluated(&self) -> Option<&SchemaNode> {
        self.unevaluated.as_ref()
    }
    fn mark_evaluated_indexes(&self, instance: &Value, indexes: &mut Vec<bool>) {
        if let Some(limit) = self.items {
            for idx in indexes.iter_mut().take(limit) {
                *idx = true;
            }
        }

        if let Some(ref_) = &self.ref_ {
            ref_.mark_evaluated_indexes(instance, indexes);
        }

        if let Some(recursive_ref) = &self.recursive_ref {
            recursive_ref.mark_evaluated_indexes(instance, indexes);
        }

        if let Some(conditional) = &self.conditional {
            conditional.mark_evaluated_indexes(instance, indexes);
        }
        if let Value::Array(items) = instance {
            for (item, is_evaluated) in items.iter().zip(indexes.iter_mut()) {
                if *is_evaluated {
                    continue;
                }
                if let Some(validator) = &self.contains {
                    if validator.is_valid(item) {
                        *is_evaluated = true;
                        continue;
                    }
                }
                if let Some(validator) = &self.unevaluated {
                    if validator.is_valid(item) {
                        *is_evaluated = true;
                    }
                }
            }
        }

        if let Some(combinator) = &self.all_of {
            if combinator
                .subschemas
                .iter()
                .all(|(v, _)| v.is_valid(instance))
            {
                combinator.mark_evaluated_indexes(instance, indexes);
            }
        }

        if let Some(combinator) = &self.any_of {
            combinator.mark_evaluated_indexes(instance, indexes);
        }

        if let Some(combinator) = &self.one_of {
            if combinator
                .subschemas
                .iter()
                .filter(|(v, _)| v.is_valid(instance))
                .count()
                == 1
            {
                combinator.mark_evaluated_indexes(instance, indexes);
            }
        }
    }
}

struct DefaultItemsFilter {
    unevaluated: Option<SchemaNode>,
    contains: Option<SchemaNode>,
    ref_: Option<Box<Self>>,
    dynamic_ref: Option<Box<Self>>,
    items: bool,
    prefix_items: Option<usize>,
    conditional: Option<Box<ConditionalFilter<Self>>>,
    all_of: Option<CombinatorFilter<Self>>,
    any_of: Option<CombinatorFilter<Self>>,
    one_of: Option<CombinatorFilter<Self>>,
}

impl ItemsFilter for DefaultItemsFilter {
    fn new<'a>(
        ctx: &'a compiler::Context<'a>,
        parent: &'a Map<String, Value>,
    ) -> Result<DefaultItemsFilter, ValidationError<'a>> {
        let mut ref_ = None;

        if let Some(Value::String(reference)) = parent.get("$ref") {
            let (contents, resolver, draft) = ctx.lookup(reference)?.into_inner();
            let vocabularies = ctx.registry.find_vocabularies(draft, contents);
            let referenced_ctx = ctx.with_resolver_and_draft(
                resolver,
                draft,
                vocabularies,
                ctx.location().clone(),
            );
            if let Value::Object(subschema) = contents {
                ref_ = Some(Box::new(
                    Self::new(&referenced_ctx, subschema).map_err(|error| error.to_owned())?,
                ));
            }
        }

        let mut dynamic_ref = None;

        if let Some(Value::String(reference)) = parent.get("$dynamicRef") {
            let (contents, resolver, draft) = ctx.lookup(reference)?.into_inner();
            let vocabularies = ctx.registry.find_vocabularies(draft, contents);
            let referenced_ctx = ctx.with_resolver_and_draft(
                resolver,
                draft,
                vocabularies,
                ctx.location().clone(),
            );
            if let Value::Object(subschema) = contents {
                dynamic_ref = Some(Box::new(
                    Self::new(&referenced_ctx, subschema).map_err(|error| error.to_owned())?,
                ));
            }
        }

        let mut conditional = None;

        if let Some(subschema) = parent.get("if") {
            let mut then_ = None;
            if let Some(Value::Object(subschema)) = parent.get("then") {
                then_ = Some(Self::new(ctx, subschema)?);
            }
            let mut else_ = None;
            if let Some(Value::Object(subschema)) = parent.get("else") {
                else_ = Some(Self::new(ctx, subschema)?);
            }
            conditional = Some(Box::new(ConditionalFilter {
                condition: compiler::compile(ctx, ctx.as_resource_ref(subschema))?,
                if_: subschema
                    .as_object()
                    .map(|parent| Self::new(ctx, parent))
                    .transpose()?,
                then_,
                else_,
            }));
        }

        let mut prefix_items = None;
        if let Some(Some(items)) = parent.get("prefixItems").map(Value::as_array) {
            prefix_items = Some(items.len());
        }

        let mut contains = None;
        if let Some(subschema) = parent.get("contains") {
            contains = Some(compiler::compile(ctx, ctx.as_resource_ref(subschema))?);
        };
        let mut unevaluated = None;
        if let Some(subschema) = parent.get("unevaluatedItems") {
            unevaluated = Some(compiler::compile(ctx, ctx.as_resource_ref(subschema))?);
        };
        let mut all_of = None;
        if let Some(Some(subschemas)) = parent.get("allOf").map(Value::as_array) {
            all_of = Some(CombinatorFilter::new(ctx, subschemas)?);
        };
        let mut any_of = None;
        if let Some(Some(subschemas)) = parent.get("anyOf").map(Value::as_array) {
            any_of = Some(CombinatorFilter::new(ctx, subschemas)?);
        };

        let mut one_of = None;
        if let Some(Some(subschemas)) = parent.get("oneOf").map(Value::as_array) {
            one_of = Some(CombinatorFilter::new(ctx, subschemas)?);
        };

        Ok(DefaultItemsFilter {
            unevaluated,
            contains,
            ref_,
            dynamic_ref,
            items: parent.contains_key("items"),
            prefix_items,
            conditional,
            all_of,
            any_of,
            one_of,
        })
    }
    fn unevaluated(&self) -> Option<&SchemaNode> {
        self.unevaluated.as_ref()
    }

    fn mark_evaluated_indexes(&self, instance: &Value, indexes: &mut Vec<bool>) {
        if self.items {
            for idx in indexes {
                *idx = true;
            }
            return;
        }

        if let Some(ref_) = &self.ref_ {
            ref_.mark_evaluated_indexes(instance, indexes);
        }

        if let Some(dynamic_ref) = &self.dynamic_ref {
            dynamic_ref.mark_evaluated_indexes(instance, indexes);
        }

        if let Some(limit) = self.prefix_items {
            for idx in indexes.iter_mut().take(limit) {
                *idx = true;
            }
        }
        if let Some(conditional) = &self.conditional {
            conditional.mark_evaluated_indexes(instance, indexes);
        }
        if let Value::Array(items) = instance {
            for (item, is_evaluated) in items.iter().zip(indexes.iter_mut()) {
                if *is_evaluated {
                    continue;
                }
                if let Some(validator) = &self.contains {
                    if validator.is_valid(item) {
                        *is_evaluated = true;
                        continue;
                    }
                }
                if let Some(validator) = &self.unevaluated {
                    if validator.is_valid(item) {
                        *is_evaluated = true;
                    }
                }
            }
        }

        if let Some(combinator) = &self.all_of {
            if combinator
                .subschemas
                .iter()
                .all(|(v, _)| v.is_valid(instance))
            {
                combinator.mark_evaluated_indexes(instance, indexes);
            }
        }

        if let Some(combinator) = &self.any_of {
            combinator.mark_evaluated_indexes(instance, indexes);
        }

        if let Some(combinator) = &self.one_of {
            if combinator
                .subschemas
                .iter()
                .filter(|(v, _)| v.is_valid(instance))
                .count()
                == 1
            {
                combinator.mark_evaluated_indexes(instance, indexes);
            }
        }
    }
}

struct CombinatorFilter<F> {
    subschemas: Vec<(SchemaNode, Option<F>)>,
}

impl<F: ItemsFilter> CombinatorFilter<F> {
    fn mark_evaluated_indexes(&self, instance: &Value, indexes: &mut Vec<bool>) {
        for (validator, subschema) in &self.subschemas {
            if validator.is_valid(instance) {
                if let Some(subschema) = subschema {
                    subschema.mark_evaluated_indexes(instance, indexes);
                }
            }
        }
    }
}

impl<F: ItemsFilter> CombinatorFilter<F> {
    fn new<'a>(
        ctx: &'a compiler::Context,
        subschemas: &'a [Value],
    ) -> Result<CombinatorFilter<F>, ValidationError<'a>> {
        let mut buffer = Vec::with_capacity(subschemas.len());
        for subschema in subschemas {
            let validator = compiler::compile(ctx, ctx.as_resource_ref(subschema))?;
            let filter = subschema
                .as_object()
                .map(|parent| F::new(ctx, parent))
                .transpose()?;
            buffer.push((validator, filter));
        }
        Ok(CombinatorFilter { subschemas: buffer })
    }
}

struct ConditionalFilter<F> {
    condition: SchemaNode,
    if_: Option<F>,
    then_: Option<F>,
    else_: Option<F>,
}

impl<F: ItemsFilter> ConditionalFilter<F> {
    fn mark_evaluated_indexes(&self, instance: &Value, indexes: &mut Vec<bool>) {
        if self.condition.is_valid(instance) {
            if let Some(if_) = &self.if_ {
                if_.mark_evaluated_indexes(instance, indexes);
            }
            if let Some(then_) = &self.then_ {
                then_.mark_evaluated_indexes(instance, indexes);
            }
        } else if let Some(else_) = &self.else_ {
            else_.mark_evaluated_indexes(instance, indexes);
        }
    }
}

pub(crate) fn compile<'a>(
    ctx: &'a compiler::Context,
    parent: &'a Map<String, Value>,
    schema: &'a Value,
) -> Option<CompilationResult<'a>> {
    match schema.as_bool() {
        Some(true) => None,
        _ => {
            if ctx.draft() == Draft::Draft201909 {
                Some(UnevaluatedItemsValidator::<Draft2019ItemsFilter>::compile(
                    ctx, parent,
                ))
            } else {
                Some(UnevaluatedItemsValidator::<DefaultItemsFilter>::compile(
                    ctx, parent,
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn relative_references_use_the_referenced_resource() {
        for (draft, tuple_keyword) in [
            (crate::Draft::Draft201909, "items"),
            (crate::Draft::Draft202012, "prefixItems"),
        ] {
            let mut schema = json!({
                "$id": "https://example.com/root",
                "$defs": {
                    "array": {
                        "$id": "arrays/",
                        "$defs": {"head": {"$id": "head"}},
                        "$ref": "head"
                    }
                },
                "$ref": "arrays/",
                "unevaluatedItems": false
            });
            schema["$defs"]["array"]["$defs"]["head"][tuple_keyword] = json!([true]);
            crate::tests_util::is_valid_with_draft(draft, &schema, &json!([1]));
            crate::tests_util::is_not_valid_with_draft(draft, &schema, &json!([1, 2]));
        }
    }

    #[test]
    fn successful_combinators_only_contribute_annotations() {
        for (draft, tuple_keyword) in [
            (crate::Draft::Draft201909, "items"),
            (crate::Draft::Draft202012, "prefixItems"),
        ] {
            for keyword in ["anyOf", "oneOf"] {
                let mut successful = json!({});
                successful[tuple_keyword] = json!([true]);
                let mut failed = json!({"minItems": 3});
                failed[tuple_keyword] = json!([true, true]);
                let mut schema = json!({"unevaluatedItems": false});
                schema[keyword] = json!([successful, failed]);
                crate::tests_util::is_valid_with_draft(draft, &schema, &json!([1]));
                crate::tests_util::is_not_valid_with_draft(draft, &schema, &json!([1, 2]));

                // A successful boolean branch contributes no annotations.
                schema[keyword] = json!([true, schema[keyword][1].clone()]);
                crate::tests_util::is_not_valid_with_draft(draft, &schema, &json!([1, 2]));
            }
        }
    }

    #[test]
    fn boolean_conditions_select_annotation_branch() {
        for (draft, tuple_keyword) in [
            (crate::Draft::Draft201909, "items"),
            (crate::Draft::Draft202012, "prefixItems"),
        ] {
            for (condition, branch) in [(true, "then"), (false, "else")] {
                let mut schema = json!({"if": condition, "unevaluatedItems": false});
                schema[branch] = json!({});
                schema[branch][tuple_keyword] = json!([true]);
                crate::tests_util::is_valid_with_draft(draft, &schema, &json!([1]));
                crate::tests_util::is_not_valid_with_draft(draft, &schema, &json!([1, 2]));
            }
        }
    }

    #[test]
    fn draft_2019_boolean_items() {
        for items in [true, false] {
            let schema = json!({"items": items, "unevaluatedItems": false});
            crate::tests_util::is_valid_with_draft(crate::Draft::Draft201909, &schema, &json!([]));
            let validator = crate::options()
                .with_draft(crate::Draft::Draft201909)
                .build(&schema)
                .expect("Boolean items is a schema");
            assert_eq!(validator.is_valid(&json!([1])), items);
        }
    }

    #[test]
    fn test_unevaluated_items_with_recursion() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "allOf": [
                {
                    "$ref": "#/$defs/array_1"
                }
            ],
            "unevaluatedItems": false,
            "$defs": {
                "array_1": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "string"
                        },
                        {
                            "allOf": [
                                {
                                    "$ref": "#/$defs/array_2"
                                }
                            ],
                            "type": "array",
                            "unevaluatedItems": false
                        }
                    ]
                },
                "array_2": {
                    "type": "array",
                    "prefixItems": [
                        {
                            "type": "number"
                        },
                        {
                            "allOf": [
                                {
                                    "$ref": "#/$defs/array_1"
                                }
                            ],
                            "type": "array",
                            "unevaluatedItems": false
                        }
                    ]
                }
            }
        });

        let validator = crate::validator_for(&schema).expect("Schema should compile");

        // This instance should fail validation because the nested array has an unevaluated item
        let instance = json!([
            "string",
            [
                42,
                [
                    "string",
                    [
                        42,
                        "unexpected" // This item should cause validation to fail
                    ]
                ]
            ]
        ]);

        assert!(!validator.is_valid(&instance));
        assert!(validator.validate(&instance).is_err());

        // This instance should pass validation as all items are evaluated
        let valid_instance = json!(["string", [42, ["string", [42]]]]);

        assert!(validator.is_valid(&valid_instance));
        assert!(validator.validate(&valid_instance).is_ok());
    }
}
