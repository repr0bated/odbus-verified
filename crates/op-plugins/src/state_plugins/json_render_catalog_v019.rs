//! One-time structured import of `@json-render/shadcn` 0.19.0 component schemas.
//! Source commit: e2d00faeaabe2871ca18a4594a9ec39a245f9b6c.
//! Source `packages/shadcn/src/catalog.ts` SHA-256:
//! e8d1387428b5317a672bc599d026751bd9a1a32b0d77538f5da6cea967513c07.

use super::{ComponentDecl, JsonValue};

pub(super) fn components() -> Vec<ComponentDecl> {
    vec![
        component(
            "Card",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"title":{"type":["string","null"]},"description":{"type":["string","null"]},"maxWidth":{"anyOf":[{"type":"string","enum":["sm","md","lg","full"]},{"type":"null"}]},"centered":{"type":["boolean","null"]},"className":{"description":"Additional CSS classes","type":["string","null"]}},"required":["title","description","maxWidth","centered","className"],"additionalProperties":false}),
            &["default"],
            &[],
            "Container card for content sections. Use for forms/content boxes, NOT for page headers.",
        ),
        component(
            "Stack",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"direction":{"anyOf":[{"type":"string","enum":["horizontal","vertical"]},{"type":"null"}]},"gap":{"anyOf":[{"type":"string","enum":["none","sm","md","lg","xl"]},{"type":"null"}]},"align":{"anyOf":[{"type":"string","enum":["start","center","end","stretch"]},{"type":"null"}]},"justify":{"anyOf":[{"type":"string","enum":["start","center","end","between","around"]},{"type":"null"}]},"className":{"description":"Additional CSS classes","type":["string","null"]}},"required":["direction","gap","align","justify","className"],"additionalProperties":false}),
            &["default"],
            &[],
            "Flex container for layouts",
        ),
        component(
            "Grid",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"columns":{"type":["number","null"]},"gap":{"anyOf":[{"type":"string","enum":["sm","md","lg","xl"]},{"type":"null"}]},"className":{"description":"Additional CSS classes","type":["string","null"]}},"required":["columns","gap","className"],"additionalProperties":false}),
            &["default"],
            &[],
            "Grid layout (1-6 columns)",
        ),
        component(
            "Separator",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"orientation":{"anyOf":[{"type":"string","enum":["horizontal","vertical"]},{"type":"null"}]}},"required":["orientation"],"additionalProperties":false}),
            &[],
            &[],
            "Visual separator line",
        ),
        component(
            "Tabs",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"tabs":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["label","value"],"additionalProperties":false}},"defaultValue":{"type":["string","null"]},"value":{"type":["string","null"]}},"required":["tabs","defaultValue","value"],"additionalProperties":false}),
            &["default"],
            &["change"],
            "Tab navigation. Use { $bindState } on value for active tab binding.",
        ),
        component(
            "Accordion",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"title":{"type":"string"},"content":{"type":"string"}},"required":["title","content"],"additionalProperties":false}},"type":{"anyOf":[{"type":"string","enum":["single","multiple"]},{"type":"null"}]}},"required":["items","type"],"additionalProperties":false}),
            &[],
            &[],
            "Collapsible sections. Items as [{title, content}]. Type 'single' (default) or 'multiple'.",
        ),
        component(
            "Collapsible",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"title":{"type":"string"},"defaultOpen":{"type":["boolean","null"]}},"required":["title","defaultOpen"],"additionalProperties":false}),
            &["default"],
            &[],
            "Collapsible section with trigger. Children render inside.",
        ),
        component(
            "Dialog",
            "overlay",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"title":{"type":"string"},"description":{"type":["string","null"]},"openPath":{"type":"string"}},"required":["title","description","openPath"],"additionalProperties":false}),
            &["default"],
            &[],
            "Modal dialog. Set openPath to a boolean state path. Use setState to toggle.",
        ),
        component(
            "Drawer",
            "overlay",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"title":{"type":"string"},"description":{"type":["string","null"]},"openPath":{"type":"string"}},"required":["title","description","openPath"],"additionalProperties":false}),
            &["default"],
            &[],
            "Bottom sheet drawer. Set openPath to a boolean state path. Use setState to toggle.",
        ),
        component(
            "Carousel",
            "layout",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"title":{"type":["string","null"]},"description":{"type":["string","null"]}},"required":["title","description"],"additionalProperties":false}}},"required":["items"],"additionalProperties":false}),
            &[],
            &[],
            "Horizontally scrollable carousel of cards.",
        ),
        component(
            "Table",
            "data",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"columns":{"type":"array","items":{"type":"string"}},"rows":{"type":"array","items":{"type":"array","items":{"type":"string"}}},"caption":{"type":["string","null"]}},"required":["columns","rows","caption"],"additionalProperties":false}),
            &[],
            &[],
            "Data table. columns: header labels. rows: 2D array of cell strings, e.g. [[\"Alice\",\"admin\"],[\"Bob\",\"user\"]].",
        ),
        component(
            "Heading",
            "typography",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"text":{"type":"string"},"level":{"anyOf":[{"type":"string","enum":["h1","h2","h3","h4"]},{"type":"null"}]}},"required":["text","level"],"additionalProperties":false}),
            &[],
            &[],
            "Heading text (h1-h4)",
        ),
        component(
            "Text",
            "typography",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"text":{"type":"string"},"variant":{"anyOf":[{"type":"string","enum":["body","caption","muted","lead","code"]},{"type":"null"}]}},"required":["text","variant"],"additionalProperties":false}),
            &[],
            &[],
            "Paragraph text",
        ),
        component(
            "Image",
            "media",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"src":{"type":["string","null"]},"alt":{"type":"string"},"width":{"type":["number","null"]},"height":{"type":["number","null"]}},"required":["src","alt","width","height"],"additionalProperties":false}),
            &[],
            &[],
            "Image component. Renders an img tag when src is provided, otherwise a placeholder.",
        ),
        component(
            "Avatar",
            "data",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"src":{"type":["string","null"]},"name":{"type":"string"},"size":{"anyOf":[{"type":"string","enum":["sm","md","lg"]},{"type":"null"}]}},"required":["src","name","size"],"additionalProperties":false}),
            &[],
            &[],
            "User avatar with fallback initials",
        ),
        component(
            "Badge",
            "data",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"text":{"type":"string"},"variant":{"anyOf":[{"type":"string","enum":["default","secondary","destructive","outline"]},{"type":"null"}]}},"required":["text","variant"],"additionalProperties":false}),
            &[],
            &[],
            "Status badge",
        ),
        component(
            "Alert",
            "feedback",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"title":{"type":"string"},"message":{"type":["string","null"]},"type":{"anyOf":[{"type":"string","enum":["info","success","warning","error"]},{"type":"null"}]}},"required":["title","message","type"],"additionalProperties":false}),
            &[],
            &[],
            "Alert banner",
        ),
        component(
            "Progress",
            "feedback",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"value":{"type":"number"},"max":{"type":["number","null"]},"label":{"type":["string","null"]}},"required":["value","max","label"],"additionalProperties":false}),
            &[],
            &[],
            "Progress bar (value 0-100)",
        ),
        component(
            "Skeleton",
            "feedback",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"width":{"type":["string","null"]},"height":{"type":["string","null"]},"rounded":{"type":["boolean","null"]}},"required":["width","height","rounded"],"additionalProperties":false}),
            &[],
            &[],
            "Loading placeholder skeleton",
        ),
        component(
            "Spinner",
            "feedback",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"size":{"anyOf":[{"type":"string","enum":["sm","md","lg"]},{"type":"null"}]},"label":{"type":["string","null"]}},"required":["size","label"],"additionalProperties":false}),
            &[],
            &[],
            "Loading spinner indicator",
        ),
        component(
            "Tooltip",
            "overlay",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"content":{"type":"string"},"text":{"type":"string"}},"required":["content","text"],"additionalProperties":false}),
            &[],
            &[],
            "Hover tooltip. Shows content on hover over text.",
        ),
        component(
            "Popover",
            "overlay",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"trigger":{"type":"string"},"content":{"type":"string"}},"required":["trigger","content"],"additionalProperties":false}),
            &[],
            &[],
            "Popover that appears on click of trigger.",
        ),
        component(
            "Input",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"name":{"type":"string"},"type":{"anyOf":[{"type":"string","enum":["text","email","password","number"]},{"type":"null"}]},"placeholder":{"type":["string","null"]},"value":{"type":["string","null"]},"checks":{"anyOf":[{"type":"array","items":{"type":"object","properties":{"type":{"type":"string"},"message":{"type":"string"},"args":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}},"required":["type","message"],"additionalProperties":false}},{"type":"null"}]},"validateOn":{"anyOf":[{"type":"string","enum":["change","blur","submit"]},{"type":"null"}]}},"required":["label","name","type","placeholder","value","checks","validateOn"],"additionalProperties":false}),
            &[],
            &["submit", "focus", "blur"],
            "Text input field. Use { $bindState } on value for two-way binding. Use checks for validation (e.g. required, email, minLength). validateOn controls timing (default: blur).",
        ),
        component(
            "Textarea",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"name":{"type":"string"},"placeholder":{"type":["string","null"]},"rows":{"type":["number","null"]},"value":{"type":["string","null"]},"checks":{"anyOf":[{"type":"array","items":{"type":"object","properties":{"type":{"type":"string"},"message":{"type":"string"},"args":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}},"required":["type","message"],"additionalProperties":false}},{"type":"null"}]},"validateOn":{"anyOf":[{"type":"string","enum":["change","blur","submit"]},{"type":"null"}]}},"required":["label","name","placeholder","rows","value","checks","validateOn"],"additionalProperties":false}),
            &[],
            &[],
            "Multi-line text input. Use { $bindState } on value for binding. Use checks for validation. validateOn controls timing (default: blur).",
        ),
        component(
            "Select",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"name":{"type":"string"},"options":{"type":"array","items":{"type":"string"}},"placeholder":{"type":["string","null"]},"value":{"type":["string","null"]},"checks":{"anyOf":[{"type":"array","items":{"type":"object","properties":{"type":{"type":"string"},"message":{"type":"string"},"args":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}},"required":["type","message"],"additionalProperties":false}},{"type":"null"}]},"validateOn":{"anyOf":[{"type":"string","enum":["change","blur","submit"]},{"type":"null"}]}},"required":["label","name","options","placeholder","value","checks","validateOn"],"additionalProperties":false}),
            &[],
            &["change"],
            "Dropdown select input. Use { $bindState } on value for binding. Use checks for validation. validateOn controls timing (default: change).",
        ),
        component(
            "Checkbox",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"name":{"type":"string"},"checked":{"type":["boolean","null"]},"checks":{"anyOf":[{"type":"array","items":{"type":"object","properties":{"type":{"type":"string"},"message":{"type":"string"},"args":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}},"required":["type","message"],"additionalProperties":false}},{"type":"null"}]},"validateOn":{"anyOf":[{"type":"string","enum":["change","blur","submit"]},{"type":"null"}]}},"required":["label","name","checked","checks","validateOn"],"additionalProperties":false}),
            &[],
            &["change"],
            "Checkbox input. Use { $bindState } on checked for binding. Use checks for validation. validateOn controls timing (default: change).",
        ),
        component(
            "Radio",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"name":{"type":"string"},"options":{"type":"array","items":{"type":"string"}},"value":{"type":["string","null"]},"checks":{"anyOf":[{"type":"array","items":{"type":"object","properties":{"type":{"type":"string"},"message":{"type":"string"},"args":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}},"required":["type","message"],"additionalProperties":false}},{"type":"null"}]},"validateOn":{"anyOf":[{"type":"string","enum":["change","blur","submit"]},{"type":"null"}]}},"required":["label","name","options","value","checks","validateOn"],"additionalProperties":false}),
            &[],
            &["change"],
            "Radio button group. Use { $bindState } on value for binding. Use checks for validation. validateOn controls timing (default: change).",
        ),
        component(
            "Switch",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"name":{"type":"string"},"checked":{"type":["boolean","null"]},"checks":{"anyOf":[{"type":"array","items":{"type":"object","properties":{"type":{"type":"string"},"message":{"type":"string"},"args":{"type":"object","propertyNames":{"type":"string"},"additionalProperties":{}}},"required":["type","message"],"additionalProperties":false}},{"type":"null"}]},"validateOn":{"anyOf":[{"type":"string","enum":["change","blur","submit"]},{"type":"null"}]}},"required":["label","name","checked","checks","validateOn"],"additionalProperties":false}),
            &[],
            &["change"],
            "Toggle switch. Use { $bindState } on checked for binding. Use checks for validation. validateOn controls timing (default: change).",
        ),
        component(
            "Slider",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":["string","null"]},"min":{"type":["number","null"]},"max":{"type":["number","null"]},"step":{"type":["number","null"]},"value":{"type":["number","null"]}},"required":["label","min","max","step","value"],"additionalProperties":false}),
            &[],
            &["change"],
            "Range slider input. Use { $bindState } on value for binding.",
        ),
        component(
            "Button",
            "action",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"variant":{"anyOf":[{"type":"string","enum":["primary","secondary","danger"]},{"type":"null"}]},"disabled":{"type":["boolean","null"]}},"required":["label","variant","disabled"],"additionalProperties":false}),
            &[],
            &["press"],
            "Clickable button. Bind on.press for handler.",
        ),
        component(
            "Link",
            "navigation",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"href":{"type":"string"}},"required":["label","href"],"additionalProperties":false}),
            &[],
            &["press"],
            "Anchor link. Bind on.press for click handler.",
        ),
        component(
            "DropdownMenu",
            "action",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"items":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["label","value"],"additionalProperties":false}},"value":{"type":["string","null"]}},"required":["label","items","value"],"additionalProperties":false}),
            &[],
            &["select"],
            "Dropdown menu with trigger button and selectable items. Use { $bindState } on value for selected item binding.",
        ),
        component(
            "Toggle",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"label":{"type":"string"},"pressed":{"type":["boolean","null"]},"variant":{"anyOf":[{"type":"string","enum":["default","outline"]},{"type":"null"}]}},"required":["label","pressed","variant"],"additionalProperties":false}),
            &[],
            &["change"],
            "Toggle button. Use { $bindState } on pressed for state binding.",
        ),
        component(
            "ToggleGroup",
            "form",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"items":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["label","value"],"additionalProperties":false}},"type":{"anyOf":[{"type":"string","enum":["single","multiple"]},{"type":"null"}]},"value":{"type":["string","null"]}},"required":["items","type","value"],"additionalProperties":false}),
            &[],
            &["change"],
            "Group of toggle buttons. Type 'single' (default) or 'multiple'. Use { $bindState } on value.",
        ),
        component(
            "ButtonGroup",
            "action",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"buttons":{"type":"array","items":{"type":"object","properties":{"label":{"type":"string"},"value":{"type":"string"}},"required":["label","value"],"additionalProperties":false}},"selected":{"type":["string","null"]}},"required":["buttons","selected"],"additionalProperties":false}),
            &[],
            &["change"],
            "Segmented button group. Use { $bindState } on selected for selected value.",
        ),
        component(
            "Pagination",
            "navigation",
            serde_json::json!({"$schema":"http://json-schema.org/draft-07/schema#","type":"object","properties":{"totalPages":{"type":"number"},"page":{"type":["number","null"]}},"required":["totalPages","page"],"additionalProperties":false}),
            &[],
            &["change"],
            "Page navigation. Use { $bindState } on page for current page number.",
        ),
    ]
}

fn component(
    name: &str,
    category: &str,
    props_schema: JsonValue,
    slots: &[&str],
    events: &[&str],
    description: &str,
) -> ComponentDecl {
    let props = props_schema
        .get("properties")
        .and_then(JsonValue::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default();
    ComponentDecl {
        name: name.into(),
        category: category.into(),
        props,
        props_schema,
        slots: slots.iter().map(|value| (*value).into()).collect(),
        events: events.iter().map(|value| (*value).into()).collect(),
        description: description.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn pinned_import_has_exact_component_set_and_closed_required_props() {
        let components = components();
        let names: Vec<_> = components.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Card",
                "Stack",
                "Grid",
                "Separator",
                "Tabs",
                "Accordion",
                "Collapsible",
                "Dialog",
                "Drawer",
                "Carousel",
                "Table",
                "Heading",
                "Text",
                "Image",
                "Avatar",
                "Badge",
                "Alert",
                "Progress",
                "Skeleton",
                "Spinner",
                "Tooltip",
                "Popover",
                "Input",
                "Textarea",
                "Select",
                "Checkbox",
                "Radio",
                "Switch",
                "Slider",
                "Button",
                "Link",
                "DropdownMenu",
                "Toggle",
                "ToggleGroup",
                "ButtonGroup",
                "Pagination",
            ]
        );
        for component in &components {
            let schema = component.props_schema.as_object().unwrap();
            assert_eq!(schema.get("type"), Some(&serde_json::json!("object")));
            assert_eq!(
                schema.get("additionalProperties"),
                Some(&serde_json::json!(false)),
                "{} props must remain closed",
                component.name
            );
            let properties: BTreeSet<_> = schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let required: BTreeSet<_> = schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect();
            assert_eq!(
                required, properties,
                "{} required set drifted",
                component.name
            );
            assert_eq!(
                component
                    .props
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
                properties,
                "{} property inventory drifted",
                component.name
            );
        }
    }

    #[test]
    fn pinned_button_schema_preserves_nullable_and_required_semantics() {
        let button = components()
            .into_iter()
            .find(|item| item.name == "Button")
            .unwrap();
        assert_eq!(
            button.props_schema,
            serde_json::json!({
                "$schema":"http://json-schema.org/draft-07/schema#",
                "type":"object",
                "properties":{
                    "label":{"type":"string"},
                    "variant":{"anyOf":[
                        {"type":"string","enum":["primary","secondary","danger"]},
                        {"type":"null"}
                    ]},
                    "disabled":{"type":["boolean","null"]}
                },
                "required":["label","variant","disabled"],
                "additionalProperties":false
            })
        );
        assert!(jsonschema::validate(
            &button.props_schema,
            &serde_json::json!({"label":"Save","variant":null,"disabled":null})
        )
        .is_ok());
        assert!(jsonschema::validate(
            &button.props_schema,
            &serde_json::json!({"label":"Save","variant":null})
        )
        .is_err());
        assert!(jsonschema::validate(
            &button.props_schema,
            &serde_json::json!({
                "label":"Save","variant":null,"disabled":null,"invented":true
            })
        )
        .is_err());
    }
}
