//! Runtime specification contract derived from the pinned json-render core types.

use serde_json::{json, Map, Value};

use super::{ActionDecl, ComponentDecl, JsonSchemaMode, RendererTarget};

pub(super) fn renderer_actions(
    actions: &[ActionDecl],
    renderer: RendererTarget,
) -> Vec<&ActionDecl> {
    actions
        .iter()
        .filter(|action| action.renderers.contains(&renderer))
        .collect()
}

pub(super) fn spec_schema(
    components: &[ComponentDecl],
    actions: &[ActionDecl],
    renderer: RendererTarget,
    mode: JsonSchemaMode,
) -> Value {
    let strict = matches!(mode, JsonSchemaMode::Strict);
    let action_binding = action_binding_schema(actions, renderer);
    let variants: Vec<_> = components
        .iter()
        .map(|component| element_schema(component, &action_binding, strict))
        .collect();
    json!({
        "$schema":"http://json-schema.org/draft-07/schema#",
        "title":"Manifest-pinned json-render runtime specification",
        "type":"object",
        "properties":{
            "root":{"type":"string","minLength":1},
            "elements":{"type":"object","additionalProperties":{"oneOf":variants}},
            "state":{"type":"object"}
        },
        "required":["root","elements"],
        "additionalProperties":!strict,
        "definitions":{"visibilityCondition":visibility_schema()}
    })
}

fn element_schema(component: &ComponentDecl, binding: &Value, strict: bool) -> Value {
    let event_properties: Map<_, _> = component
        .events
        .iter()
        .map(|event| (event.clone(), binding.clone()))
        .collect();
    json!({
        "type":"object",
        "properties":{
            "type":{"const":component.name},
            "props":runtime_props_schema(&component.props_schema),
            "children":{"type":"array","items":{"type":"string"}},
            "visible":{"$ref":"#/definitions/visibilityCondition"},
            "on":{"type":"object","properties":event_properties,"additionalProperties":false},
            "repeat":{
                "type":"object",
                "properties":{"statePath":{"type":"string","minLength":1},"key":{"type":"string","minLength":1}},
                "required":["statePath"],
                "additionalProperties":false
            },
            "watch":{"type":"object","additionalProperties":binding}
        },
        "required":["type","props","children"],
        "additionalProperties":!strict
    })
}

fn runtime_props_schema(literal: &Value) -> Value {
    let mut schema = literal.clone();
    if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
        for property in properties.values_mut() {
            *property = json!({"anyOf":[property.clone(), prop_expression_schema()]});
        }
    }
    schema
}

fn prop_expression_schema() -> Value {
    let exact_string = |key: &str| {
        let mut properties = Map::new();
        properties.insert(key.to_string(), json!({"type":"string"}));
        json!({
            "type":"object","properties":properties,"required":[key],"additionalProperties":false
        })
    };
    json!({"oneOf":[
        exact_string("$state"), exact_string("$item"), exact_string("$bindState"),
        exact_string("$bindItem"), exact_string("$template"),
        {"type":"object","properties":{"$index":{"const":true}},"required":["$index"],"additionalProperties":false},
        {"type":"object","properties":{"$computed":{"type":"string"},"args":{"type":"object"}},"required":["$computed"],"additionalProperties":false},
        {"type":"object","properties":{"$cond":{"$ref":"#/definitions/visibilityCondition"},"$then":{},"$else":{}},"required":["$cond","$then","$else"],"additionalProperties":false},
        format_directive(), math_directive(),
        {"type":"object","properties":{"$concat":{"type":"array"}},"required":["$concat"],"additionalProperties":false},
        {"type":"object","properties":{"$count":{}},"required":["$count"],"additionalProperties":false},
        {"type":"object","properties":{"$truncate":{},"length":{"type":"number"},"suffix":{"type":"string"}},"required":["$truncate"],"additionalProperties":false},
        {"type":"object","properties":{"$pluralize":{},"zero":{"type":"string"},"one":{"type":"string"},"other":{"type":"string"}},"required":["$pluralize","one","other"],"additionalProperties":false},
        {"type":"object","properties":{"$join":{},"separator":{"type":"string"}},"required":["$join"],"additionalProperties":false},
        {"type":"object","properties":{"$t":{"type":"string"},"params":{"type":"object"}},"required":["$t"],"additionalProperties":false}
    ]})
}

fn format_directive() -> Value {
    json!({"type":"object","properties":{
        "$format":{"enum":["date","currency","number","percent"]},"value":{},
        "locale":{"type":"string"},"currency":{"type":"string"},"notation":{"type":"string"},
        "style":{"type":"string"},"options":{"type":"object"},"now":{"type":"number"}
    },"required":["$format","value"],"additionalProperties":false})
}

fn math_directive() -> Value {
    json!({"type":"object","properties":{
        "$math":{"enum":["add","subtract","multiply","divide","mod","min","max","round","floor","ceil","abs"]},
        "a":{},"b":{}
    },"required":["$math"],"additionalProperties":false})
}

fn action_binding_schema(actions: &[ActionDecl], renderer: RendererTarget) -> Value {
    let actions = renderer_actions(actions, renderer);
    if actions.is_empty() {
        return json!(false);
    }
    let variants: Vec<_> = actions
        .iter()
        .map(|action| {
            let required = if matches!(action.name.as_str(), "pop" | "validateForm") {
                json!(["action"])
            } else {
                json!(["action", "params"])
            };
            json!({"type":"object","properties":{
                "action":{"const":action.name},"params":action.params_schema,
                "confirm":confirm_schema(),"onSuccess":outcome_schema(true),
                "onError":outcome_schema(false),"preventDefault":{"type":"boolean"}
            },"required":required,"additionalProperties":false})
        })
        .collect();
    let single = json!({"oneOf":variants});
    json!({"oneOf":[single.clone(),{"type":"array","items":single,"minItems":1}]})
}

pub(super) fn builtin_action_params_schema(action: &str) -> Value {
    let (properties, required): (Value, &[&str]) = match action {
        "setState" => (
            json!({"statePath":dynamic_action_value(json!({"type":"string","minLength":1})),"value":{}}),
            &["statePath", "value"],
        ),
        "pushState" => (
            json!({"statePath":dynamic_action_value(json!({"type":"string","minLength":1})),"value":{},"clearStatePath":dynamic_action_value(json!({"type":"string","minLength":1}))}),
            &["statePath", "value"],
        ),
        "removeState" => (
            json!({"statePath":dynamic_action_value(json!({"type":"string","minLength":1})),"index":dynamic_action_value(json!({"type":"integer","minimum":0}))}),
            &["statePath", "index"],
        ),
        "validateForm" => (
            json!({"statePath":dynamic_action_value(json!({"type":"string","minLength":1}))}),
            &[],
        ),
        "push" => (
            json!({"screen":dynamic_action_value(json!({"type":"string","minLength":1}))}),
            &["screen"],
        ),
        "pop" => (json!({}), &[]),
        _ => unreachable!("renderer action inventory is closed"),
    };
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

fn dynamic_action_value(literal: Value) -> Value {
    json!({"oneOf":[
        literal,
        {"type":"object","properties":{"$state":{"type":"string"}},"required":["$state"],"additionalProperties":false}
    ]})
}

fn confirm_schema() -> Value {
    json!({"type":"object","properties":{
        "title":{"type":"string"},"message":{"type":"string"},"confirmLabel":{"type":"string"},
        "cancelLabel":{"type":"string"},"variant":{"enum":["default","danger"]}
    },"required":["title","message"],"additionalProperties":false})
}

fn outcome_schema(success: bool) -> Value {
    let mut variants = vec![
        json!({"type":"object","properties":{"set":{"type":"object"}},"required":["set"],"additionalProperties":false}),
        json!({"type":"object","properties":{"action":{"type":"string"}},"required":["action"],"additionalProperties":false}),
    ];
    if success {
        variants.push(json!({"type":"object","properties":{"navigate":{"type":"string"}},"required":["navigate"],"additionalProperties":false}));
    }
    json!({"oneOf":variants})
}

fn visibility_schema() -> Value {
    let number_or_state = json!({"oneOf":[{"type":"number"},{"type":"object","properties":{"$state":{"type":"string"}},"required":["$state"],"additionalProperties":false}]});
    let comparison = |selector: &str, selector_schema: Value| {
        let mut properties = Map::new();
        properties.insert(selector.to_string(), selector_schema);
        properties.insert("eq".into(), json!({}));
        properties.insert("neq".into(), json!({}));
        properties.insert("gt".into(), number_or_state.clone());
        properties.insert("gte".into(), number_or_state.clone());
        properties.insert("lt".into(), number_or_state.clone());
        properties.insert("lte".into(), number_or_state.clone());
        properties.insert("not".into(), json!({"const":true}));
        json!({
            "type":"object","properties":properties,"required":[selector],"additionalProperties":false
        })
    };
    let single = json!({"oneOf":[
        comparison("$state",json!({"type":"string"})),
        comparison("$item",json!({"type":"string"})),
        comparison("$index",json!({"const":true}))
    ]});
    json!({"oneOf":[
        {"type":"boolean"}, single.clone(), {"type":"array","items":single},
        {"type":"object","properties":{"$and":{"type":"array","items":{"$ref":"#/definitions/visibilityCondition"}}},"required":["$and"],"additionalProperties":false},
        {"type":"object","properties":{"$or":{"type":"array","items":{"$ref":"#/definitions/visibilityCondition"}}},"required":["$or"],"additionalProperties":false}
    ]})
}
